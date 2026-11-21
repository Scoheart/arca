//! Private Passbolt revisions through Arca's official-SDK bridge.

use super::{
    is_revision, process, CreateFailure, CredentialReference, Store, MAX_KEY_BYTES, UNOWNED_REVISION,
};
use process::{Diagnostics, Failure};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
use url::{Host, Url};
use zeroize::Zeroizing;

const PROTOCOL: u8 = 1;
const DEFAULT_COMMAND: &str = "arca-passbolt";
const DEADLINE: Duration = Duration::from_secs(30);
const OUTPUT_LIMIT: usize = 32 * 1024;
const PASSWORD_LIMIT: usize = 4096;
const RESPONSE: &str = "credential_store_response: The Passbolt helper returned an invalid response.";

const DIAGNOSTICS: Diagnostics = Diagnostics {
    start: "credential_store_passbolt_start: Could not start the Arca Passbolt helper.",
    rejected: "credential_store_passbolt_failed: The Arca Passbolt helper exited unexpectedly.",
    io: "credential_store_passbolt_io: Could not safely exchange data with the Arca Passbolt helper.",
    timeout: "credential_store_passbolt_timeout: The Arca Passbolt helper did not finish in time.",
    output_limit: "credential_store_passbolt_output_limit: The Arca Passbolt helper exceeded its output limit.",
};

pub(super) struct Passbolt<'a> {
    pub(super) command: &'a str,
    pub(super) server_url: &'a str,
    pub(super) user_id: &'a str,
    pub(super) private_key_path: &'a str,
    pub(super) password_env: &'a str,
    pub(super) server_fingerprint: &'a str,
    pub(super) folder_id: Option<&'a str>,
}

#[derive(Serialize)]
struct Request<'a> {
    protocol: u8,
    operation: &'a str,
    server_url: &'a str,
    server_fingerprint: &'a str,
    user_id: &'a str,
    folder_id: &'a str,
    private_key_path: &'a str,
    password: &'a str,
    revision: &'a str,
    locator: &'a str,
    key: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply<'a> {
    protocol: u8,
    status: Status,
    locator: Option<&'a str>,
    secret: Option<&'a str>,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Ok,
    InvalidRequest,
    KeyFile,
    ServerKey,
    Authentication,
    PrivateFolder,
    UnsupportedFormat,
    RequestFailed,
    AlreadyExists,
    Ambiguous,
    Unowned,
    Missing,
    CreateUnconfirmed,
    RetirementUnconfirmed,
}

impl Status {
    fn error(self) -> &'static str {
        match self {
            Self::InvalidRequest => RESPONSE,
            Self::KeyFile => "credential_store_passbolt_key: Use an owner-only Passbolt private key file.",
            Self::ServerKey => "credential_store_passbolt_server: Verify HTTPS trust and the expected Passbolt server OpenPGP fingerprint.",
            Self::Authentication => "credential_store_passbolt_auth: Check the Passbolt account UUID, private key and passphrase.",
            Self::PrivateFolder => "credential_store_passbolt_folder: Use a Passbolt folder owned solely by the configured account.",
            Self::UnsupportedFormat => "credential_store_passbolt_format: This Passbolt server does not allow a supported personal password resource.",
            Self::RequestFailed => "credential_store_passbolt_request: The Passbolt request could not be completed.",
            Self::AlreadyExists => "credential_store_revision_exists: A Passbolt resource already claims this revision.",
            Self::Ambiguous => "credential_store_ambiguous: Multiple Passbolt resources match this revision.",
            Self::Unowned => UNOWNED_REVISION,
            Self::Missing => "credential_store_passbolt_missing: The Passbolt credential is missing or unavailable to the configured account.",
            Self::CreateUnconfirmed => "credential_store_passbolt_create: Passbolt did not confirm creation of the credential revision.",
            Self::RetirementUnconfirmed => "credential_store_passbolt_retire: Passbolt did not confirm retirement of the credential revision.",
            Self::Ok => RESPONSE,
        }
    }
}

pub(super) fn valid_server(value: &str) -> bool {
    server_url(value).is_some()
}

fn server_url(value: &str) -> Option<Url> {
    if !value.starts_with("https://") || value.len() > 2048 || value.contains('\\')
        || value.chars().any(|character| character.is_whitespace() || character.is_control())
    {
        return None;
    }
    let url = Url::parse(value).ok()?;
    (url.host().is_some() && url.username().is_empty() && url.password().is_none()
        && url.query().is_none() && url.fragment().is_none()).then_some(url)
}

