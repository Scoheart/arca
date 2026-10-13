#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;

use arca_core::consumption::{
    commit_asset_operation, plan_reapply_mcp, plan_reapply_model, plan_set_agent_consumption,
    plan_update_agent_capabilities, AgentConsumptionSelection, AssetCommitRequest, McpReapplyScope,
    PlanReapplyMcpRequest, PlanReapplyModelRequest, PlanSetAgentConsumptionRequest,
    PlanUpdateAgentCapabilitiesRequest,
};
use arca_core::models::save_profile;
use arca_core::registry::write_manual_entry;
use arca_core::settings::mutate_settings;
use arca_core::testenv::TestHome;
use arca_core::types::{ModelProfile, ModelProtocol, RegistryConfig, RegistryEntry, StdioConfig};

fn commit(plan: arca_core::consumption::AssetOperationPlan) {
    commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap();
}

fn profile() -> ModelProfile {
    ModelProfile {
        id: "parent-safe-model".into(),
        provider_id: Some("parent-safe-provider".into()),
        name: "Parent-safe Model".into(),
        provider: "custom".into(),
        model_vendor: None,
        native_ids: Default::default(),
        protocol: ModelProtocol::OpenaiResponses,
        base_url: "https://example.invalid/v1".into(),
        endpoint_path: String::new(),
        model: "reviewed-model".into(),
        env_key: None,
        context_window: None,
        max_output_tokens: None,
        reasoning: Some(false),
    }
}

fn mcp() -> RegistryEntry {
    RegistryEntry {
        name: "parent-safe".into(),
        description: String::new(),
        tags: Vec::new(),
        config: RegistryConfig {
            stdio: Some(StdioConfig {
                command: "parent-safe-server".into(),
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

fn substitute_parent_with_outside_symlink(parent: &std::path::Path, outside: &std::path::Path) {
    let retained = parent.with_extension("retained-by-test");
    fs::rename(parent, retained).unwrap();
    fs::create_dir(outside).unwrap();
    symlink(outside, parent).unwrap();
}

fn write_external_skill(root: &std::path::Path, name: &str) {
    let skill = root.join(name);
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Parent-safe fixture\n---\n"),
    )
    .unwrap();
}

#[test]
fn model_reapply_rejects_a_reviewed_missing_target_after_parent_becomes_symlink() {
    let home = TestHome::new("model-reapply-parent-symlink");
    save_profile(profile(), None).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "grok-build".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec!["parent-safe-model".into()],
            },
        })
        .unwrap(),
    );

    let target = home.home.join(".grok/config.toml");
    fs::remove_file(&target).unwrap();
    let plan = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: "parent-safe-model".into(),
    })
    .unwrap();
    let outside = home.home.join("outside-model-parent");
    substitute_parent_with_outside_symlink(target.parent().unwrap(), &outside);
    let error = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap_err();

    assert!(
        error.contains("changed after review") || error.contains("asset_target_unsafe"),
        "{error}"
    );
    assert!(!outside.join("config.toml").exists());
}

#[test]
fn mcp_reapply_rejects_a_reviewed_missing_target_after_parent_becomes_symlink() {
    let home = TestHome::new("mcp-reapply-parent-symlink");
    write_manual_entry(&mcp()).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "codex".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["parent-safe::stdio".into()],
            },
        })
        .unwrap(),
    );

    let target = home.home.join(".codex/config.toml");
    fs::remove_file(&target).unwrap();
    let plan = plan_reapply_mcp(PlanReapplyMcpRequest {
        asset_key: "parent-safe::stdio".into(),
        scope: McpReapplyScope::Agent {
            agent_id: "codex".into(),
        },
    })
    .unwrap();
    let outside = home.home.join("outside-mcp-parent");
    substitute_parent_with_outside_symlink(target.parent().unwrap(), &outside);
    let error = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap_err();

    assert!(
        error.contains("changed after review") || error.contains("asset_target_unsafe"),
        "{error}"
    );
    assert!(!outside.join("config.toml").exists());
}

#[test]
fn skill_path_migration_rejects_a_reviewed_missing_destination_after_parent_becomes_symlink() {
    let home = TestHome::new("skill-migration-parent-symlink");
    fs::create_dir_all(home.home.join(".codex")).unwrap();
    write_external_skill(&home.home.join(".agents/skills"), "shared-notes");
    mutate_settings(|settings| {
        settings.skill_assignments = Some(
            [(
                "shared-notes".into(),
                ["agents-user".into()].into_iter().collect(),
            )]
            .into_iter()
            .collect(),
        );
    })
    .unwrap();

    let destination_parent = home.home.join(".codex-private/skills");
    fs::create_dir_all(&destination_parent).unwrap();
    let mut patch = arca_core::application::agents::get_configuration_patch("codex").unwrap();
    patch.skill.as_mut().unwrap().global_dir = "~/.codex-private/skills".into();
    let plan = plan_update_agent_capabilities(PlanUpdateAgentCapabilitiesRequest {
        agent_id: "codex".into(),
        patch,
    })
    .unwrap();
    assert_eq!(
        plan.target_files,
        vec!["~/.codex-private/skills/shared-notes"]
    );

    let outside = home.home.join("outside-skill-migration-parent");
    substitute_parent_with_outside_symlink(&destination_parent, &outside);
    let error = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap_err();

    assert_eq!(
        error,
        "asset_operation_stale: an Agent target changed after review"
    );
    assert!(!outside.join("shared-notes").exists());
}
