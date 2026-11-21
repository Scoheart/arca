use crate::domain::assets::ApiKeyDelivery;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedCredentialRoute {
    NativeEnvReference,
    ArcaCommand,
    OpenCodeAuthStore,
    ClaudeDesktopProfile,
    Plaintext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentCredentialCapabilities {
    #[serde(default)]
    pub native_sources: Vec<String>,
    pub credential_command: bool,
    pub agent_store: bool,
    pub plaintext: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub fn agent_capabilities(agent_id: &str) -> AgentCredentialCapabilities {
    let mut capabilities = AgentCredentialCapabilities {
        native_sources: Vec::new(),
        credential_command: false,
        agent_store: false,
        plaintext: false,
        note: None,
    };
    match agent_id {
        "claude-code" | "codex" => {
            capabilities.native_sources.push("helper".into());
            capabilities.credential_command = true;
        }
        "pi" => {
            capabilities.native_sources.push("helper".into());
            capabilities.credential_command = true;
            capabilities.plaintext = true;
        }
        "claude-desktop" => capabilities.agent_store = true,
        "zcode" => {
            capabilities.plaintext = true;
            capabilities.note = Some("ZCode requires explicit plaintext delivery to its private native config; the Provider keeps its selected credential store".into());
        }
        "opencode" | "opencode-desktop" => {
            capabilities.native_sources = vec!["env".into(), "file".into()];
            capabilities.agent_store = true;
            capabilities.plaintext = true;
        }
        "kilo-desktop" => {
            capabilities.native_sources = vec!["env".into(), "file".into()];
            capabilities.note = Some("Kilo Desktop supports native environment and file references".into());
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
        "mistral-vibe" | "goose" | "deepseek-harness" | "deepseek-harness-desktop" => capabilities.native_sources.push("env".into()),
        "qoder" | "minimax-code" | "kimi-code" | "kimi-code-desktop" | "warp" => {
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
    if capabilities.credential_command {
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

/// Choose a native route independently of the Provider's credential store.
pub fn select_delivery(
    agent_id: &str,
    delivery: &ApiKeyDelivery,
) -> Result<PreparedCredentialRoute, String> {
    use PreparedCredentialRoute::{ArcaCommand, NativeEnvReference, Plaintext};
    let capabilities = agent_capabilities(agent_id);
    let has_env = capabilities.native_sources.iter().any(|kind| kind == "env");
    let native_store = match agent_id {
        "opencode" | "opencode-desktop" => Some(PreparedCredentialRoute::OpenCodeAuthStore),
        "claude-desktop" => Some(PreparedCredentialRoute::ClaudeDesktopProfile),
        _ => None,
    };
    let route = match delivery {
        ApiKeyDelivery::Plaintext if capabilities.plaintext => Some(Plaintext),
        ApiKeyDelivery::Env if has_env => Some(NativeEnvReference),
        ApiKeyDelivery::Command if capabilities.credential_command => Some(ArcaCommand),
        ApiKeyDelivery::AgentStore => native_store,
        ApiKeyDelivery::Auto if capabilities.credential_command => Some(ArcaCommand),
        ApiKeyDelivery::Auto => native_store.or_else(|| has_env.then_some(NativeEnvReference)),
        _ => None,
    };
    route.ok_or_else(|| format!(
        "credential_delivery_unsupported: {agent_id} does not support the selected credential delivery"
    ))
}

pub(crate) fn validate_env_name(name: &str) -> Result<(), String> {
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
