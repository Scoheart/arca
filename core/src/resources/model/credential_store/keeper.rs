//! Keeper Secrets Manager application credentials through the official ksm CLI.
use super::{is_revision, process, secret_text, CreateFailure, CredentialReference, Store, UNOWNED_REVISION};
use base64::{engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD}, Engine};
use process::{Diagnostics, Failure};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::ser::{Formatter, Serializer};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;
use zeroize::Zeroizing;

const CONFIG_ENV: &str = "KSM_CONFIG";
const RECORD_TYPE: &str = "login";
const KEY_FIELD: &str = "password";
const REVISION_LABEL: &str = "Arca Credential Revision";
const MARKER_PREFIX: &str = "arca-provider-credential:v1:";
const DEADLINE: Duration = Duration::from_secs(15);
const MAX_CONFIG_BYTES: usize = 16 * 1024;
const OUTPUT_LIMIT: usize = 512 * 1024;
const AUTH: &str = "credential_store_ksm_config: Set the base64 configuration of the bound Keeper Secrets Manager client in the configured environment variable.";
const RESPONSE: &str = "credential_store_ksm_response: Keeper Secrets Manager CLI returned an unexpected response.";
const RETIRE_UNCONFIRMED: &str = "credential_store_ksm_retirement: Keeper could not confirm deletion of this exact credential record.";

const DIAGNOSTICS: Diagnostics = Diagnostics {
    start: "credential_store_ksm_start: Could not start Keeper Secrets Manager CLI.",
    rejected: "credential_store_ksm_failed: Keeper Secrets Manager CLI rejected the request. Check the client configuration and shared folder access.",
    io: "credential_store_ksm_io: Could not safely exchange data with Keeper Secrets Manager CLI.",
    timeout: "credential_store_ksm_timeout: Keeper Secrets Manager CLI did not finish in time.",
    output_limit: "credential_store_ksm_output_limit: Keeper Secrets Manager CLI exceeded its output limit.",
};

pub(super) struct Keeper<'a> {
    pub(super) command: &'a str,
    pub(super) folder_uid: &'a str,
    pub(super) client_id: &'a str,
    pub(super) config_env: &'a str,
}

pub(super) fn is_uid(value: &str) -> bool {
    let mut bytes = [0; 18];
    value.len() == 22 && URL_SAFE_NO_PAD.decode_slice(value, &mut bytes).is_ok_and(|len| {
        len == 16 && bytes[..len].iter().any(|byte| *byte != 0)
    })
}

pub(super) fn is_client_id(value: &str) -> bool {
    let mut bytes = [0; 66];
    value.len() == 88 && STANDARD.decode_slice(value, &mut bytes).is_ok_and(|len| {
        len == 64 && bytes[..len].iter().any(|byte| *byte != 0)
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientConfig<'a> {
    client_id: &'a str,
    hostname: &'a str,
    private_key: &'a str,
    app_key: &'a str,
    app_owner_public_key: &'a str,
}

#[derive(Deserialize)]
struct Folder {
    uid: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct RevisionMatch {
    uid: String,
    record_type: String,
    value: String,
}

#[derive(Deserialize)]
struct Record {
    uid: String,
    #[serde(rename = "type")]
    kind: String,
    fields: Vec<Field>,
    custom: Vec<Field>,
}

#[derive(Deserialize)]
struct Field {
    #[serde(rename = "type")]
    kind: String,
    label: Option<String>,
    value: Vec<FieldValue>,
}

impl Field {
    fn single_value(&self) -> Option<&str> {
        let [value] = self.value.as_slice() else { return None };
        Some(value.0.as_str())
    }
}

struct FieldValue(Zeroizing<String>);

impl<'de> Deserialize<'de> for FieldValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|value| Self(Zeroizing::new(value)))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteResult {
    uid: String,
    response_code: String,
    error: Option<FieldValue>,
}

#[derive(Serialize)]
struct RecordFile<'a> {
    version: &'static str,
    kind: &'static str,
    data: [RecordTemplate<'a>; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordTemplate<'a> {
    record_type: &'static str,
    title: &'a str,
    notes: &'a str,
    fields: [FieldTemplate<'a>; 1],
    custom_fields: [FieldTemplate<'a>; 1],
}

#[derive(Serialize)]
struct FieldTemplate<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'static str>,
    value: [&'a str; 1],
}

// ksm's file loader uses YAML for /dev/stdin. Generate YAML-compatible
// quoted scalars, preserving Unicode including code points YAML would fold.
struct PipeRecordFormatter;

impl Formatter for PipeRecordFormatter {
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self, writer: &mut W, fragment: &str,
    ) -> io::Result<()> {
        if fragment.bytes().all(|byte| byte < 127) {
            return writer.write_all(fragment.as_bytes());
        }
        for ch in fragment.chars() {
            let scalar = u32::from(ch);
            match scalar {
                0..=126 => writer.write_all(&[scalar as u8])?,
                127..=0xffff => write!(writer, "\\u{scalar:04x}")?,
                _ => write!(writer, "\\U{scalar:08x}")?,
            }
        }
        Ok(())
    }
}

struct Session<'a> {
    executable: PathBuf,
    config_env: &'a str,
    config: Zeroizing<String>,
    folder_uid: &'a str,
}

impl Keeper<'_> {
    fn connect(&self) -> Result<Session<'_>, String> {
        let executable = process::executable(self.command, "ksm")
            .and_then(|path| process::scoped_path(&path))
            .ok_or("credential_store_ksm_missing: Install Keeper Secrets Manager CLI or configure its absolute path.")?;
        if !is_uid(self.folder_uid) || !is_client_id(self.client_id) {
            return Err(AUTH.into());
        }
        let config = Zeroizing::new(std::env::var(self.config_env).map_err(|_| AUTH)?);
        let encoded = config.trim();
        if encoded.is_empty() || encoded.len() > MAX_CONFIG_BYTES {
            return Err(AUTH.into());
        }
        let mut decoded = Zeroizing::new(vec![0; encoded.len()]);
        let len = URL_SAFE.decode_slice(encoded, &mut decoded)
            .or_else(|_| STANDARD.decode_slice(encoded, &mut decoded))
            .map_err(|_| AUTH)?;
        let client: ClientConfig<'_> = serde_json::from_slice(&decoded[..len]).map_err(|_| AUTH)?;
        if client.client_id != self.client_id {
            return Err("credential_store_ksm_client: The Keeper configuration does not match the registered client ID.".into());
        }
        if [client.private_key, client.app_key, client.app_owner_public_key].into_iter()
            .any(|value| value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_graphic()))
        {
            return Err(AUTH.into());
        }
        if !matches!(
            client.hostname,
            "US" | "EU" | "AU" | "JP" | "CA" | "GOV" |
            "keepersecurity.com" | "keepersecurity.eu" | "keepersecurity.com.au" |
            "keepersecurity.jp" | "keepersecurity.ca" | "govcloud.keepersecurity.us"
        ) {
            return Err("credential_store_ksm_region: The configuration must use a Keeper Secrets Manager region supported by ksm.".into());
        }
        Ok(Session { executable, config_env: self.config_env, config, folder_uid: self.folder_uid })
    }
}

