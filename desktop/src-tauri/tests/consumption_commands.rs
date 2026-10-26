use desktop_lib::commands;
use arca_core::application::assets::{CentralAssetDraft, PlanUpdateCentralAssetRequest};
use arca_core::application::operations::{
    CancelOperationRequest, CommitOperationRequest, OperationPlan, PlanOperationRequest,
};
use arca_core::application::skills::SkillOperationKind;
use arca_core::domain::types::{RegistryConfig, RegistryEntry, StdioConfig};
use serde_json::json;

#[test]
fn lifecycle_requests_keep_the_tagged_core_wire_shape() {
    let request: PlanUpdateCentralAssetRequest = serde_json::from_value(json!({
        "draft": {
            "domain": "mcp",
            "entry": {
                "name": "local",
                "description": "",
                "tags": [],
                "config": {"stdio": {"command": "local-server"}}
            }
        }
    }))
    .unwrap();
    assert!(matches!(
        request.draft,
        CentralAssetDraft::Mcp {
            existing_key: None,
            ..
        }
    ));

    let unknown = serde_json::from_value::<PlanUpdateCentralAssetRequest>(json!({
        "draft": {
            "domain": "model",
            "profile": {
                "id": "work",
                "name": "Work",
                "protocol": "openai-responses",
                "base_url": "https://example.invalid",
                "model": "model"
            },
            "agent_ids": ["codex"]
        }
    }));
    assert!(
        unknown.is_err(),
        "central drafts must reject hidden Agent selection"
    );
}

#[test]
fn unified_operation_envelopes_match_the_tauri_json_contract() {
    let asset_plan: PlanOperationRequest = serde_json::from_value(json!({
        "operation": "set_agent_consumption",
        "request": {
            "agent_id": "codex",
            "selection": {
                "domain": "mcp",
                "asset_keys": ["local::stdio"]
            }
        }
    }))
    .unwrap();
    assert!(matches!(
        asset_plan,
        PlanOperationRequest::SetAgentConsumption(_)
    ));

    let clear_mcp: PlanOperationRequest = serde_json::from_value(json!({
        "operation": "clear_agent_mcp",
        "request": { "agent_id": "qoder" }
    }))
    .unwrap();
    assert!(matches!(clear_mcp, PlanOperationRequest::ClearAgentMcp(_)));

    let skill_plan: PlanOperationRequest = serde_json::from_value(json!({
        "operation": "assign_skill",
        "request": {
            "skill_name": "review-changes",
            "agent_ids": ["codex"],
            "enabled": true
        }
    }))
    .unwrap();
    assert!(matches!(skill_plan, PlanOperationRequest::AssignSkill(_)));

    let asset_commit: CommitOperationRequest = serde_json::from_value(json!({
        "domain": "asset",
        "request": {
            "operation_id": "asset-operation",
            "candidate_hash": "asset-candidate"
        }
    }))
    .unwrap();
    assert!(matches!(asset_commit, CommitOperationRequest::Asset { .. }));

    let removed_conflict_protocol = serde_json::from_value::<CommitOperationRequest>(json!({
        "domain": "asset",
        "request": {
            "operation_id": "asset-operation",
            "candidate_hash": "asset-candidate",
            "conflict_confirmation": null
        }
    }));
    assert!(
        removed_conflict_protocol.is_err(),
        "removed conflict confirmation fields must not be silently accepted"
    );

    let skill_commit: CommitOperationRequest = serde_json::from_value(json!({
        "domain": "skill",
        "kind": "assignment",
        "request": {
            "operation_id": "skill-operation",
            "candidate_hash": "skill-candidate",
            "findings_confirmation": null
        }
    }))
    .unwrap();
    assert!(matches!(
        skill_commit,
        CommitOperationRequest::Skill {
            kind: SkillOperationKind::Assignment,
            ..
        }
    ));

    let asset_cancel: CancelOperationRequest = serde_json::from_value(json!({
        "domain": "asset",
        "operation_id": "asset-operation"
    }))
    .unwrap();
    assert!(matches!(asset_cancel, CancelOperationRequest::Asset { .. }));

    let skill_cancel: CancelOperationRequest = serde_json::from_value(json!({
        "domain": "skill",
        "operation_id": "skill-operation"
    }))
    .unwrap();
    assert!(matches!(skill_cancel, CancelOperationRequest::Skill { .. }));
}

