use crate::domain::assets::ApiKeyDelivery;
use crate::domain::types::ApiKeySource;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const MAX_CREDENTIAL_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialEvidence {
    pub source_kind: String,
    pub source_identity: String,
    pub secret_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialValidationView {
    pub source_kind: String,
    pub source_identity: String,
    pub message: String,
}

pub struct ResolvedCredential {
    bytes: Zeroizing<Vec<u8>>,
    evidence: CredentialEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PreparedCredentialRoute {
    NativeEnvReference,
    NativeFileReference,
    NativeHelper,
    ArcaKeychainHelper,
    OpenCodeAuthStore,
    ClaudeDesktopProfile,
    Plaintext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentCredentialCapabilities {
    #[serde(default)]
    pub native_sources: Vec<String>,
    pub arca_keychain_helper: bool,
    pub agent_store: bool,
    pub plaintext: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub fn agent_capabilities(agent_id: &str) -> AgentCredentialCapabilities {
    let mut capabilities = AgentCredentialCapabilities {
        native_sources: Vec::new(),
        arca_keychain_helper: false,
        agent_store: false,
        plaintext: false,
        note: None,
    };
    match agent_id {
        "claude-code" | "codex" => {
            capabilities.native_sources.push("helper".into());
            capabilities.arca_keychain_helper = true;
        }
        "pi" => {
            capabilities.native_sources.push("helper".into());
            capabilities.arca_keychain_helper = true;
            capabilities.plaintext = true;
        }
        "claude-desktop" => capabilities.agent_store = true,
        "zcode" => {
            capabilities.plaintext = true;
            capabilities.note = Some("ZCode requires explicit plaintext delivery to its private native config; central credentials remain in Keychain".into());
        }
        "opencode" | "opencode-desktop" => {
            capabilities.native_sources = vec!["env".into(), "file".into()];
            capabilities.agent_store = true;
            capabilities.plaintext = true;
        }
        "kilo-desktop" => {
            capabilities.native_sources = vec!["env".into(), "file".into()];
            capabilities.note = Some("Kilo Desktop supports native environment/file references; Keychain credentials are not exported to its config".into());
        }
        "kilo-code" | "qoder-desktop" => {
            capabilities.native_sources = vec!["env".into()];
            if agent_id == "kilo-code" {
                capabilities.native_sources.push("file".into());
            }
            capabilities.plaintext = true;
        }
        "qoder-cli" | "qwen-code" | "grok-build" | "crush" | "hermes" | "factory-droid" => {
            capabilities.native_sources.push("env".into());
        }
        "mistral-vibe" | "goose" => capabilities.native_sources.push("env".into()),
        "qoder" | "minimax-code" | "kimi-code" | "kimi-code-desktop" => {
            capabilities.note = Some("credential delivery requires guided Agent setup".into());
        }
        _ => {
            capabilities.note =
                Some("credential delivery has not been verified for this Agent".into());
        }
    }
    capabilities
}

pub fn available_deliveries(agent_id: &str) -> Vec<ApiKeyDelivery> {
    let capabilities = agent_capabilities(agent_id);
    let mut deliveries = vec![ApiKeyDelivery::Auto];
    if capabilities.native_sources.iter().any(|kind| kind == "env") {
        deliveries.push(ApiKeyDelivery::Env);
    }
    if capabilities.arca_keychain_helper
        || capabilities
            .native_sources
            .iter()
            .any(|kind| kind == "helper")
    {
        deliveries.push(ApiKeyDelivery::Command);
    }
    if capabilities.agent_store {
        deliveries.push(ApiKeyDelivery::AgentStore);
    }
    if capabilities.plaintext {
        deliveries.push(ApiKeyDelivery::Plaintext);
    }
    deliveries
}

/// Map a stored Agent default onto a delivery that Agent can actually write.
/// Unset settings are plaintext; Agents without a plaintext writer fall back
/// to Auto so Grok-style env-only Agents keep working.
pub fn resolve_delivery(agent_id: &str, requested: &ApiKeyDelivery) -> ApiKeyDelivery {
    let available = available_deliveries(agent_id);
    // A stale/unsupported ZCode policy must not silently become plaintext.
    if agent_id == "zcode" { return requested.clone(); }
    if available.iter().any(|delivery| delivery == requested) {
        return requested.clone();
    }
    if available
        .iter()
        .any(|delivery| *delivery == ApiKeyDelivery::Plaintext)
    {
        return ApiKeyDelivery::Plaintext;
    }
    ApiKeyDelivery::Auto
}

pub fn select_delivery(
    agent_id: &str,
    source: &ApiKeySource,
    delivery: &ApiKeyDelivery,
) -> Result<PreparedCredentialRoute, String> {
    let capabilities = agent_capabilities(agent_id);
    match delivery {
        ApiKeyDelivery::AgentStore if capabilities.agent_store => match agent_id {
            "opencode" | "opencode-desktop" => Ok(PreparedCredentialRoute::OpenCodeAuthStore),
            "claude-desktop" => Ok(PreparedCredentialRoute::ClaudeDesktopProfile),
            _ => Err(unsupported_delivery(agent_id, "agent-store")),
        },
        ApiKeyDelivery::AgentStore => Err(unsupported_delivery(agent_id, "agent-store")),
        ApiKeyDelivery::Plaintext if capabilities.plaintext => {
            Ok(PreparedCredentialRoute::Plaintext)
        }
        ApiKeyDelivery::Plaintext => Err(unsupported_delivery(agent_id, "plaintext")),
        ApiKeyDelivery::Env if capabilities.native_sources.iter().any(|kind| kind == "env") => {
            Ok(PreparedCredentialRoute::NativeEnvReference)
        }
        ApiKeyDelivery::Env => Err(unsupported_delivery(agent_id, "env")),
        ApiKeyDelivery::Command if capabilities.arca_keychain_helper => {
            Ok(PreparedCredentialRoute::ArcaKeychainHelper)
        }
        ApiKeyDelivery::Command
            if capabilities
                .native_sources
                .iter()
                .any(|kind| kind == "helper") =>
        {
            Ok(PreparedCredentialRoute::NativeHelper)
        }
        ApiKeyDelivery::Command => Err(unsupported_delivery(agent_id, "command")),
        ApiKeyDelivery::Auto => {
            let source_kind = match source {
                ApiKeySource::ArcaStore => "arca-store",
                ApiKeySource::Env { .. } => "env",
                ApiKeySource::File { .. } => "file",
                ApiKeySource::Helper { .. } => "helper",
            };
            if capabilities
                .native_sources
                .iter()
                .any(|kind| kind == source_kind)
            {
                return Ok(match source_kind {
                    "env" => PreparedCredentialRoute::NativeEnvReference,
                    "file" => PreparedCredentialRoute::NativeFileReference,
                    "helper" => PreparedCredentialRoute::NativeHelper,
                    _ => unreachable!(),
                });
            }
            if matches!(source, ApiKeySource::ArcaStore) && capabilities.arca_keychain_helper {
                return Ok(PreparedCredentialRoute::ArcaKeychainHelper);
            }
            if capabilities.agent_store {
                return match agent_id {
                    "opencode" | "opencode-desktop" => Ok(PreparedCredentialRoute::OpenCodeAuthStore),
                    "claude-desktop" => Ok(PreparedCredentialRoute::ClaudeDesktopProfile),
                    _ => Err(unsupported_delivery(agent_id, "auto")),
                };
            }
            Err(unsupported_delivery(agent_id, "auto"))
        }
    }
}

fn unsupported_delivery(agent_id: &str, delivery: &str) -> String {
    format!(
        "credential_delivery_unsupported: {agent_id} does not support {delivery} for this credential source"
    )
}

impl fmt::Debug for ResolvedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedCredential")
            .field("evidence", &self.evidence)
            .finish_non_exhaustive()
    }
}

impl ResolvedCredential {
    pub fn expose_for_delivery(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub fn evidence(&self) -> &CredentialEvidence {
        &self.evidence
    }
}

fn resolved_bytes(
    bytes: Vec<u8>,
    source_kind: &str,
    source_identity: String,
) -> Result<ResolvedCredential, String> {
    let bytes = Zeroizing::new(bytes);
    if bytes.is_empty() || bytes.contains(&b'\n') || bytes.contains(&b'\r') || bytes.contains(&0) {
        return Err(format!(
            "credential_{source_kind}_invalid_output: credential must contain exactly one non-empty line"
        ));
    }
    let digest = hex::encode(Sha256::digest(bytes.as_slice()));
    Ok(ResolvedCredential {
        bytes,
        evidence: CredentialEvidence {
            source_kind: source_kind.into(),
            source_identity,
            secret_sha256: digest,
        },
    })
}

pub fn resolve_source(
    source: &ApiKeySource,
    arca_store_value: Option<Vec<u8>>,
) -> Result<ResolvedCredential, String> {
    validate_source(source)?;
    match source {
        ApiKeySource::ArcaStore => resolved_bytes(
            arca_store_value.ok_or_else(|| {
                "credential_missing: Arca secure storage has no API Key for this Provider"
                    .to_string()
            })?,
            "arca-store",
            "system-keychain".into(),
        ),
        ApiKeySource::Env { name } => resolved_bytes(
            std::env::var(name)
                .map_err(|_| format!("credential_missing: environment variable {name} is not set"))?
                .into_bytes(),
            "env",
            name.clone(),
        ),
        ApiKeySource::File { path } => resolve_file(Path::new(path)),
        ApiKeySource::Helper { .. } => resolve_helper(source),
    }
}

pub fn validate_for_ui(source: &ApiKeySource) -> Result<CredentialValidationView, String> {
    validate_source(source)?;
    let evidence = match source {
        ApiKeySource::File { path } => resolve_file(Path::new(path))?.evidence().clone(),
        ApiKeySource::Helper { .. } => resolve_helper(source)?.evidence().clone(),
        ApiKeySource::Env { name } => {
            return Ok(CredentialValidationView {
                source_kind: "env".into(),
                source_identity: name.clone(),
                message: "Environment variable name is valid; its value is resolved by the Agent environment.".into(),
            })
        }
        ApiKeySource::ArcaStore => {
            return Ok(CredentialValidationView {
                source_kind: "arca-store".into(),
                source_identity: "system-keychain".into(),
                message: "Arca secure storage is available; the key is checked when the Provider is saved.".into(),
            })
        }
    };
    Ok(CredentialValidationView {
        source_kind: evidence.source_kind,
        source_identity: evidence.source_identity,
        message: "Credential source returned one valid private value.".into(),
    })
}

pub fn validate_source(source: &ApiKeySource) -> Result<(), String> {
    match source {
        ApiKeySource::ArcaStore => Ok(()),
        ApiKeySource::Env { name } => validate_env_name(name),
        ApiKeySource::File { path } => {
            if !Path::new(path).is_absolute() {
                Err("credential_file_insecure: key file path must be absolute".into())
            } else {
                Ok(())
            }
        }
        ApiKeySource::Helper {
            command,
            args,
            ttl_ms,
        } => validate_helper(command, args, *ttl_ms),
    }
}

fn validate_env_name(name: &str) -> Result<(), String> {
    let mut bytes = name.bytes();
    let valid = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err("credential_env_invalid: environment variable name is invalid".into())
    }
}

fn validate_helper(command: &str, args: &[String], ttl_ms: Option<u64>) -> Result<(), String> {
    let trusted_system_command = matches!(command, "security" | "op" | "pass" | "secret-tool");
    let executable_name = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command);
    let is_shell = matches!(
        executable_name,
        "sh" | "bash"
            | "zsh"
            | "dash"
            | "fish"
            | "csh"
            | "tcsh"
            | "cmd.exe"
            | "powershell.exe"
            | "pwsh"
    );
    if !(Path::new(command).is_absolute() || trusted_system_command)
        || command.contains('\0')
        || args.iter().any(|arg| arg.contains('\0'))
        || is_shell
    {
        return Err(
            "credential_helper_invalid: command must be an absolute executable or trusted system command"
                .into(),
        );
    }
    if ttl_ms.is_some_and(|ttl| !(1_000..=3_600_000).contains(&ttl)) {
        return Err("credential_helper_invalid: ttl_ms must be between 1000 and 3600000".into());
    }
    Ok(())
}

pub fn resolve_file(path: &Path) -> Result<ResolvedCredential, String> {
    if !path.is_absolute() {
        return Err("credential_file_insecure: key file path must be absolute".into());
    }
    let symlink_metadata = std::fs::symlink_metadata(path).map_err(|_| {
        "credential_file_insecure: key file is missing or cannot be inspected".to_string()
    })?;
    if symlink_metadata.file_type().is_symlink() {
        return Err("credential_file_insecure: key file must not be a symlink".into());
    }

    #[cfg(unix)]
    let file: std::fs::File = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )
    .map(std::fs::File::from)
    .map_err(|_| "credential_file_insecure: key file could not be opened safely".to_string())?;

    #[cfg(not(unix))]
    let mut file = std::fs::File::open(path)
        .map_err(|_| "credential_file_insecure: key file could not be opened safely".to_string())?;

    let metadata = file
        .metadata()
        .map_err(|_| "credential_file_insecure: key file metadata is unavailable".to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_CREDENTIAL_BYTES {
        return Err("credential_file_insecure: key file must be a small regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err("credential_file_insecure: key file permissions must be 0600".into());
        }
        if metadata.uid() != rustix::process::getuid().as_raw() {
            return Err(
                "credential_file_insecure: key file must be owned by the current user".into(),
            );
        }
    }

    let mut bytes = Zeroizing::new(Vec::with_capacity(metadata.len() as usize));
    file.take(MAX_CREDENTIAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "credential_file_insecure: key file could not be read".to_string())?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    if bytes.is_empty() || bytes.contains(&b'\n') || bytes.contains(&b'\r') || bytes.contains(&0) {
        return Err(
            "credential_file_insecure: key file must contain exactly one non-empty line".into(),
        );
    }
    resolved_bytes(bytes.to_vec(), "file", path.display().to_string())
}

pub fn resolve_helper(source: &ApiKeySource) -> Result<ResolvedCredential, String> {
    let ApiKeySource::Helper {
        command,
        args,
        ttl_ms,
    } = source
    else {
        return Err("credential_helper_invalid: source is not a helper".into());
    };
    validate_helper(command, args, *ttl_ms)?;

    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "credential_helper_failed: helper could not be started".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "credential_helper_failed: helper stdout is unavailable".to_string())?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_CREDENTIAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < Duration::from_secs(10) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err("credential_helper_failed: helper timed out".into());
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err("credential_helper_failed: helper status is unavailable".into());
            }
        }
    };
    let mut bytes = Zeroizing::new(
        reader
            .join()
            .map_err(|_| "credential_helper_failed: helper output reader failed".to_string())?
            .map_err(|_| "credential_helper_failed: helper output could not be read".to_string())?,
    );
    if !status.success() {
        return Err("credential_helper_failed: helper exited unsuccessfully".into());
    }
    if bytes.len() > MAX_CREDENTIAL_BYTES as usize {
        return Err("credential_helper_invalid_output: helper output is too large".into());
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    if bytes.is_empty() || bytes.contains(&b'\n') || bytes.contains(&b'\r') || bytes.contains(&0) {
        return Err(
            "credential_helper_invalid_output: helper must emit exactly one non-empty line".into(),
        );
    }
    resolved_bytes(bytes.to_vec(), "helper", command.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;

    fn isolated_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("arca-credential-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn validates_environment_variable_names_without_reading_a_secret() {
        assert!(validate_source(&ApiKeySource::Env {
            name: "MAX_AI_API_KEY".into(),
        })
        .is_ok());
        assert_eq!(
            validate_source(&ApiKeySource::Env {
                name: "not-valid-key".into(),
            })
            .unwrap_err(),
            "credential_env_invalid: environment variable name is invalid"
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_source_accepts_only_a_private_regular_file() {
        let root = isolated_dir("file");
        let private = root.join("private.key");
        fs::write(&private, b"super-secret\n").unwrap();
        fs::set_permissions(&private, fs::Permissions::from_mode(0o600)).unwrap();

        let resolved = resolve_file(&private).unwrap();
        assert_eq!(resolved.expose_for_delivery(), b"super-secret");
        assert_eq!(resolved.evidence().source_kind, "file");
        assert!(!resolved.evidence().secret_sha256.contains("super-secret"));

        let public = root.join("public.key");
        fs::write(&public, b"super-secret").unwrap();
        fs::set_permissions(&public, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve_file(&public)
            .unwrap_err()
            .starts_with("credential_file_insecure:"));

        let link = root.join("link.key");
        symlink(&private, &link).unwrap();
        assert!(resolve_file(&link)
            .unwrap_err()
            .starts_with("credential_file_insecure:"));

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn file_source_rejects_empty_and_multiline_values() {
        let root = isolated_dir("shape");
        for (name, bytes) in [
            ("empty", b"".as_slice()),
            ("multi", b"first\nsecond".as_slice()),
        ] {
            let path = root.join(name);
            fs::write(&path, bytes).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(resolve_file(&path)
                .unwrap_err()
                .starts_with("credential_file_insecure:"));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn helper_executes_a_structured_command_without_a_shell() {
        let root = isolated_dir("helper");
        let helper = root.join("credential-helper");
        fs::write(&helper, b"#!/bin/sh\nprintf '%s\\n' \"$1\"\n").unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
        let source = ApiKeySource::Helper {
            command: helper.display().to_string(),
            args: vec!["helper-secret".into()],
            ttl_ms: Some(1_000),
        };

        let resolved = resolve_helper(&source).unwrap();
        assert_eq!(resolved.expose_for_delivery(), b"helper-secret");
        assert_eq!(resolved.evidence().source_kind, "helper");
        assert!(!format!("{resolved:?}").contains("helper-secret"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn helper_rejects_shell_programs_and_multiline_output() {
        assert!(validate_source(&ApiKeySource::Helper {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), "printf secret".into()],
            ttl_ms: None,
        })
        .unwrap_err()
        .starts_with("credential_helper_invalid:"));
    }

    #[test]
    fn auto_delivery_uses_native_routes_and_never_plaintext() {
        use crate::domain::assets::ApiKeyDelivery;

        assert_eq!(
            select_delivery(
                "opencode",
                &ApiKeySource::Env {
                    name: "OPENROUTER_API_KEY".into()
                },
                &ApiKeyDelivery::Auto
            )
            .unwrap(),
            PreparedCredentialRoute::NativeEnvReference
        );
        assert_eq!(
            select_delivery("pi", &ApiKeySource::ArcaStore, &ApiKeyDelivery::Auto).unwrap(),
            PreparedCredentialRoute::ArcaKeychainHelper
        );
        assert_eq!(
            select_delivery("opencode", &ApiKeySource::ArcaStore, &ApiKeyDelivery::Auto).unwrap(),
            PreparedCredentialRoute::OpenCodeAuthStore
        );
        assert!(
            select_delivery("goose", &ApiKeySource::ArcaStore, &ApiKeyDelivery::Auto)
                .unwrap_err()
                .starts_with("credential_delivery_unsupported:")
        );
    }

    #[test]
    fn explicit_delivery_requires_a_verified_agent_capability() {
        use crate::domain::assets::ApiKeyDelivery;

        assert_eq!(
            select_delivery(
                "opencode",
                &ApiKeySource::File {
                    path: "/private/key".into()
                },
                &ApiKeyDelivery::AgentStore
            )
            .unwrap(),
            PreparedCredentialRoute::OpenCodeAuthStore
        );
        assert_eq!(
            select_delivery(
                "opencode",
                &ApiKeySource::ArcaStore,
                &ApiKeyDelivery::Plaintext
            )
            .unwrap(),
            PreparedCredentialRoute::Plaintext
        );
        assert!(
            select_delivery("goose", &ApiKeySource::ArcaStore, &ApiKeyDelivery::Plaintext)
                .unwrap_err()
                .starts_with("credential_delivery_unsupported:")
        );
        assert_eq!(
            select_delivery("opencode", &ApiKeySource::ArcaStore, &ApiKeyDelivery::Env).unwrap(),
            PreparedCredentialRoute::NativeEnvReference
        );
        assert_eq!(
            select_delivery("pi", &ApiKeySource::ArcaStore, &ApiKeyDelivery::Command).unwrap(),
            PreparedCredentialRoute::ArcaKeychainHelper
        );
    }

    #[test]
    fn default_plaintext_resolves_per_agent_capability() {
        use crate::domain::assets::ApiKeyDelivery;

        assert!(available_deliveries("opencode").contains(&ApiKeyDelivery::Plaintext));
        assert!(available_deliveries("pi").contains(&ApiKeyDelivery::Plaintext));
        assert!(available_deliveries("kilo-code").contains(&ApiKeyDelivery::Plaintext));
        assert!(!available_deliveries("grok-build").contains(&ApiKeyDelivery::Plaintext));
        assert_eq!(
            resolve_delivery("opencode", &ApiKeyDelivery::Plaintext),
            ApiKeyDelivery::Plaintext
        );
        assert_eq!(
            resolve_delivery("grok-build", &ApiKeyDelivery::Plaintext),
            ApiKeyDelivery::Auto
        );
        assert_eq!(
            resolve_delivery("claude-code", &ApiKeyDelivery::Plaintext),
            ApiKeyDelivery::Auto
        );
        assert!(available_deliveries("claude-code").contains(&ApiKeyDelivery::Command));
    }
}