impl Session<'_> {
    fn run(&self, args: &[&str], input: Option<&[u8]>) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let mut command = process::command(&self.executable, self.config_env);
        command.args(["--no-cache", "--no-color", "--profile-name", "_default"])
            .args(args)
            .env(CONFIG_ENV, self.config.trim())
            .env("PYTHON_KEYRING_BACKEND", "keyring.backends.null.Keyring");
        process::run(command, input, OUTPUT_LIMIT, DEADLINE, &DIAGNOSTICS)
    }

    fn check_folder(&self) -> Result<(), String> {
        let folder = format!("--folder={}", self.folder_uid);
        let output = self.run(&["folder", "list", "--json", &folder], None).map_err(Failure::message)?;
        let folders: Vec<Folder> = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        match folders.as_slice() {
            [entry] if entry.kind == "dir" && entry.uid == self.folder_uid => Ok(()),
            _ => Err("credential_store_ksm_folder: The Keeper client cannot verify the configured folder UID.".into()),
        }
    }

    fn find(&self, revision: &str) -> Result<Option<String>, String> {
        if !is_revision(revision) {
            return Err(UNOWNED_REVISION.into());
        }
        let marker = format!("{MARKER_PREFIX}{revision}");
        let folder = format!("--folder={}", self.folder_uid);
        let query = format!("$.custom[?(@.type=='text' & @.label=='{REVISION_LABEL}' & @.value[0]=='{marker}')].value");
        let output = self.run(
            &["secret", "list", "--json", &folder, "--query", &query, "--show-value"], None,
        ).map_err(Failure::message)?;
        let records: Vec<RevisionMatch> = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        match records.as_slice() {
            [] => Ok(None),
            [record] if is_uid(&record.uid) && record.record_type == RECORD_TYPE && record.value == marker => {
                Ok(Some(record.uid.clone()))
            }
            [_] => Err(UNOWNED_REVISION.into()),
            _ => Err("credential_store_ambiguous: Multiple Keeper records match this credential revision.".into()),
        }
    }

    fn read_exact(&self, locator: &str, revision: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        let uid = format!("--uid={locator}");
        let output = self.run(
            &["secret", "get", "--json", "--force-array", "--deflate", &uid], None,
        ).map_err(Failure::message)?;
        let records: Vec<Record> = serde_json::from_slice(&output).map_err(|_| RESPONSE)?;
        let [record] = records.as_slice() else { return Err(UNOWNED_REVISION.into()) };
        if record.uid != locator || record.kind != RECORD_TYPE {
            return Err(UNOWNED_REVISION.into());
        }
        let marker = format!("{MARKER_PREFIX}{revision}");
        let mut ownership_fields = record.custom.iter().filter(|field| field.label.as_deref() == Some(REVISION_LABEL));
        let owner = ownership_fields.next().ok_or(UNOWNED_REVISION)?;
        if owner.kind != "text" || owner.single_value() != Some(&marker) || ownership_fields.next().is_some() {
            return Err(UNOWNED_REVISION.into());
        }
        let mut keys = record.fields.iter().filter(|field| field.kind == KEY_FIELD);
        let key = keys.next().and_then(Field::single_value).ok_or(UNOWNED_REVISION)?;
        if keys.next().is_some() {
            return Err(UNOWNED_REVISION.into());
        }
        secret_text(key.as_bytes())?;
        Ok(Zeroizing::new(key.as_bytes().to_vec()))
    }
}

