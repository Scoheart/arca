//! NordPass Provider credentials managed through the official np CLI.
use super::{is_revision, process, CreateFailure, CredentialReference, Store, UNOWNED_REVISION};
use crate::safe_write::{acquire_settings_lock, SettingsLock};
use process::{Diagnostics, Failure};
use serde::{de::IgnoredAny, Deserialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

const PASSWORD: &str = "password";
const DEADLINE: Duration = Duration::from_secs(15);
const OUTPUT_LIMIT: usize = 512 * 1024;
const RESPONSE: &str = "credential_store_np_response: NordPass CLI returned an unexpected response.";
const FOLDER: &str = "credential_store_np_folder: Choose a personal NordPass folder with a unique name and no surrounding whitespace.";
const TITLE_CONFLICT: &str = "credential_store_ambiguous: A NordPass title conflicts with the credential ID.";
const DIAGNOSTICS: Diagnostics = Diagnostics {
    start: "credential_store_np_start: Could not start NordPass CLI.",
    rejected: "credential_store_np_failed: NordPass CLI rejected the request. Complete its first-run setup and sign in and unlock the configured account.",
    io: "credential_store_np_io: Could not exchange data with NordPass CLI.",
    timeout: "credential_store_np_timeout: NordPass CLI did not finish in time.",
    output_limit: "credential_store_np_output_limit: NordPass CLI exceeded its output limit.",
};

pub(super) struct NordPass<'a> {
    pub(super) command: &'a str,
    pub(super) account_email: &'a str,
    pub(super) folder_id: &'a str,
}

pub(super) fn valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else { return false };
    value.len() <= 254 && !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        && !value.chars().any(|character| character.is_whitespace() || character.is_control())
}

pub(super) fn is_id(value: &str) -> bool {
    is_revision(value)
}

#[derive(Deserialize)]
struct Folder {
    id: String,
    title: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct Summary {
    id: String,
    title: String,
    #[serde(rename = "type")]
    kind: String,
    folder: Option<String>,
    username: Option<String>,
}

impl Summary {
    fn owns(&self, folder: &str, marker: &str) -> bool {
        is_id(&self.id) && self.kind == PASSWORD
            && self.folder.as_deref() == Some(folder)
            && self.username.as_deref() == Some(marker)
    }
}

#[derive(Deserialize)]
struct CreatedItem {
    id: String,
    title: String,
    #[serde(rename = "type")]
    kind: String,
    folder: String,
    status: String,
}

#[derive(Deserialize)]
struct Item<'a> {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    username: Option<String>,
    #[serde(rename = "custom:arca_folder_id")]
    folder_id: Option<String>,
    #[serde(rename = "custom:arca_encoding")]
    encoding: Option<String>,
    #[serde(rename = "deletedAt")]
    deleted_at: Option<IgnoredAny>,
    #[serde(borrow)]
    password: Option<Password<'a>>,
}

impl Item<'_> {
    fn owns(&self, id: &str, folder_id: &str, marker: &str) -> bool {
        self.id == id && self.kind == PASSWORD && self.username.as_deref() == Some(marker)
            && self.folder_id.as_deref() == Some(folder_id)
    }

    fn key(&self) -> Result<Zeroizing<Vec<u8>>, String> {
        let password = self.password.as_ref()
            .ok_or("credential_store_np_field: This NordPass item has no Arca credential.")?;
        match self.encoding.as_deref() {
            None => Ok(Zeroizing::new(password.0.as_bytes().to_vec())),
            Some("base64") => super::super::decode_base64(&password.0)
                .map(Zeroizing::new)
                .map_err(|_| RESPONSE.into()),
            Some(_) => Err(RESPONSE.into()),
        }
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
struct Password<'a>(#[serde(borrow)] Cow<'a, str>);

impl Drop for Password<'_> {
    fn drop(&mut self) {
        if let Cow::Owned(value) = &mut self.0 {
            value.zeroize();
        }
    }
}

#[derive(Deserialize)]
struct TrashedItems {
    trashed: Vec<String>,
    failed: Vec<IgnoredAny>,
}

struct Session<'a> {
    _lock: SettingsLock,
    executable: PathBuf,
    account_email: &'a str,
    folder_id: &'a str,
}

