#![cfg(unix)]

mod support;

use arca_core::consumption::{
    commit_asset_operation, plan_delete_central_asset, plan_reapply_mcp, plan_set_active_model,
    plan_set_agent_consumption, plan_set_mcp_enabled, plan_set_model_enabled,
    plan_set_skill_enabled, plan_update_central_asset, AgentConsumptionSelection,
    AssetCommitRequest, AssetRef, CentralAssetDraft, McpReapplyScope,
    PlanDeleteCentralAssetRequest, PlanReapplyMcpRequest, PlanSetActiveModelRequest,
    PlanSetAgentConsumptionRequest, PlanSetMcpEnabledRequest, PlanSetModelEnabledRequest,
    PlanSetSkillEnabledRequest, PlanUpdateCentralAssetRequest,
};
use arca_core::models::{apply_profile, list_profiles, reconcile_active_models, save_profile};
use arca_core::registry::{read_registry, write_manual_entry};
use arca_core::settings::{load_settings, mutate_settings, UiSettings};
use arca_core::testenv::TestHome;
use arca_core::types::{
    HttpConfig, ModelProfile, ModelProtocol, RegistryConfig, RegistryEntry, StdioConfig,
};
use std::fs;
use support::skills::SkillsFixture;

fn commit(plan: arca_core::consumption::AssetOperationPlan) {
    commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap();
}

fn arca_profile_id(profile_id: &str) -> String {
    format!(
        "arca_{}",
        profile_id
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn mcp(command: &str) -> RegistryEntry {
    named_mcp("local", command)
}

fn named_mcp(name: &str, command: &str) -> RegistryEntry {
    RegistryEntry {
        name: name.into(),
        description: "Local fixture".into(),
        tags: vec!["test".into()],
        config: RegistryConfig {
            stdio: Some(StdioConfig {
                command: command.into(),
                args: Some(vec!["serve".into()]),
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
fn workbuddy_pause_survives_central_update_and_unassign_removes_the_entry() {
    let home = TestHome::new("workbuddy-central-pause");
    write_manual_entry(&mcp("old-server")).unwrap();
    commit(plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "workbuddy".into(),
        selection: AgentConsumptionSelection::Mcp { asset_keys: vec!["local::stdio".into()] },
    }).unwrap());
    commit(plan_set_mcp_enabled(PlanSetMcpEnabledRequest {
        agent_id: "workbuddy".into(), asset_key: "local::stdio".into(), enabled: false,
    }).unwrap());
    commit(plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp { existing_key: Some("local::stdio".into()), entry: Box::new(mcp("new-server")) },
    }).unwrap());
    let path = home.home.join(".workbuddy-ai/mcp.json");
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["local"]["command"], "new-server");
    assert_eq!(value["mcpServers"]["local"]["disabled"], true);
    assert!(!arca_core::disabled::load_disabled().contains_key("workbuddy"));
    commit(plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "workbuddy".into(),
        selection: AgentConsumptionSelection::Mcp { asset_keys: vec![] },
    }).unwrap());
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert!(value["mcpServers"].get("local").is_none());
}

#[test]
fn unrelated_mcp_drift_does_not_block_or_get_overwritten_by_central_update() {
    let home = TestHome::new("central-mcp-unrelated-drift");
    write_manual_entry(&named_mcp("alpha", "alpha-old")).unwrap();
    write_manual_entry(&named_mcp("beta", "beta-old")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["alpha::stdio".into(), "beta::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    let customized = fs::read_to_string(&target)
        .unwrap()
        .replace("beta-old", "beta-custom");
    fs::write(&target, customized).unwrap();

    let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp {
            existing_key: Some("alpha::stdio".into()),
            entry: Box::new(named_mcp("alpha", "alpha-new")),
        },
    })
    .unwrap();
    assert!(plan.can_commit);
    assert!(plan.warnings.is_empty());
    commit(plan);

    let updated = fs::read_to_string(target).unwrap();
    assert!(updated.contains("alpha-new"));
    assert!(updated.contains("beta-custom"));
    assert!(!updated.contains("beta-old"));
}

#[test]
fn central_update_rejects_a_consumer_added_after_review() {
    let _home = TestHome::new("central-mcp-new-consumer-stale");
    write_manual_entry(&named_mcp("alpha", "alpha-old")).unwrap();
    let stale = plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp {
            existing_key: Some("alpha::stdio".into()),
            entry: Box::new(named_mcp("alpha", "alpha-new")),
        },
    })
    .unwrap();

    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["alpha::stdio".into()],
            },
        })
        .unwrap(),
    );

    let error = commit_asset_operation(AssetCommitRequest {
        operation_id: stale.operation_id,
        candidate_hash: stale.candidate_hash,
    })
    .unwrap_err();
    assert_eq!(
        error,
        "asset_operation_stale: central asset consumers changed after review"
    );
    let alpha = read_registry()
        .into_iter()
        .find(|entry| entry.key() == "alpha::stdio")
        .unwrap();
    assert_eq!(alpha.config.stdio.unwrap().command, "alpha-old");
}

