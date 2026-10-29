//! One revision-bound, process-local authorization session for managed API keys.
//! The aggregate lives only in Keychain; source items keep native Agent compatibility.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::{Duration, Instant, SystemTime}};
use zeroize::{Zeroize, Zeroizing};

pub(super) const VAULT_SERVICE: &str = "com.scoheart.arca.credential-session.v1";
pub(super) const MAX_AGE: Duration = Duration::from_secs(8 * 60 * 60);
const MAX_ENTRIES: usize = 512;
const MAX_KEY_BYTES: usize = 64 * 1024;
const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
pub(super) type Revision = (u64, u64);
type ReadResult = Result<Option<Vec<u8>>, String>;

pub(super) fn managed(service: &str) -> bool {
    service.strip_prefix("com.scoheart.mux.model-provider.").is_some_and(|id| !id.is_empty())
        || service.strip_prefix("com.scoheart.mux.model-profile.")
            .is_some_and(|id| !id.is_empty() && !id.starts_with("__asset-operation-rollback__."))
}

pub(super) trait Backend {
    fn revision(&self, service: &str) -> Result<Option<Revision>, String>;
    fn read(&self, service: &str, interactive: bool) -> ReadResult;
    fn write_vault(&self, bytes: &[u8]) -> Result<(), String>;
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    revision: Revision,
    secret: Vec<u8>,
}
impl Drop for Entry { fn drop(&mut self) { self.secret.zeroize(); } }

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    entries: BTreeMap<String, Entry>,
}
impl Default for Document {
    fn default() -> Self { Self { version: 1, entries: BTreeMap::new() } }
}
impl Document {
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_DOCUMENT_BYTES { return Err(invalid()); }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.entries.len() > MAX_ENTRIES
            || self.entries.iter().any(|(service, entry)| !managed(service) || entry.secret.len() > MAX_KEY_BYTES) {
            return Err(invalid());
        }
        Ok(())
    }
    fn encode(&self) -> Result<Zeroizing<Vec<u8>>, String> {
        self.validate()?;
        let bytes = Zeroizing::new(serde_json::to_vec(self).map_err(|_| invalid())?);
        if bytes.len() > MAX_DOCUMENT_BYTES { return Err(invalid()); }
        Ok(bytes)
    }
}
fn invalid() -> String { "credential_vault_invalid: Arca credential vault is invalid or unsupported; original keys were preserved".into() }
fn locked() -> String { "credential_session_locked: Unlock Arca credentials to continue".into() }
fn changed() -> String { "credential_session_refresh_required: A stored credential changed; unlock Arca credentials again".into() }

#[derive(Default)]
pub(super) struct Session {
    document: Option<Document>,
    vault_revision: Option<Revision>,
    expires: Option<Instant>,
    expires_wall: Option<SystemTime>,
    failure: Option<String>,
}

/// Resource observation must never wait for an interactive unlock while it
/// holds the workspace read gate. Only use an existing, revision-checked
/// session; explicit authorization and transaction reads keep their own path.
pub(super) fn observe(session: &std::sync::Mutex<Session>, backend: &impl Backend, service: &str, now: Instant) -> ReadResult {
    let mut state = session.try_lock().map_err(|error| match error {
        std::sync::TryLockError::WouldBlock => "credential_session_busy: Credential authorization is pending".to_string(),
        std::sync::TryLockError::Poisoned(_) => "credential_session_unavailable: Credential session failed".to_string(),
    })?;
    state.read(backend, service, now)
}

