//! A dedicated Bitwarden CLI profile can address Bitwarden or Vaultwarden.
use super::{is_revision, process, CreateFailure, CredentialReference, Store, MAX_KEY_BYTES, UNOWNED_REVISION};
use crate::safe_write::{acquire_settings_lock, SettingsLock};
use process::{Diagnostics, Failure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::{Host, Url};
use zeroize::Zeroizing;

const SESSION_ENV: &str = "BW_SESSION";
const SECRET: &str = "api_key";
const REVISION: &str = "arca_revision";
const SECURE_NOTE: u8 = 2;
const TEXT: u8 = 0;
const HIDDEN: u8 = 1;
const DEADLINE: Duration = Duration::from_secs(15);
const OUTPUT_LIMIT: usize = 512 * 1024;
const RESPONSE: &str = "credential_store_bw_response: Bitwarden CLI returned an unexpected response.";
const DIAGNOSTICS: Diagnostics = Diagnostics {
    start: "credential_store_bw_start: Could not start Bitwarden CLI.",
    rejected: "credential_store_bw_failed: Bitwarden CLI rejected the request. Check the session, server and folder.",
    io: "credential_store_bw_io: Could not exchange data with Bitwarden CLI.",
    timeout: "credential_store_bw_timeout: Bitwarden CLI did not finish in time.",
    output_limit: "credential_store_bw_output_limit: Bitwarden CLI exceeded its output limit.",
};

pub(super) struct Bitwarden<'a> {
    pub(super) command: &'a str,
    pub(super) server_url: &'a str,
    pub(super) appdata_dir: &'a str,
    pub(super) user_id: &'a str,
    pub(super) folder_id: &'a str,
    pub(super) session_env: &'a str,
}

pub(super) fn normalize_server(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    if value.len() > 2048
        || value.chars().any(|character| character.is_whitespace() || character.is_control())
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.scheme() != "https"
    {
        return None;
    }
    Some(url.as_str().trim_end_matches('/').to_owned())
}

fn loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain("localhost")) => true,
        _ => false,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    server_url: Option<String>,
    user_id: Option<String>,
    status: String,
}

#[derive(Deserialize)]
struct Folder {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item<'a> {
    id: String,
    name: String,
    #[serde(rename = "type")]
    kind: u8,
    folder_id: Option<String>,
    organization_id: Option<String>,
    deleted_date: Option<String>,
    archived_date: Option<String>,
    #[serde(borrow, default)]
    fields: Vec<Field<'a>>,
}

impl Item<'_> {
    fn field(&self, name: &str, kind: u8) -> Option<&str> {
        let mut fields = self.fields.iter().filter(|field| field.name == name);
        let field = fields.next()?;
        (field.kind == kind && fields.next().is_none()).then_some(field.value.as_ref())
    }
}

#[derive(Deserialize)]
struct Field<'a> {
    name: String,
    #[serde(rename = "type")]
    kind: u8,
    #[serde(borrow)]
    value: Cow<'a, str>,
}

impl Drop for Field<'_> {
    fn drop(&mut self) {
        if let Cow::Owned(value) = &mut self.value {
            zeroize::Zeroize::zeroize(value);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ItemTemplate<'a> {
    name: &'a str,
    notes: &'a str,
    #[serde(rename = "type")]
    kind: u8,
    folder_id: &'a str,
    secure_note: SecureNote,
    fields: [FieldTemplate<'a>; 2],
}

#[derive(Serialize)]
struct SecureNote {
    #[serde(rename = "type")]
    kind: u8,
}

#[derive(Serialize)]
struct FieldTemplate<'a> {
    name: &'static str,
    #[serde(rename = "type")]
    kind: u8,
    value: &'a str,
}

struct Session<'a> {
    _lock: SettingsLock,
    command: PathBuf,
    appdata_dir: PathBuf,
    session_env: &'a str,
    session_key: Zeroizing<String>,
    folder_id: &'a str,
}

impl Bitwarden<'_> {
    fn connect(&self) -> Result<Session<'_>, String> {
        let executable = process::executable(self.command, "bw")
            .and_then(|path| process::scoped_path(&path))
            .ok_or("credential_store_bw_missing: Configure the Bitwarden CLI executable.")?;
        let appdata_dir = process::scoped_path(Path::new(self.appdata_dir))
            .filter(|path| path.is_dir())
            .ok_or("credential_store_bw_profile: Configure a dedicated Bitwarden CLI data directory.")?;
        let session_key = Zeroizing::new(std::env::var(self.session_env).unwrap_or_default());
        if session_key.trim().is_empty() || session_key.len() > MAX_KEY_BYTES {
            return Err("credential_store_bw_locked: Unlock the dedicated CLI profile and set its session environment variable.".into());
        }
        #[cfg(any(test, debug_assertions))]
        if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some()
            && !Url::parse(self.server_url).is_ok_and(|url| loopback(&url))
        {
            return Err("credential_store_bw_isolated: Bitwarden fixtures require a loopback server.".into());
        }

        let profile_hash = hex::encode(Sha256::digest(appdata_dir.as_os_str().as_encoded_bytes()));
        let lock_path = crate::paths::arca_dir().join("staging").join(format!("bitwarden-{profile_hash}"));
        let lock = acquire_settings_lock(&lock_path)
            .map_err(|_| "credential_store_bw_busy: Another Arca operation is using this Bitwarden CLI profile.")?;
        let session = Session {
            _lock: lock,
            command: executable,
            appdata_dir,
            session_env: self.session_env,
            session_key,
            folder_id: self.folder_id,
        };
        let output = session.run(&["status"], None).map_err(Failure::message)?;
        let status: Status = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        if status.user_id.as_deref() != Some(self.user_id)
            || status.server_url.as_deref().and_then(normalize_server) != normalize_server(self.server_url)
        {
            return Err("credential_store_bw_account: The CLI profile does not match the configured server and account.".into());
        }
        if status.status != "unlocked" {
            return Err("credential_store_bw_locked: Unlock the configured Bitwarden CLI profile.".into());
        }
        session.run(&["sync"], None).map_err(Failure::message)?;
        Ok(session)
    }
}