#[test]
fn mcp_central_update_propagates_and_delete_cascades() {
    let home = TestHome::new("central-mcp-e2e");
    write_manual_entry(&mcp("old-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    assert!(fs::read_to_string(&target).unwrap().contains("old-server"));

    commit(
        plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Mcp {
                existing_key: Some("local::stdio".into()),
                entry: Box::new(mcp("new-server")),
            },
        })
        .unwrap(),
    );
    let updated = fs::read_to_string(&target).unwrap();
    assert!(updated.contains("new-server"));
    assert!(!updated.contains("old-server"));
    assert_eq!(
        load_settings().mcp_consumptions.unwrap()["claude-code"]
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["local::stdio"]
    );

    commit(
        plan_delete_central_asset(PlanDeleteCentralAssetRequest {
            asset: AssetRef::Mcp {
                key: "local::stdio".into(),
            },
            source_id: Some("manual".into()),
        })
        .unwrap(),
    );
    assert!(!read_registry()
        .iter()
        .any(|entry| entry.key() == "local::stdio"));
    assert!(!load_settings()
        .mcp_consumptions
        .unwrap_or_default()
        .contains_key("claude-code"));
    assert!(!fs::read_to_string(target).unwrap().contains("local"));
}

#[test]
fn mcp_rename_atomically_migrates_identity_consumers_and_enabled_state() {
    let home = TestHome::new("central-mcp-rename");
    write_manual_entry(&named_mcp("old-name", "rename-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["old-name::stdio".into()],
            },
        })
        .unwrap(),
    );
    let disable = plan_set_mcp_enabled(PlanSetMcpEnabledRequest {
        agent_id: "claude-code".into(),
        asset_key: "old-name::stdio".into(),
        enabled: false,
    })
    .unwrap();
    assert_eq!(disable.consumption_state_changes.len(), 1);
    let state = &disable.consumption_state_changes[0];
    assert_eq!(state.agent_id, "claude-code");
    assert_eq!(
        state.asset,
        AssetRef::Mcp {
            key: "old-name::stdio".into()
        }
    );
    assert!(state.before_enabled);
    assert!(!state.after_enabled);
    assert_eq!(state.affected_agent_ids, vec!["claude-code"]);
    assert!(state.target.is_none());
    commit(disable);

    let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp {
            existing_key: Some("old-name::stdio".into()),
            entry: Box::new(named_mcp("new-name", "rename-server")),
        },
    })
    .unwrap();
    assert_eq!(plan.central_changes.len(), 2);
    assert!(plan.relationship_changes.iter().any(|change| {
        change.asset
            == (AssetRef::Mcp {
                key: "old-name::stdio".into(),
            })
    }));
    assert!(plan.relationship_changes.iter().any(|change| {
        change.asset
            == (AssetRef::Mcp {
                key: "new-name::stdio".into(),
            })
    }));
    commit(plan);

    let registry = read_registry();
    assert!(!registry
        .iter()
        .any(|entry| entry.key() == "old-name::stdio"));
    assert!(registry
        .iter()
        .any(|entry| entry.key() == "new-name::stdio"));
    let settings = load_settings();
    let records = &settings.mcp_consumptions.unwrap()["claude-code"];
    assert!(!records.contains_key("old-name::stdio"));
    assert_eq!(records["new-name::stdio"].asset_key, "new-name::stdio");
    assert!(!records["new-name::stdio"].enabled);
    let target = fs::read_to_string(home.home.join(".claude.json")).unwrap_or_default();
    assert!(!target.contains("old-name"));
    assert!(!target.contains("new-name"));
}

