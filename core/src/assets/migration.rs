//! MCP adoption into the central asset model.

use super::lifecycle::store_pending_mcp_entry;
use super::planner::{finalize_plan_with, LifecycleBinding};
use super::types::{
    AssetOperationKind, AssetOperationPlan, AssetRef, CentralAssetAction, CentralAssetChange,
    DomainPlan,
};
use crate::agents::load_agents;
use crate::domain::types::{transport_of, McpConfig, RegistryEntry};
use crate::paths::{local_sources_dir, settings_file};
use crate::resources::mcp::disabled::load_disabled;
use crate::resources::mcp::ops::scan_installed;
use crate::resources::mcp::registry::read_registry;
use crate::resources::mcp::scanner::scan_agents_with_enabled;
use crate::settings::load_settings_strict;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum McpAdoptionStatus {
    ExternalAdded,
    ExternalChanged,
}

/// Read-only migration evidence. Config values may contain credentials, so the
/// projection exposes hashes only. The raw config is re-read and bound to an
/// in-memory payload during planning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpAdoptionCandidate {
    pub agent_id: String,
    pub asset_key: String,
    pub enabled: bool,
    pub status: McpAdoptionStatus,
    pub config_hash: String,
    pub fingerprint: String,
    pub settings_hash: String,
    pub target_hash: String,
    pub candidate_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanMcpAdoptionRequest {
    pub asset_key: String,
    pub agent_id: String,
    pub candidate_fingerprint: String,
}

#[derive(Debug, Clone)]
struct ObservedConfig {
    agent_id: String,
    asset_key: String,
    enabled: bool,
    config: McpConfig,
}

pub fn list_mcp_adoption_candidates() -> Result<Vec<McpAdoptionCandidate>, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let settings_hash = hash_optional(fs::read(settings_file()).ok().as_deref());
    let configs = observed_configs();
    let mut candidates: Vec<_> = scan_installed(None)
        .into_iter()
        .filter(|item| item.scope == "global")
        .filter_map(|item| {
            let asset_key = format!("{}::{}", item.name, item.transport);
            let desired_enabled = settings
                .mcp_consumptions
                .as_ref()
                .and_then(|records| records.get(&item.agent))
                .and_then(|records| records.get(&asset_key))
                .map(|record| record.enabled);
            if desired_enabled == Some(item.enabled) && !item.customized {
                return None;
            }
            let observed = configs.iter().find(|candidate| {
                candidate.agent_id == item.agent
                    && candidate.asset_key == asset_key
                    && candidate.enabled == item.enabled
            })?;
            let status = if desired_enabled.is_some() {
                McpAdoptionStatus::ExternalChanged
            } else {
                McpAdoptionStatus::ExternalAdded
            };
            let config_hash = hash_serializable(&observed.config);
            let fingerprint = hash_fields(&[
                item.agent.as_bytes(),
                asset_key.as_bytes(),
                if item.enabled {
                    b"enabled"
                } else {
                    b"disabled"
                },
                config_hash.as_bytes(),
            ]);
            let target_hash = if item.file_path.is_empty() {
                hash_optional(None)
            } else {
                hash_optional(fs::read(&item.file_path).ok().as_deref())
            };
            let candidate_hash = hash_fields(&[
                fingerprint.as_bytes(),
                settings_hash.as_bytes(),
                target_hash.as_bytes(),
            ]);
            Some(McpAdoptionCandidate {
                agent_id: item.agent,
                asset_key,
                enabled: item.enabled,
                status,
                config_hash,
                fingerprint,
                settings_hash: settings_hash.clone(),
                target_hash,
                candidate_hash,
            })
        })
        .collect();
    candidates.sort_by(|left, right| {
        left.asset_key
            .cmp(&right.asset_key)
            .then_with(|| left.agent_id.cmp(&right.agent_id))
    });
    Ok(candidates)
}

