#![cfg(unix)]

mod support;

use arca_core::consumption::{
    commit_asset_operation, list_consumption_inventory, plan_reapply_model,
    plan_set_agent_consumption, plan_set_asset_consumers, plan_update_asset_consumers,
    AgentConsumptionSelection, AssetCommitRequest, AssetRef, ConsumptionStatus,
    McpConsumptionRecord, PlanReapplyModelRequest, PlanSetAgentConsumptionRequest,
    PlanSetAssetConsumersRequest, PlanUpdateAssetConsumersRequest,
};
use arca_core::models::{apply_profile, save_profile};
use arca_core::ops::install;
use arca_core::r#override::OverridePatch;
use arca_core::registry::write_manual_entry;
use arca_core::settings::{mutate_settings, AgentConfigPathOverride};
use arca_core::testenv::TestHome;
use arca_core::types::{ModelProfile, ModelProtocol, RegistryConfig, RegistryEntry, StdioConfig};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use support::skills::SkillsFixture;

fn central_mcp() {
    write_manual_entry(&RegistryEntry {
        name: "local".into(),
        description: "Local fixture".into(),
        tags: Vec::new(),
        config: RegistryConfig {
            stdio: Some(StdioConfig {
                command: "local-server".into(),
                args: Some(vec!["serve".into()]),
                env: None,
                cwd: None,
            }),
            http: None,
        },
        origin: None,
        repo: None,
    })
    .unwrap();
}

#[test]
fn mcp_inventory_reconciles_desired_and_external_without_writes() {
    let home = TestHome::new("consume-mcp");
    central_mcp();
    install(
        "local",
        "stdio",
        "global",
        &["claude-code".into(), "codex".into()],
        None,
        &HashMap::new(),
    )
    .unwrap();
    mutate_settings(|settings| {
        settings.mcp_consumptions = Some(BTreeMap::from([(
            "claude-code".into(),
            BTreeMap::from([(
                "local::stdio".into(),
                McpConsumptionRecord {
                    asset_key: "local::stdio".into(),
                    enabled: true,
                    overrides: OverridePatch::default(),
                },
            )]),
        )]));
    })
    .unwrap();
    let settings_path = home.home.join(".arca/settings.json");
    let claude_path = home.home.join(".claude.json");
    let codex_path = home.home.join(".codex/config.toml");
    let before = [
        fs::read(&settings_path).unwrap(),
        fs::read(&claude_path).unwrap(),
        fs::read(&codex_path).unwrap(),
    ];

    let first = list_consumption_inventory().unwrap();
    let second = list_consumption_inventory().unwrap();
    assert_eq!(first.revision, second.revision);
    assert_eq!(first.consumptions, second.consumptions);
    assert_eq!(first.external, second.external);
    assert!(first.consumptions.iter().any(|item| {
        item.agent_id == "claude-code"
            && item.asset
                == (AssetRef::Mcp {
                    key: "local::stdio".into(),
                })
            && item.status == ConsumptionStatus::Synced
    }));
    assert!(first.external.iter().any(|item| {
        item.agent_id == "codex"
            && item.reason.as_deref() == Some("mcp_adoptable")
            && item.status == ConsumptionStatus::ExternalAdded
    }));
    assert_eq!(
        before,
        [
            fs::read(settings_path).unwrap(),
            fs::read(claude_path).unwrap(),
            fs::read(codex_path).unwrap(),
        ]
    );
}

#[test]
fn model_assignment_remains_visible_when_target_is_missing() {
    let _home = TestHome::new("consume-model");
    mutate_settings(|settings| {
        settings.model_profiles = Some(BTreeMap::from([(
            "inventory-profile".into(),
            ModelProfile {
                id: "inventory-profile".into(),
                provider_id: None,
                name: "Inventory".into(),
                provider: "custom".into(),
                model_vendor: None,
                native_ids: Default::default(),
                protocol: ModelProtocol::AnthropicMessages,
                base_url: "https://example.invalid".into(),
                endpoint_path: String::new(),
                model: "example".into(),
                env_key: None,
                context_window: None,
                max_output_tokens: None,
                reasoning: Some(false),
            },
        )]));
        settings.model_assignments = Some(BTreeMap::from([(
            "claude-code".into(),
            "inventory-profile".into(),
        )]));
    })
    .unwrap();

    let inventory = list_consumption_inventory().unwrap();
    let model = inventory
        .consumptions
        .iter()
        .find(|item| matches!(&item.asset, AssetRef::Model { .. }))
        .unwrap();
    assert!(model.desired);
    assert!(!model.observed);
    assert_eq!(model.status, ConsumptionStatus::ExternalRemoved);
    assert_eq!(model.reason.as_deref(), Some("model_target_missing"));
}

fn model_profile() -> ModelProfile {
    ModelProfile {
        id: "inventory-profile".into(),
        provider_id: None,
        name: "Inventory".into(),
        provider: "custom".into(),
        model_vendor: None,
        native_ids: Default::default(),
        protocol: ModelProtocol::OpenaiResponses,
        base_url: "https://example.invalid".into(),
        endpoint_path: String::new(),
        model: "example".into(),
        env_key: None,
        context_window: None,
        max_output_tokens: None,
        reasoning: Some(false),
    }
}