#[test]
fn mcp_customized_toggle_requires_explicit_convergence() {
    let home = TestHome::new("central-mcp-toggle-drift");
    write_manual_entry(&mcp("managed-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    let customized = fs::read_to_string(&target)
        .unwrap()
        .replace("managed-server", "customized-server");
    fs::write(&target, &customized).unwrap();

    let disable = plan_set_mcp_enabled(PlanSetMcpEnabledRequest {
        agent_id: "claude-code".into(),
        asset_key: "local::stdio".into(),
        enabled: false,
    })
    .unwrap();
    assert!(!disable.can_commit);
    assert_eq!(
        disable.warnings,
        vec!["claude-code / mcp:local::stdio: mcp_config_drift"]
    );
    assert_eq!(disable.consumption_state_changes.len(), 1);
    let rejected = commit_asset_operation(AssetCommitRequest {
        operation_id: disable.operation_id.clone(),
        candidate_hash: disable.candidate_hash.clone(),
    })
    .unwrap_err();
    assert!(
        rejected.starts_with("asset_operation_blocked:"),
        "{rejected}"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), customized);
    assert!(load_settings().mcp_consumptions.unwrap()["claude-code"]["local::stdio"].enabled);
}

#[test]
fn mcp_missing_target_toggle_remains_hard_blocked() {
    let home = TestHome::new("central-mcp-toggle-missing");
    write_manual_entry(&mcp("managed-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    fs::remove_file(home.home.join(".claude.json")).unwrap();

    let plan = plan_set_mcp_enabled(PlanSetMcpEnabledRequest {
        agent_id: "claude-code".into(),
        asset_key: "local::stdio".into(),
        enabled: false,
    })
    .unwrap();
    assert!(!plan.can_commit);
    assert_eq!(
        plan.warnings,
        vec!["claude-code / mcp:local::stdio: mcp_target_missing"]
    );
    assert_eq!(plan.consumption_state_changes.len(), 1);
}

#[test]
fn mcp_rename_conflict_fails_without_mutating_existing_identity() {
    let _home = TestHome::new("central-mcp-rename-conflict");
    write_manual_entry(&named_mcp("old-name", "old-server")).unwrap();
    write_manual_entry(&named_mcp("taken-name", "taken-server")).unwrap();

    let error = plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp {
            existing_key: Some("old-name::stdio".into()),
            entry: Box::new(named_mcp("taken-name", "replacement-server")),
        },
    })
    .unwrap_err();

    assert!(error.starts_with("asset_identity_conflict:"));
    let registry = read_registry();
    assert_eq!(
        registry
            .iter()
            .find(|entry| entry.key() == "old-name::stdio")
            .unwrap()
            .config
            .stdio
            .as_ref()
            .unwrap()
            .command,
        "old-server"
    );
    assert_eq!(
        registry
            .iter()
            .find(|entry| entry.key() == "taken-name::stdio")
            .unwrap()
            .config
            .stdio
            .as_ref()
            .unwrap()
            .command,
        "taken-server"
    );
}

#[test]
fn mcp_rename_rejects_empty_names_and_transport_changes() {
    let _home = TestHome::new("central-mcp-rename-invalid");
    write_manual_entry(&named_mcp("old-name", "old-server")).unwrap();

    let mut empty = named_mcp(" ", "replacement-server");
    empty.name = " ".into();
    let empty_error = plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp {
            existing_key: Some("old-name::stdio".into()),
            entry: Box::new(empty),
        },
    })
    .unwrap_err();
    assert!(empty_error.starts_with("invalid_asset: MCP name is required"));

    let transport_error = plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp {
            existing_key: Some("old-name::stdio".into()),
            entry: Box::new(RegistryEntry {
                name: "new-name".into(),
                description: "Transport change".into(),
                tags: vec![],
                config: RegistryConfig {
                    stdio: None,
                    http: Some(HttpConfig {
                        kind: "http".into(),
                        url: "https://example.com/mcp".into(),
                        headers: None,
                    }),
                },
                origin: None,
                repo: None,
            }),
        },
    })
    .unwrap_err();
    assert!(transport_error.starts_with("asset_transport_change:"));
    assert!(read_registry()
        .iter()
        .any(|entry| entry.key() == "old-name::stdio"));
    assert!(!read_registry()
        .iter()
        .any(|entry| entry.key() == "new-name::http"));
}

#[test]
fn mcp_rename_rejects_a_stale_catalog_without_partial_migration() {
    let home = TestHome::new("central-mcp-rename-stale");
    write_manual_entry(&named_mcp("old-name", "old-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["old-name::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    let before = fs::read_to_string(&target).unwrap();
    let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp {
            existing_key: Some("old-name::stdio".into()),
            entry: Box::new(named_mcp("new-name", "new-server")),
        },
    })
    .unwrap();

    write_manual_entry(&named_mcp("concurrent-name", "concurrent-server")).unwrap();
    let error = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap_err();

    assert!(error.starts_with("asset_operation_stale:"));
    assert!(read_registry()
        .iter()
        .any(|entry| entry.key() == "old-name::stdio"));
    assert!(!read_registry()
        .iter()
        .any(|entry| entry.key() == "new-name::stdio"));
    let records = &load_settings().mcp_consumptions.unwrap()["claude-code"];
    assert!(records.contains_key("old-name::stdio"));
    assert!(!records.contains_key("new-name::stdio"));
    assert_eq!(fs::read_to_string(target).unwrap(), before);
}

#[test]
fn central_mcp_create_does_not_touch_agent_targets() {
    let home = TestHome::new("central-mcp-create");
    let target = home.home.join(".claude.json");
    let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp {
            existing_key: None,
            entry: Box::new(mcp("central-only")),
        },
    })
    .unwrap();
    assert!(plan.affected_agent_ids.is_empty());
    commit(plan);
    assert!(!target.exists());
    assert!(load_settings().mcp_consumptions.is_none());
}

#[test]
fn drifted_consumer_requires_explicit_convergence_before_central_update() {
    let home = TestHome::new("central-mcp-drift");
    write_manual_entry(&mcp("old-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    let customized = fs::read_to_string(&target)
        .unwrap()
        .replace("old-server", "custom-server");
    fs::write(&target, customized).unwrap();

    let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
        draft: CentralAssetDraft::Mcp {
            existing_key: Some("local::stdio".into()),
            entry: Box::new(mcp("new-server")),
        },
    })
    .unwrap();
    assert!(!plan.can_commit);
    let rejected = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id.clone(),
        candidate_hash: plan.candidate_hash.clone(),
    })
    .unwrap_err();
    assert!(rejected.starts_with("asset_operation_blocked:"));
    assert_eq!(
        read_registry()
            .into_iter()
            .find(|entry| entry.key() == "local::stdio")
            .unwrap()
            .config
            .stdio
            .unwrap()
            .command,
        "old-server"
    );
    assert!(fs::read_to_string(&target)
        .unwrap()
        .contains("custom-server"));

    assert!(fs::read_to_string(target)
        .unwrap()
        .contains("custom-server"));
}

