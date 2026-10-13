#![cfg(unix)]

mod support;

use arca_core::consumption::{
    cancel_asset_operation, commit_asset_operation, plan_reapply_skill, plan_set_skill_enabled,
    AssetCommitRequest, AssetRef, ConsumptionStatus, PlanReapplySkillRequest,
    PlanSetSkillEnabledRequest,
};
use arca_core::settings::mutate_settings;
use arca_core::skills::hash_tree;
use std::fs;
use std::os::unix::fs::symlink;
use support::skills::{assert_managed_link, managed_record, write_skill, SkillsFixture};

fn commit(
    plan: &arca_core::consumption::AssetOperationPlan,
) -> Result<arca_core::consumption::ConsumptionInventory, String> {
    commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id.clone(),
        candidate_hash: plan.candidate_hash.clone(),
    })
}

#[test]
fn shared_missing_skill_restore_repairs_and_then_becomes_a_clean_noop() {
    let fixture = SkillsFixture::missing_managed_link("review-changes", "agents-user");
    let target = fixture.target("agents-user", "review-changes");
    let central = fixture.central("review-changes");

    let plan = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "review-changes".into(),
    })
    .unwrap();
    assert!(plan.can_commit);
    assert!(plan.relationship_changes.is_empty());
    assert_eq!(plan.target_files, vec!["~/.agents/skills/review-changes"]);
    assert_eq!(
        plan.affected_agent_ids,
        vec!["codex", "copilot-cli", "cursor", "gemini", "opencode"]
    );
    for agent_id in &plan.affected_agent_ids {
        assert!(plan.warnings.contains(&format!(
            "{agent_id} / skill:review-changes: skill_target_missing"
        )));
    }

    let inventory = commit(&plan).unwrap();
    assert_managed_link(target.clone(), central.clone());
    for agent_id in ["codex", "copilot-cli", "cursor", "gemini", "opencode"] {
        let row = inventory
            .consumptions
            .iter()
            .find(|row| {
                row.agent_id == agent_id
                    && row.asset
                        == (AssetRef::Skill {
                            name: "review-changes".into(),
                        })
            })
            .unwrap();
        assert!(row.desired && row.observed);
        assert_eq!(row.status, ConsumptionStatus::Synced);
    }

    let link_before = fs::read_link(&target).unwrap();
    let clean = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "review-changes".into(),
    })
    .unwrap();
    assert!(clean.can_commit);
    assert!(clean.central_changes.is_empty());
    assert!(clean.relationship_changes.is_empty());
    assert!(clean.target_files.is_empty());
    assert!(clean.warnings.is_empty());
    commit(&clean).unwrap();
    assert_eq!(fs::read_link(target).unwrap(), link_before);
}

#[test]
fn skill_reapply_rejects_unassigned_and_every_foreign_target_kind() {
    let unassigned = SkillsFixture::managed("safe");
    let error = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "safe".into(),
    })
    .unwrap_err();
    assert!(error.starts_with("skill_consumption_missing:"), "{error}");
    drop(unassigned);

    let directory = SkillsFixture::missing_managed_link("safe", "agents-user");
    let directory_target = directory.target("agents-user", "safe");
    write_skill(&directory_target, "safe", "Foreign directory");
    let directory_error = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "safe".into(),
    })
    .unwrap_err();
    assert!(
        directory_error.starts_with("skill_reapply_target_unsafe:"),
        "{directory_error}"
    );
    assert!(directory_target.join("SKILL.md").is_file());
    drop(directory);

    let file = SkillsFixture::missing_managed_link("safe", "agents-user");
    let file_target = file.target("agents-user", "safe");
    fs::write(&file_target, b"foreign-file").unwrap();
    let file_error = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "safe".into(),
    })
    .unwrap_err();
    assert!(
        file_error.starts_with("skill_reapply_target_unsafe:"),
        "{file_error}"
    );
    assert_eq!(fs::read(&file_target).unwrap(), b"foreign-file");
    drop(file);

    let link = SkillsFixture::missing_managed_link("safe", "agents-user");
    let link_target = link.target("agents-user", "safe");
    let foreign_destination = link.home.home.join("foreign-missing");
    symlink(&foreign_destination, &link_target).unwrap();
    let link_error = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "safe".into(),
    })
    .unwrap_err();
    assert!(
        link_error.starts_with("skill_reapply_target_unsafe:"),
        "{link_error}"
    );
    assert_eq!(fs::read_link(link_target).unwrap(), foreign_destination);
}

