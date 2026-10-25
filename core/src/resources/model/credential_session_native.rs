//! macOS adapter: one vault session shared by runtime and transaction reads.
use super::{credential_session::{Backend, Revision, Session, VAULT_SERVICE}, keychain, KEYCHAIN_ACCOUNT};
use std::sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use serde::Serialize;

static SESSION: OnceLock<Result<Arc<Mutex<Session>>, String>> = OnceLock::new();
static CLOSING: AtomicBool = AtomicBool::new(false);
static SYNC_DEFERRED: AtomicBool = AtomicBool::new(false);
struct Native;
impl Backend for Native {
    fn revision(&self, service: &str) -> Result<Option<Revision>, String> {
        keychain::revision(service, KEYCHAIN_ACCOUNT)
            .map_err(|_| "credential_session_locked: Cannot verify stored credentials".into())
    }
    fn read(&self, service: &str, interactive: bool) -> Result<Option<Vec<u8>>, String> {
        keychain::password_with_ui(service, KEYCHAIN_ACCOUNT, interactive)
    }
    fn write_vault(&self, bytes: &[u8]) -> Result<(), String> {
        keychain::set_vault_password(VAULT_SERVICE, KEYCHAIN_ACCOUNT, bytes)
    }
}

#[derive(Serialize)]
pub(crate) struct Status {
    unlocked: bool,
    expires_in_seconds: u64,
    sync_deferred: bool,
}

pub(super) fn status() -> Status {
    let remaining = SESSION.get().and_then(|session| session.as_ref().ok())
        .and_then(|session| session.try_lock().ok())
        .and_then(|state| state.remaining(Instant::now()));
    Status { unlocked: remaining.is_some(), expires_in_seconds: remaining.map_or(0, |value| value.as_secs()),
        sync_deferred: SYNC_DEFERRED.load(Ordering::SeqCst) }
}

fn session() -> Result<&'static Arc<Mutex<Session>>, String> {
    if CLOSING.load(Ordering::SeqCst) { return Err("credential_session_locked: Arca is exiting".into()); }
    SESSION.get_or_init(|| {
        let session = Arc::new(Mutex::new(Session::default()));
        let weak = Arc::downgrade(&session);
        std::thread::Builder::new().name("credential-session-expiry".into()).spawn(move || loop {
            std::thread::sleep(Duration::from_secs(1));
            let Some(session) = weak.upgrade() else { break; };
            if let Ok(mut state) = session.try_lock() { state.expire(Instant::now()); };
        }).map_err(|_| "credential_session_unavailable: Cannot schedule secure session cleanup".to_string())?;
        Ok(session)
    }).as_ref().map_err(Clone::clone)
}
fn vault_lock() -> Result<crate::safe_write::SettingsLock, String> {
    crate::safe_write::acquire_settings_lock(&crate::paths::arca_dir().join("locks/credential-session"))
        .map_err(|_| "credential_session_busy: Another Arca process is updating credentials".into())
}
fn services() -> Result<Vec<String>, String> {
    let settings = crate::settings::load_settings_strict().map_err(|_| "credential_session_unavailable: Cannot read Provider inventory".to_string())?;
    let mut services = std::collections::BTreeSet::new();
    for provider in settings.model_providers.into_iter().flat_map(|providers| providers.into_values()) {
        services.insert(super::provider_keychain_service(&provider.id));
    }
    for profile in settings.model_profiles.into_iter().flat_map(|profiles| profiles.into_values()) {
        services.insert(super::legacy_keychain_service(&profile.id));
        if let Some(provider) = profile.provider_id { services.insert(super::provider_keychain_service(&provider)); }
    }
    Ok(services.into_iter().collect())
}
fn check_closing(state: &mut Session) -> Result<(), String> {
    if CLOSING.load(Ordering::SeqCst) { state.clear(); Err("credential_session_locked: Arca is exiting".into()) }
    else { Ok(()) }
}
pub(super) fn unlock(retry: bool) -> Result<Status, String> {
    let session = session()?;
    let mut state = session.lock().map_err(|_| "credential_session_unavailable: Credential session failed".to_string())?;
    let _lock = vault_lock()?;
    let was_unlocked = state.remaining(Instant::now()).is_some();
    state.unlock(&Native, &services()?, Instant::now(), retry)?;
    check_closing(&mut state)?;
    if retry || !was_unlocked { SYNC_DEFERRED.store(false, Ordering::SeqCst); }
    Ok(Status { unlocked: true, expires_in_seconds: state.remaining(Instant::now()).map_or(0, |value| value.as_secs()),
        sync_deferred: SYNC_DEFERRED.load(Ordering::SeqCst) })
}
pub(super) fn password(service: &str) -> Result<Option<Vec<u8>>, String> {
    if Native.revision(service)?.is_none() {
        if let Some(Ok(session)) = SESSION.get() {
            if let Ok(mut state) = session.try_lock() { state.forget(service); }
        }
        return Ok(None);
    }
    let session = session()?;
    let mut state = session.lock().map_err(|_| "credential_session_unavailable: Credential session failed".to_string())?;
    let _lock = vault_lock()?;
    state.unlock(&Native, &services()?, Instant::now(), false)?;
    check_closing(&mut state)?;
    state.read(&Native, service, Instant::now())
}
fn sync(state: &mut Session, service: &str, value: Option<&[u8]>) {
    let deferred = state.synchronize(&Native, service, value, Instant::now()).is_err();
    // A later unrelated save cannot prove an earlier missing entry was repaired.
    // Only a full successful unlock may clear this warning.
    if deferred { SYNC_DEFERRED.store(true, Ordering::SeqCst); }
}
pub(super) fn set_password(service: &str, bytes: &[u8]) -> Result<(), String> {
    let session = session()?;
    let mut state = session.lock().map_err(|_| "credential_session_unavailable: Credential session failed".to_string())?;
    let _lock = vault_lock()?;
    state.unlock(&Native, &services()?, Instant::now(), false)?;
    check_closing(&mut state)?;
    // An explicit save may finish legacy source-item permission migration.
    // Never misrepresent a write-ACL failure as repairable by rereading the vault.
    keychain::set_password(service, KEYCHAIN_ACCOUNT, bytes)?;
    sync(&mut state, service, Some(bytes));
    Ok(())
}
pub(super) fn remove<T>(service: &str, remove: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let session = session()?;
    let mut state = session.lock().map_err(|_| "credential_session_unavailable: Credential session failed".to_string())?;
    let _lock = vault_lock()?;
    state.unlock(&Native, &services()?, Instant::now(), false)?;
    check_closing(&mut state)?;
    let result = remove()?;
    sync(&mut state, service, None);
    Ok(result)
}
pub(super) fn clear() {
    CLOSING.store(true, Ordering::SeqCst);
    if let Some(Ok(session)) = SESSION.get() {
        if let Ok(mut state) = session.try_lock() { state.clear(); }
    }
}
