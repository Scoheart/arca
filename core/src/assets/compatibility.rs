//! Compatibility projection across MCP, Model, and Skill assets.

use super::types::{validate_mcp_asset_key, AssetRef};
use crate::agents::load_agents;
use crate::resources::mcp::registry::read_registry;
use crate::resources::model::{
    credential_present, model_agent_capability, profile_credential_issue,
    profile_endpoint_compatibility_issue,
};
use crate::resources::skill::skill_agent_capability;
use crate::settings::load_settings_strict;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityReason {
    /// Stable machine-readable reason used by filters and tests.
    pub code: String,
    /// User-facing explanation owned by core, not reconstructed by React.
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityView {
    pub compatible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<CompatibilityReason>,
    #[serde(default)]
    pub affected_agent_ids: Vec<String>,
}

impl CompatibilityView {
    fn supported(affected_agent_ids: Vec<String>) -> Self {
        Self {
            compatible: true,
            reason: None,
            affected_agent_ids,
        }
    }

    fn unsupported(code: &str, message: impl Into<String>) -> Self {
        Self {
            compatible: false,
            reason: Some(CompatibilityReason {
                code: code.into(),
                message: message.into(),
            }),
            affected_agent_ids: Vec::new(),
        }
    }
}

/// Resolve a central asset and project whether the canonical Agent can consume
/// it. Missing assets are incompatible; observed external items are handled by
/// inventory and must first be imported before this service can select them.
pub fn compatibility_for(agent_id: &str, asset: &AssetRef) -> Result<CompatibilityView, String> {
    asset.validate().map_err(|error| error.to_string())?;
    let agents = load_agents();
    let Some(agent) = agents.get(agent_id) else {
        return Ok(CompatibilityView::unsupported(
            "agent_unknown",
            "Agent 不在当前中央目录中。",
        ));
    };
    if !agent.enabled {
        return Ok(CompatibilityView::unsupported(
            "agent_disabled",
            "Agent 已在 Arca 中停用。",
        ));
    }
    match asset {
        AssetRef::Mcp { key } => mcp_compatibility(agent_id, key),
        AssetRef::Model { profile_id } => model_compatibility(agent_id, profile_id),
        AssetRef::ModelProvider { .. } => Ok(CompatibilityView::unsupported(
            "asset_not_consumable",
            "Provider 由 Model 引用，不能直接关联 Agent。",
        )),
        AssetRef::Skill { name } => skill_compatibility(agent_id, name),
    }
}

fn mcp_compatibility(agent_id: &str, key: &str) -> Result<CompatibilityView, String> {
    McpCompatibilityResolver::load().resolve(agent_id, key)
}

/// Validate a complete MCP selection with one registry and Agent-catalog read.
/// Applying an MCP relationship rewrites the Agent-owned MCP section, so every
/// selected asset must remain compatible even when only one key was added.
pub(crate) fn require_mcp_selection_compatible(
    agent_id: &str,
    keys: &[String],
) -> Result<(), String> {
    if keys.is_empty() {
        return Ok(());
    }
    let resolver = McpCompatibilityResolver::load();
    for key in keys {
        let view = resolver.resolve(agent_id, key)?;
        if let Some(reason) = view.reason {
            return Err(format!("{}: {}", reason.code, reason.message));
        }
    }
    Ok(())
}

pub(crate) struct McpCompatibilityResolver {
    central: BTreeSet<String>,
    agents: BTreeMap<String, crate::domain::types::AgentDefinition>,
}

impl McpCompatibilityResolver {
    pub(crate) fn load() -> Self {
        Self {
            central: read_registry()
                .into_iter()
                .map(|entry| entry.key())
                .collect(),
            agents: load_agents(),
        }
    }

    pub(crate) fn resolve(&self, agent_id: &str, key: &str) -> Result<CompatibilityView, String> {
        validate_mcp_asset_key(key).map_err(|error| error.to_string())?;
        if !self.central.contains(key) {
            return Ok(CompatibilityView::unsupported(
                "asset_missing",
                "中央 MCP 资产不存在；请先导入资产库。",
            ));
        }
        let Some(agent) = self.agents.get(agent_id) else {
            return Ok(CompatibilityView::unsupported(
                "agent_unknown",
                "Agent 不在当前中央目录中。",
            ));
        };
        if !agent.enabled {
            return Ok(CompatibilityView::unsupported(
                "agent_disabled",
                "Agent 已在 Arca 中停用。",
            ));
        }
        let transport = key
            .rsplit_once("::")
            .map(|(_, transport)| transport)
            .expect("validated MCP key");
        let compatible = agent
            .transports
            .as_ref()
            .map(|transports| transports.iter().any(|item| item == transport))
            .unwrap_or_else(|| matches!(transport, "stdio" | "http"));
        if !compatible {
            return Ok(CompatibilityView::unsupported(
                "mcp_transport_unsupported",
                format!("此 Agent 不支持 {transport} MCP transport。"),
            ));
        }
        Ok(CompatibilityView::supported(vec![agent_id.to_string()]))
    }

    pub(crate) fn contains_asset(&self, key: &str) -> bool {
        self.central.contains(key)
    }
}

fn model_compatibility(agent_id: &str, profile_id: &str) -> Result<CompatibilityView, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let Some(profile) = settings
        .model_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(profile_id))
    else {
        return Ok(CompatibilityView::unsupported(
            "asset_missing",
            "中央 Model Profile 不存在。",
        ));
    };
    let Some(agent) = model_agent_capability(agent_id) else {
        return Ok(CompatibilityView::unsupported(
            "model_agent_unsupported",
            "此 Agent 没有可管理的 Model 消费接口。",
        ));
    };
    if agent.mode != "managed" {
        return Ok(CompatibilityView::unsupported(
            "model_guided_only",
            agent.note,
        ));
    }
    if !agent.supported_protocols.contains(&profile.protocol) {
        return Ok(CompatibilityView::unsupported(
            "model_protocol_unsupported",
            "此 Agent 不支持该 Profile 的 protocol。",
        ));
    }
    if let Some((code, message)) = profile_endpoint_compatibility_issue(agent_id, profile) {
        return Ok(CompatibilityView::unsupported(&code, message));
    }
    if let Some((code, message)) = profile_credential_issue(
        agent_id,
        profile,
        credential_present(profile_id),
        &settings.model_selection(agent_id).delivery_for(profile_id),
    ) {
        return Ok(CompatibilityView::unsupported(code, message));
    }
    Ok(CompatibilityView::supported(vec![agent_id.to_string()]))
}

