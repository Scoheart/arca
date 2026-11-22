//! Keep enough durable evidence to recover a backend write or retire an owned
//! revision. Neither the credential payload nor a digest of it is recorded.
use super::{
    bitwarden, identifier, is_revision, keeper, nordpass, one_password, openbao, passbolt, proton_pass, CredentialReference, CredentialStoreBackend,
    CredentialStoreConfig, KEYCHAIN_ID,
};
use crate::paths::arca_dir;
use crate::safe_write::{acquire_settings_lock, write_private_if_unchanged, SettingsLock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 4096;
const JOURNAL_ERROR: &str = "credential_store_journal: Arca could not verify or save the credential revision receipt.";
const BINDING_ERROR: &str = "credential_store_operation_mismatch: This credential operation is \
    bound to another Provider or store.";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    operation_id: String,
    store_id: String,
    backend: BackendIdentity,
    provider_id: String,
    state: State,
}

impl Record {
    fn decode(text: &str, revision: &str) -> Result<Self, String> {
        let record: Self = serde_json::from_str(text).map_err(|_| JOURNAL_ERROR.to_owned())?;
        let locator = match &record.state {
            State::Attempted | State::Retryable => None,
            State::Created { locator } | State::Verified { locator }
            | State::Retiring { locator } | State::Retired { locator } => Some(locator.as_str()),
        };
        let valid_backend = match &record.backend {
            BackendIdentity::Keychain => record.store_id == KEYCHAIN_ID && locator.is_none_or(|id| id == revision),
            BackendIdentity::OnePassword { vault_id } => record.store_id != KEYCHAIN_ID
                && one_password::is_id(vault_id) && locator.is_none_or(one_password::is_id),
            BackendIdentity::Bitwarden { server_url, user_id, folder_id } => record.store_id != KEYCHAIN_ID
                && bitwarden::normalize_server(server_url).is_some()
                && is_revision(user_id) && is_revision(folder_id) && locator.is_none_or(is_revision),
            BackendIdentity::Keeper { folder_uid, client_id } => record.store_id != KEYCHAIN_ID
                && keeper::is_uid(folder_uid) && keeper::is_client_id(client_id) && locator.is_none_or(keeper::is_uid),
            BackendIdentity::NordPass { account_email, folder_id } => record.store_id != KEYCHAIN_ID
                && nordpass::valid_email(account_email) && nordpass::is_id(folder_id) && locator.is_none_or(nordpass::is_id),
            BackendIdentity::ProtonPass { user_id, share_id } => record.store_id != KEYCHAIN_ID
                && proton_pass::is_id(user_id) && proton_pass::is_id(share_id) && locator.is_none_or(proton_pass::is_id),
            BackendIdentity::KeePassXc { database_path_digest } => record.store_id != KEYCHAIN_ID
                && database_path_digest.len() == 64 && database_path_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                && locator.is_none_or(is_revision),
            BackendIdentity::Passbolt { server_url, user_id, server_fingerprint, folder_id } => record.store_id != KEYCHAIN_ID
                && passbolt::valid_server(server_url) && passbolt::valid_fingerprint(server_fingerprint)
                && is_revision(user_id) && folder_id.as_deref().is_none_or(is_revision) && locator.is_none_or(is_revision),
            BackendIdentity::OpenBao { server_url, mount, prefix } => record.store_id != KEYCHAIN_ID
                && openbao::normalize_server(server_url).is_some()
                && openbao::valid_mount(mount) && openbao::valid_prefix(prefix) && locator.is_none_or(|id| id == revision),
        };
        if record.version != VERSION || record.operation_id != revision || !identifier(&record.store_id)
            || record.provider_id.is_empty() || record.provider_id.len() > 256
            || record.provider_id.chars().any(char::is_control) || !valid_backend
        {
            return Err(JOURNAL_ERROR.into());
        }
        Ok(record)
    }

