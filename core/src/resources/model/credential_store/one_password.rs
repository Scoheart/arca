//! 1Password Provider credentials managed through the official CLI.

use super::{process, CreateFailure, CredentialReference, Store, MAX_KEY_BYTES, UNOWNED_REVISION};
use process::{Diagnostics, Failure};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use zeroize::Zeroizing;

const TOKEN_ENV: &str = "OP_SERVICE_ACCOUNT_TOKEN";
const OWNED_TAG: &str = "arca-provider-credential";
const REVISION_TAG_PREFIX: &str = "arca-provider-revision-";
const CATEGORY: &str = "API_CREDENTIAL";
const FIELD: &str = "credential";
const DEADLINE: Duration = Duration::from_secs(15);
const MAX_METADATA_BYTES: usize = 512 * 1024;
const INVALID_METADATA: &str = "credential_store_response: 1Password CLI returned invalid item or vault metadata.";
const ARCHIVE_UNCONFIRMED: &str = "credential_store_archive_unconfirmed: Arca could not verify \
    the 1Password item state. Resume retirement to check it again.";
const OP_MISSING: Failure = Failure::NotStarted(
    "credential_store_op_missing: Install 1Password CLI or configure its absolute executable path.",
);
const TOKEN_MISSING: Failure = Failure::NotStarted(
    "credential_store_token_missing: Set the configured 1Password service account token environment variable.",
);

const DIAGNOSTICS: Diagnostics = Diagnostics {
    start: "credential_store_op_start: Could not start 1Password CLI. Check the executable path and permissions.",
    rejected: "credential_store_op_failed: 1Password CLI rejected the request. Check the service account token and vault access.",
    io: "credential_store_op_io: Could not safely exchange data with 1Password CLI.",
    timeout: "credential_store_op_timeout: 1Password CLI did not finish in time.",
    output_limit: "credential_store_op_output_limit: 1Password CLI exceeded its output limit.",
};

pub(super) struct OnePassword<'a> {
    pub(super) command: &'a str,
    pub(super) vault_id: &'a str,
    pub(super) token_env: &'a str,
}

#[derive(Deserialize)]
struct ItemMetadata {
    id: String,
    vault: VaultMetadata,
    category: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct VaultMetadata {
    id: String,
}

#[derive(Serialize)]
struct ItemTemplate<'a> {
    category: &'static str,
    fields: [FieldTemplate<'a>; 1],
}

#[derive(Serialize)]
struct FieldTemplate<'a> {
    id: &'static str,
    label: &'static str,
    #[serde(rename = "type")]
    kind: &'static str,
    value: &'a str,
}

impl OnePassword<'_> {
    fn run(
        &self, args: &[&str], input: Option<&[u8]>, limit: usize,
    ) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let path = process::executable(self.command, "op").ok_or(OP_MISSING)?;
        let executable = process::scoped_path(&path).ok_or(Failure::NotStarted(
            "credential_store_op_isolated: Credential tests require an isolated 1Password CLI fixture.",
        ))?;
        let token = Zeroizing::new(std::env::var(self.token_env).map_err(|_| TOKEN_MISSING)?);
        if token.trim().is_empty() || token.len() > MAX_KEY_BYTES {
            return Err(TOKEN_MISSING);
        }
        let mut command = process::command(&executable, self.token_env);
        command.args(args);
        command.env(TOKEN_ENV, token.as_str())
            .env("OP_BIOMETRIC_UNLOCK_ENABLED", "false")
            .env("OP_CACHE", "false");
        process::run(command, input, limit, DEADLINE, &DIAGNOSTICS)
    }

    fn revision_item(&self, revision: &str, include_archive: bool) -> Result<Option<String>, String> {
        let tag = revision_tag(revision);
        let mut args = vec!["item", "list", "--vault", self.vault_id, "--tags", &tag, "--format", "json"];
        if include_archive {
            args.push("--include-archive");
        }
        let output = self.run(&args, None, MAX_METADATA_BYTES).map_err(Failure::message)?;
        let items: Vec<ItemMetadata> = serde_json::from_slice(&output).map_err(|_| INVALID_METADATA)?;
        // The CLI tag filter includes nested tags. Require the exact tag in
        // the response, in the configured vault, with the Arca ownership tag.
        let mut matches = items.into_iter().filter(|item| item.tags.contains(&tag));
        let Some(item) = matches.next() else { return Ok(None); };
        if matches.next().is_some() {
            return Err("credential_store_ambiguous: More than one 1Password item claims this revision.".into());
        }
        if !self.owns(&item, &tag) {
            return Err(UNOWNED_REVISION.into());
        }
        Ok(Some(item.id))
    }

    fn owns(&self, item: &ItemMetadata, tag: &str) -> bool {
        is_id(&item.id) && item.vault.id == self.vault_id && item.category == CATEGORY
            && item.tags.iter().any(|value| value == OWNED_TAG)
            && item.tags.iter().any(|value| value == tag)
    }
}

