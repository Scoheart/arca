use desktop_lib::commands;
use arca_core::skills::{
    PlanInstallRequest, PlanRepairRequest, RepairKind, SkillCommitRequest, SkillError,
};
use serde_json::{json, Value};

fn serialized_error(error: SkillError) -> Value {
    serde_json::to_value(commands::SkillCommandError::from(error)).unwrap()
}

#[test]
fn skill_errors_serialize_as_stable_path_free_envelopes() {
    let plan_stale = serialized_error(SkillError::PlanStale {
        message: "target changed".into(),
    });
    assert_eq!(
        plan_stale,
        json!({"code": "plan_stale", "message": "target changed"})
    );

    let network = serialized_error(SkillError::Network {
        message: "rate limited".into(),
        retry_at: Some("Thu, 17 Jul 2026 12:00:00 GMT".into()),
    });
    assert_eq!(network["code"], "network");
    assert_eq!(network["retry_at"], "Thu, 17 Jul 2026 12:00:00 GMT");
    assert!(network.get("findings_hash").is_none());

    let confirmation = serialized_error(SkillError::ConfirmationRequired {
        message: "review findings".into(),
        findings_hash: "findings-v1".into(),
    });
    assert_eq!(confirmation["code"], "confirmation_required");
    assert_eq!(confirmation["findings_hash"], "findings-v1");
    assert!(confirmation.get("retry_at").is_none());

    for error in [
        SkillError::InvalidManifest {
            message: "bad manifest".into(),
            path: "/secret/manifest-sentinel".into(),
        },
        SkillError::UnsafePath {
            message: "unsafe entry".into(),
            path: "/secret/unsafe-sentinel".into(),
        },
        SkillError::Conflict {
            message: "target changed".into(),
            path: "/secret/conflict-sentinel".into(),
        },
        SkillError::Io {
            message: "read failed".into(),
            path: Some("/secret/io-sentinel".into()),
        },
    ] {
        let serialized = serialized_error(error);
        assert!(!serialized.to_string().contains("/secret/"));
        assert!(serialized.get("path").is_none());
    }

    assert_eq!(
        serialized_error(SkillError::LimitExceeded {
            limit: "file_count".into(),
            actual: 10_001,
            allowed: 10_000,
        }),
        json!({
            "code": "limit_exceeded",
            "message": "file_count limit exceeded: 10001 > 10000"
        })
    );
}

#[test]
fn every_core_skill_error_variant_has_a_stable_command_code() {
    let cases = [
        (
            SkillError::InvalidManifest {
                message: "manifest".into(),
                path: "hidden".into(),
            },
            "invalid_manifest",
        ),
        (
            SkillError::UnsafePath {
                message: "unsafe".into(),
                path: "hidden".into(),
            },
            "unsafe_path",
        ),
        (
            SkillError::LimitExceeded {
                limit: "bytes".into(),
                actual: 2,
                allowed: 1,
            },
            "limit_exceeded",
        ),
        (
            SkillError::InvalidSource {
                message: "source".into(),
            },
            "invalid_source",
        ),
        (
            SkillError::Network {
                message: "network".into(),
                retry_at: None,
            },
            "network",
        ),
        (
            SkillError::Conflict {
                message: "conflict".into(),
                path: "hidden".into(),
            },
            "conflict",
        ),
        (
            SkillError::PlanStale {
                message: "stale".into(),
            },
            "plan_stale",
        ),
        (
            SkillError::ConfirmationRequired {
                message: "confirm".into(),
                findings_hash: "hash".into(),
            },
            "confirmation_required",
        ),
        (
            SkillError::RecoveryRequired {
                message: "recover".into(),
            },
            "recovery_required",
        ),
        (
            SkillError::Io {
                message: "io".into(),
                path: None,
            },
            "io",
        ),
    ];

    for (error, expected_code) in cases {
        assert_eq!(error.into_command_parts().code, expected_code);
    }
}