    fn into_owned(self, include_retired: bool) -> Option<(String, CredentialReference)> {
        let locator = match self.state {
            State::Verified { locator } | State::Retiring { locator } => locator,
            State::Retired { locator } if include_retired => locator,
            _ => return None,
        };
        Some((self.provider_id, CredentialReference {
            store_id: self.store_id,
            locator,
            revision: self.operation_id,
        }))
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum BackendIdentity {
    Keychain,
    OnePassword { vault_id: String },
    Bitwarden { server_url: String, user_id: String, folder_id: String },
    Keeper { folder_uid: String, client_id: String },
    #[serde(rename = "nordpass")]
    NordPass { account_email: String, folder_id: String },
    ProtonPass { user_id: String, share_id: String },
    #[serde(rename = "keepassxc")]
    KeePassXc { database_path_digest: String },
    Passbolt { server_url: String, user_id: String, server_fingerprint: String, folder_id: Option<String> },
    #[serde(rename = "openbao")]
    OpenBao { server_url: String, mount: String, prefix: String },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(super) enum State {
    Attempted,
    Retryable,
    Created { locator: String },
    Verified { locator: String },
    Retiring { locator: String },
    Retired { locator: String },
}

pub(super) struct Journal {
    _lock: SettingsLock,
    path: PathBuf,
    record: Option<Record>,
    expected: Option<Zeroizing<String>>,
}

impl Journal {
    pub(super) fn open(
        config: &CredentialStoreConfig, operation_id: &str, provider_id: Option<&str>,
    ) -> Result<Self, String> {
        let path = revision_path(operation_id)?;
        let lock = acquire_settings_lock(&path).map_err(|_| JOURNAL_ERROR.to_owned())?;
        let backend = match &config.backend {
            CredentialStoreBackend::Keychain => BackendIdentity::Keychain,
            CredentialStoreBackend::OnePassword { vault_id, .. } => {
                BackendIdentity::OnePassword { vault_id: vault_id.clone() }
            }
            CredentialStoreBackend::Bitwarden { server_url, user_id, folder_id, .. } => {
                BackendIdentity::Bitwarden {
                    server_url: server_url.clone(),
                    user_id: user_id.clone(),
                    folder_id: folder_id.clone(),
                }
            }
            CredentialStoreBackend::Keeper { folder_uid, client_id, .. } => BackendIdentity::Keeper {
                folder_uid: folder_uid.clone(),
                client_id: client_id.clone(),
            },
            CredentialStoreBackend::NordPass { account_email, folder_id, .. } => BackendIdentity::NordPass {
                account_email: account_email.clone(),
                folder_id: folder_id.clone(),
            },
            CredentialStoreBackend::ProtonPass { user_id, share_id, .. } => BackendIdentity::ProtonPass {
                user_id: user_id.clone(),
                share_id: share_id.clone(),
            },
            CredentialStoreBackend::KeePassXc { database_path, .. } => BackendIdentity::KeePassXc {
                database_path_digest: hex::encode(Sha256::digest(database_path.as_bytes())),
            },
            CredentialStoreBackend::Passbolt { server_url, user_id, server_fingerprint, folder_id, .. } => {
                BackendIdentity::Passbolt {
                    server_url: server_url.clone(),
                    user_id: user_id.clone(),
                    server_fingerprint: server_fingerprint.clone(),
                    folder_id: folder_id.clone(),
                }
            }
            CredentialStoreBackend::OpenBao { server_url, mount, prefix, .. } => BackendIdentity::OpenBao {
                server_url: server_url.clone(),
                mount: mount.clone(),
                prefix: prefix.clone(),
            },
        };
        let expected = load(&path)?;
        let record = match expected.as_ref() {
            Some(bytes) => {
                let record = Record::decode(bytes, operation_id)?;
                if record.store_id != config.id || record.backend != backend
                    || provider_id.is_some_and(|id| record.provider_id != id)
                {
                    return Err(BINDING_ERROR.into());
                }
                Some(record)
            }
            None => provider_id.map(|id| Record {
                version: VERSION,
                operation_id: operation_id.into(),
                store_id: config.id.clone(),
                backend,
                provider_id: id.into(),
                state: State::Retryable,
            }),
        };
        Ok(Self { _lock: lock, path, record, expected })
    }

    pub(super) fn state(&self) -> Option<&State> {
        self.record.as_ref().map(|record| &record.state)
    }

    pub(super) fn record(&mut self, state: State) -> Result<(), String> {
        let record = self.record.as_mut().ok_or(JOURNAL_ERROR)?;
        record.state = state;
        let next = serde_json::to_string(record).map_err(|_| JOURNAL_ERROR.to_owned())?;
        write_private_if_unchanged(
            &self.path,
            self.expected.as_ref().map(|value| value.as_str()),
            &next,
        ).map_err(|_| JOURNAL_ERROR.to_owned())?;
        self.expected = Some(Zeroizing::new(next));
        Ok(())
    }
}

pub(super) fn owned_revision(revision: &str) -> Result<Option<(String, CredentialReference)>, String> {
    let record = read_record(&revision_path(revision)?, revision)?;
    Ok(record.and_then(|record| record.into_owned(true)))
}

pub(super) fn list_owned_revisions() -> Result<Vec<(String, CredentialReference)>, String> {
    let mut owned = records()?.into_iter().filter_map(|record| record.into_owned(false)).collect::<Vec<_>>();
    owned.sort_by(|(left_provider, left_ref), (right_provider, right_ref)| {
        (left_provider, &left_ref.revision).cmp(&(right_provider, &right_ref.revision))
    });
    Ok(owned)
}

pub(super) fn lock_provider(provider_id: &str) -> Result<SettingsLock, String> {
    let id = hex::encode(Sha256::digest(provider_id.as_bytes()));
    acquire_settings_lock(&journal_dir()?.join(format!("provider-{id}")))
        .map_err(|_| "credential_store_provider_busy: another operation is saving this Provider's credential".into())
}

pub(super) fn unconfirmed_revision(config: &CredentialStoreConfig, provider_id: &str) -> Result<Option<String>, String> {
    let mut unresolved = records()?.into_iter()
        .filter(|record| record.store_id == config.id && record.provider_id == provider_id
            && matches!(record.state, State::Attempted | State::Created { .. }))
        .map(|record| record.operation_id);
    let revision = unresolved.next();
    if unresolved.next().is_some() {
        return Err("credential_store_ambiguous: multiple credential writes for this Provider need reconciliation".into());
    }
    Ok(revision)
}

fn records() -> Result<Vec<Record>, String> {
    let directory = journal_dir()?;
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        _ => return Err(JOURNAL_ERROR.into()),
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(directory).map_err(|_| JOURNAL_ERROR.to_owned())? {
        let path = entry.map_err(|_| JOURNAL_ERROR.to_owned())?.path();
        let Some(revision) = path.file_name().and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json")).filter(|value| is_revision(value)) else {
            continue;
        };
        if let Some(record) = read_record(&path, revision)? {
            records.push(record);
        }
    }
    Ok(records)
}

pub(super) fn verify_publishable(provider_id: &str, reference: &CredentialReference) -> Result<(), String> {
    let path = revision_path(&reference.revision)?;
    match read_record(&path, &reference.revision)? {
        Some(Record { provider_id: owner, store_id, state: State::Verified { locator }, .. })
            if owner == provider_id && store_id == reference.store_id && locator == reference.locator => Ok(()),
        _ => Err("credential_store_unpublishable: The credential revision is no longer verified for this Provider.".into()),
    }
}

fn read_record(path: &Path, revision: &str) -> Result<Option<Record>, String> {
    load(path)?.map(|text| Record::decode(&text, revision)).transpose()
}

fn revision_path(revision: &str) -> Result<PathBuf, String> {
    if !is_revision(revision) {
        return Err("credential_store_invalid_reference: Invalid credential revision ID.".into());
    }
    Ok(journal_dir()?.join(format!("{revision}.json")))
}

fn journal_dir() -> Result<PathBuf, String> {
    let root = arca_dir();
    if !root.is_absolute() {
        return Err(JOURNAL_ERROR.into());
    }
    Ok(root.join("staging/provider-credentials"))
}

#[cfg(unix)]
fn load(path: &Path) -> Result<Option<Zeroizing<String>>, String> {
    use crate::safe_write::{capture_parent_directory, read_path_state_anchored_limited, AnchoredPathState};

    let parent = capture_parent_directory(path).map_err(|_| JOURNAL_ERROR.to_owned())?;
    match read_path_state_anchored_limited(path, &parent, MAX_RECORD_BYTES)
        .map_err(|_| JOURNAL_ERROR.to_owned())?
    {
        AnchoredPathState::Missing => Ok(None),
        AnchoredPathState::File { bytes, mode: Some(mode), .. } if mode & 0o077 == 0 => {
            let text = std::str::from_utf8(&bytes).map_err(|_| JOURNAL_ERROR.to_owned())?;
            Ok(Some(Zeroizing::new(text.to_owned())))
        }
        _ => Err(JOURNAL_ERROR.into()),
    }
}

#[cfg(not(unix))]
fn load(_path: &Path) -> Result<Option<Zeroizing<String>>, String> {
    Err("credential_store_unavailable: Credential revision storage requires Unix.".into())
}