impl NordPass<'_> {
    fn connect(&self) -> Result<Session<'_>, String> {
        let executable = match process::executable(self.command, "np") {
            None if self.command == "np" => dirs::home_dir()
                .and_then(|home| process::executable(home.join(".local/bin/np").to_str()?, "np")),
            executable => executable,
        }
        .and_then(|path| process::scoped_path(&path))
        .ok_or("credential_store_np_missing: Install NordPass CLI or configure its absolute executable path.")?;
        let account_hash = hex::encode(Sha256::digest(self.account_email.to_lowercase().as_bytes()));
        let lock_path = crate::paths::arca_dir().join("staging").join(format!("nordpass-{account_hash}"));
        let lock = acquire_settings_lock(&lock_path)
            .map_err(|_| "credential_store_np_busy: Another Arca operation is using this NordPass account.")?;
        Ok(Session { _lock: lock, executable, account_email: self.account_email, folder_id: self.folder_id })
    }
}

impl Session<'_> {
    fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let mut command = process::command(&self.executable, "");
        command.args(["--account", self.account_email])
            .args(args)
            .env("NORDPASS_CLI_NO_UPDATE_CHECK", "1");
        process::run(command, input, OUTPUT_LIMIT, DEADLINE, &DIAGNOSTICS)
    }

    fn sync(&self) -> Result<(), String> {
        self.run(&["sync"], None).map(|_| ()).map_err(|failure| failure.message().into())
    }

    fn folders(&self) -> Result<Vec<Folder>, String> {
        let output = self.run(&["folder", "list", "--format", "json"], None).map_err(Failure::message)?;
        serde_json::from_slice(&output).map_err(|_| RESPONSE.into())
    }

    fn folder_name<'a>(&self, folders: &'a [Folder]) -> Result<&'a str, String> {
        // np 1.0.0 accepts a folder name for item commands. Its listing can
        // select a shared folder with that name, so check both folder types.
        let mut matches = folders.iter().filter(|folder| folder.id == self.folder_id);
        let folder = matches.next().ok_or(FOLDER)?;
        if folder.kind != "personal" || matches.next().is_some() || folder.title.is_empty()
            || folder.title.contains('\0') || folder.title.trim_matches(cli_whitespace) != folder.title
        {
            return Err(FOLDER.into());
        }
        let folded_name = folder.title.to_lowercase();
        if folders.iter().any(|other| other.id != self.folder_id && other.title.to_lowercase() == folded_name) {
            return Err(FOLDER.into());
        }
        Ok(&folder.title)
    }

    fn list_items(&self, filters: &[&str]) -> Result<Vec<Summary>, String> {
        let mut args = vec!["item", "list", "--format", "json"];
        args.extend_from_slice(filters);
        let output = self.run(&args, None).map_err(Failure::message)?;
        serde_json::from_slice(&output).map_err(|_| RESPONSE.into())
    }

    fn find_revision(&self, folder: &str, marker: &str) -> Result<Option<String>, String> {
        // The CLI searches titles only. Password item listings include the
        // username, so recovery can find a revision without reading any keys.
        let items = self.list_items(&["--type", PASSWORD, "--folder", folder])?;
        let mut matches = items.into_iter().filter(|item| item.username.as_deref() == Some(marker));
        let Some(item) = matches.next() else { return Ok(None); };
        if matches.next().is_some() {
            return Err("credential_store_ambiguous: Multiple NordPass items claim this credential revision.".into());
        }
        if !item.owns(folder, marker) {
            return Err(UNOWNED_REVISION.into());
        }
        Ok(Some(item.id))
    }

    fn item(&self, id: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        self.run(&["item", "get", id, "--format", "json"], None)
            .map_err(|failure| failure.message().into())
    }

    fn item_trashed(&self, id: &str, marker: &str) -> Result<bool, String> {
        let output = self.item(id)?;
        let item: Item<'_> = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        if !item.owns(id, self.folder_id, marker) {
            return Err(UNOWNED_REVISION.into());
        }
        Ok(item.deleted_at.is_some())
    }
}