#[test]
fn confirmed_skill_reapply_still_refuses_a_target_that_changed_after_review() {
    let fixture = SkillsFixture::missing_managed_link("safe", "agents-user");
    let target = fixture.target("agents-user", "safe");
    let plan = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "safe".into(),
    })
    .unwrap();
    write_skill(&target, "safe", "Appeared after review");

    let error = commit(&plan).unwrap_err();
    assert_eq!(
        error,
        "asset_operation_stale: an Agent target changed after review"
    );
    assert!(target.join("SKILL.md").is_file());
    cancel_asset_operation(&plan.operation_id).unwrap();
}

#[test]
fn skill_reapply_becomes_stale_when_a_new_agent_joins_the_shared_target() {
    let fixture = SkillsFixture::installed_agents(&["codex"]);
    let central = fixture.central("safe");
    write_skill(&central, "safe", "Managed fixture");
    let content_hash = hash_tree(&central).unwrap();
    mutate_settings(|settings| {
        settings
            .managed_skills
            .get_or_insert_default()
            .insert("safe".into(), managed_record("safe", &content_hash));
        settings.skill_assignments.get_or_insert_default().insert(
            "safe".into(),
            ["agents-user".to_string()].into_iter().collect(),
        );
    })
    .unwrap();
    let target = fixture.target("agents-user", "safe");
    let settings_path = fixture.home.home.join(".arca/settings.json");
    let settings_before = fs::read(&settings_path).unwrap();
    let central_before = fs::read(central.join("SKILL.md")).unwrap();

    let plan = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "safe".into(),
    })
    .unwrap();
    assert_eq!(plan.affected_agent_ids, vec!["codex"]);
    assert!(fs::symlink_metadata(&target).is_err());

    fs::create_dir_all(fixture.home.home.join("Library/Application Support/Cursor")).unwrap();
    let error = commit(&plan).unwrap_err();
    assert_eq!(
        error,
        "asset_operation_stale: Skill target graph changed after review"
    );
    assert!(fs::symlink_metadata(&target).is_err());
    assert_eq!(fs::read(settings_path).unwrap(), settings_before);
    assert_eq!(fs::read(central.join("SKILL.md")).unwrap(), central_before);
    cancel_asset_operation(&plan.operation_id).unwrap();
}

#[test]
fn skill_reapply_enforces_a_disabled_desired_relationship_without_deleting_foreign_data() {
    let fixture = SkillsFixture::managed_on_targets("safe", &["agents-user"]);
    let target = fixture.target("agents-user", "safe");
    let central = fixture.central("safe");
    let disable = plan_set_skill_enabled(PlanSetSkillEnabledRequest {
        target_id: None,
        agent_id: "codex".into(),
        name: "safe".into(),
        enabled: false,
    })
    .unwrap();
    commit(&disable).unwrap();
    symlink(&central, &target).unwrap();

    let plan = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "safe".into(),
    })
    .unwrap();
    commit(&plan).unwrap();
    assert!(fs::symlink_metadata(&target).is_err());

    write_skill(&target, "safe", "Foreign disabled target");
    let error = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "safe".into(),
    })
    .unwrap_err();
    assert!(error.starts_with("skill_reapply_target_unsafe:"), "{error}");
    assert!(target.join("SKILL.md").is_file());
}

#[test]
fn skill_reapply_rejects_a_disabled_agent_without_writing() {
    let fixture = SkillsFixture::missing_managed_link("safe", "agents-user");
    let target = fixture.target("agents-user", "safe");
    arca_core::agents::set_enabled("codex", false).unwrap();

    let error = plan_reapply_skill(PlanReapplySkillRequest {
        agent_id: "codex".into(),
        name: "safe".into(),
    })
    .unwrap_err();
    assert!(error.starts_with("agent_disabled:"), "{error}");
    assert!(fs::symlink_metadata(target).is_err());
}

#[test]
fn repeated_skill_enable_is_a_core_noop_even_when_the_link_is_missing() {
    let fixture = SkillsFixture::missing_managed_link("safe", "agents-user");
    let target = fixture.target("agents-user", "safe");
    let plan = plan_set_skill_enabled(PlanSetSkillEnabledRequest {
        target_id: None,
        agent_id: "codex".into(),
        name: "safe".into(),
        enabled: true,
    })
    .unwrap();

    assert!(plan.can_commit);
    assert!(plan.central_changes.is_empty());
    assert!(plan.relationship_changes.is_empty());
    assert!(plan.consumption_state_changes.is_empty());
    assert!(plan.target_files.is_empty());
    commit(&plan).unwrap();
    assert!(fs::symlink_metadata(target).is_err());
}