fn skill_compatibility(agent_id: &str, name: &str) -> Result<CompatibilityView, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    if !settings
        .managed_skills
        .as_ref()
        .is_some_and(|skills| skills.contains_key(name))
    {
        return Ok(CompatibilityView::unsupported(
            "asset_missing",
            "中央 Skill 资产不存在；请先安装到资产库。",
        ));
    }
    let Some(capability) =
        skill_agent_capability(agent_id).map_err(|error| format!("{error:?}"))?
    else {
        return Ok(CompatibilityView::unsupported(
            "skill_capability_unverified",
            "此 Agent 没有经过核验的 Skills 物理目标。",
        ));
    };
    if !capability.installed {
        return Ok(CompatibilityView::unsupported(
            "agent_not_installed",
            "未检测到此 Agent，无法安全写入 Skills 目标。",
        ));
    }
    Ok(CompatibilityView::supported(capability.affected_agent_ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        HttpConfig, ModelProfile, ModelProtocol, RegistryConfig, RegistryEntry, StdioConfig,
    };
    use crate::resources::mcp::registry::write_manual_entry;
    use crate::resources::skill::{
        ManagedSkillRecord, RiskLevel, SkillContentKind, SkillRiskSummary, SkillSource,
    };
    use crate::settings::mutate_settings;
    use crate::testenv::TestHome;

    fn add_mcp() {
        write_manual_entry(&RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "local-server".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();
        write_manual_entry(&RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: None,
                http: Some(HttpConfig {
                    kind: "http".into(),
                    url: "https://example.invalid/mcp".into(),
                    headers: None,
                }),
            },
            origin: None,
            repo: None,
        })
        .unwrap();
    }

    fn add_profile(protocol: ModelProtocol) {
        crate::resources::model::save_profile(
            ModelProfile {
                id: "work".into(),
                provider_id: Some("custom-provider".into()),
                name: "Work".into(),
                provider: "custom".into(),
                model_vendor: None,
                native_ids: Default::default(),
                protocol,
                base_url: "https://example.invalid".into(),
                endpoint_path: String::new(),
                model: "model".into(),
                env_key: None,
                context_window: None,
                max_output_tokens: None,
                reasoning: Some(false),
            },
            None,
        )
        .unwrap();
    }

    fn add_skill() {
        mutate_settings(|settings| {
            settings.managed_skills.get_or_insert_default().insert(
                "review-changes".into(),
                ManagedSkillRecord {
                    name: "review-changes".into(),
                    description: "Review changes".into(),
                    content_kind: SkillContentKind::Instructions,
                    source: SkillSource::Local {
                        path: "~/fixture".into(),
                        subpath: "review-changes".into(),
                    },
                    resolved_revision: None,
                    content_hash: "sha256:test".into(),
                    installed_at: "2026-07-18T00:00:00Z".into(),
                    updated_at: "2026-07-18T00:00:00Z".into(),
                    risk: SkillRiskSummary {
                        level: RiskLevel::Low,
                        findings: Vec::new(),
                        finding_count: 0,
                        findings_truncated: false,
                    },
                    update: Default::default(),
                },
            );
        })
        .unwrap();
    }

    #[test]
    fn mcp_uses_canonical_agent_transport_capability() {
        let _home = TestHome::new("compat-mcp");
        add_mcp();
        assert!(
            compatibility_for(
                "claude-code",
                &AssetRef::Mcp {
                    key: "local::stdio".into()
                }
            )
            .unwrap()
            .compatible
        );

        let unsupported = compatibility_for(
            "boltai",
            &AssetRef::Mcp {
                key: "local::http".into(),
            },
        )
        .unwrap();
        assert_eq!(
            unsupported.reason.unwrap().code,
            "mcp_transport_unsupported"
        );
    }

    #[test]
    fn model_distinguishes_protocol_and_managed_mode() {
        let _home = TestHome::new("compat-model");
        add_profile(ModelProtocol::AnthropicMessages);
        assert!(
            compatibility_for(
                "claude-code",
                &AssetRef::Model {
                    profile_id: "work".into()
                }
            )
            .unwrap()
            .compatible
        );
        assert_eq!(
            compatibility_for(
                "codex",
                &AssetRef::Model {
                    profile_id: "work".into()
                }
            )
            .unwrap()
            .reason
            .unwrap()
            .code,
            "model_protocol_unsupported"
        );
        assert!(
            compatibility_for(
                "grok-build",
                &AssetRef::Model {
                    profile_id: "work".into()
                }
            )
            .unwrap()
            .compatible
        );
    }

    #[test]
    fn grok_build_rejects_a_keychain_only_credential() {
        let _home = TestHome::new("compat-model-grok-keychain");
        add_profile(ModelProtocol::OpenaiCompletions);
        crate::resources::model::apply_credential_update("work", Some("secret")).unwrap();

        let unsupported = compatibility_for(
            "grok-build",
            &AssetRef::Model {
                profile_id: "work".into(),
            },
        )
        .unwrap();
        assert_eq!(
            unsupported.reason.unwrap().code,
            "grok_build_env_key_required"
        );

        mutate_settings(|settings| {
            let provider = settings
                .model_providers
                .as_mut()
                .unwrap()
                .get_mut("custom-provider")
                .unwrap();
            provider.api_key_source = Some(crate::domain::types::ApiKeySource::Env {
                name: "OPENROUTER_API_KEY".into(),
            });
            provider.env_key = Some("OPENROUTER_API_KEY".into());
        })
        .unwrap();
        assert!(
            compatibility_for(
                "grok-build",
                &AssetRef::Model {
                    profile_id: "work".into(),
                },
            )
            .unwrap()
            .compatible
        );
    }

    #[test]
    fn opencode_accepts_a_keychain_only_credential_with_default_plaintext() {
        let _home = TestHome::new("compat-model-opencode-keychain");
        add_profile(ModelProtocol::OpenaiCompletions);
        crate::resources::model::apply_credential_update("work", Some("secret")).unwrap();

        assert!(
            compatibility_for(
                "opencode",
                &AssetRef::Model {
                    profile_id: "work".into(),
                },
            )
            .unwrap()
            .compatible
        );
    }

    #[test]
    fn skill_requires_verified_capability_and_install_probe() {
        let home = TestHome::new("compat-skill");
        add_skill();
        assert_eq!(
            compatibility_for(
                "claude-code",
                &AssetRef::Skill {
                    name: "review-changes".into()
                }
            )
            .unwrap()
            .reason
            .unwrap()
            .code,
            "agent_not_installed"
        );

        std::fs::create_dir_all(home.home.join(".claude")).unwrap();
        let compatible = compatibility_for(
            "claude-code",
            &AssetRef::Skill {
                name: "review-changes".into(),
            },
        )
        .unwrap();
        assert!(compatible.compatible);
        assert_eq!(compatible.affected_agent_ids, vec!["claude-code"]);
    }
}
