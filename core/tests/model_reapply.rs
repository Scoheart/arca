#![cfg(unix)]

use std::fs;

use arca_core::consumption::{
    commit_asset_operation, plan_reapply_model, plan_remove_agent_consumption,
    plan_set_active_model, plan_set_agent_consumption, plan_set_model_enabled,
    AgentConsumptionSelection, AssetCommitRequest, AssetRef, ConsumptionStatus,
    PlanReapplyModelRequest, PlanRemoveAgentConsumptionRequest, PlanSetActiveModelRequest,
    PlanSetAgentConsumptionRequest, PlanSetModelEnabledRequest, RelationshipAction,
};
use arca_core::models::save_profile;
use arca_core::settings::{load_settings, mutate_settings};
use arca_core::testenv::TestHome;
use arca_core::types::{ModelProfile, ModelProtocol};

fn profile(id: &str, model: &str) -> ModelProfile {
    ModelProfile {
        id: id.into(),
        provider_id: Some(format!("{id}-provider")),
        name: id.into(),
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

fn setup_two_profiles(test_name: &str) -> (TestHome, String, String) {
    let home = TestHome::new(test_name);
    save_profile(profile("primary", "primary-model"), None).unwrap();
    save_profile(profile("secondary", "secondary-model"), None).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "grok-build".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec!["primary".into(), "secondary".into()],
            },
        })
        .unwrap(),
    );
    let selection = load_settings().model_selection("grok-build");
    let active = selection.active_profile_id.unwrap();
    let inactive = if active == "primary" {
        "secondary".to_string()
    } else {
        "primary".to_string()
    };
    (home, active, inactive)
}

fn setup_unsupported_releasable_profile(test_name: &str) -> (TestHome, String) {
    let home = TestHome::new(test_name);
    let mut primary = profile("primary", "primary-model");
    primary.provider_id = None;
    let mut releasable = profile("releasable", "releasable-model");
    releasable.provider_id = None;
    save_profile(primary, None).unwrap();
    save_profile(releasable.clone(), None).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "opencode".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec!["primary".into(), "releasable".into()],
            },
        })
        .unwrap(),
    );
    commit(
        plan_set_active_model(PlanSetActiveModelRequest {
            agent_id: "opencode".into(),
            profile_id: "primary".into(),
        })
        .unwrap(),
    );

    // Keep a Keychain secret while removing the environment reference from
    // the assigned Profile. The final payload is still the exact one Arca
    // originally wrote, but compatibility now reports model_env_key_required.
    releasable.env_key = Some("RELEASABLE_API_KEY".into());
    save_profile(releasable, Some("test-secret".into())).unwrap();
    mutate_settings(|settings| {
        settings
            .model_profiles
            .as_mut()
            .unwrap()
            .get_mut("releasable")
            .unwrap()
            .env_key = None;
        let mut selection = settings.model_selection("opencode");
        selection.default_delivery = arca_core::domain::assets::ApiKeyDelivery::Env;
        if let Some(record) = selection.profiles.get_mut("releasable") {
            record.credential.delivery = arca_core::domain::assets::ApiKeyDelivery::Env;
        }
        settings.set_model_selection("opencode", selection);
    })
    .unwrap();

    let inventory = arca_core::consumption::list_consumption_inventory().unwrap();
    let row = inventory
        .consumptions
        .iter()
        .find(|row| {
            row.agent_id == "opencode"
                && row.asset
                    == (AssetRef::Model {
                        profile_id: "releasable".into(),
                    })
        })
        .unwrap();
    assert_eq!(row.status, ConsumptionStatus::Unsupported);
    assert_eq!(row.reason.as_deref(), Some("model_env_key_required"));
    assert!(
        fs::read_to_string(home.home.join(".config/opencode/opencode.json"))
            .unwrap()
            .contains("releasable-model")
    );
    (home, "releasable".into())
}