#[test]
fn mcp_reapply_repairs_drift_without_changing_the_central_asset() {
    let home = TestHome::new("central-mcp-reapply");
    write_manual_entry(&mcp("managed-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace("managed-server", "tampered-server");
    fs::write(&target, drifted).unwrap();

    let plan = plan_reapply_mcp(PlanReapplyMcpRequest {
        asset_key: "local::stdio".into(),
        scope: McpReapplyScope::All,
    })
    .unwrap();
    assert!(plan.can_commit);
    assert_eq!(
        plan.central_changes[0].summary,
        vec![
            "重新同步 MCP 配置",
            "将更新 1 个已明确选择的 Agent",
            "中央配置保持不变",
        ]
    );
    commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap();

    let repaired = fs::read_to_string(target).unwrap();
    assert!(repaired.contains("managed-server"));
    assert!(!repaired.contains("tampered-server"));
    assert_eq!(
        read_registry()
            .into_iter()
            .find(|entry| entry.key() == "local::stdio")
            .unwrap()
            .config
            .stdio
            .unwrap()
            .command,
        "managed-server"
    );
}

#[test]
fn mcp_reapply_is_exact_by_default_and_all_repairs_only_drifted_consumers() {
    let home = TestHome::new("central-mcp-reapply-exact-scope");
    write_manual_entry(&mcp("managed-server")).unwrap();
    for agent_id in ["claude-code", "codex"] {
        commit(
            plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
                agent_id: agent_id.into(),
                selection: AgentConsumptionSelection::Mcp {
                    asset_keys: vec!["local::stdio".into()],
                },
            })
            .unwrap(),
        );
    }
    let claude = home.home.join(".claude.json");
    let codex = home.home.join(".codex/config.toml");
    fs::write(
        &claude,
        fs::read_to_string(&claude)
            .unwrap()
            .replace("managed-server", "claude-drift"),
    )
    .unwrap();
    fs::write(
        &codex,
        fs::read_to_string(&codex)
            .unwrap()
            .replace("managed-server", "codex-drift"),
    )
    .unwrap();

    let exact = plan_reapply_mcp(PlanReapplyMcpRequest {
        asset_key: "local::stdio".into(),
        scope: McpReapplyScope::Agent {
            agent_id: "claude-code".into(),
        },
    })
    .unwrap();
    assert_eq!(exact.affected_agent_ids, vec!["claude-code"]);
    commit_asset_operation(AssetCommitRequest {
        operation_id: exact.operation_id,
        candidate_hash: exact.candidate_hash,
    })
    .unwrap();
    assert!(fs::read_to_string(&claude)
        .unwrap()
        .contains("managed-server"));
    assert!(fs::read_to_string(&codex).unwrap().contains("codex-drift"));

    let clean_claude = fs::read(&claude).unwrap();
    let all = plan_reapply_mcp(PlanReapplyMcpRequest {
        asset_key: "local::stdio".into(),
        scope: McpReapplyScope::All,
    })
    .unwrap();
    assert_eq!(all.affected_agent_ids, vec!["codex"]);
    commit_asset_operation(AssetCommitRequest {
        operation_id: all.operation_id,
        candidate_hash: all.candidate_hash,
    })
    .unwrap();
    assert_eq!(fs::read(&claude).unwrap(), clean_claude);
    assert!(fs::read_to_string(codex)
        .unwrap()
        .contains("managed-server"));
}

#[test]
fn mcp_reapply_clean_and_empty_scopes_are_core_noops() {
    let _home = TestHome::new("central-mcp-reapply-noop");
    write_manual_entry(&mcp("managed-server")).unwrap();
    let empty = plan_reapply_mcp(PlanReapplyMcpRequest {
        asset_key: "local::stdio".into(),
        scope: McpReapplyScope::All,
    })
    .unwrap();
    assert!(empty.central_changes.is_empty());
    assert!(empty.target_files.is_empty());
    assert!(empty.affected_agent_ids.is_empty());

    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    let clean = plan_reapply_mcp(PlanReapplyMcpRequest {
        asset_key: "local::stdio".into(),
        scope: McpReapplyScope::Agent {
            agent_id: "claude-code".into(),
        },
    })
    .unwrap();
    assert!(clean.central_changes.is_empty());
    assert!(clean.target_files.is_empty());
    assert!(clean.affected_agent_ids.is_empty());
}