#[test]
fn unassigned_model_configuration_requires_explicit_convergence_before_takeover() {
    let home = TestHome::new("consume-model-external");
    let target = home.home.join(".codex/config.toml");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(
        &target,
        "model = \"external-model\"\nmodel_provider = \"external-provider\"\n",
    )
    .unwrap();
    mutate_settings(|settings| {
        settings.model_profiles = Some(BTreeMap::from([(
            "inventory-profile".into(),
            model_profile(),
        )]));
    })
    .unwrap();

    let inventory = list_consumption_inventory().unwrap();
    assert!(inventory.external.iter().any(|item| {
        item.agent_id == "codex"
            && matches!(&item.asset, AssetRef::Model { .. })
            && item.reason.as_deref() == Some("model_external_unmanaged")
    }));
    let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "codex".into(),
        selection: AgentConsumptionSelection::Model {
            profile_ids: vec!["inventory-profile".into()],
        },
    })
    .unwrap();
    assert!(!plan.can_commit);
    assert!(plan
        .warnings
        .iter()
        .any(|warning| warning.contains("model_external_unmanaged")));
    let rejected = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id.clone(),
        candidate_hash: plan.candidate_hash.clone(),
    })
    .unwrap_err();
    assert!(rejected.starts_with("asset_operation_blocked:"));
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "model = \"external-model\"\nmodel_provider = \"external-provider\"\n"
    );

    assert_eq!(
        fs::read_to_string(target).unwrap(),
        "model = \"external-model\"\nmodel_provider = \"external-provider\"\n"
    );
}

#[test]
fn multiple_external_models_keep_distinct_candidate_identities() {
    let home = TestHome::new("consume-model-multiple-external");
    let target = home.home.join(".config/opencode/opencode.json");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(
        target,
        r#"{
  "model": "legacy/active",
  "provider": {
    "legacy": {
      "npm": "@ai-sdk/openai-compatible",
      "options": {"baseURL": "https://openrouter.ai/api/v1", "apiKey": "{env:OPENROUTER_API_KEY}"},
      "models": {"active": {"name": "Active"}, "inactive": {"name": "Inactive"}}
    }
  }
}"#,
    )
    .unwrap();

    let inventory = list_consumption_inventory().unwrap();
    let rows = inventory
        .external
        .iter()
        .filter(|item| item.agent_id == "opencode" && matches!(&item.asset, AssetRef::Model { .. }))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    let ids = rows
        .iter()
        .filter_map(|item| match &item.asset {
            AssetRef::Model { profile_id } => Some(profile_id.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().all(|id| id.starts_with("external-")));
    assert_eq!(
        rows.iter()
            .filter(|item| item
                .available_actions
                .contains(&arca_core::consumption::ConvergenceAction::AdoptObserved))
            .count(),
        1
    );
}

#[test]
fn ambiguous_model_configuration_cannot_be_taken_over() {
    let home = TestHome::new("consume-model-ambiguous");
    let target = home.home.join(".codex/config.toml");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(
        &target,
        "model = \"first\"\nmodel = \"second\"\nmodel_provider = \"external\"\n",
    )
    .unwrap();
    mutate_settings(|settings| {
        settings.model_profiles = Some(BTreeMap::from([(
            "inventory-profile".into(),
            model_profile(),
        )]));
    })
    .unwrap();

    let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "codex".into(),
        selection: AgentConsumptionSelection::Model {
            profile_ids: vec!["inventory-profile".into()],
        },
    })
    .unwrap();
    assert!(!plan.can_commit);
    assert!(plan
        .warnings
        .iter()
        .any(|warning| warning.contains("model_external_conflicted")));
    assert_eq!(
        fs::read_to_string(target).unwrap(),
        "model = \"first\"\nmodel = \"second\"\nmodel_provider = \"external\"\n"
    );
}

#[test]
fn drifted_model_requires_explicit_reapply_while_relationship_assignment_is_a_noop() {
    let home = TestHome::new("consume-model-repair");
    save_profile(model_profile(), None).unwrap();
    apply_profile("codex", "inventory-profile").unwrap();
    let target = home.home.join(".codex/config.toml");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace("model = \"example\"", "model = \"tampered\"");
    fs::write(&target, drifted).unwrap();

    let relationship_plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "codex".into(),
        selection: AgentConsumptionSelection::Model {
            profile_ids: vec!["inventory-profile".into()],
        },
    })
    .unwrap();
    assert!(relationship_plan.target_files.is_empty());
    assert!(relationship_plan.central_changes.is_empty());
    assert!(relationship_plan.relationship_changes.is_empty());
    assert!(fs::read_to_string(&target).unwrap().contains("tampered"));

    let plan = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "codex".into(),
        profile_id: "inventory-profile".into(),
    })
    .unwrap();
    assert!(plan.can_commit);
    assert!(plan
        .warnings
        .iter()
        .any(|warning| warning.contains("model_owned_fields_drift")));

    let inventory = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap();
    assert!(inventory.consumptions.iter().any(|item| {
        item.agent_id == "codex"
            && item.status == ConsumptionStatus::Synced
            && item.asset
                == (AssetRef::Model {
                    profile_id: "inventory-profile".into(),
                })
    }));
    let repaired = fs::read_to_string(target).unwrap();
    assert!(repaired.contains("model = \"example\""));
    assert!(!repaired.contains("tampered"));
}