#[test]
fn targeted_reapply_repairs_only_the_requested_profile_and_clean_retry_is_a_noop() {
    let (home, active, inactive) = setup_two_profiles("model-reapply-targeted");
    let target = home.home.join(".grok/config.toml");
    let active_model = format!("{active}-model");
    let inactive_model = format!("{inactive}-model");
    let bytes = fs::read_to_string(&target).unwrap();
    let drifted = bytes
        .replace(&active_model, "requested-drift")
        .replace(&inactive_model, "other-profile-drift");
    assert_ne!(bytes, drifted);
    fs::write(&target, &drifted).unwrap();

    let plan = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: active.clone(),
    })
    .unwrap();
    assert_eq!(
        plan.central_changes
            .iter()
            .map(|change| change.asset.clone())
            .collect::<Vec<_>>(),
        vec![AssetRef::Model {
            profile_id: active.clone()
        }]
    );
    assert_eq!(plan.target_files, vec!["~/.grok/config.toml"]);
    commit(plan);

    let repaired = fs::read_to_string(&target).unwrap();
    assert!(repaired.contains(&active_model), "{repaired}");
    assert!(repaired.contains("other-profile-drift"), "{repaired}");
    assert!(!repaired.contains("requested-drift"), "{repaired}");

    let clean = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: active,
    })
    .unwrap();
    assert!(clean.target_files.is_empty());
    assert!(clean.central_changes.is_empty());
}

#[test]
fn reapply_rejects_an_observed_but_not_desired_current_profile_without_writing() {
    let (home, desired, observed) = setup_two_profiles("model-reapply-active-direction");
    let target = home.home.join(".grok/config.toml");
    let desired_marker = format!("default = \"{}\"", arca_profile_id(&desired));
    let observed_marker = format!("default = \"{}\"", arca_profile_id(&observed));
    let before = fs::read_to_string(&target).unwrap();
    assert!(before.contains(&desired_marker));
    let drifted = before.replace(&desired_marker, &observed_marker);
    fs::write(&target, &drifted).unwrap();

    let error = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: observed,
    })
    .unwrap_err();
    assert!(error.starts_with("model_reapply_active_profile_required:"));
    assert_eq!(fs::read_to_string(&target).unwrap(), drifted);

    let repair = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: desired,
    })
    .unwrap();
    commit(repair);
    assert!(fs::read_to_string(target)
        .unwrap()
        .contains(&desired_marker));
}

#[test]
fn reapply_materializes_the_desired_disabled_state_only_for_an_exact_managed_entry() {
    let (home, _active, inactive) = setup_two_profiles("model-reapply-disabled");
    let target = home.home.join(".grok/config.toml");
    let managed = fs::read_to_string(&target).unwrap();
    let inactive_model = format!("{inactive}-model");
    assert!(managed.contains(&inactive_model));

    commit(
        plan_set_model_enabled(PlanSetModelEnabledRequest {
            agent_id: "grok-build".into(),
            profile_id: inactive.clone(),
            enabled: false,
        })
        .unwrap(),
    );
    assert!(!fs::read_to_string(&target)
        .unwrap()
        .contains(&inactive_model));

    fs::write(&target, &managed).unwrap();
    let repair = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: inactive.clone(),
    })
    .unwrap();
    commit(repair);
    let cleared = fs::read_to_string(&target).unwrap();
    assert!(!cleared.contains(&inactive_model));
    let inventory = arca_core::consumption::list_consumption_inventory().unwrap();
    assert!(inventory.consumptions.iter().any(|row| {
        row.agent_id == "grok-build"
            && row.asset
                == (AssetRef::Model {
                    profile_id: inactive.clone(),
                })
            && row.enabled == Some(false)
            && row.status == ConsumptionStatus::Synced
    }));

    let customized = managed.replace(&inactive_model, "inactive-customization");
    fs::write(&target, &customized).unwrap();
    let error = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: inactive,
    })
    .unwrap_err();
    assert!(error.starts_with("model_reapply_unsafe_clear:"));
    assert_eq!(fs::read_to_string(target).unwrap(), customized);
}

#[test]
fn unsupported_exact_profile_can_be_disabled_without_rematerializing_it() {
    let (home, profile_id) =
        setup_unsupported_releasable_profile("model-unsupported-disable-release");
    let target = home.home.join(".config/opencode/opencode.json");

    let plan = plan_set_model_enabled(PlanSetModelEnabledRequest {
        agent_id: "opencode".into(),
        profile_id: profile_id.clone(),
        enabled: false,
    })
    .unwrap();
    assert!(plan.can_commit, "{:?}", plan.warnings);
    commit(plan);

    let selection = load_settings().model_selection("opencode");
    assert!(!selection.profiles[&profile_id].enabled);
    assert!(!fs::read_to_string(target)
        .unwrap()
        .contains("releasable-model"));
}

