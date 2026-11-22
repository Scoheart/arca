use super::{is_revision, secret_text, CreateFailure, CredentialReference, Store, UNOWNED_REVISION};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use std::{collections::HashMap, io::Read, time::Duration};
use ureq::{
    http::{HeaderValue, Method, Request, Response, StatusCode},
    tls::{RootCerts, TlsConfig},
    Agent, AsSendBody, Body,
};
use url::{Host, Url};
use uuid::Uuid;
use zeroize::Zeroizing;

const VERSION: u64 = 1;
const MAX_RESPONSE_BYTES: u64 = 512 * 1024;
const RESPONSE: &str = "credential_store_openbao_response: OpenBao did not return a valid KV v2 response.";
const MISSING: &str = "credential_store_missing: The OpenBao credential revision could not be found.";
const EXISTS: &str = "credential_store_revision_exists: This OpenBao credential revision already exists.";
const AUTH: &str = "credential_store_openbao_auth: Check the OpenBao token environment variable and its permissions.";

pub(super) struct OpenBao<'a> {
    pub(super) server_url: &'a str,
    pub(super) mount: &'a str,
    pub(super) prefix: &'a str,
    pub(super) token_env: &'a str,
}

fn isolated() -> bool {
    cfg!(test) || crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some()
}

pub(super) fn normalize_server(value: &str) -> Option<String> {
    let (_, authority) = value.split_once("://")?;
    let authority = authority.strip_suffix('/').unwrap_or(authority);
    if value.len() > 2048 || value.chars().any(|c| c.is_control() || c.is_whitespace())
        || authority.contains(['/', '\\', '@', '?', '#'])
    {
        return None;
    }
    let url = Url::parse(value).ok()?;
    let loopback = match url.host()? {
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
        Host::Domain(_) => false,
    };
    let isolated = isolated();
    if isolated && !loopback {
        return None;
    }
    match url.scheme() {
        "https" => Some(url.origin().ascii_serialization()),
        "http" if isolated && cfg!(any(test, debug_assertions)) => {
            Some(url.origin().ascii_serialization())
        }
        _ => None,
    }
}

pub(super) fn valid_mount(value: &str) -> bool {
    value.len() <= 256 && valid_path(value)
}

pub(super) fn valid_prefix(value: &str) -> bool {
    value.len() <= 512 && valid_path(value)
}

fn valid_path(value: &str) -> bool {
    value.split('/').all(|part| {
        part.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
            && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    })
}

struct Session {
    agent: Agent,
    origin: String,
    token: HeaderValue,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct KvConfig {
    #[serde(rename = "cas_required")]
    _cas_required: bool,
    #[serde(rename = "max_versions")]
    _max_versions: u64,
}

#[derive(Deserialize)]
struct Metadata {
    version: u64,
    destroyed: bool,
    deletion_time: String,
}

#[derive(Deserialize)]
struct VersionedCredential {
    data: Option<Credential<Secret>>,
    metadata: Metadata,
}

#[derive(Deserialize, Serialize)]
struct Credential<Key> {
    arca_credential_owner: String,
    api_key: Key,
}

struct Secret(Zeroizing<String>);

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|key| Self(Zeroizing::new(key)))
    }
}

#[derive(Serialize)]
struct Cas {
    cas: u8,
}

#[derive(Serialize)]
struct CreateRequest<'a> {
    options: Cas,
    data: Credential<&'a str>,
}

enum Revision {
    Missing,
    Retired,
    Active(Zeroizing<Vec<u8>>),
}