#[test]
fn mcp_reapply_rejects_disabled_agents_in_exact_and_all_scopes_without_writing() {
    let home = TestHome::new("central-mcp-reapply-agent-disabled");
    write_manual_entry(&mcp("managed-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace("managed-server", "disabled-agent-drift");
    fs::write(&target, &drifted).unwrap();
    arca_core::agents::set_enabled("claude-code", false).unwrap();

    for scope in [
        McpReapplyScope::Agent {
            agent_id: "claude-code".into(),
        },
        McpReapplyScope::All,
    ] {
        let error = plan_reapply_mcp(PlanReapplyMcpRequest {
            asset_key: "local::stdio".into(),
            scope,
        })
        .unwrap_err();
        assert!(error.starts_with("agent_disabled:"), "{error}");
        assert_eq!(fs::read_to_string(&target).unwrap(), drifted);
    }
}

#[test]
fn repeated_mcp_enable_is_a_core_noop_even_with_physical_drift() {
    let home = TestHome::new("central-mcp-enabled-core-noop");
    write_manual_entry(&mcp("managed-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace("managed-server", "preserved-drift");
    fs::write(&target, &drifted).unwrap();

    let plan = plan_set_mcp_enabled(PlanSetMcpEnabledRequest {
        agent_id: "claude-code".into(),
        asset_key: "local::stdio".into(),
        enabled: true,
    })
    .unwrap();
    assert!(plan.can_commit);
    assert!(plan.central_changes.is_empty());
    assert!(plan.relationship_changes.is_empty());
    assert!(plan.consumption_state_changes.is_empty());
    assert!(plan.target_files.is_empty());
    commit(plan);
    assert_eq!(fs::read_to_string(target).unwrap(), drifted);
}

#[test]
fn mcp_reapply_preserves_a_disabled_desired_relationship() {
    let home = TestHome::new("central-mcp-reapply-disabled");
    write_manual_entry(&mcp("managed-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    let managed = fs::read_to_string(&target).unwrap();
    commit(
        plan_set_mcp_enabled(PlanSetMcpEnabledRequest {
            agent_id: "claude-code".into(),
            asset_key: "local::stdio".into(),
            enabled: false,
        })
        .unwrap(),
    );

    assert!(!fs::read_to_string(&target)
        .unwrap()
        .contains("managed-server"));

    let clean = plan_reapply_mcp(PlanReapplyMcpRequest {
        asset_key: "local::stdio".into(),
        scope: McpReapplyScope::All,
    })
    .unwrap();
    assert!(clean.central_changes.is_empty());
    assert!(clean.target_files.is_empty());
    commit(clean);

    fs::write(&target, managed).unwrap();
    let repair = plan_reapply_mcp(PlanReapplyMcpRequest {
        asset_key: "local::stdio".into(),
        scope: McpReapplyScope::Agent {
            agent_id: "claude-code".into(),
        },
    })
    .unwrap();
    commit_asset_operation(AssetCommitRequest {
        operation_id: repair.operation_id,
        candidate_hash: repair.candidate_hash,
    })
    .unwrap();

    assert!(!fs::read_to_string(target)
        .unwrap()
        .contains("managed-server"));
    assert!(
        !load_settings().mcp_consumptions.unwrap()["claude-code"]["local::stdio"].enabled,
        "reapply must preserve the desired disabled state"
    );
}

#[test]
fn mcp_reapply_rejects_a_catalog_change_after_review() {
    let home = TestHome::new("central-mcp-reapply-stale-catalog");
    write_manual_entry(&mcp("reviewed-server")).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".claude.json");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace("reviewed-server", "local-customization");
    fs::write(&target, &drifted).unwrap();

    let plan = plan_reapply_mcp(PlanReapplyMcpRequest {
        asset_key: "local::stdio".into(),
        scope: McpReapplyScope::All,
    })
    .unwrap();
    write_manual_entry(&mcp("changed-after-review")).unwrap();
    let error = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap_err();

    assert!(
        error.contains("central MCP catalog changed after review"),
        "{error}"
    );
    assert_eq!(fs::read_to_string(target).unwrap(), drifted);
}

#[test]
fn reviewed_mcp_plan_survives_unrelated_preferences_and_assets() {
    let _home = TestHome::new("central-mcp-semantic-preconditions");
    write_manual_entry(&named_mcp("alpha", "alpha-server")).unwrap();
    let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "claude-code".into(),
        selection: AgentConsumptionSelection::Mcp {
            asset_keys: vec!["alpha::stdio".into()],
        },
    })
    .unwrap();

    mutate_settings(|settings| {
        settings.ui = Some(UiSettings {
            pinned_agents: vec!["codex".into()],
            locale: Some("en-US".into()),
            ..Default::default()
        });
    })
    .unwrap();
    write_manual_entry(&named_mcp("beta", "beta-server")).unwrap();

    let inventory = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap();

    assert!(inventory.consumptions.iter().any(|item| {
        item.agent_id == "claude-code"
            && item.asset
                == AssetRef::Mcp {
                    key: "alpha::stdio".into(),
                }
    }));
    assert_eq!(
        load_settings().ui.and_then(|ui| ui.locale).as_deref(),
        Some("en-US")
    );
}

fn model(model: &str) -> ModelProfile {
    ModelProfile {
        id: "work".into(),
        provider_id: Some("work-provider".into()),
        name: "Work".into(),
        provider: "custom".into(),
        model_vendor: None,
        native_ids: Default::default(),
        protocol: ModelProtocol::OpenaiResponses,
        base_url: "https://example.invalid/v1".into(),
        endpoint_path: String::new(),
        model: model.into(),
        env_key: None,
        context_window: Some(128_000),
        max_output_tokens: Some(8_192),
        reasoning: Some(true),
    }
}

#[test]
fn model_edit_propagates_without_dropping_assignment_and_delete_cascades() {
    let home = TestHome::new("central-model-e2e");
    save_profile(model("old-model"), None).unwrap();
    apply_profile("codex", "work").unwrap();
    let target = home.home.join(".codex/config.toml");
    assert!(fs::read_to_string(&target).unwrap().contains("old-model"));

    commit(
        plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Model {
                existing_id: Some("work".into()),
                profile: Box::new(model("new-model")),
                credential: None,
            },
        })
        .unwrap(),
    );
    let updated = fs::read_to_string(&target).unwrap();
    assert!(updated.contains("new-model"));
    assert!(!updated.contains("old-model"));
    assert_eq!(load_settings().model_assignments.unwrap()["codex"], "work");

    commit(
        plan_delete_central_asset(PlanDeleteCentralAssetRequest {
            asset: AssetRef::Model {
                profile_id: "work".into(),
            },
            source_id: None,
        })
        .unwrap(),
    );
    assert!(list_profiles().is_empty());
    assert!(!load_settings()
        .model_assignments
        .unwrap_or_default()
        .contains_key("codex"));
    let cleared = fs::read_to_string(target).unwrap();
    assert!(!cleared.contains("work"));
    assert!(!cleared.contains("new-model"));
}

#[test]
fn grok_build_consumes_and_switches_central_profiles() {
    let home = TestHome::new("central-model-grok-build");
    let mut responses = model("gpt-custom");
    responses.id = "openai-work".into();
    responses.provider_id = Some("openai-provider".into());
    responses.env_key = Some("OPENAI_WORK_API_KEY".into());
    let mut messages = model("claude-custom");
    messages.id = "anthropic-work".into();
    messages.provider_id = Some("anthropic-provider".into());
    messages.protocol = ModelProtocol::AnthropicMessages;
    messages.env_key = Some("ANTHROPIC_WORK_API_KEY".into());
    save_profile(responses.clone(), None).unwrap();
    save_profile(messages.clone(), None).unwrap();

    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "grok-build".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec![responses.id.clone()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".grok/config.toml");
    let first = fs::read_to_string(&target).unwrap();
    assert!(first.contains("api_backend = \"responses\""));
    assert!(first.contains("env_key = \"OPENAI_WORK_API_KEY\""));

    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "grok-build".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec![messages.id.clone()],
            },
        })
        .unwrap(),
    );
    let switched = fs::read_to_string(target).unwrap();
    assert!(switched.contains("api_backend = \"messages\""));
    assert!(switched.contains("env_key = \"ANTHROPIC_WORK_API_KEY\""));
    assert!(!switched.contains("OPENAI_WORK_API_KEY"));
    assert_eq!(
        load_settings().model_assignments.unwrap()["grok-build"],
        messages.id
    );
}