impl Store for OnePassword<'_> {
    fn check(&self) -> Result<(), String> {
        let output = self.run(&["vault", "get", self.vault_id, "--format", "json"], None, MAX_METADATA_BYTES)
            .map_err(Failure::message)?;
        let vault: VaultMetadata = serde_json::from_slice(&output).map_err(|_| INVALID_METADATA)?;
        if vault.id != self.vault_id {
            return Err(INVALID_METADATA.into());
        }
        Ok(())
    }

    fn create(&self, revision: &str, provider_id: &str, _provider_name: &str, key: &str) -> Result<String, CreateFailure> {
        let template = ItemTemplate {
            category: CATEGORY,
            fields: [FieldTemplate { id: FIELD, label: FIELD, kind: "CONCEALED", value: key }],
        };
        let mut payload = Zeroizing::new(Vec::with_capacity(key.len() * 6 + 256));
        serde_json::to_writer(&mut *payload, &template).map_err(|_| {
            CreateFailure::NotStarted("credential_store_invalid_key: Could not encode the credential safely.".into())
        })?;
        let tag = revision_tag(revision);
        let tags = format!("{OWNED_TAG},{tag}");
        let title = format!("Arca · {provider_id}");
        let output = self.run(
            &["item", "create", "-", "--vault", self.vault_id, "--title", &title,
                "--tags", &tags, "--format", "json"],
            Some(&payload), MAX_METADATA_BYTES,
        ).map_err(|failure| match failure {
            Failure::NotStarted(message) => CreateFailure::NotStarted(message.into()),
            Failure::Started(message) => CreateFailure::Unconfirmed(message.into()),
        })?;
        let item: ItemMetadata = serde_json::from_slice(&output)
            .map_err(|_| CreateFailure::Unconfirmed(INVALID_METADATA.into()))?;
        if !self.owns(&item, &tag) {
            return Err(CreateFailure::Unconfirmed(INVALID_METADATA.into()));
        }
        Ok(item.id)
    }

    fn recover(&self, revision: &str) -> Result<Option<String>, String> {
        self.revision_item(revision, false)
    }

    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
        let secret_reference = format!("op://{}/{}/{FIELD}", self.vault_id, reference.locator);
        self.run(&["read", &secret_reference, "--no-newline"], None, MAX_KEY_BYTES)
            .map_err(|failure| failure.message().into())
    }

    fn read_for_observation(&self, reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        self.read(reference).map(|key| Some(key.to_vec()))
    }

    fn retire(&self, reference: &CredentialReference) -> Result<(), String> {
        let Some(item) = self.revision_item(&reference.revision, false)? else {
            // Recover a lost archive reply using the matching archived item.
            return match self.revision_item(&reference.revision, true)? {
                Some(item) if item == reference.locator => Ok(()),
                Some(_) => Err(UNOWNED_REVISION.into()),
                None => Err(ARCHIVE_UNCONFIRMED.into()),
            };
        };
        if item != reference.locator {
            return Err(UNOWNED_REVISION.into());
        }
        self.run(
            &["item", "delete", &reference.locator, "--archive", "--vault", self.vault_id],
            None, MAX_METADATA_BYTES,
        ).map_err(Failure::message)?;
        Ok(())
    }
}

pub(super) fn is_id(value: &str) -> bool {
    value.len() == 26 && value.bytes().all(|byte| matches!(byte, b'a'..=b'z' | b'2'..=b'7'))
}

fn revision_tag(revision: &str) -> String {
    format!("{REVISION_TAG_PREFIX}{revision}")
}