pub(super) fn valid_fingerprint(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn decode_reply(output: &[u8]) -> Result<Reply<'_>, &'static str> {
    let reply: Reply<'_> = serde_json::from_slice(output).map_err(|_| RESPONSE)?;
    if reply.protocol != PROTOCOL || (reply.status != Status::Ok && (reply.secret.is_some() || reply.locator.is_some())) {
        return Err(RESPONSE);
    }
    Ok(reply)
}

impl Passbolt<'_> {
    fn run(
        &self, operation: &str, revision: &str, locator: &str, key: &str,
    ) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let url = server_url(self.server_url).ok_or(Failure::NotStarted(RESPONSE))?;
        let loopback = match url.host() {
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            _ => false,
        };
        if (cfg!(test) || crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some()) && !loopback {
            return Err(Failure::NotStarted(
                "credential_store_passbolt_isolated: Passbolt fixtures require a loopback HTTPS server.",
            ));
        }
        let executable = process::executable(self.command, DEFAULT_COMMAND)
            .as_deref().and_then(process::scoped_path).ok_or(Failure::NotStarted(
                "credential_store_passbolt_missing_helper: Install the Arca Passbolt helper or choose its absolute path.",
            ))?;
        let private_key = process::scoped_path(Path::new(self.private_key_path)).ok_or(Failure::NotStarted(
            "credential_store_passbolt_key: Use an owner-only Passbolt private key file.",
        ))?;
        let password_error = Failure::NotStarted(
            "credential_store_passbolt_passphrase: Set the configured Passbolt private key passphrase environment variable.",
        );
        let password = Zeroizing::new(std::env::var(self.password_env).map_err(|_| password_error)?);
        if password.is_empty() || password.len() > MAX_KEY_BYTES {
            return Err(password_error);
        }
        let request = Request {
            protocol: PROTOCOL,
            operation,
            user_id: self.user_id,
            server_url: url.as_str(),
            server_fingerprint: self.server_fingerprint,
            folder_id: self.folder_id.unwrap_or_default(),
            private_key_path: private_key.to_str().ok_or(Failure::NotStarted(RESPONSE))?,
            password: password.as_str(),
            revision,
            locator,
            key,
        };
        let mut payload = Zeroizing::new(Vec::with_capacity(6 * (password.len() + key.len()) + 8192));
        serde_json::to_writer(&mut *payload, &request).map_err(|_| Failure::NotStarted(RESPONSE))?;
        let command = process::command(&executable, self.password_env);
        process::run(command, Some(&payload), OUTPUT_LIMIT, DEADLINE, &DIAGNOSTICS)
    }

    fn check_status(&self, operation: &str, revision: &str, locator: &str) -> Result<(), String> {
        let output = self.run(operation, revision, locator, "").map_err(Failure::message)?;
        let reply = decode_reply(&output)?;
        if reply.status == Status::Ok && reply.locator.is_none() && reply.secret.is_none() {
            Ok(())
        } else {
            Err(reply.status.error().into())
        }
    }
}

impl Store for Passbolt<'_> {
    fn check(&self) -> Result<(), String> {
        self.check_status("check", "", "")
    }

    fn create(&self, revision: &str, _provider_id: &str, key: &str) -> Result<String, CreateFailure> {
        if key.chars().count() > PASSWORD_LIMIT {
            return Err(CreateFailure::NotStarted(
                "credential_store_invalid_key: Passbolt supports API keys up to 4096 characters.".into(),
            ));
        }
        let output = self.run("create", revision, "", key).map_err(|failure| match failure {
            Failure::NotStarted(message) => CreateFailure::NotStarted(message.into()),
            Failure::Started(message) => CreateFailure::Unconfirmed(message.into()),
        })?;
        let reply = decode_reply(&output).map_err(|error| CreateFailure::Unconfirmed(error.into()))?;
        match reply.status {
            Status::Ok => reply.locator.filter(|id| is_revision(id) && reply.secret.is_none())
                .map(str::to_owned).ok_or_else(|| CreateFailure::Unconfirmed(RESPONSE.into())),
            Status::CreateUnconfirmed => Err(CreateFailure::Unconfirmed(reply.status.error().into())),
            Status::Missing | Status::RetirementUnconfirmed => Err(CreateFailure::Unconfirmed(RESPONSE.into())),
            status => Err(CreateFailure::NotStarted(status.error().into())),
        }
    }

    fn recover(&self, revision: &str) -> Result<Option<String>, String> {
        let output = self.run("recover", revision, "", "").map_err(Failure::message)?;
        let reply = decode_reply(&output)?;
        if reply.status != Status::Ok {
            return Err(reply.status.error().into());
        }
        if reply.secret.is_some() || reply.locator.is_some_and(|id| !is_revision(id)) {
            return Err(RESPONSE.into());
        }
        Ok(reply.locator.map(str::to_owned))
    }

    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
        let output = self.run("read", &reference.revision, &reference.locator, "")
            .map_err(Failure::message)?;
        let reply = decode_reply(&output)?;
        if reply.status != Status::Ok {
            return Err(reply.status.error().into());
        }
        let secret = reply.secret.filter(|_| reply.locator.is_none()).ok_or(RESPONSE)?;
        super::super::decode_base64(secret).map(Zeroizing::new)
            .map_err(|_| RESPONSE.into())
    }

    fn read_for_observation(&self, reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        self.read(reference).map(|key| Some(key.to_vec()))
    }

    fn retire(&self, reference: &CredentialReference) -> Result<(), String> {
        self.check_status("retire", &reference.revision, &reference.locator)
    }
}
