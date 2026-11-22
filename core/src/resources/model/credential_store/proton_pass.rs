//! A dedicated user profile and an exact Proton Pass vault share.
use super::{process, secret_text, CreateFailure, CredentialReference, Store, UNOWNED_REVISION};
use crate::safe_write::{acquire_settings_lock, SettingsLock};
use process::{Diagnostics, Failure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

const SESSION_DIR_ENV: &str = "PROTON_PASS_SESSION_DIR";
const REVISION_FIELD: &str = "arca_revision";
const KEY_FIELD: &str = "api_key";
const DEADLINE: Duration = Duration::from_secs(15);
const OUTPUT_LIMIT: usize = 512 * 1024;
const RESPONSE: &str = "credential_store_proton_pass_response: Proton Pass CLI returned an unexpected response.";
const LOCKED: &str = "credential_store_proton_pass_locked: Sign in to the configured Proton Pass CLI profile and unlock its session.";
const DIAGNOSTICS: Diagnostics = Diagnostics {
    start: "credential_store_proton_pass_start: Could not start Proton Pass CLI.",
    rejected: "credential_store_proton_pass_failed: Proton Pass CLI rejected the request. Check the CLI session and vault share.",
    io: "credential_store_proton_pass_io: Could not exchange data with Proton Pass CLI.",
    timeout: "credential_store_proton_pass_timeout: Proton Pass CLI did not finish in time.",
    output_limit: "credential_store_proton_pass_output_limit: Proton Pass CLI exceeded its output limit.",
};

pub(super) struct ProtonPass<'a> {
    pub(super) command: &'a str,
    pub(super) session_dir: &'a str,
    pub(super) user_id: &'a str,
    pub(super) share_id: &'a str,
}

// Proton exposes these IDs as opaque strings. Retain their case and any padding.
pub(super) fn is_id(value: &str) -> bool {
    let body = value.trim_end_matches('=');
    !body.is_empty() && value.len() <= 256 && value.len() - body.len() <= 2
        && body.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn supported_version(output: &[u8]) -> bool {
    std::str::from_utf8(output).ok()
        .and_then(|output| output.strip_prefix("Proton Pass CLI "))
        .and_then(|output| output.split_whitespace().next())
        .and_then(|version| semver::Version::parse(version).ok())
        .is_some_and(|version| version >= semver::Version::new(2, 6, 0))
}

fn revision_suffix(revision: &str) -> String {
    format!(" [Arca revision {revision}]")
}

#[derive(Deserialize)]
struct Info {
    id: String,
    env: Option<String>,
    personal_access_token_name: Option<String>,
}

#[derive(Deserialize)]
struct Shares {
    shares: Vec<Share>,
}

#[derive(Deserialize)]
struct Share {
    id: String,
    share_type: String,
    share_role: ShareRole,
}

#[derive(Deserialize)]
enum ShareRole {
    Owner,
    Manager,
    Editor,
    Viewer,
    Custom { permission: u16 },
}

impl ShareRole {
    fn writable(&self) -> bool {
        match self {
            Self::Owner | Self::Manager | Self::Editor => true,
            Self::Viewer => false,
            Self::Custom { permission } => {
                const ADMIN: u16 = 1;
                // Proton's CLI checks its Delete permission when trashing items.
                const READ_CREATE_DELETE: u16 = (1 << 1) | (1 << 2) | (1 << 5);
                permission & ADMIN == ADMIN || permission & READ_CREATE_DELETE == READ_CREATE_DELETE
            }
        }
    }
}

#[derive(Deserialize)]
struct Items {
    items: Vec<ItemSummary>,
}

#[derive(Deserialize)]
struct ItemSummary {
    id: String,
    share_id: String,
    title: String,
    item_type: String,
    state: ItemState,
}

#[derive(Deserialize, Eq, PartialEq)]
enum ItemState {
    Active,
    Trashed,
}

#[derive(Deserialize)]
struct ItemDetails<'a> {
    #[serde(borrow)]
    item: Item<'a>,
}

#[derive(Deserialize)]
struct Item<'a> {
    id: String,
    share_id: String,
    state: ItemState,
    #[serde(borrow)]
    content: ItemData<'a>,
}

#[derive(Deserialize)]
struct ItemData<'a> {
    #[serde(rename = "content")]
    _note: Note,
    #[serde(borrow)]
    extra_fields: Vec<Field<'a>>,
}

#[derive(Deserialize)]
enum Note {
    Note(()),
}

#[derive(Deserialize)]
struct Field<'a> {
    name: String,
    #[serde(borrow)]
    content: FieldValue<'a>,
}

#[derive(Deserialize)]
enum FieldValue<'a> {
    Text(#[serde(borrow)] FieldText<'a>),
    Hidden(#[serde(borrow)] FieldText<'a>),
    Totp(serde::de::IgnoredAny),
    Timestamp(serde::de::IgnoredAny),
}

#[derive(Deserialize)]
#[serde(transparent)]
struct FieldText<'a>(#[serde(borrow)] Cow<'a, str>);