#[test]
fn idempotent_asset_consumer_edits_never_repair_unrelated_model_drift() {
    let home = TestHome::new("consume-model-idempotent-assets");
    save_profile(model_profile(), None).unwrap();
    let assigned = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "codex".into(),
        selection: AgentConsumptionSelection::Model {
            profile_ids: vec!["inventory-profile".into()],
        },
    })
    .unwrap();
    commit_asset_operation(AssetCommitRequest {
        operation_id: assigned.operation_id,
        candidate_hash: assigned.candidate_hash,
    })
    .unwrap();

    let target = home.home.join(".codex/config.toml");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace("model = \"example\"", "model = \"unrelated-drift\"");
    fs::write(&target, drifted.as_bytes()).unwrap();
    let reviewed_bytes = fs::read(&target).unwrap();

    let assert_noop = |plan: arca_core::consumption::AssetOperationPlan| {
        assert!(plan.can_commit);
        assert!(plan.relationship_changes.is_empty());
        assert!(plan.model_state_changes.is_empty());
        assert!(plan.consumption_state_changes.is_empty());
        assert!(plan.warnings.is_empty());
        assert!(plan.target_files.is_empty());
        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();
        assert_eq!(fs::read(&target).unwrap(), reviewed_bytes);
    };

    assert_noop(
        plan_set_asset_consumers(PlanSetAssetConsumersRequest {
            asset: AssetRef::Model {
                profile_id: "inventory-profile".into(),
            },
            agent_ids: vec!["codex".into()],
        })
        .unwrap(),
    );
    assert_noop(
        plan_update_asset_consumers(PlanUpdateAssetConsumersRequest {
            asset: AssetRef::Model {
                profile_id: "inventory-profile".into(),
            },
            add_agent_ids: vec!["codex".into()],
            remove_agent_ids: Vec::new(),
        })
        .unwrap(),
    );
    assert_noop(
        plan_update_asset_consumers(PlanUpdateAssetConsumersRequest {
            asset: AssetRef::Model {
                profile_id: "inventory-profile".into(),
            },
            add_agent_ids: Vec::new(),
            remove_agent_ids: vec!["claude-code".into()],
        })
        .unwrap(),
    );
}

#[test]
fn model_plan_snapshots_and_writes_the_configured_override_path() {
    let home = TestHome::new("consume-model-custom-path");
    save_profile(model_profile(), None).unwrap();
    mutate_settings(|settings| {
        settings.agent_config_paths = Some(BTreeMap::from([(
            "codex".into(),
            AgentConfigPathOverride {
                model_paths: Some(vec!["~/.custom/codex-model.toml".into()]),
                skills_global_dir: None,
                ..Default::default()
            },
        )]));
    })
    .unwrap();

    let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "codex".into(),
        selection: AgentConsumptionSelection::Model {
            profile_ids: vec!["inventory-profile".into()],
        },
    })
    .unwrap();
    assert_eq!(plan.target_files, ["~/.custom/codex-model.toml"]);
    let inventory = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap();
    assert!(inventory
        .consumptions
        .iter()
        .any(|item| { item.agent_id == "codex" && item.status == ConsumptionStatus::Synced }));
    assert!(home.home.join(".custom/codex-model.toml").is_file());
    assert!(!home.home.join(".codex/config.toml").exists());
}

#[test]
fn shared_skill_target_projects_every_affected_agent() {
    let fixture = SkillsFixture::managed_on_targets("review-changes", &["agents-user"]);
    let inventory = list_consumption_inventory().unwrap();
    let consumers: Vec<_> = inventory
        .consumptions
        .iter()
        .filter(|item| {
            item.asset
                == (AssetRef::Skill {
                    name: "review-changes".into(),
                })
        })
        .map(|item| (item.agent_id.as_str(), item.status.clone()))
        .collect();
    assert!(consumers.contains(&("codex", ConsumptionStatus::Synced)));
    assert!(consumers.contains(&("cursor", ConsumptionStatus::Synced)));
    assert!(consumers.contains(&("gemini", ConsumptionStatus::Synced)));

    fs::remove_file(fixture.target("agents-user", "review-changes")).unwrap();
    let drifted = list_consumption_inventory().unwrap();
    assert!(drifted.consumptions.iter().any(|item| {
        matches!(&item.asset, AssetRef::Skill { name } if name == "review-changes")
            && item.reason.as_deref() == Some("skill_target_missing")
    }));
}
