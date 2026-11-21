//! Arca-owned, immutable Provider credential revisions.
mod bitwarden;
mod journal;
mod keepassxc;
mod one_password;
mod openbao;
mod passbolt;
mod process;

pub(crate) use crate::domain::credentials::{
    CredentialReference, CredentialStoreBackend, CredentialStoreConfig,
};
use journal::{Journal, State};
use std::path::Path;
use uuid::Uuid;
use zeroize::Zeroizing;

const KEYCHAIN_ID: &str = "keychain";
const KEYCHAIN_SERVICE_PREFIX: &str = "com.scoheart.arca.provider-credential.";
const MAX_KEY_BYTES: usize = 64 * 1024;
const UNCONFIRMED_CREATE: &str = "credential_store_create_unconfirmed: Credential creation is \
    unconfirmed. Retry the Provider save with the same API Key to check for the previous write.";
const UNOWNED_REVISION: &str = "credential_store_unowned: Arca could not verify ownership of this credential revision.";

trait Store {
    fn check(&self) -> Result<(), String>;
    fn preflight_create(&self, _revision: &str) -> Result<(), String> {
        Ok(())
    }
    fn create(&self, revision: &str, provider_id: &str, key: &str) -> Result<String, CreateFailure>;
    fn recover(&self, revision: &str) -> Result<Option<String>, String>;
    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String>;
    fn read_for_observation(&self, _reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
    fn retire(&self, reference: &CredentialReference) -> Result<(), String>;
}

enum CreateFailure {
    NotStarted(String),
    Unconfirmed(String),
}

pub(crate) fn keychain_store() -> CredentialStoreConfig {
    CredentialStoreConfig {
        id: KEYCHAIN_ID.into(),
        name: "macOS Keychain".into(),
        backend: CredentialStoreBackend::Keychain,
    }
}

pub(crate) fn validate(config: &CredentialStoreConfig) -> Result<(), String> {
    if !identifier(&config.id) || config.name.trim().is_empty()
        || config.name.len() > 256 || config.name.chars().any(char::is_control)
    {
        return Err("credential_store_invalid: Enter a valid credential store ID and name.".into());
    }
    if config.id == KEYCHAIN_ID && !matches!(&config.backend, CredentialStoreBackend::Keychain) {
        return Err("credential_store_invalid: The Keychain store ID is reserved.".into());
    }
    match &config.backend {
        CredentialStoreBackend::Keychain if config.id == KEYCHAIN_ID => Ok(()),
        CredentialStoreBackend::Keychain => {
            Err("credential_store_invalid: Use the built-in macOS Keychain store.".into())
        }
        CredentialStoreBackend::OnePassword { command, vault_id, token_env } => {
            if command != "op" && !absolute_path(command) {
                return Err("credential_store_invalid: Enter op or the absolute path to 1Password CLI.".into());
            }
            if !one_password::is_id(vault_id) {
                return Err("credential_store_invalid: Enter the 26-character 1Password vault ID.".into());
            }
            if !env_name(token_env) {
                return Err("credential_store_invalid: Enter the service account token environment variable name.".into());
            }
            Ok(())
        }
        CredentialStoreBackend::Bitwarden {
            command, server_url, appdata_dir, user_id, folder_id, session_env,
        } => {
            if command != "bw" && !absolute_path(command) {
                return Err("credential_store_invalid: Enter bw or the absolute path to Bitwarden CLI.".into());
            }
            if bitwarden::normalize_server(server_url).is_none() {
                return Err("credential_store_invalid: Use an HTTPS Bitwarden server URL.".into());
            }
            if !absolute_path(appdata_dir) {
                return Err("credential_store_invalid: Enter the absolute path to a dedicated Bitwarden CLI data directory.".into());
            }
            if !is_revision(user_id) || !is_revision(folder_id) {
                return Err("credential_store_invalid: Enter the Bitwarden account and personal folder UUIDs.".into());
            }
            if !env_name(session_env) {
                return Err("credential_store_invalid: Enter the environment variable name for the Bitwarden session.".into());
            }
            Ok(())
        }
        CredentialStoreBackend::KeePassXc { command, database_path, password_env, key_file } => {
            if command != "keepassxc-cli" && !absolute_path(command) {
                return Err("credential_store_invalid: Enter keepassxc-cli or its absolute path.".into());
            }
            if !absolute_path(database_path) || !key_file.as_deref().is_none_or(absolute_path) {
                return Err("credential_store_invalid: Enter absolute paths for the KeePassXC database and optional key file.".into());
            }
            if !env_name(password_env) {
                return Err("credential_store_invalid: Enter the environment variable name for the database password.".into());
            }
            Ok(())
        }
        CredentialStoreBackend::Passbolt {
            command, server_url, user_id, private_key_path, password_env, server_fingerprint, folder_id,
        } => {
            if command != "arca-passbolt" && !absolute_path(command) {
                return Err("credential_store_invalid: Enter arca-passbolt or the absolute path to the Passbolt helper.".into());
            }
            if !passbolt::valid_server(server_url) || !passbolt::valid_fingerprint(server_fingerprint) {
                return Err("credential_store_invalid: Enter the HTTPS Passbolt URL and server OpenPGP fingerprint.".into());
            }
            if !is_revision(user_id) || !folder_id.as_deref().is_none_or(is_revision) {
                return Err("credential_store_invalid: Enter the Passbolt account UUID and optional folder UUID.".into());
            }
            if !absolute_path(private_key_path) || !env_name(password_env) {
                return Err("credential_store_invalid: Enter the absolute Passbolt private key path and passphrase environment variable name.".into());
            }
            Ok(())
        }
        CredentialStoreBackend::OpenBao { server_url, mount, prefix, token_env } => {
            if openbao::normalize_server(server_url).is_none() {
                return Err("credential_store_invalid: Enter a valid OpenBao HTTPS server URL.".into());
            }
            if !openbao::valid_mount(mount) || !openbao::valid_prefix(prefix) {
                return Err("credential_store_invalid: Enter a KV v2 mount and a dedicated OpenBao path prefix.".into());
            }
            if !env_name(token_env) {
                return Err("credential_store_invalid: Enter the environment variable name for the OpenBao token.".into());
            }
            Ok(())
        }
    }
}

fn absolute_path(value: &str) -> bool {
    value.len() <= 4096 && !value.chars().any(char::is_control) && Path::new(value).is_absolute()
}

pub(crate) fn check(config: &CredentialStoreConfig) -> Result<(), String> {
    validate(config)?;
    backend(config).check()
}

pub(crate) fn create_revision(
    config: &CredentialStoreConfig,
    operation_id: &str,
    provider_id: &str,
    key: &[u8],
) -> Result<CredentialReference, String> {
    validate(config)?;
    if !is_revision(operation_id) || provider_id.is_empty() || provider_id.len() > 256
        || provider_id.chars().any(char::is_control)
    {
        return Err("credential_store_invalid: Invalid Provider or credential operation ID.".into());
    }
    let key_text = secret_text(key)?;
    let _provider_guard = journal::lock_provider(provider_id)?;
    let unresolved = journal::unconfirmed_revision(config, provider_id)?;
    let revision = unresolved.as_deref().unwrap_or(operation_id);
    let backend = backend(config);
    let mut journal = Journal::open(config, revision, Some(provider_id))?;
    let locator = match journal.state() {
        None | Some(State::Retryable) => {
            backend.preflight_create(revision)?;
            journal.record(State::Attempted)?;
            match backend.create(revision, provider_id, key_text) {
                Ok(locator) => locator,
                Err(CreateFailure::NotStarted(error)) => {
                    journal.record(State::Retryable)?;
                    return Err(error);
                }
                Err(CreateFailure::Unconfirmed(error)) => {
                    return Err(format!("{UNCONFIRMED_CREATE} {error}"));
                }
            }
        }
        // A missing item in a backend listing is never proof that an
        // interrupted or timed-out write did not reach the store.
        Some(State::Attempted) => backend.recover(revision)
            .map_err(|error| format!("{UNCONFIRMED_CREATE} {error}"))?
            .ok_or(UNCONFIRMED_CREATE.to_owned())?,
        Some(State::Created { locator } | State::Verified { locator }) => locator.clone(),
        Some(State::Retiring { .. } | State::Retired { .. }) => {
            return Err("credential_store_retired: This credential revision is retiring or retired.".into());
        }
    };
    let reference = CredentialReference {
        store_id: config.id.clone(),
        locator,
        revision: revision.into(),
    };
    validate_reference(config, &reference)?;
    if matches!(journal.state(), Some(State::Attempted)) {
        journal.record(State::Created { locator: reference.locator.clone() })?;
    }
    let stored = backend.read(&reference)?;
    secret_text(&stored)?;
    if stored.as_slice() != key {
        if revision != operation_id {
            // Keep an ownership receipt for the recovered item. A later save
            // can proceed and this unreferenced item can be retired explicitly.
            journal.record(State::Verified { locator: reference.locator.clone() })?;
        }
        return Err("credential_store_key_mismatch: The stored credential differs from the supplied key.".into());
    }
    if !matches!(journal.state(), Some(State::Verified { .. })) {
        journal.record(State::Verified { locator: reference.locator.clone() })?;
    }
    Ok(reference)
}

pub(crate) fn read(
    config: &CredentialStoreConfig, reference: &CredentialReference,
) -> Result<Zeroizing<Vec<u8>>, String> {
    validate_reference(config, reference)?;
    let value = backend(config).read(reference)?;
    secret_text(&value)?;
    Ok(value)
}

pub(crate) fn read_for_observation(
    config: &CredentialStoreConfig, reference: &CredentialReference,
) -> Result<Option<Vec<u8>>, String> {
    validate_reference(config, reference)?;
    backend(config).read_for_observation(reference)
}

pub(crate) fn retire(config: &CredentialStoreConfig, reference: &CredentialReference) -> Result<(), String> {
    validate_reference(config, reference)?;
    let mut journal = Journal::open(config, &reference.revision, None)?;
    match journal.state() {
        Some(State::Verified { locator }) if locator == &reference.locator => {
            journal.record(State::Retiring { locator: reference.locator.clone() })?;
        }
        Some(State::Retired { locator }) if locator == &reference.locator => return Ok(()),
        Some(State::Retiring { locator }) if locator == &reference.locator => {}
        _ => return Err(UNOWNED_REVISION.into()),
    }
    backend(config).retire(reference)?;
    journal.record(State::Retired { locator: reference.locator.clone() })
}

/// Includes retired revisions so a repeated retirement remains idempotent.
pub(crate) fn owned_revision(revision: &str) -> Result<Option<(String, CredentialReference)>, String> {
    journal::owned_revision(revision)
}

/// Lists verified and retiring revisions. Callers determine which are inactive
/// by comparing these references with the current settings.
pub(crate) fn list_owned_revisions() -> Result<Vec<(String, CredentialReference)>, String> {
    journal::list_owned_revisions()
}

/// Recheck the prepared reference while the caller holds the settings mutation lock.
pub(crate) fn verify_publishable(provider_id: &str, reference: &CredentialReference) -> Result<(), String> {
    journal::verify_publishable(provider_id, reference)
}

fn validate_reference(config: &CredentialStoreConfig, reference: &CredentialReference) -> Result<(), String> {
    validate(config)?;
    let valid_locator = match &config.backend {
        CredentialStoreBackend::Keychain | CredentialStoreBackend::OpenBao { .. } => reference.locator == reference.revision,
        CredentialStoreBackend::OnePassword { .. } => one_password::is_id(&reference.locator),
        CredentialStoreBackend::Bitwarden { .. }
        | CredentialStoreBackend::KeePassXc { .. }
        | CredentialStoreBackend::Passbolt { .. } => is_revision(&reference.locator),
    };
    if config.id != reference.store_id || !valid_locator || !is_revision(&reference.revision) {
        return Err("credential_store_invalid_reference: Invalid credential revision reference.".into());
    }
    Ok(())
}

fn backend(config: &CredentialStoreConfig) -> Box<dyn Store + '_> {
    match &config.backend {
        CredentialStoreBackend::Keychain => Box::new(Keychain),
        CredentialStoreBackend::OnePassword { command, vault_id, token_env } => Box::new(
            one_password::OnePassword { command, vault_id, token_env },
        ),
        CredentialStoreBackend::Bitwarden {
            command, server_url, appdata_dir, user_id, folder_id, session_env,
        } => Box::new(bitwarden::Bitwarden {
            command, server_url, appdata_dir, user_id, folder_id, session_env,
        }),
        CredentialStoreBackend::KeePassXc { command, database_path, password_env, key_file } => Box::new(
            keepassxc::KeePassXc { command, database_path, password_env, key_file: key_file.as_deref() },
        ),
        CredentialStoreBackend::Passbolt {
            command, server_url, user_id, private_key_path, password_env, server_fingerprint, folder_id,
        } => Box::new(passbolt::Passbolt {
            command, server_url, user_id, private_key_path, password_env, server_fingerprint,
            folder_id: folder_id.as_deref(),
        }),
        CredentialStoreBackend::OpenBao { server_url, mount, prefix, token_env } => Box::new(
            openbao::OpenBao { server_url, mount, prefix, token_env },
        ),
    }
}