impl OpenBao<'_> {
    fn session(&self) -> Result<Session, String> {
        let origin = normalize_server(self.server_url).ok_or(
            "credential_store_openbao_url: Use an HTTPS OpenBao server origin, or an isolated loopback fixture.",
        )?;
        if !valid_mount(self.mount) || !valid_prefix(self.prefix) {
            return Err("credential_store_openbao_path: Enter a valid OpenBao mount and prefix.".into());
        }
        let token = Zeroizing::new(std::env::var(self.token_env).map_err(|_| AUTH)?);
        if token.is_empty() || token.len() > 8192 || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(AUTH.into());
        }
        let mut header = HeaderValue::from_str(&token).map_err(|_| AUTH)?;
        header.set_sensitive(true);
        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .https_only(origin.starts_with("https://"))
            .max_redirects(0)
            .max_response_header_size(16 * 1024)
            .proxy(None)
            .timeout_connect(Some(Duration::from_secs(3)))
            .timeout_global(Some(Duration::from_secs(6)))
            .tls_config(TlsConfig::builder().root_certs(RootCerts::PlatformVerifier).build())
            .build()
            .new_agent();
        Ok(Session { agent, origin, token: header })
    }

    fn path(&self, operation: &str, revision: &str) -> String {
        format!("{}/{operation}/{}/{revision}", self.mount, self.prefix)
    }

    fn check_mount(&self, session: &Session) -> Result<(), String> {
        let response = session.send(Method::GET, &format!("{}/config", self.mount), ())?;
        expect_status(&response, StatusCode::OK)?;
        decode::<Envelope<KvConfig>>(response).map(|_| ())
    }

    fn lookup(&self, session: &Session, revision: &str, version: Option<u64>) -> Result<Revision, String> {
        if !is_revision(revision) {
            return Err(UNOWNED_REVISION.into());
        }
        let path = self.path("data", revision);
        let query = version.map_or_else(String::new, |version| format!("?version={version}"));
        let response = session.send(Method::GET, &format!("{path}{query}"), ())?;
        let missing = response.status() == StatusCode::NOT_FOUND;
        if !missing {
            expect_status(&response, StatusCode::OK)?;
        }
        let Envelope { data } = decode::<Envelope<Option<VersionedCredential>>>(response)?;
        let Some(data) = data else {
            return if missing { Ok(Revision::Missing) } else { Err(RESPONSE.into()) };
        };
        if data.metadata.version != VERSION {
            return Err(UNOWNED_REVISION.into());
        }
        if missing && data.data.is_none()
            && (data.metadata.destroyed || !data.metadata.deletion_time.is_empty())
        {
            return Ok(Revision::Retired);
        }
        let mut credential = data.data.ok_or(UNOWNED_REVISION)?;
        if missing || data.metadata.destroyed || credential.arca_credential_owner != session.owner(&path) {
            return Err(UNOWNED_REVISION.into());
        }
        secret_text(credential.api_key.0.as_bytes())?;
        let key = std::mem::take(&mut *credential.api_key.0);
        Ok(Revision::Active(Zeroizing::new(key.into_bytes())))
    }
}

impl Session {
    fn send(&self, method: Method, path: &str, body: impl AsSendBody) -> Result<Response<Body>, String> {
        let request = Request::builder()
            .method(method)
            .uri(format!("{}/v1/{path}", self.origin))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-Vault-Token", self.token.clone())
            .body(body)
            .map_err(|_| RESPONSE)?;
        self.agent.run(request).map_err(|error| match error {
            ureq::Error::Timeout(_) => "credential_store_openbao_timeout: OpenBao did not respond in time.".into(),
            _ => "credential_store_openbao_connection: Could not connect to OpenBao.".into(),
        })
    }

    fn owner(&self, path: &str) -> String {
        format!("com.scoheart.arca.provider-credential:v1:{}/v1/{path}", self.origin)
    }
}