impl Drop for FieldText<'_> {
    fn drop(&mut self) {
        if let Cow::Owned(value) = &mut self.0 {
            value.zeroize();
        }
    }
}

impl Item<'_> {
    fn field(&self, name: &str) -> Option<&FieldValue<'_>> {
        let mut fields = self.content.extra_fields.iter().filter(|field| field.name == name);
        let field = fields.next()?;
        fields.next().is_none().then_some(&field.content)
    }

    fn ensure_active(&self) -> Result<(), String> {
        if self.state == ItemState::Active {
            Ok(())
        } else {
            Err("credential_store_proton_pass_trashed: This credential revision is in Proton Pass Trash.".into())
        }
    }
}

#[derive(Serialize)]
struct NoteTemplate<'a> {
    title: &'a str,
    note: &'a str,
    extra_fields: [FieldTemplate<'a>; 2],
}

#[derive(Serialize)]
struct FieldTemplate<'a> {
    kind: &'static str,
    title: &'static str,
    value: &'a str,
}

struct Session<'a> {
    _lock: SettingsLock,
    command: PathBuf,
    directory: PathBuf,
    share_id: &'a str,
    share_arg: String,
    writable: bool,
}

impl ProtonPass<'_> {
    fn connect(&self) -> Result<Session<'_>, String> {
        let command = process::executable(self.command, "pass-cli")
            .and_then(|path| process::scoped_path(&path))
            .ok_or("credential_store_proton_pass_missing: Configure the Proton Pass CLI executable.")?;
        let directory = process::scoped_path(Path::new(self.session_dir))
            .filter(|path| path.is_dir())
            .ok_or("credential_store_proton_pass_profile: Configure a dedicated Proton Pass CLI session directory.")?;
        let profile = directory.join(".session");
        if !profile.symlink_metadata().is_ok_and(|metadata| metadata.is_dir())
            || !profile.join("session.json").symlink_metadata().is_ok_and(|metadata| metadata.is_file())
        {
            return Err(LOCKED.into());
        }

        let profile_hash = hex::encode(Sha256::digest(directory.as_os_str().as_encoded_bytes()));
        let lock_path = crate::paths::arca_dir().join("staging").join(format!("proton-pass-{profile_hash}"));
        let lock = acquire_settings_lock(&lock_path)
            .map_err(|_| "credential_store_proton_pass_busy: Another Arca operation is using this Proton Pass CLI profile.")?;
        let mut session = Session {
            _lock: lock,
            command,
            directory,
            share_id: self.share_id,
            share_arg: format!("--share-id={}", self.share_id),
            writable: false,
        };

        let version = session.run(&["--version"], None).map_err(Failure::message)?;
        if !supported_version(&version) {
            return Err("credential_store_proton_pass_version: Install Proton Pass CLI 2.6.0 or newer.".into());
        }
        let info = session.run(&["info", "--output", "json"], None).map_err(Failure::message)?;
        let info: Info = serde_json::from_slice(&info).map_err(|_| RESPONSE)?;
        if info.id != self.user_id || info.env.is_some() || info.personal_access_token_name.is_some() {
            return Err("credential_store_proton_pass_account: The CLI profile does not match the configured Proton Pass account.".into());
        }
        let shares = session.run(&["share", "list", "--only-vaults", "true", "--output", "json"], None)
            .map_err(Failure::message)?;
        let shares: Shares = serde_json::from_slice(&shares).map_err(|_| RESPONSE)?;
        let mut matches = shares.shares.iter().filter(|share| share.id == self.share_id);
        let share = matches.next().filter(|share| share.share_type == "Vault" && matches.next().is_none())
            .ok_or("credential_store_proton_pass_share: The configured Proton Pass vault share could not be confirmed.")?;
        session.writable = share.share_role.writable();
        Ok(session)
    }
}