#[test]
fn lifecycle_command_returns_a_secret_free_review_plan() {
    let home = arca_core::testenv::TestHome::new("tauri-consumption-command");
    let plan = tauri::async_runtime::block_on(commands::plan_operation(
        PlanOperationRequest::UpdateCentralAsset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Mcp {
                existing_key: None,
                entry: Box::new(RegistryEntry {
                    name: "local".into(),
                    description: String::new(),
                    tags: Vec::new(),
                    config: RegistryConfig {
                        stdio: Some(StdioConfig {
                            command: "private-command-sentinel".into(),
                            args: None,
                            env: None,
                            cwd: None,
                        }),
                        http: None,
                    },
                    origin: None,
                    repo: None,
                }),
            },
        }),
    ))
    .unwrap();
    let OperationPlan::Asset { plan } = plan else {
        panic!("MCP lifecycle must return an asset operation");
    };
    let plan = *plan;
    assert_eq!(plan.central_changes.len(), 1);
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("private-command-sentinel"));
    let persisted = std::fs::read_to_string(
        home.home
            .join(".arca/staging/consumption")
            .join(&plan.operation_id)
            .join("plan.json"),
    )
    .unwrap();
    assert!(!persisted.contains("private-command-sentinel"));
    tauri::async_runtime::block_on(commands::cancel_operation(CancelOperationRequest::Asset {
        operation_id: plan.operation_id,
    }))
    .unwrap();
}

#[test]
fn desktop_registers_the_unified_surface_without_legacy_mutations() {
    let source = include_str!("../src/lib.rs");
    for command in [
        "commands::get_workspace_snapshot",
        "commands::list_agent_capabilities",
        "commands::plan_operation",
        "commands::commit_operation",
        "commands::cancel_operation",
    ] {
        assert!(source.contains(command), "missing registration: {command}");
    }
    for command in [
        "commands::plan_set_agent_consumption",
        "commands::plan_set_mcp_enabled",
        "commands::plan_set_skill_enabled",
        "commands::plan_set_model_enabled",
        "commands::plan_set_active_model",
        "commands::plan_set_asset_consumers",
        "commands::plan_update_agent_capabilities",
        "commands::plan_update_central_asset",
        "commands::plan_delete_central_asset",
        "commands::commit_asset_operation",
        "commands::cancel_asset_operation",
    ] {
        assert!(source.contains(command), "missing registration: {command}");
    }
    for legacy in [
        "commands::apply_model_profile",
        "commands::save_model_profile",
        "commands::delete_model_profile",
        "commands::upsert_registry_entry",
        "commands::delete_registry_entry",
        "commands::resync_entry",
        "commands::forget_entry",
        "commands::apply_install",
        "commands::uninstall",
        "commands::disable_mcp",
        "commands::enable_mcp",
        "commands::delete_mcp",
        "commands::plan_skill_assignment",
    ] {
        assert!(
            !source.contains(legacy),
            "legacy Desktop mutation is registered: {legacy}"
        );
    }

    let commands = include_str!("../src/commands.rs");
    for dead_command in [
        "pub fn apply_model_profile",
        "pub fn save_model_profile",
        "pub fn delete_model_profile",
        "pub fn upsert_registry_entry",
        "pub fn delete_registry_entry",
        "pub fn resync_entry",
        "pub fn forget_entry",
        "pub fn apply_install",
        "pub fn uninstall",
        "pub fn disable_mcp",
        "pub fn enable_mcp",
        "pub fn delete_mcp",
        "pub async fn plan_skill_assignment",
    ] {
        assert!(
            !commands.contains(dead_command),
            "dead legacy command is still compiled: {dead_command}"
        );
    }
}
