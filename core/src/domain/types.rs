//! Persisted and wire-safe value objects shared by all resource domains.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Wire protocol used by a reusable model endpoint profile. These values match
/// the provider identifiers supported by the first managed Agent set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ModelProtocol {
    AnthropicMessages,
    OpenaiResponses,
    OpenaiCompletions,
    GeminiGenerateContent,
}

impl ModelProtocol {
    /// The request path appended by the protocol clients managed by Arca.
    pub fn default_endpoint_path(&self) -> &'static str {
        match self {
            Self::AnthropicMessages => "/v1/messages",
            Self::OpenaiResponses => "/responses",
            Self::OpenaiCompletions => "/chat/completions",
            Self::GeminiGenerateContent => "/models/{model}:generateContent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelProviderProtocolConfig {
    pub endpoint_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthRequirement {
    Required,
    Optional,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ApiKeySource {
    #[serde(alias = "mux-store")]
    ArcaStore,
    Env {
        name: String,
    },
    File {
        path: String,
    },
    Helper {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ttl_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelProviderConfig {
    /// Stable Arca identity for one account/endpoint configuration. The
    /// provider kind (for example `openrouter`) is intentionally not unique.
    pub id: String,
    pub name: String,
    /// API/billing channel such as `openrouter`, `anthropic`, or `custom`.
    pub provider: String,
    /// Shared connection root. Protocol-specific request paths live below.
    pub base_url: String,
    /// Optional full model-catalog endpoint. When absent, Arca derives one from
    /// the Provider protocol endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_catalog_url: Option<String>,
    /// Optional account/key page for this connection; otherwise use the
    /// built-in Provider template's portal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portal_url: Option<String>,
    /// Enabled protocols and their editable relative request paths.
    pub protocols: BTreeMap<ModelProtocol, ModelProviderProtocolConfig>,
    pub auth_requirement: AuthRequirement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_source: Option<ApiKeySource>,
    /// Hydrated compatibility field for adapters that have not yet moved to a
    /// prepared credential route. New persistence uses `api_key_source` only.
    #[serde(skip)]
    pub env_key: Option<String>,
}

#[derive(Deserialize)]
struct ModelProviderConfigWire {
    id: String,
    name: String,
    provider: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    model_catalog_url: Option<String>,
    #[serde(default)]
    portal_url: Option<String>,
    #[serde(default)]
    protocols: BTreeMap<ModelProtocol, ModelProviderProtocolConfig>,
    /// Settings v3/v4 stored one protocol-client base URL per protocol.
    #[serde(default)]
    endpoints: BTreeMap<ModelProtocol, String>,
    #[serde(default)]
    env_key: Option<String>,
    #[serde(default)]
    auth_requirement: Option<AuthRequirement>,
    #[serde(default)]
    api_key_source: Option<ApiKeySource>,
}

impl<'de> Deserialize<'de> for ModelProviderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ModelProviderConfigWire::deserialize(deserializer)?;
        if !wire.endpoints.is_empty()
            && (!wire.base_url.trim().is_empty() || !wire.protocols.is_empty())
        {
            return Err(serde::de::Error::custom(format!(
                "model_provider_schema_conflict: Provider '{}' mixes legacy endpoints with Base URL / Endpoint Path fields",
                wire.name
            )));
        }
        let (base_url, protocols) = if wire.endpoints.is_empty() {
            (wire.base_url, wire.protocols)
        } else {
            migrate_legacy_provider_endpoints(&wire.name, &wire.endpoints)
                .map_err(serde::de::Error::custom)?
        };
        let api_key_source = wire.api_key_source.or_else(|| {
            wire.env_key
                .clone()
                .map(|name| ApiKeySource::Env { name })
        });
        let auth_requirement = wire.auth_requirement.unwrap_or_else(|| {
            infer_auth_requirement(&wire.provider, &base_url, api_key_source.is_some())
        });
        let env_key = match &api_key_source {
            Some(ApiKeySource::Env { name }) => Some(name.clone()),
            _ => None,
        };
        Ok(Self {
            id: wire.id,
            name: wire.name,
            provider: wire.provider,
            base_url,
            model_catalog_url: wire.model_catalog_url,
            portal_url: wire.portal_url,
            protocols,
            auth_requirement,
            api_key_source,
            env_key,
        })
    }
}

fn infer_auth_requirement(
    provider: &str,
    base_url: &str,
    has_source: bool,
) -> AuthRequirement {
    let provider = provider.trim().to_ascii_lowercase();
    let keyless_provider = matches!(
        provider.as_str(),
        "ollama" | "lm-studio" | "vllm" | "bilibili-index-translate"
    ) || url::Url::parse(base_url).ok().is_some_and(|url| {
        matches!(
            url.host_str(),
            Some("localhost" | "127.0.0.1" | "::1")
        )
    });
    if keyless_provider {
        AuthRequirement::None
    } else if provider.is_empty() || provider == "custom" {
        AuthRequirement::Optional
    } else if has_source {
        AuthRequirement::Required
    } else {
        AuthRequirement::Required
    }
}

pub(crate) fn migrate_legacy_provider_endpoints(
    provider_name: &str,
    endpoints: &BTreeMap<ModelProtocol, String>,
) -> Result<(String, BTreeMap<ModelProtocol, ModelProviderProtocolConfig>), String> {
    let mut shared_origin = None::<String>;
    let mut parsed_endpoints = Vec::new();
    for (protocol, endpoint) in endpoints {
        let endpoint = endpoint.trim().trim_end_matches('/');
        let parsed = url::Url::parse(endpoint).map_err(|error| {
            format!(
                "model_provider_endpoint_migration_blocked: Provider '{provider_name}' has an invalid legacy endpoint: {error}"
            )
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(format!(
                "model_provider_endpoint_migration_blocked: Provider '{provider_name}' has a legacy endpoint that cannot be represented safely"
            ));
        }
        let mut origin_url = parsed.clone();
        origin_url.set_path("");
        origin_url.set_query(None);
        origin_url.set_fragment(None);
        let endpoint_origin = origin_url.as_str().trim_end_matches('/').to_string();
        if shared_origin
            .as_ref()
            .is_some_and(|existing| !existing.eq_ignore_ascii_case(&endpoint_origin))
        {
            return Err(format!(
                "model_provider_endpoint_migration_blocked: Provider '{provider_name}' uses multiple legacy endpoint origins; split it into separate Providers before upgrading"
            ));
        }
        shared_origin.get_or_insert(endpoint_origin);
        parsed_endpoints.push((protocol.clone(), endpoint.to_string(), parsed));
    }
    let shared_legacy_base = parsed_endpoints
        .first()
        .map(|(_, endpoint, _)| endpoint.clone())
        .filter(|first| {
            parsed_endpoints
                .iter()
                .all(|(_, endpoint, _)| endpoint == first)
        });
    let base_url = shared_legacy_base
        .clone()
        .unwrap_or_else(|| shared_origin.unwrap_or_default());
    let protocols = parsed_endpoints
        .into_iter()
        .map(|(protocol, _, parsed)| {
            let prefix = if shared_legacy_base.is_some() {
                ""
            } else {
                parsed.path().trim_end_matches('/')
            };
            let endpoint_path = format!("{prefix}{}", protocol.default_endpoint_path());
            (protocol, ModelProviderProtocolConfig { endpoint_path })
        })
        .collect();
    Ok((base_url, protocols))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelProfile {
    pub id: String,
    pub name: String,
    /// Stable reference to the shared Provider configuration. Missing only in
    /// pre-v3 settings and transient import drafts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// API/计费渠道，例如 `openrouter`、`anthropic` 或 `custom`。
    /// v1 Profile 没有该字段；v2 migration 会在接管前补齐。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    /// 模型开发商。它与访问渠道正交，例如通过 OpenRouter 使用 Claude
    /// 时 provider 是 `openrouter`，model_vendor 是 `anthropic`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_vendor: Option<String>,
    /// Agent-native provider/model keys retained during explicit adoption.
    /// New profiles leave this empty and use the stable native ownership namespace.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub native_ids: BTreeMap<String, String>,
    pub protocol: ModelProtocol,
    /// Provider-owned runtime field. Settings v5 strips it before persistence.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_url: String,
    /// Provider-owned runtime field. Settings v5 strips it before persistence.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub endpoint_path: String,
    pub model: String,
    /// Optional environment variable name used by Agents such as Grok Build
    /// that natively resolve per-model credentials from the process environment.
    /// This is metadata only; the credential value is never stored here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    /// Missing means the Agent/provider decides. Explicit booleans preserve a
    /// user's on/off override and remain wire-compatible with older settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StdioConfig {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

fn default_http_type() -> String {
    "http".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpConfig {
    // Many real configs write an http server as just `{ "url": "…" }` (no type),
    // or use `streamable-http` / `sse`. Default the type so those are still
    // recognized during a scan instead of being dropped.
    #[serde(rename = "type", default = "default_http_type")]
    pub kind: String, // "http" | "sse" | "streamable-http" | …
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum McpConfig {
    Stdio(StdioConfig),
    Http(HttpConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RegistryConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdio: Option<StdioConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryEntry {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub config: RegistryConfig,
    /// Where this entry came from. Absent for builtin entries (those are
    /// inferred at runtime); present for entries written to ~/.arca/registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<RegistryOrigin>,
    /// Optional homepage / source repository URL (e.g. a GitHub repo). Shown as a
    /// link in the UI; free-form, not part of the entry's identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
}

/// Transport bucket of a config: "stdio" (local process) or "http" (remote,
/// covers http+sse). This is the second half of a registry entry's identity.
pub fn transport_of(config: &McpConfig) -> &'static str {
    match config {
        McpConfig::Stdio(_) => "stdio",
        McpConfig::Http(_) => "http",
    }
}

impl RegistryEntry {
    /// Transport bucket of this entry ("stdio" | "http"). An entry carries
    /// exactly one transport; if both were somehow present, stdio wins.
    pub fn transport(&self) -> &'static str {
        if self.config.stdio.is_some() {
            "stdio"
        } else {
            "http"
        }
    }

    /// Composite identity: `name::transport`. Two entries with the same name
    /// but different transports (e.g. figma stdio vs http) are distinct.
    pub fn key(&self) -> String {
        format!("{}::{}", self.name, self.transport())
    }
}

/// Provenance of a catalog entry.
/// `kind` is one of:
///   - "discovered" — scanned from a local app config (`agent`/`scope` set),
///   - "manual"     — created by the user by hand,
///   - "remote"     — came from a subscribed remote source (`source` = its id),
///   - "local"      — came from a local file source (`source` = its id).
///
/// `agent`/`scope` describe the source app for discovered entries; `source`
/// references the owning `SourceDef` for remote/local entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryOrigin {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Id of the `SourceDef` this entry came from (remote/local sources only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

fn default_mcp_key() -> String {
    "mcpServers".to_string()
}

/// A user-added catalog source: either a subscribed remote URL or a local file.
/// The actual servers are parsed from a cached copy on disk under
/// `~/.arca/assets/mcps/sources/<kind>/<id>.<ext>`. There is intentionally no
/// "builtin" kind —
/// the catalog is entirely user-driven (subscribe / add local).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceDef {
    pub id: String,
    pub kind: String, // "remote" | "local"
    pub name: String,
    /// Remote sources: the subscribed URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Local sources: the original picked path (stored portably as `~/…`), used
    /// for re-copy on refresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub format: String, // "json" | "toml"
    #[serde(default = "default_mcp_key")]
    pub key: String, // config section key, default "mcpServers"
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_count: Option<u32>,
    /// Last fetch/parse error, if any (keeps the source visible but flagged).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<McpConfig> for RegistryConfig {
    fn from(cfg: McpConfig) -> Self {
        match cfg {
            McpConfig::Stdio(c) => RegistryConfig {
                stdio: Some(c),
                http: None,
            },
            McpConfig::Http(c) => RegistryConfig {
                stdio: None,
                http: Some(c),
            },
        }
    }
}

impl SourceDef {
    /// A subscribed remote URL source, enabled, stamped now, `mcpServers` key.
    pub fn new_remote(id: String, name: String, url: String, format: String, now: String) -> Self {
        Self {
            url: Some(url),
            ..Self::base(id, "remote", name, format, now)
        }
    }

    /// A local-file source (imported or managed), enabled, stamped now.
    pub fn new_local(
        id: String,
        name: String,
        path: Option<String>,
        format: String,
        now: String,
    ) -> Self {
        Self {
            path,
            ..Self::base(id, "local", name, format, now)
        }
    }

    fn base(id: String, kind: &str, name: String, format: String, now: String) -> Self {
        Self {
            id,
            kind: kind.into(),
            name,
            url: None,
            path: None,
            format,
            key: "mcpServers".into(),
            enabled: true,
            added_at: Some(now.clone()),
            synced_at: Some(now),
            server_count: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSkillsDirectory {
    pub target_id: String,
    pub global_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AgentInstallProbe {
    Path { path: String },
    Command { name: String },
    MacBundle { bundle_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSkillsCapability {
    pub target_id: String,
    pub global_dir: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<AgentSkillsDirectory>,
    pub docs: String,
    pub evidence: String,
    pub verified_at: String,
    #[serde(default)]
    pub probes: Vec<AgentInstallProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentDefinition {
    pub global: Option<String>,
    pub project: Option<String>,
    pub format: String, // "json" | "toml" | "yaml"
    pub key: String,
    /// Interpret a dotted JSON `key` as an object path instead of a literal
    /// top-level property name. This stays opt-in so historical and custom
    /// Agent definitions retain their original literal-key behavior.
    #[serde(default, skip_serializing_if = "is_false")]
    pub key_path: bool,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<bool>,
    /// Display metadata and evidence are intentionally part of the definition:
    /// the UI must distinguish a verified writable target from a discovered
    /// catalog entry whose on-disk schema is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>, // "official" | "catalog" | "custom"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
    /// Wire-format metadata. Missing values retain the legacy standard map
    /// behavior so existing custom Agent definitions remain compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>, // "map" | "list"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transports: Option<Vec<String>>,
    /// Required root fields to add when an adapter materializes its managed
    /// capability. Existing values are never overwritten; adapters that
    /// support nested defaults merge only missing descendants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_defaults: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<AgentSkillsCapability>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_entry_roundtrips_stdio() {
        let json = r#"{"name":"git","description":"d","tags":["builtin"],
            "config":{"stdio":{"command":"npx","args":["-y","x"]}}}"#;
        let e: RegistryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(e.name, "git");
        assert_eq!(e.config.stdio.as_ref().unwrap().command, "npx");
    }

    #[test]
    fn model_reasoning_preserves_legacy_booleans_and_omits_auto() {
        let missing: ModelProfile = serde_json::from_value(serde_json::json!({
            "id": "auto",
            "name": "Auto",
            "provider": "openrouter",
            "protocol": "openai-responses",
            "base_url": "https://openrouter.ai/api/v1",
            "model": "example/auto"
        }))
        .unwrap();
        assert_eq!(missing.reasoning, None);
        assert!(serde_json::to_value(&missing)
            .unwrap()
            .get("reasoning")
            .is_none());

        for (wire, expected) in [(true, Some(true)), (false, Some(false))] {
            let mut value = serde_json::to_value(&missing).unwrap();
            value["reasoning"] = serde_json::Value::Bool(wire);
            let profile: ModelProfile = serde_json::from_value(value).unwrap();
            assert_eq!(profile.reasoning, expected);
            assert_eq!(
                serde_json::to_value(profile).unwrap()["reasoning"],
                serde_json::Value::Bool(wire)
            );
        }
    }

    #[test]
    fn gemini_generate_content_has_a_stable_wire_name_and_dynamic_endpoint() {
        let protocol = ModelProtocol::GeminiGenerateContent;
        assert_eq!(
            serde_json::to_string(&protocol).unwrap(),
            "\"gemini-generate-content\""
        );
        assert_eq!(
            protocol.default_endpoint_path(),
            "/models/{model}:generateContent"
        );
        assert_eq!(
            serde_json::from_str::<ModelProtocol>("\"gemini-generate-content\"").unwrap(),
            protocol
        );
    }

    #[test]
    fn api_key_source_roundtrips_all_non_secret_forms() {
        let cases = [
            ApiKeySource::ArcaStore,
            ApiKeySource::Env {
                name: "MAX_AI_API_KEY".into(),
            },
            ApiKeySource::File {
                path: "/private/maxai.key".into(),
            },
            ApiKeySource::Helper {
                command: "/usr/local/bin/maxai-key".into(),
                args: vec!["--profile".into(), "coding".into()],
                ttl_ms: Some(300_000),
            },
        ];

        for source in cases {
            let json = serde_json::to_value(&source).unwrap();
            assert_eq!(serde_json::from_value::<ApiKeySource>(json).unwrap(), source);
        }
    }

    #[test]
    fn legacy_provider_env_key_migrates_to_an_environment_source() {
        let provider: ModelProviderConfig = serde_json::from_value(serde_json::json!({
            "id": "max-ai",
            "name": "Max AI",
            "provider": "custom",
            "base_url": "https://max.example/v1",
            "protocols": {
                "openai-completions": { "endpoint_path": "/chat/completions" }
            },
            "env_key": "MAX_AI_API_KEY"
        }))
        .unwrap();

        assert_eq!(provider.auth_requirement, AuthRequirement::Optional);
        assert_eq!(
            provider.api_key_source,
            Some(ApiKeySource::Env {
                name: "MAX_AI_API_KEY".into()
            })
        );
        let persisted = serde_json::to_value(provider).unwrap();
        assert!(persisted.get("env_key").is_none());
        assert_eq!(persisted["api_key_source"]["kind"], "env");
    }

    #[test]
    fn public_keyless_provider_infers_no_auth_requirement() {
        let provider: ModelProviderConfig = serde_json::from_value(serde_json::json!({
            "id": "index-translate",
            "name": "Bilibili Index-Translate",
            "provider": "bilibili-index-translate",
            "base_url": "https://index-translate.bilibili.com/v1",
            "protocols": {
                "openai-completions": { "endpoint_path": "/chat/completions" }
            }
        }))
        .unwrap();

        assert_eq!(provider.auth_requirement, AuthRequirement::None);
    }
}