impl Session<'_> {
    fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let mut command = process::command(&self.command, SESSION_DIR_ENV);
        command.args(args)
            .env(SESSION_DIR_ENV, &self.directory)
            .env("PROTON_PASS_KEY_PROVIDER", "keyring")
            .env("PROTON_PASS_NO_UPDATE_CHECK", "1")
            .env("PROTON_PASS_DISABLE_TELEMETRY", "1")
            .env("PASS_LOG_LEVEL", "off")
            .env("MUON_LOG_LEVEL", "off");
        process::run(command, input, OUTPUT_LIMIT, DEADLINE, &DIAGNOSTICS)
    }

    fn ensure_writable(&self) -> Result<(), String> {
        if self.writable {
            Ok(())
        } else {
            Err("credential_store_proton_pass_role: Select a Proton Pass vault share that permits creating and trashing items.".into())
        }
    }

    fn find(&self, revision: &str) -> Result<Option<String>, String> {
        // The default JSON list contains titles and IDs. Only open the exact
        // candidate after checking for duplicates, including items in Trash.
        let output = self.run(&["item", "list", &self.share_arg, "--output", "json"], None)
            .map_err(Failure::message)?;
        let list: Items = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        let suffix = revision_suffix(revision);
        let mut matches = list.items.into_iter().filter(|item| item.title.ends_with(&suffix));
        let Some(item) = matches.next() else { return Ok(None) };
        if matches.next().is_some() {
            return Err("credential_store_ambiguous: Multiple Proton Pass items match this revision.".into());
        }
        if item.share_id != self.share_id || item.item_type != "note" || item.state != ItemState::Active {
            return Err(UNOWNED_REVISION.into());
        }
        self.with_owned_item(&item.id, revision, |item| item.ensure_active())?;
        Ok(Some(item.id))
    }

    fn with_owned_item<T>(
        &self,
        locator: &str,
        revision: &str,
        inspect: impl FnOnce(&Item<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        if !is_id(locator) {
            return Err(RESPONSE.into());
        }
        let item_arg = format!("--item-id={locator}");
        let output = self.run(&["item", "view", &self.share_arg, &item_arg, "--output", "json"], None)
            .map_err(Failure::message)?;
        let details: ItemDetails<'_> = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        let item = &details.item;
        if item.id != locator || item.share_id != self.share_id
            || !matches!(item.field(REVISION_FIELD), Some(FieldValue::Text(marker)) if marker.0.as_ref() == revision)
        {
            return Err(UNOWNED_REVISION.into());
        }
        inspect(item)
    }
}

impl Store for ProtonPass<'_> {
    fn check(&self) -> Result<(), String> {
        self.connect()?.ensure_writable()
    }

    fn create(&self, revision: &str, provider_id: &str, provider_name: &str, key: &str) -> Result<String, CreateFailure> {
        let session = self.connect().map_err(CreateFailure::NotStarted)?;
        session.ensure_writable().map_err(CreateFailure::NotStarted)?;
        if session.find(revision).map_err(CreateFailure::NotStarted)?.is_some() {
            return Err(CreateFailure::NotStarted(
                "credential_store_revision_exists: This Proton Pass credential revision already exists.".into(),
            ));
        }

        let title = format!("{provider_name}{}", revision_suffix(revision));
        let note = format!("Arca Provider: {provider_id}");
        let template = NoteTemplate {
            title: &title,
            note: &note,
            extra_fields: [
                FieldTemplate { kind: "text", title: REVISION_FIELD, value: revision },
                FieldTemplate { kind: "hidden", title: KEY_FIELD, value: key },
            ],
        };
        let mut input = Zeroizing::new(Vec::with_capacity(6 * (key.len() + title.len() + note.len()) + 512));
        serde_json::to_writer(&mut *input, &template)
            .map_err(|_| CreateFailure::NotStarted(RESPONSE.into()))?;
        let output = session.run(&["item", "create", "note", &session.share_arg, "--from-template", "-"], Some(&input))
            .map_err(|failure| match failure {
                Failure::NotStarted(message) => CreateFailure::NotStarted(message.into()),
                Failure::Started(message) => CreateFailure::Unconfirmed(message.into()),
            })?;
        let locator = std::str::from_utf8(&output).ok()
            .map(|value| value.trim_end_matches(['\r', '\n']))
            .filter(|value| is_id(value))
            .ok_or_else(|| CreateFailure::Unconfirmed(RESPONSE.into()))?;
        session.with_owned_item(locator, revision, |item| item.ensure_active())
            .map_err(CreateFailure::Unconfirmed)?;
        Ok(locator.into())
    }

    fn recover(&self, revision: &str) -> Result<Option<String>, String> {
        self.connect()?.find(revision)
    }

    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
        self.connect()?.with_owned_item(&reference.locator, &reference.revision, |item| {
            item.ensure_active()?;
            if let Some(FieldValue::Hidden(key)) = item.field(KEY_FIELD) {
                secret_text(key.0.as_bytes())?;
                Ok(Zeroizing::new(key.0.as_bytes().to_vec()))
            } else {
                Err("credential_store_proton_pass_field: The Proton Pass item has no Arca credential field.".into())
            }
        })
    }

    fn read_for_observation(&self, reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        self.read(reference).map(|key| Some(key.to_vec()))
    }

    fn retire(&self, reference: &CredentialReference) -> Result<(), String> {
        let session = self.connect()?;
        let locator = &reference.locator;
        let revision = &reference.revision;
        let is_trashed = |item: &Item<'_>| Ok(item.state == ItemState::Trashed);
        if session.with_owned_item(locator, revision, is_trashed)? {
            return Ok(());
        }
        session.ensure_writable()?;
        let item_arg = format!("--item-id={locator}");
        session.run(&["item", "trash", &session.share_arg, &item_arg], None).map_err(Failure::message)?;
        if session.with_owned_item(locator, revision, is_trashed)? {
            Ok(())
        } else {
            Err("credential_store_proton_pass_trash: Moving the Proton Pass item to Trash is unconfirmed.".into())
        }
    }
}