pub fn plan_mcp_adoption(request: PlanMcpAdoptionRequest) -> Result<AssetOperationPlan, String> {
    super::types::validate_mcp_asset_key(&request.asset_key).map_err(|error| error.to_string())?;
    let candidate = list_mcp_adoption_candidates()?
        .into_iter()
        .find(|candidate| {
            candidate.asset_key == request.asset_key && candidate.agent_id == request.agent_id
        })
        .ok_or_else(|| "observation_stale: MCP observation is no longer available".to_string())?;
    if candidate.fingerprint != request.candidate_fingerprint {
        return Err("observation_stale: MCP observation changed after review".into());
    }

    let central = read_registry()
        .into_iter()
        .find(|entry| entry.key() == request.asset_key);
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let name = request
        .asset_key
        .rsplit_once("::")
        .map(|(name, _)| name)
        .ok_or_else(|| "invalid MCP asset key".to_string())?;
    if !load_agents().contains_key(&request.agent_id) {
        return Err(format!(
            "observation_stale: unknown Agent {}",
            request.agent_id
        ));
    }
    let existing: Vec<String> = settings
        .mcp_consumptions
        .as_ref()
        .and_then(|records| records.get(&request.agent_id))
        .map(|records| records.keys().cloned().collect())
        .unwrap_or_default();
    if existing.iter().any(|key| {
        key != &request.asset_key
            && key
                .rsplit_once("::")
                .is_some_and(|(existing_name, _)| existing_name == name)
    }) {
        return Err(format!(
            "mcp_identity_conflict: {} already manages another MCP named {name}",
            request.agent_id
        ));
    }
    let mut desired: std::collections::BTreeSet<String> = existing.iter().cloned().collect();
    desired.insert(request.asset_key.clone());
    let before = BTreeMap::from([(request.agent_id.clone(), existing)]);
    let after = BTreeMap::from([(request.agent_id.clone(), desired.into_iter().collect())]);

    let enabled = BTreeMap::from([(request.agent_id.clone(), candidate.enabled)]);
    let mut draft_hash = None;
    let mut central_changes = Vec::new();
    let mut extra_target_files = Vec::new();
    let observed = observed_configs()
        .into_iter()
        .find(|observed| {
            observed.asset_key == request.asset_key && observed.agent_id == request.agent_id
        })
        .ok_or_else(|| "observation_stale: MCP config is unavailable".to_string())?;
    let entry = RegistryEntry {
        name: name.to_string(),
        description: central
            .as_ref()
            .map(|entry| entry.description.clone())
            .unwrap_or_default(),
        tags: central
            .as_ref()
            .map(|entry| entry.tags.clone())
            .unwrap_or_default(),
        config: observed.config.into(),
        origin: None,
        repo: None,
    };
    let central_changed = central
        .as_ref()
        .is_none_or(|current| current.config != entry.config);
    let pending_entry = if central_changed {
        let hash = super::payload_hash::hash(&entry)?;
        draft_hash = Some(hash);
        central_changes.push(CentralAssetChange {
            asset: AssetRef::Mcp {
                key: request.asset_key.clone(),
            },
            action: if central.is_some() {
                CentralAssetAction::Update
            } else {
                CentralAssetAction::Create
            },
            summary: vec![
                "采用这个 Agent 当前观测到的 MCP 配置".into(),
                "敏感连接值不进入计划、日志或界面".into(),
            ],
        });
        extra_target_files.push(
            local_sources_dir()
                .join("manual.json")
                .to_string_lossy()
                .into_owned(),
        );
        Some(entry)
    } else {
        None
    };

    let plan = finalize_plan_with(
        AssetOperationKind::Adopt,
        DomainPlan::Mcp { before, after },
        central_changes,
        extra_target_files,
        Some(LifecycleBinding::McpAdopt {
            key: request.asset_key,
            draft_hash,
            enabled,
        }),
    )?;
    if let Some(entry) = pending_entry {
        store_pending_mcp_entry(&plan.operation_id, entry);
    }
    Ok(plan)
}

fn observed_configs() -> Vec<ObservedConfig> {
    let agents = load_agents();
    let mut observed = scan_agents_with_enabled(&agents, None, true)
        .into_iter()
        .filter(|(item, _)| item.scope == "global")
        .map(|(item, enabled)| ObservedConfig {
            asset_key: format!("{}::{}", item.name, transport_of(&item.config)),
            agent_id: item.agent,
            enabled,
            config: item.config,
        })
        .collect::<Vec<_>>();
    for (agent_id, entries) in load_disabled() {
        for entry in entries.into_iter().filter(|entry| entry.scope == "global") {
            if observed.iter().any(|item| item.agent_id == agent_id
                && item.asset_key == format!("{}::{}", entry.name, entry.transport)) {
                continue;
            }
            observed.push(ObservedConfig {
                asset_key: format!("{}::{}", entry.name, entry.transport),
                agent_id: agent_id.clone(),
                enabled: false,
                config: entry.config,
            });
        }
    }
    observed
}

fn hash_optional(bytes: Option<&[u8]>) -> String {
    match bytes {
        Some(bytes) => hex::encode(Sha256::digest(bytes)),
        None => "missing".into(),
    }
}

fn hash_serializable(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("serializable migration value");
    hex::encode(Sha256::digest(bytes))
}