#[test]
fn grok_build_delete_preserves_an_unmanaged_model_without_failing_verification() {
    let home = TestHome::new("central-model-grok-build-delete-with-external");
    let target = home.home.join(".grok/config.toml");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(
        &target,
        "[models]\ndefault = \"private\"\n\n[model.private]\nmodel = \"keep\"\n",
    )
    .unwrap();

    let mut profile = model("delete-me");
    profile.id = "a".into();
    profile.provider_id = Some("delete-provider".into());
    profile.env_key = Some("DELETE_ME_API_KEY".into());
    save_profile(profile.clone(), None).unwrap();
    apply_profile("grok-build", &profile.id).unwrap();

    commit(
        plan_delete_central_asset(PlanDeleteCentralAssetRequest {
            asset: AssetRef::Model {
                profile_id: profile.id.clone(),
            },
            source_id: None,
        })
        .unwrap(),
    );

    let cleared = fs::read_to_string(target).unwrap();
    assert!(cleared.contains("model.private"));
    assert!(cleared.contains("model = \"keep\""));
    assert!(!cleared.contains(&arca_profile_id(&profile.id)));
    assert!(list_profiles().is_empty());
}

#[test]
fn grok_build_keeps_multiple_profiles_and_falls_back_when_current_is_disabled() {
    let home = TestHome::new("central-model-grok-build-multiple");
    let mut first = model("first-model");
    first.id = "first".into();
    first.provider_id = Some("first-provider".into());
    first.env_key = Some("FIRST_API_KEY".into());
    let mut second = model("second-model");
    second.id = "second".into();
    second.provider_id = Some("second-provider".into());
    second.env_key = Some("SECOND_API_KEY".into());
    save_profile(first.clone(), None).unwrap();
    save_profile(second.clone(), None).unwrap();

    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "grok-build".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec![first.id.clone(), second.id.clone()],
            },
        })
        .unwrap(),
    );
    let initial = load_settings().model_selection("grok-build");
    assert_eq!(initial.profiles.len(), 2);
    let initial_active = initial.active_profile_id.unwrap();
    let switched = if initial_active == first.id {
        second.id.clone()
    } else {
        first.id.clone()
    };
    commit(
        plan_set_active_model(PlanSetActiveModelRequest {
            agent_id: "grok-build".into(),
            profile_id: switched.clone(),
        })
        .unwrap(),
    );

    let disable = plan_set_model_enabled(PlanSetModelEnabledRequest {
        agent_id: "grok-build".into(),
        profile_id: switched.clone(),
        enabled: false,
    })
    .unwrap();
    assert!(disable.consumption_state_changes.iter().any(|change| {
        change.agent_id == "grok-build"
            && change.asset
                == (AssetRef::Model {
                    profile_id: switched.clone(),
                })
            && change.before_enabled
            && !change.after_enabled
            && change.affected_agent_ids == ["grok-build"]
    }));
    commit(disable);
    let disabled = load_settings().model_selection("grok-build");
    assert_eq!(
        disabled.active_profile_id.as_deref(),
        Some(initial_active.as_str())
    );
    assert!(!disabled.profiles[&switched].enabled);
    assert!(disabled.profiles[&initial_active].enabled);
    let target = home.home.join(".grok/config.toml");
    let disabled_config = fs::read_to_string(&target).unwrap();
    let removed_env = if switched == first.id {
        "FIRST_API_KEY"
    } else {
        "SECOND_API_KEY"
    };
    assert!(!disabled_config.contains(removed_env));

    let switch_plan = plan_set_active_model(PlanSetActiveModelRequest {
        agent_id: "grok-build".into(),
        profile_id: switched.clone(),
    })
    .unwrap();
    assert!(switch_plan.model_state_changes.iter().any(|change| {
        change.profile_id == switched
            && change.after.enabled
            && change.after.active
            && !change.before.enabled
            && !change.before.active
    }));
    commit(switch_plan);
    let reenabled = load_settings().model_selection("grok-build");
    assert_eq!(
        reenabled.active_profile_id.as_deref(),
        Some(switched.as_str())
    );
    assert!(reenabled.profiles[&switched].enabled);
    assert!(reenabled.profiles[&initial_active].enabled);
    let reenabled_config = fs::read_to_string(&target).unwrap();
    assert!(reenabled_config.contains("FIRST_API_KEY"));
    assert!(reenabled_config.contains("SECOND_API_KEY"));

    let native = fs::read_to_string(&target).unwrap();
    let switched_marker = format!("default = \"{}\"", arca_profile_id(&switched));
    let initial_marker = format!("default = \"{}\"", arca_profile_id(&initial_active));
    assert!(native.contains(&switched_marker));
    fs::write(&target, native.replace(&switched_marker, &initial_marker)).unwrap();
    reconcile_active_models().unwrap();
    assert_eq!(
        load_settings()
            .model_selection("grok-build")
            .active_profile_id,
        Some(initial_active)
    );
}