impl Store for NordPass<'_> {
    fn check(&self) -> Result<(), String> {
        let session = self.connect()?;
        session.sync()?;
        session.folder_name(&session.folders()?).map(|_| ())
    }

    fn create(&self, revision: &str, _provider_id: &str, provider_name: &str, key: &str) -> Result<String, CreateFailure> {
        if provider_name.is_empty() || provider_name.trim_matches(cli_whitespace) != provider_name
            || provider_name.contains('\0')
        {
            return Err(CreateFailure::NotStarted(
                "credential_store_np_title: Enter a NordPass item name without surrounding whitespace.".into(),
            ));
        }
        let session = self.connect().map_err(CreateFailure::NotStarted)?;
        session.sync().map_err(CreateFailure::NotStarted)?;
        let folders = session.folders().map_err(CreateFailure::NotStarted)?;
        let folder = session.folder_name(&folders).map_err(CreateFailure::NotStarted)?;
        let marker = revision_marker(revision);
        if session.find_revision(folder, &marker).map_err(CreateFailure::NotStarted)?.is_some() {
            return Err(CreateFailure::NotStarted(
                "credential_store_revision_exists: This NordPass credential revision already exists.".into(),
            ));
        }
        let folder_marker = format!("arca_folder_id={}", self.folder_id);
        let mut args = vec![
            "item", "create", "--type", PASSWORD, "--name", provider_name, "--folder", folder,
            "--username", &marker, "--text-field", &folder_marker, "--password", "-", "--format", "json",
        ];
        // np trims trailing whitespace from both piped and file-based secrets.
        let encoded = key.chars().next_back().filter(|character| cli_whitespace(*character))
            .map(|_| Zeroizing::new(super::super::encode_base64(key.as_bytes())));
        if encoded.is_some() {
            args.extend(["--text-field", "arca_encoding=base64"]);
        }
        let input = encoded.as_ref().map_or(key.as_bytes(), |value| value.as_bytes());
        let output = session.run(&args, Some(input)).map_err(|failure| match failure {
            Failure::NotStarted(message) => CreateFailure::NotStarted(message.into()),
            Failure::Started(message) => CreateFailure::Unconfirmed(message.into()),
        })?;
        let item: CreatedItem = serde_json::from_slice(&output)
            .map_err(|_| CreateFailure::Unconfirmed(RESPONSE.into()))?;
        if !is_id(&item.id) || item.title != provider_name || item.kind != PASSWORD
            || item.folder != folder || item.status != "created"
        {
            return Err(CreateFailure::Unconfirmed(RESPONSE.into()));
        }
        Ok(item.id)
    }

    fn recover(&self, revision: &str) -> Result<Option<String>, String> {
        let session = self.connect()?;
        session.sync()?;
        let folders = session.folders()?;
        let folder = session.folder_name(&folders)?;
        let marker = revision_marker(revision);
        let Some(id) = session.find_revision(folder, &marker)? else { return Ok(None); };
        if session.item_trashed(&id, &marker)? {
            return Err(UNOWNED_REVISION.into());
        }
        Ok(Some(id))
    }

    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
        let session = self.connect()?;
        let folders = session.folders()?;
        let folder = session.folder_name(&folders)?;
        let marker = revision_marker(&reference.revision);
        if session.find_revision(folder, &marker)?.as_deref() != Some(reference.locator.as_str()) {
            return Err(UNOWNED_REVISION.into());
        }
        let output = session.item(&reference.locator)?;
        let item: Item<'_> = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        if item.deleted_at.is_some() || !item.owns(&reference.locator, self.folder_id, &marker) {
            return Err(UNOWNED_REVISION.into());
        }
        item.key()
    }

    fn read_for_observation(&self, reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        self.read(reference).map(|key| Some(key.to_vec()))
    }

    fn retire(&self, reference: &CredentialReference) -> Result<(), String> {
        let session = self.connect()?;
        session.sync()?;
        let marker = revision_marker(&reference.revision);
        if session.item_trashed(&reference.locator, &marker)? {
            return Ok(());
        }
        let folders = session.folders()?;
        let folder = session.folder_name(&folders)?;
        if session.find_revision(folder, &marker)?.as_deref() != Some(reference.locator.as_str()) {
            return Err(UNOWNED_REVISION.into());
        }
        // np resolves handles by ID first and falls back to a title. Refuse
        // an exact title collision before requesting a soft delete.
        let collisions = session.list_items(&["--search", &reference.locator])?;
        if folders.iter().any(|folder| folder.title.eq_ignore_ascii_case(&reference.locator))
            || collisions.iter().any(|item| item.id != reference.locator
            && item.title.eq_ignore_ascii_case(&reference.locator))
        {
            return Err(TITLE_CONFLICT.into());
        }
        let output = session.run(&["item", "delete", &reference.locator, "--yes", "--format", "json"], None)
            .map_err(Failure::message)?;
        let result: TrashedItems = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        if result.trashed.as_slice() != [reference.locator.as_str()] || !result.failed.is_empty() {
            return Err(RESPONSE.into());
        }
        Ok(())
    }
}

fn revision_marker(revision: &str) -> String {
    format!("arca-provider-revision-{revision}")
}

fn cli_whitespace(character: char) -> bool {
    matches!(character, '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