impl Session<'_> {
    fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let mut command = process::command(&self.command, self.session_env);
        command.arg("--nointeraction").args(args)
            .env("BITWARDENCLI_APPDATA_DIR", &self.appdata_dir)
            .env(SESSION_ENV, self.session_key.as_str());
        process::run(command, input, OUTPUT_LIMIT, DEADLINE, &DIAGNOSTICS)
    }

    fn check_folder(&self) -> Result<(), String> {
        let output = self.run(&["get", "folder", self.folder_id], None).map_err(Failure::message)?;
        let folder: Folder = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        if folder.id != self.folder_id {
            return Err("credential_store_bw_folder: The configured personal folder could not be confirmed.".into());
        }
        Ok(())
    }

    fn owns(&self, item: &Item<'_>, revision: &str, trash: bool) -> bool {
        is_revision(&item.id)
            && item.kind == SECURE_NOTE
            && item.folder_id.as_deref() == Some(self.folder_id)
            && item.organization_id.is_none()
            && item.deleted_date.is_some() == trash
            && item.archived_date.is_none()
            && item.name == title(revision)
            && item.field(REVISION, TEXT) == Some(revision)
    }

    fn find(&self, revision: &str, trash: bool) -> Result<Option<String>, String> {
        let mut args = vec!["list", "items", "--folderid", self.folder_id, "--search", revision];
        if trash {
            args.push("--trash");
        }
        let output = self.run(&args, None).map_err(Failure::message)?;
        let items: Vec<Item<'_>> = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        let name = title(revision);
        let mut matches = items.into_iter().filter(|item| item.name == name);
        let Some(item) = matches.next() else { return Ok(None) };
        if matches.next().is_some() {
            return Err("credential_store_ambiguous: Multiple Bitwarden items match this revision.".into());
        }
        if !self.owns(&item, revision, trash) {
            return Err(UNOWNED_REVISION.into());
        }
        Ok(Some(item.id))
    }
}

impl Store for Bitwarden<'_> {
    fn check(&self) -> Result<(), String> {
        self.connect()?.check_folder()
    }

    fn create(&self, revision: &str, provider_id: &str, key: &str) -> Result<String, CreateFailure> {
        let session = self.connect().map_err(CreateFailure::NotStarted)?;
        session.check_folder().map_err(CreateFailure::NotStarted)?;
        if session.find(revision, false).map_err(CreateFailure::NotStarted)?.is_some() {
            return Err(CreateFailure::NotStarted(
                "credential_store_revision_exists: This Bitwarden credential revision already exists.".into(),
            ));
        }
        let template = ItemTemplate {
            name: &title(revision),
            notes: &format!("Arca Provider: {provider_id}"),
            kind: SECURE_NOTE,
            folder_id: self.folder_id,
            secure_note: SecureNote { kind: 0 },
            fields: [
                FieldTemplate { name: REVISION, kind: TEXT, value: revision },
                FieldTemplate { name: SECRET, kind: HIDDEN, value: key },
            ],
        };
        let mut json = Zeroizing::new(Vec::with_capacity(key.len() * 6 + 512));
        serde_json::to_writer(&mut *json, &template)
            .map_err(|_| CreateFailure::NotStarted(RESPONSE.into()))?;
        let mut encoded = Zeroizing::new(super::super::encode_base64(&json));
        encoded.push('\n');
        let output = session.run(&["create", "item"], Some(encoded.as_bytes()))
            .map_err(|failure| match failure {
                Failure::NotStarted(message) => CreateFailure::NotStarted(message.into()),
                Failure::Started(message) => CreateFailure::Unconfirmed(message.into()),
            })?;
        let item: Item<'_> = serde_json::from_slice(&output)
            .map_err(|_| CreateFailure::Unconfirmed(RESPONSE.into()))?;
        if !session.owns(&item, revision, false) {
            return Err(CreateFailure::Unconfirmed(UNOWNED_REVISION.into()));
        }
        Ok(item.id)
    }

    fn recover(&self, revision: &str) -> Result<Option<String>, String> {
        self.connect()?.find(revision, false)
    }

    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
        let session = self.connect()?;
        let output = session.run(&["get", "item", &reference.locator], None).map_err(Failure::message)?;
        let item: Item<'_> = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        if item.id != reference.locator || !session.owns(&item, &reference.revision, false) {
            return Err(UNOWNED_REVISION.into());
        }
        item.field(SECRET, HIDDEN)
            .map(|key| Zeroizing::new(key.as_bytes().to_vec()))
            .ok_or_else(|| "credential_store_bw_field: The Bitwarden item has no Arca credential field.".into())
    }

    fn read_for_observation(&self, reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        self.read(reference).map(|key| Some(key.to_vec()))
    }

    fn retire(&self, reference: &CredentialReference) -> Result<(), String> {
        let session = self.connect()?;
        match session.find(&reference.revision, false)? {
            Some(id) if id == reference.locator => {
                session.run(&["delete", "item", &reference.locator], None).map_err(Failure::message)?;
                Ok(())
            }
            None if session.find(&reference.revision, true)?.as_deref() == Some(reference.locator.as_str()) => Ok(()),
            _ => Err(UNOWNED_REVISION.into()),
        }
    }
}

fn title(revision: &str) -> String {
    format!("Arca Provider {revision}")
}