#[test]
fn model_switch_preserves_drifted_old_current_and_unrelated_third_profile() {
    let home = TestHome::new("central-model-scoped-conflict-replacement");
    let profiles = ["first", "second", "third"].map(|id| {
        let mut profile = model(&format!("{id}-model"));
        profile.id = id.into();
        profile.provider_id = Some(format!("{id}-provider"));
        profile
    });
    for profile in &profiles {
        save_profile(profile.clone(), None).unwrap();
    }

    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "grok-build".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: profiles.iter().map(|profile| profile.id.clone()).collect(),
            },
        })
        .unwrap(),
    );
    let selection = load_settings().model_selection("grok-build");
    assert_eq!(selection.active_profile_id.as_deref(), Some("first"));

    let target = home.home.join(".grok/config.toml");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace("first-model", "first-reviewed-drift")
        .replace("third-model", "third-unreviewed-drift");
    fs::write(&target, &drifted).unwrap();

    let plan = plan_set_active_model(PlanSetActiveModelRequest {
        agent_id: "grok-build".into(),
        profile_id: "second".into(),
    })
    .unwrap();
    assert!(plan.can_commit, "{:?}", plan.warnings);
    assert!(plan.warnings.is_empty());
    assert!(plan
        .model_state_changes
        .iter()
        .any(|change| change.profile_id == "first"));
    assert!(plan
        .model_state_changes
        .iter()
        .any(|change| change.profile_id == "second"));
    assert!(!plan
        .model_state_changes
        .iter()
        .any(|change| change.profile_id == "third"));

    commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap();

    let switched = fs::read_to_string(target).unwrap();
    assert!(switched.contains("first-reviewed-drift"));
    assert!(!switched.contains("first-model"));
    assert!(switched.contains("third-unreviewed-drift"));
    assert!(!switched.contains("third-model"));
    assert_eq!(
        load_settings()
            .model_selection("grok-build")
            .active_profile_id
            .as_deref(),
        Some("second")
    );
}