impl Store for Keeper<'_> {
    fn check(&self) -> Result<(), String> {
        self.connect()?.check_folder()
    }

    fn create(&self, revision: &str, provider_id: &str, provider_name: &str, key: &str) -> Result<String, CreateFailure> {
        let session = self.connect().map_err(CreateFailure::NotStarted)?;
        session.check_folder().map_err(CreateFailure::NotStarted)?;
        if session.find(revision).map_err(CreateFailure::NotStarted)?.is_some() {
            return Err(CreateFailure::NotStarted(
                "credential_store_revision_exists: This Keeper credential revision already exists.".into(),
            ));
        }
        let title = if provider_name.trim().is_empty() { provider_id } else { provider_name };
        let notes = format!("Managed by Arca for Provider {provider_id}.");
        let marker = format!("{MARKER_PREFIX}{revision}");
        let template = RecordFile {
            version: "v3",
            kind: "KeeperRecord",
            data: [RecordTemplate {
                record_type: RECORD_TYPE,
                title,
                notes: &notes,
                fields: [FieldTemplate { kind: KEY_FIELD, label: None, value: [key] }],
                custom_fields: [FieldTemplate { kind: "text", label: Some(REVISION_LABEL), value: [&marker] }],
            }],
        };
        let mut payload = Zeroizing::new(Vec::with_capacity(key.len() * 6 + title.len() * 6 + 512));
        template.serialize(&mut Serializer::with_formatter(&mut *payload, PipeRecordFormatter))
            .map_err(|_| CreateFailure::NotStarted(RESPONSE.into()))?;
        let folder = format!("--sf={}", self.folder_uid);
        let output = session.run(
            &["secret", "add", "file", &folder, "--file=/dev/stdin"], Some(&payload),
        ).map_err(|failure| match failure {
            Failure::NotStarted(message) => CreateFailure::NotStarted(message.into()),
            Failure::Started(message) => CreateFailure::Unconfirmed(message.into()),
        })?;
        let uids: Vec<String> = serde_json::from_slice(&output)
            .map_err(|_| CreateFailure::Unconfirmed(RESPONSE.into()))?;
        match uids.as_slice() {
            [uid] if is_uid(uid) => Ok(uid.clone()),
            _ => Err(CreateFailure::Unconfirmed(RESPONSE.into())),
        }
    }

    fn recover(&self, revision: &str) -> Result<Option<String>, String> {
        let session = self.connect()?;
        let Some(locator) = session.find(revision)? else { return Ok(None) };
        session.read_exact(&locator, revision)?;
        Ok(Some(locator))
    }

    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
        let session = self.connect()?;
        let locator = session.find(&reference.revision)?;
        if locator.as_deref() != Some(&reference.locator) {
            return Err(UNOWNED_REVISION.into());
        }
        session.read_exact(&reference.locator, &reference.revision)
    }

    fn read_for_observation(&self, reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        self.read(reference).map(|key| Some(key.to_vec()))
    }

    fn retire(&self, reference: &CredentialReference) -> Result<(), String> {
        let session = self.connect()?;
        match session.find(&reference.revision)?.as_deref() {
            Some(uid) if uid == reference.locator => {}
            Some(_) => return Err(UNOWNED_REVISION.into()),
            // An absent or inaccessible record does not prove a prior delete succeeded.
            None => return Err(RETIRE_UNCONFIRMED.into()),
        }
        session.read_exact(&reference.locator, &reference.revision)?;
        let uid = format!("--uid={}", reference.locator);
        let output = session.run(&["secret", "delete", "--json", &uid], None).map_err(Failure::message)?;
        let results: Vec<DeleteResult> = serde_json::from_slice(&output).map_err(|_| RETIRE_UNCONFIRMED)?;
        match results.as_slice() {
            [result] if result.uid == reference.locator && result.response_code == "ok"
                && result.error.as_ref().is_none_or(|error| error.0.is_empty()) => Ok(()),
            _ => Err(RETIRE_UNCONFIRMED.into()),
        }
    }
}