fn is_revision(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == value)
}

fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn env_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 128 && bytes.next().is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn secret_text(key: &[u8]) -> Result<&str, String> {
    let error = || "credential_store_invalid_key: The credential must be nonempty UTF-8 on a single line.".to_owned();
    if key.is_empty() || key.len() > MAX_KEY_BYTES || key.iter().any(|byte| matches!(byte, b'\0' | b'\n' | b'\r')) {
        return Err(error());
    }
    std::str::from_utf8(key).map_err(|_| error())
}

struct Keychain;

fn keychain_service(revision: &str) -> String {
    format!("{KEYCHAIN_SERVICE_PREFIX}{revision}")
}

impl Store for Keychain {
    fn check(&self) -> Result<(), String> {
        #[cfg(not(target_os = "macos"))]
        {
            #[cfg(any(test, debug_assertions))]
            if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() {
                return Ok(());
            }
            Err("credential_store_unavailable: Keychain requires macOS.".into())
        }
        #[cfg(target_os = "macos")]
        super::credential_service_exists_checked(&keychain_service(&Uuid::nil().to_string())).map(|_| ())
    }

    fn preflight_create(&self, revision: &str) -> Result<(), String> {
        if self.recover(revision)?.is_some() {
            return Err("credential_store_revision_exists: This Keychain revision already exists.".into());
        }
        Ok(())
    }

    fn create(&self, revision: &str, _provider_id: &str, key: &str) -> Result<String, CreateFailure> {
        super::create_credential_service(&keychain_service(revision), key.as_bytes())
            .map_err(CreateFailure::NotStarted)?;
        Ok(revision.into())
    }

    fn recover(&self, revision: &str) -> Result<Option<String>, String> {
        Ok(super::credential_service_exists_checked(&keychain_service(revision))?
            .then(|| revision.to_owned()))
    }

    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
        super::read_credential_service_checked(&keychain_service(&reference.revision))?
            .map(Zeroizing::new)
            .ok_or_else(|| "credential_store_missing: The Keychain credential revision could not be found.".into())
    }

    fn read_for_observation(&self, reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        let service = keychain_service(&reference.revision);
        #[cfg(target_os = "macos")]
        return super::read_credential_service_with_authorization(&service, false);
        #[cfg(not(target_os = "macos"))]
        super::read_credential_service_checked(&service)
    }

    fn retire(&self, reference: &CredentialReference) -> Result<(), String> {
        super::delete_credential_service(&keychain_service(&reference.revision))
    }
}