fn hash_fields(fields: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for field in fields {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    hex::encode(hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{commit_asset_operation, AssetCommitRequest, ConsumptionStatus};
    use crate::domain::types::{RegistryConfig, StdioConfig};
    use crate::resources::mcp::ops::{disable, install};
    use crate::resources::mcp::registry::write_manual_entry;
    use crate::testenv::TestHome;
    use std::collections::HashMap;

    fn local_entry(command: &str) -> RegistryEntry {
        RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: command.into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        }
    }

    #[test]
    fn exact_observation_is_adoptable_without_creating_a_relationship() {
        let home = TestHome::new("mcp-adopt");
        write_manual_entry(&local_entry("local-server")).unwrap();
        install(
            "local",
            "stdio",
            "global",
            &["claude-code".into()],
            None,
            &HashMap::new(),
        )
        .unwrap();
        let before = fs::read(home.home.join(".arca/settings.json")).unwrap();

        let candidates = list_mcp_adoption_candidates().unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].status, McpAdoptionStatus::ExternalAdded);
        assert_eq!(
            fs::read(home.home.join(".arca/settings.json")).unwrap(),
            before
        );
        assert!(crate::settings::load_settings().mcp_consumptions.is_none());
    }

    #[test]
    fn adoption_plan_binds_one_exact_observation_without_exposing_config() {
        let home = TestHome::new("mcp-adopt-plan");
        write_manual_entry(&local_entry("private-command")).unwrap();
        install(
            "local",
            "stdio",
            "global",
            &["claude-code".into(), "codex".into()],
            None,
            &HashMap::new(),
        )
        .unwrap();
        let candidates = list_mcp_adoption_candidates().unwrap();
        let plan = plan_mcp_adoption(PlanMcpAdoptionRequest {
            asset_key: "local::stdio".into(),
            agent_id: candidates[0].agent_id.clone(),
            candidate_fingerprint: candidates[0].fingerprint.clone(),
        })
        .unwrap();

        assert_eq!(plan.kind, AssetOperationKind::Adopt);
        assert!(plan.can_commit);
        assert_eq!(plan.relationship_changes.len(), 1);
        let persisted = fs::read_to_string(
            home.home
                .join(".arca/staging/consumption")
                .join(&plan.operation_id)
                .join("plan.json"),
        )
        .unwrap();
        assert!(!persisted.contains("private-command"));
    }

    #[test]
    fn adoption_commit_preserves_exact_agent_bytes() {
        let home = TestHome::new("mcp-adopt-commit");
        write_manual_entry(&local_entry("private-command")).unwrap();
        install(
            "local",
            "stdio",
            "global",
            &["claude-code".into()],
            None,
            &HashMap::new(),
        )
        .unwrap();
        let target = home.home.join(".claude.json");
        let before = fs::read(&target).unwrap();
        let candidates = list_mcp_adoption_candidates().unwrap();
        let plan = plan_mcp_adoption(PlanMcpAdoptionRequest {
            asset_key: "local::stdio".into(),
            agent_id: "claude-code".into(),
            candidate_fingerprint: candidates[0].fingerprint.clone(),
        })
        .unwrap();

        let inventory = commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        assert_eq!(fs::read(target).unwrap(), before);
        assert!(inventory.consumptions.iter().any(|item| {
            item.agent_id == "claude-code"
                && item.asset
                    == AssetRef::Mcp {
                        key: "local::stdio".into(),
                    }
                && item.status == ConsumptionStatus::Synced
        }));
    }

    #[test]
    fn adoption_preserves_disabled_state() {
        let _home = TestHome::new("mcp-adopt-disabled");
        write_manual_entry(&local_entry("private-command")).unwrap();
        install(
            "local",
            "stdio",
            "global",
            &["claude-code".into()],
            None,
            &HashMap::new(),
        )
        .unwrap();
        disable("local", "stdio", "global", &["claude-code".into()], None).unwrap();
        let candidates = list_mcp_adoption_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(!candidates[0].enabled);
        let plan = plan_mcp_adoption(PlanMcpAdoptionRequest {
            asset_key: "local::stdio".into(),
            agent_id: "claude-code".into(),
            candidate_fingerprint: candidates[0].fingerprint.clone(),
        })
        .unwrap();

        let inventory = commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        assert!(inventory.consumptions.iter().any(|item| {
            item.agent_id == "claude-code"
                && item.asset
                    == AssetRef::Mcp {
                        key: "local::stdio".into(),
                    }
                && item.enabled == Some(false)
                && item.status == ConsumptionStatus::Synced
        }));
    }

    #[cfg(unix)]
    #[test]
    fn external_secret_is_kept_out_of_plan_and_written_to_private_source() {
        use std::os::unix::fs::PermissionsExt;

        let home = TestHome::new("mcp-import-secret");
        fs::write(
            home.home.join(".claude.json"),
            r#"{"mcpServers":{"private":{"command":"private-server","env":{"TOKEN":"super-secret"}}}}"#,
        )
        .unwrap();
        let candidates = list_mcp_adoption_candidates().unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].status, McpAdoptionStatus::ExternalAdded);
        let plan = plan_mcp_adoption(PlanMcpAdoptionRequest {
            asset_key: "private::stdio".into(),
            agent_id: "claude-code".into(),
            candidate_fingerprint: candidates[0].fingerprint.clone(),
        })
        .unwrap();
        let plan_path = home
            .home
            .join(".arca/staging/consumption")
            .join(&plan.operation_id)
            .join("plan.json");
        assert!(!fs::read_to_string(plan_path)
            .unwrap()
            .contains("super-secret"));

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        let source = home.home.join(".arca/sources/local/manual.json");
        assert!(fs::read_to_string(&source)
            .unwrap()
            .contains("super-secret"));
        assert_eq!(
            fs::metadata(source).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