#[test]
fn nested_skill_requests_keep_the_core_snake_case_wire_shape() {
    let install: PlanInstallRequest = serde_json::from_value(json!({
        "resolution_id": "resolve-id",
        "skill_names": ["review-changes"],
        "agent_ids": ["codex"],
        "replace_conflicts": false
    }))
    .unwrap();
    assert_eq!(install.resolution_id, "resolve-id");
    assert_eq!(install.agent_ids, vec!["codex"]);

    let repair: PlanRepairRequest = serde_json::from_value(json!({
        "skill_name": "review-changes",
        "repair": {"kind": "target", "target_id": "agents-user"}
    }))
    .unwrap();
    assert_eq!(
        repair.repair,
        RepairKind::Target {
            target_id: "agents-user".into()
        }
    );

    let commit: SkillCommitRequest = serde_json::from_value(json!({
        "operation_id": "plan-id",
        "candidate_hash": "candidate-hash",
        "findings_confirmation": null
    }))
    .unwrap();
    assert_eq!(commit.operation_id, "plan-id");
}

#[test]
fn inventory_command_runs_async_inside_an_isolated_test_home() {
    let th = arca_core::testenv::TestHome::new("tauri-skills-command");
    std::fs::create_dir_all(th.home.join(".codex")).unwrap();

    let inventory = tauri::async_runtime::block_on(commands::list_skills_inventory()).unwrap();
    assert_eq!(
        inventory
            .agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>(),
        vec!["codex"]
    );

    let agents = tauri::async_runtime::block_on(commands::list_skill_agents()).unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, "codex");
}

#[test]
fn main_window_starts_hidden_without_changing_existing_window_contracts() {
    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let config: Value = serde_json::from_slice(&std::fs::read(config_path).unwrap()).unwrap();
    let window = &config["app"]["windows"][0];
    assert_eq!(window["visible"], false);
    assert_eq!(window["width"], 1200);
    assert_eq!(window["height"], 820);
    assert_eq!(window["minWidth"], 900);
    assert_eq!(window["minHeight"], 600);
    assert_eq!(window["center"], true);
    assert_eq!(window["dragDropEnabled"], false);
}

#[test]
fn startup_degrades_before_showing_and_checks_skill_updates_only_when_allowed() {
    let source = include_str!("../src/lib.rs");
    let bootstrap = source
        .find("arca_core::application::ArcaCore::bootstrap(")
        .expect("startup must delegate recovery to the core bootstrap");
    let conditional_check = source
        .find("if bootstrap.skill_updates_allowed {")
        .expect("due checking must be conditional on successful recovery");
    let due_check = source
        .find("arca_core::application::skills::check_updates_if_due()")
        .expect("startup must schedule the metadata-only due check");
    let show = source
        .find("window.show()?")
        .expect("startup must still show the initialized main window");

    assert!(bootstrap < conditional_check);
    assert!(conditional_check < due_check);
    assert!(due_check < show);

    let bootstrap_source = include_str!("../../../core/src/application/bootstrap.rs");
    let skill_recovery = bootstrap_source
        .find("classify_skill_recovery(crate::resources::skill::recover_pending()")
        .expect("bootstrap must classify Skills recovery without hiding other reads");
    let asset_recovery = bootstrap_source
        .find("crate::assets::recover_pending_asset_operations()")
        .expect("bootstrap must recover asset operations after Skills observation");
    let model_profile_migration = bootstrap_source
        .find("crate::assets::migrate_model_profiles_v2_if_needed")
        .expect("bootstrap must migrate central Model Profiles after recovery");
    let model_provider_migration = bootstrap_source
        .find("crate::resources::model::migrate_model_providers_v3_if_needed")
        .expect("bootstrap must migrate central Model Providers after Profiles");
    assert!(skill_recovery < asset_recovery);
    assert!(asset_recovery < model_profile_migration);
    assert!(model_profile_migration < model_provider_migration);
}