#[test]
fn unsupported_disabled_profile_still_cannot_be_enabled() {
    let (home, profile_id) =
        setup_unsupported_releasable_profile("model-unsupported-enable-blocked");
    let target = home.home.join(".config/opencode/opencode.json");
    commit(
        plan_set_model_enabled(PlanSetModelEnabledRequest {
            agent_id: "opencode".into(),
            profile_id: profile_id.clone(),
            enabled: false,
        })
        .unwrap(),
    );
    let before = fs::read(&target).unwrap();

    let error = plan_set_model_enabled(PlanSetModelEnabledRequest {
        agent_id: "opencode".into(),
        profile_id,
        enabled: true,
    })
    .unwrap_err();
    assert!(error.starts_with("model_env_key_required:"), "{error}");
    assert_eq!(fs::read(target).unwrap(), before);
}

#[test]
fn unsupported_exact_profile_can_be_unassigned_without_rematerializing_it() {
    let (home, profile_id) =
        setup_unsupported_releasable_profile("model-unsupported-unassign-release");
    let target = home.home.join(".config/opencode/opencode.json");

    let plan = plan_remove_agent_consumption(PlanRemoveAgentConsumptionRequest {
        agent_id: "opencode".into(),
        selection: AgentConsumptionSelection::Model {
            profile_ids: vec![profile_id.clone()],
        },
    })
    .unwrap();
    assert!(plan.can_commit, "{:?}", plan.warnings);
    assert!(plan.relationship_changes.iter().any(|change| {
        change.agent_id == "opencode"
            && change.asset
                == (AssetRef::Model {
                    profile_id: profile_id.clone(),
                })
            && change.action == RelationshipAction::Remove
    }));
    commit(plan);

    assert!(!load_settings()
        .model_selection("opencode")
        .profiles
        .contains_key(&profile_id));
    assert!(!fs::read_to_string(target)
        .unwrap()
        .contains("releasable-model"));
}

#[test]
fn unsupported_disabled_profile_can_be_explicitly_reapplied_as_a_cleanup() {
    let (home, profile_id) =
        setup_unsupported_releasable_profile("model-unsupported-disabled-reapply");
    let target = home.home.join(".config/opencode/opencode.json");
    mutate_settings(|settings| {
        let mut selection = settings.model_selection("opencode");
        selection.profiles.get_mut(&profile_id).unwrap().enabled = false;
        settings.set_model_selection("opencode", selection);
    })
    .unwrap();

    let plan = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "opencode".into(),
        profile_id: profile_id.clone(),
    })
    .unwrap();
    assert!(plan.can_commit, "{:?}", plan.warnings);
    commit(plan);

    let selection = load_settings().model_selection("opencode");
    assert!(!selection.profiles[&profile_id].enabled);
    assert!(!fs::read_to_string(target)
        .unwrap()
        .contains("releasable-model"));
}

#[test]
fn reapply_rejects_stale_target_bytes_after_review() {
    let home = TestHome::new("model-reapply-stale");
    save_profile(profile("stale", "reviewed-model"), None).unwrap();
    commit(
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "grok-build".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec!["stale".into()],
            },
        })
        .unwrap(),
    );
    let target = home.home.join(".grok/config.toml");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace("reviewed-model", "reviewed-drift");
    fs::write(&target, &drifted).unwrap();
    let plan = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: "stale".into(),
    })
    .unwrap();
    let changed_after_review = drifted.replace("reviewed-drift", "changed-after-review");
    fs::write(&target, &changed_after_review).unwrap();

    let error = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap_err();
    assert!(error.contains("changed after review"), "{error}");
    assert_eq!(fs::read_to_string(target).unwrap(), changed_after_review);
}

#[test]
fn model_reapply_rejects_a_disabled_agent_without_writing() {
    let (home, active, _inactive) = setup_two_profiles("model-reapply-agent-disabled");
    let target = home.home.join(".grok/config.toml");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace(&format!("{active}-model"), "disabled-agent-drift");
    fs::write(&target, &drifted).unwrap();
    arca_core::agents::set_enabled("grok-build", false).unwrap();

    let error = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: active,
    })
    .unwrap_err();
    assert!(error.starts_with("agent_disabled:"), "{error}");
    assert_eq!(fs::read_to_string(target).unwrap(), drifted);
}