impl Store for OpenBao<'_> {
    fn check(&self) -> Result<(), String> {
        let session = self.session()?;
        self.check_mount(&session)?;
        let probe = Uuid::nil().to_string();
        let data_path = self.path("data", &probe);
        let delete_path = self.path("delete", &probe);
        let request = serde_json::to_vec(&serde_json::json!({ "paths": [&data_path, &delete_path] }))
            .map_err(|_| RESPONSE)?;
        let response = session.send(Method::POST, "sys/capabilities-self", request.as_slice())?;
        expect_status(&response, StatusCode::OK)?;
        let answer: Envelope<HashMap<String, Vec<String>>> = decode(response)?;
        for (path, required) in [(&data_path, &["create", "read"][..]), (&delete_path, &["update"][..])] {
            let granted = answer.data.get(path).ok_or(RESPONSE)?;
            if !granted.iter().any(|permission| permission == "root")
                && !required.iter().all(|required| granted.iter().any(|permission| permission == required))
            {
                return Err(AUTH.into());
            }
        }
        Ok(())
    }

    fn preflight_create(&self, revision: &str) -> Result<(), String> {
        if !is_revision(revision) {
            return Err(UNOWNED_REVISION.into());
        }
        let session = self.session()?;
        let response = session.send(Method::GET, &self.path("data", revision), ())?;
        if response.status() == StatusCode::NOT_FOUND {
            let Envelope { data } = decode::<Envelope<Option<serde::de::IgnoredAny>>>(response)?;
            if data.is_none() {
                return Ok(());
            }
        } else {
            expect_status(&response, StatusCode::OK)?;
        }
        Err(EXISTS.into())
    }

    fn create(&self, revision: &str, _provider_id: &str, _provider_name: &str, key: &str) -> Result<String, CreateFailure> {
        let session = self.session().map_err(CreateFailure::NotStarted)?;
        if !is_revision(revision) {
            return Err(CreateFailure::NotStarted(UNOWNED_REVISION.into()));
        }
        secret_text(key.as_bytes()).map_err(CreateFailure::NotStarted)?;
        let path = self.path("data", revision);
        self.check_mount(&session).map_err(CreateFailure::NotStarted)?;
        let payload = CreateRequest {
            options: Cas { cas: 0 },
            data: Credential {
                arca_credential_owner: session.owner(&path),
                api_key: key,
            },
        };
        let mut encoded = Zeroizing::new(Vec::with_capacity(
            key.len() * 6 + payload.data.arca_credential_owner.len() + 128,
        ));
        serde_json::to_writer(&mut *encoded, &payload)
            .map_err(|_| CreateFailure::NotStarted(RESPONSE.into()))?;
        let response = session.send(Method::POST, &path, encoded.as_slice())
            .map_err(CreateFailure::Unconfirmed)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::BAD_REQUEST => return Err(CreateFailure::NotStarted(
                "credential_store_openbao_write_rejected: OpenBao rejected the create-only KV v2 write.".into(),
            )),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(CreateFailure::NotStarted(AUTH.into()));
            }
            status => return Err(CreateFailure::Unconfirmed(status_error(status))),
        }
        let answer: Envelope<Metadata> = decode(response).map_err(CreateFailure::Unconfirmed)?;
        if answer.data.version != VERSION || answer.data.destroyed {
            return Err(CreateFailure::Unconfirmed(UNOWNED_REVISION.into()));
        }
        Ok(revision.into())
    }

    fn recover(&self, revision: &str) -> Result<Option<String>, String> {
        match self.lookup(&self.session()?, revision, Some(VERSION))? {
            Revision::Active(_) => Ok(Some(revision.into())),
            Revision::Missing | Revision::Retired => Ok(None),
        }
    }

    fn read(&self, reference: &CredentialReference) -> Result<Zeroizing<Vec<u8>>, String> {
        if reference.locator != reference.revision {
            return Err(UNOWNED_REVISION.into());
        }
        match self.lookup(&self.session()?, &reference.revision, Some(VERSION))? {
            Revision::Active(key) => Ok(key),
            Revision::Missing | Revision::Retired => Err(MISSING.into()),
        }
    }

    fn read_for_observation(&self, reference: &CredentialReference) -> Result<Option<Vec<u8>>, String> {
        self.read(reference).map(|key| Some(key.to_vec()))
    }

    fn retire(&self, reference: &CredentialReference) -> Result<(), String> {
        if reference.locator != reference.revision {
            return Err(UNOWNED_REVISION.into());
        }
        let session = self.session()?;
        match self.lookup(&session, &reference.revision, None)? {
            Revision::Active(_) => {}
            Revision::Retired => return Ok(()),
            Revision::Missing => return Err(UNOWNED_REVISION.into()),
        }
        // A concurrent KV v2 write creates a new version. Deleting the exact
        // checked version preserves any such write even after the ownership read.
        let path = self.path("delete", &reference.revision);
        let body = format!(r#"{{"versions":[{VERSION}]}}"#);
        let response = session.send(Method::POST, &path, body.as_bytes())?;
        expect_status(&response, StatusCode::NO_CONTENT)?;
        match self.lookup(&session, &reference.revision, Some(VERSION))? {
            Revision::Retired => Ok(()),
            _ => Err("credential_store_openbao_retirement: OpenBao could not confirm retirement of the credential revision.".into()),
        }
    }
}

fn decode<T: DeserializeOwned>(response: Response<Body>) -> Result<T, String> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_RESPONSE_BYTES as usize));
    response.into_body().into_with_config().limit(MAX_RESPONSE_BYTES).reader()
        .read_to_end(&mut bytes).map_err(|_| RESPONSE)?;
    serde_json::from_slice(&bytes).map_err(|_| RESPONSE.into())
}

fn expect_status(response: &Response<Body>, expected: StatusCode) -> Result<(), String> {
    let status = response.status();
    if status == expected { Ok(()) } else { Err(status_error(status)) }
}

fn status_error(status: StatusCode) -> String {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => AUTH.into(),
        status => format!(
            "credential_store_openbao_http: OpenBao returned HTTP {}.",
            status.as_u16(),
        ),
    }
}