#[test]
fn shared_skill_target_expands_agent_intent_and_rejects_partial_asset_selection() {
    let _fixture = SkillsFixture::managed("review-changes");
    let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "codex".into(),
        selection: AgentConsumptionSelection::Skill {
            names: vec!["review-changes".into()],
        },
    })
    .unwrap();
    assert_eq!(
        plan.affected_agent_ids,
        vec!["codex", "copilot-cli", "cursor", "gemini", "opencode"]
    );
    assert_eq!(plan.target_files, vec!["~/.agents/skills/review-changes"]);
    commit(plan);
    assert_eq!(
        load_settings().skill_assignments.unwrap()["review-changes"],
        std::collections::BTreeSet::from(["agents-user".into()])
    );

    let error = arca_core::consumption::plan_set_asset_consumers(
        arca_core::consumption::PlanSetAssetConsumersRequest {
            asset: AssetRef::Skill {
                name: "review-changes".into(),
            },
            agent_ids: vec!["codex".into()],
        },
    )
    .unwrap_err();
    assert!(error.starts_with("skill_shared_target_conflict:"));
}

#[test]
fn shared_skill_toggle_preserves_assignment_and_changes_the_physical_target_once() {
    let fixture = SkillsFixture::managed_on_targets("review-changes", &["agents-user"]);
    let target = fixture.target("agents-user", "review-changes");

    let disable = plan_set_skill_enabled(PlanSetSkillEnabledRequest {
        target_id: None,
        agent_id: "codex".into(),
        name: "review-changes".into(),
        enabled: false,
    })
    .unwrap();
    assert_eq!(
        disable.affected_agent_ids,
        vec!["codex", "copilot-cli", "cursor", "gemini", "opencode"]
    );
    assert_eq!(
        disable.target_files,
        vec!["~/.agents/skills/review-changes"]
    );
    assert_eq!(disable.consumption_state_changes.len(), 1);
    let state = &disable.consumption_state_changes[0];
    assert_eq!(state.agent_id, "codex");
    assert_eq!(
        state.asset,
        AssetRef::Skill {
            name: "review-changes".into()
        }
    );
    assert!(state.before_enabled);
    assert!(!state.after_enabled);
    assert_eq!(
        state.affected_agent_ids,
        vec!["codex", "copilot-cli", "cursor", "gemini", "opencode"]
    );
    assert_eq!(
        state
            .target
            .as_ref()
            .map(|target| (target.target_id.as_str(), target.global_dir.as_str())),
        Some(("agents-user", "~/.agents/skills"))
    );
    commit(disable);

    let settings = load_settings();
    assert!(settings.skill_assignments.as_ref().unwrap()["review-changes"].contains("agents-user"));
    assert!(
        !settings.skill_consumptions.as_ref().unwrap()["review-changes"]["agents-user"].enabled
    );
    assert!(!target.exists());
    let disabled = arca_core::consumption::list_consumption_inventory().unwrap();
    let rows: Vec<_> = disabled
        .consumptions
        .iter()
        .filter(|item| {
            item.asset
                == (AssetRef::Skill {
                    name: "review-changes".into(),
                })
        })
        .collect();
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|item| {
        item.desired
            && !item.observed
            && item.enabled == Some(false)
            && item.status == arca_core::consumption::ConsumptionStatus::Synced
    }));
    let repair = arca_core::skills::plan_repair(arca_core::skills::PlanRepairRequest {
        skill_name: "review-changes".into(),
        repair: arca_core::skills::RepairKind::Target {
            target_id: "agents-user".into(),
        },
    })
    .unwrap_err();
    assert!(matches!(
        repair,
        arca_core::skills::SkillError::Conflict { .. }
    ));

    commit(
        plan_set_skill_enabled(PlanSetSkillEnabledRequest {
            target_id: None,
            agent_id: "cursor".into(),
            name: "review-changes".into(),
            enabled: true,
        })
        .unwrap(),
    );
    let settings = load_settings();
    assert!(settings.skill_assignments.as_ref().unwrap()["review-changes"].contains("agents-user"));
    assert!(settings.skill_consumptions.as_ref().unwrap()["review-changes"]["agents-user"].enabled);
    assert!(target.is_symlink());
    let enabled = arca_core::consumption::list_consumption_inventory().unwrap();
    assert!(enabled
        .consumptions
        .iter()
        .filter(|item| item.asset
            == (AssetRef::Skill {
                name: "review-changes".into(),
            }))
        .all(|item| {
            item.observed
                && item.enabled == Some(true)
                && item.status == arca_core::consumption::ConsumptionStatus::Synced
        }));
}

#[test]
fn claude_skill_plan_reports_one_write_target_and_opencode_as_affected() {
    let _fixture = SkillsFixture::managed("frontend-design");
    let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "claude-code".into(),
        selection: AgentConsumptionSelection::Skill {
            names: vec!["frontend-design".into()],
        },
    })
    .unwrap();

    assert_eq!(plan.affected_agent_ids, vec!["claude-code", "opencode"]);
    assert_eq!(plan.target_files, vec!["~/.claude/skills/frontend-design"]);

    commit(plan);
    assert_eq!(
        load_settings().skill_assignments.unwrap()["frontend-design"],
        std::collections::BTreeSet::from(["claude-user".into()])
    );
}