#[test]
fn model_reapply_binds_the_reviewed_credential_presence() {
    let (home, active, _inactive) = setup_two_profiles("model-reapply-credential-state");
    let target = home.home.join(".grok/config.toml");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace(&format!("{active}-model"), "credential-state-drift");
    fs::write(&target, &drifted).unwrap();
    let plan = plan_reapply_model(PlanReapplyModelRequest {
        agent_id: "grok-build".into(),
        profile_id: active.clone(),
    })
    .unwrap();

    let saved = std::env::var_os("ARCA_TEST_MODEL_CREDENTIAL_PROFILES");
    std::env::set_var("ARCA_TEST_MODEL_CREDENTIAL_PROFILES", &active);
    let error = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id,
        candidate_hash: plan.candidate_hash,
    })
    .unwrap_err();
    match saved {
        Some(value) => std::env::set_var("ARCA_TEST_MODEL_CREDENTIAL_PROFILES", value),
        None => std::env::remove_var("ARCA_TEST_MODEL_CREDENTIAL_PROFILES"),
    }

    assert!(
        error.contains("credential state changed after review"),
        "{error}"
    );
    assert_eq!(fs::read_to_string(target).unwrap(), drifted);
}

#[test]
fn repeated_model_enable_and_use_are_core_noops_even_with_physical_drift() {
    let (home, active, _inactive) = setup_two_profiles("model-state-core-noop");
    let target = home.home.join(".grok/config.toml");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace(&format!("{active}-model"), "preserved-drift");
    fs::write(&target, &drifted).unwrap();

    for plan in [
        plan_set_model_enabled(PlanSetModelEnabledRequest {
            agent_id: "grok-build".into(),
            profile_id: active.clone(),
            enabled: true,
        })
        .unwrap(),
        plan_set_active_model(PlanSetActiveModelRequest {
            agent_id: "grok-build".into(),
            profile_id: active,
        })
        .unwrap(),
    ] {
        assert!(plan.can_commit);
        assert!(plan.central_changes.is_empty());
        assert!(plan.relationship_changes.is_empty());
        assert!(plan.model_state_changes.is_empty());
        assert!(plan.target_files.is_empty());
        commit(plan);
        assert_eq!(fs::read_to_string(&target).unwrap(), drifted);
    }
}

#[test]
fn use_and_disable_require_reapply_when_the_exact_profile_payload_is_drifted() {
    let (home, _active, inactive) = setup_two_profiles("model-state-change-drift-gate");
    let target = home.home.join(".grok/config.toml");
    let inactive_model = format!("{inactive}-model");
    let drifted = fs::read_to_string(&target)
        .unwrap()
        .replace(&inactive_model, "inactive-owned-fields-drift");
    fs::write(&target, &drifted).unwrap();

    let switch = plan_set_active_model(PlanSetActiveModelRequest {
        agent_id: "grok-build".into(),
        profile_id: inactive.clone(),
    })
    .unwrap();
    assert!(!switch.can_commit);
    assert!(switch.warnings.iter().any(|warning| {
        warning.contains(&format!("model:{inactive}"))
            && warning.ends_with("model_owned_fields_drift")
    }));
    assert_eq!(fs::read_to_string(&target).unwrap(), drifted);

    let disable = plan_set_model_enabled(PlanSetModelEnabledRequest {
        agent_id: "grok-build".into(),
        profile_id: inactive.clone(),
        enabled: false,
    })
    .unwrap();
    assert!(!disable.can_commit);
    assert!(disable.warnings.iter().any(|warning| {
        warning.contains(&format!("model:{inactive}"))
            && warning.ends_with("model_owned_fields_drift")
    }));
    assert_eq!(fs::read_to_string(target).unwrap(), drifted);
}

#[test]
fn enable_requires_reapply_when_a_disabled_profile_is_still_materialized() {
    let (home, _active, inactive) = setup_two_profiles("model-enable-disabled-drift-gate");
    let target = home.home.join(".grok/config.toml");
    let managed = fs::read_to_string(&target).unwrap();
    commit(
        plan_set_model_enabled(PlanSetModelEnabledRequest {
            agent_id: "grok-build".into(),
            profile_id: inactive.clone(),
            enabled: false,
        })
        .unwrap(),
    );
    fs::write(&target, &managed).unwrap();

    let enable = plan_set_model_enabled(PlanSetModelEnabledRequest {
        agent_id: "grok-build".into(),
        profile_id: inactive.clone(),
        enabled: true,
    })
    .unwrap();
    assert!(!enable.can_commit);
    assert!(enable.warnings.iter().any(|warning| {
        warning.contains(&format!("model:{inactive}"))
            && warning.ends_with("model_disabled_state_drift")
    }));
    assert_eq!(fs::read_to_string(target).unwrap(), managed);
}