impl Session {
    pub(super) fn clear(&mut self) { *self = Self::default(); }
    pub(super) fn forget(&mut self, service: &str) {
        if let Some(document) = self.document.as_mut() { document.entries.remove(service); }
    }
    pub(super) fn expire(&mut self, now: Instant) {
        if self.expires.is_some() && self.remaining(now).is_none() { self.clear(); }
    }
    pub(super) fn remaining(&self, now: Instant) -> Option<Duration> {
        let uptime = self.expires?.checked_duration_since(now)?;
        let wall = self.expires_wall?.duration_since(SystemTime::now()).ok()?;
        // Sleep must count toward the maximum; a wall-clock rollback cannot
        // extend authorization beyond the original maximum either.
        if wall > MAX_AGE { return None; }
        Some(uptime.min(wall))
    }
    /// Caller serializes the whole native import/merge with cooperating writers.
    pub(super) fn unlock(&mut self, backend: &impl Backend, services: &[String], now: Instant, retry: bool) -> Result<(), String> {
        self.expire(now);
        if !retry {
            if self.document.is_some() { return Ok(()); }
            if let Some(error) = &self.failure { return Err(error.clone()); }
        }
        // Do not retain an earlier session while a new authorization is pending.
        self.document = None;
        self.vault_revision = None;
        self.expires = None;
        self.expires_wall = None;
        let result = self.load(backend, services);
        match result {
            Ok((document, revision)) => {
                self.document = Some(document);
                self.vault_revision = revision;
                self.expires = Some(Instant::now() + MAX_AGE);
                self.expires_wall = Some(SystemTime::now() + MAX_AGE);
                self.failure = None;
                Ok(())
            }
            Err(error) => {
                let error = if error.starts_with("credential_session_") || error.starts_with("credential_vault_") {
                    error
                } else { "credential_session_locked: Arca credentials could not be unlocked".into() };
                self.document = None;
                self.vault_revision = None;
                self.expires = None;
                self.expires_wall = None;
                self.failure = Some(error.clone());
                Err(error)
            }
        }
    }
    fn load(&self, backend: &impl Backend, services: &[String]) -> Result<(Document, Option<Revision>), String> {
        let before = backend.revision(VAULT_SERVICE)?;
        let mut document = match backend.read(VAULT_SERVICE, true)? {
            Some(bytes) => Document::decode(&Zeroizing::new(bytes))?,
            None if before.is_none() => Document::default(),
            None => return Err(changed()),
        };
        if backend.revision(VAULT_SERVICE)? != before { return Err(changed()); }
        let mut dirty = false;
        for service in services.iter().filter(|service| managed(service)) {
            let revision = backend.revision(service)?;
            match revision {
                None => { dirty |= document.entries.remove(service).is_some(); }
                Some(revision) if document.entries.get(service).is_some_and(|entry| entry.revision == revision) => {}
                Some(revision) => {
                    let value = Zeroizing::new(backend.read(service, true)?.ok_or_else(changed)?);
                    if backend.revision(service)? != Some(revision) { return Err(changed()); }
                    if value.len() > MAX_KEY_BYTES { return Err(invalid()); }
                    document.entries.insert(service.clone(), Entry { revision, secret: value.to_vec() });
                    dirty = true;
                }
            }
        }
        // Keep live entries absent from this catalog snapshot: another process
        // may be between saving its source key and committing its metadata.
        for service in document.entries.keys().cloned().collect::<Vec<_>>() {
            let revision = backend.revision(&service)?;
            if document.entries.get(&service).is_some_and(|entry| Some(entry.revision) != revision) {
                if revision.is_some() && services.contains(&service) { return Err(changed()); }
                document.entries.remove(&service);
                dirty = true;
            }
        }
        document.validate()?;
        if dirty || before.is_none() && !document.entries.is_empty() {
            let bytes = document.encode()?;
            if backend.revision(VAULT_SERVICE)? != before { return Err(changed()); }
            backend.write_vault(&bytes)?;
        }
        Ok((document, backend.revision(VAULT_SERVICE)?))
    }
    /// Ordinary navigation cannot trigger legacy-item authentication dialogs.
    pub(super) fn read(&mut self, backend: &impl Backend, service: &str, now: Instant) -> ReadResult {
        self.expire(now);
        let document = self.document.as_mut().ok_or_else(|| self.failure.clone().unwrap_or_else(locked))?;
        let revision = backend.revision(service)?;
        if revision.is_none() { document.entries.remove(service); return Ok(None); }
        if !document.entries.get(service).is_some_and(|entry| Some(entry.revision) == revision) {
            document.entries.remove(service);
            return Err(changed());
        }
        let entry = document.entries.get(service).ok_or_else(changed)?;
        Ok(Some(entry.secret.clone()))
    }
    /// Source writes remain authoritative. A failed derived-vault save leaves
    /// the source and this process's current value intact; the caller reports it.
    pub(super) fn synchronize(&mut self, backend: &impl Backend, service: &str, value: Option<&[u8]>, now: Instant) -> Result<(), String> {
        self.expire(now);
        let revision = backend.revision(service)?;
        if let Some(value) = value {
            let actual = Zeroizing::new(backend.read(service, false)?.ok_or_else(changed)?);
            if actual.as_slice() != value || backend.revision(service)? != revision { return Err(changed()); }
        }
        let next = match (revision, value) {
            (Some(revision), Some(value)) if value.len() <= MAX_KEY_BYTES => Some(Entry { revision, secret: value.to_vec() }),
            (None, None) => None,
            _ => return Err(changed()),
        };
        let Some(document) = self.document.as_mut() else { return Err(locked()); };
        match &next {
            Some(entry) => { document.entries.insert(service.into(), Entry { revision: entry.revision, secret: entry.secret.clone() }); }
            None => { document.entries.remove(service); }
        }
        let current_revision = backend.revision(VAULT_SERVICE)?;
        let mut merged = if current_revision == self.vault_revision {
            None
        } else {
            // Another process wrote: never overwrite it with this cached map.
            // A noninteractive read may fail; it must not open a second dialog.
            Some(Document::decode(&Zeroizing::new(backend.read(VAULT_SERVICE, false)?.ok_or_else(changed)?))?)
        };
        let candidate = if let Some(merged) = &mut merged {
            match next { Some(entry) => { merged.entries.insert(service.into(), entry); }, None => { merged.entries.remove(service); } }
            merged
        } else { document };
        let bytes = candidate.encode()?;
        if backend.revision(VAULT_SERVICE)? != current_revision || backend.revision(service)? != revision { return Err(changed()); }
        backend.write_vault(&bytes)?;
        self.vault_revision = backend.revision(VAULT_SERVICE)?;
        if let Some(merged) = merged { self.document = Some(merged); }
        Ok(())
    }
}

#[cfg(test)]
#[path = "credential_session_tests.rs"]
mod tests;
