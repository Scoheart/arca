#![cfg(unix)]

mod support;

use arca_core::settings::{load_settings, mutate_settings};
use arca_core::skills::{
    commit_install, hash_tree, plan_install, resolve_source, PlanInstallRequest, SkillError,
    SkillSource, SkillSourceInput, SkillsPaths,
};
use serde_json::json;
use std::fs;
use std::os::unix::fs::symlink;
use support::skills::{assert_managed_link, MockGithub, SkillsFixture, FIXTURE_SHA};

#[test]
fn workbuddy_editions_assign_distinct_user_skill_targets() {
    let fixture = SkillsFixture::installed_agents(&["workbuddy", "workbuddy-cn"]);
    let resolution = fixture.resolve_local(&["alpha"]);
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["alpha".into()],
        agent_ids: vec!["workbuddy".into(), "workbuddy-cn".into()],
        replace_conflicts: false,
    }).unwrap();
    let targets = plan.targets.iter().map(|target| target.target_id.as_str()).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(targets, std::collections::BTreeSet::from(["workbuddy-ai-user", "workbuddy-cn-user"]));
    commit_install(plan.confirmation()).unwrap();
    for target in ["workbuddy-ai-user", "workbuddy-cn-user"] {
        assert_managed_link(fixture.agent_target(target, "alpha"), fixture.central("alpha"));
    }
}

#[test]
fn workbuddy_links_only_its_user_target_and_preserves_marketplace_metadata() {
    let fixture = SkillsFixture::installed_agents(&["workbuddy"]);
    let resolution = fixture.resolve_local(&["alpha"]);
    let target = fixture.agent_target("workbuddy-ai-user", "alpha");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let metadata = target.parent().unwrap().join("_bm_skillid_migration.json");
    fs::write(&metadata, "{\"keep\":true}").unwrap();
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["alpha".into()],
        agent_ids: vec!["workbuddy".into()],
        replace_conflicts: false,
    }).unwrap();
    assert_eq!(plan.targets.len(), 1);
    assert_eq!(plan.targets[0].target_id, "workbuddy-ai-user");
    commit_install(plan.confirmation()).unwrap();
    assert_managed_link(target, fixture.central("alpha"));
    assert_eq!(fs::read_to_string(metadata).unwrap(), "{\"keep\":true}");
}

#[test]
fn install_plan_is_read_only_and_commit_installs_one_copy_with_minimal_links() {
    let fixture = SkillsFixture::installed_agents(&["codex", "cursor", "gemini"]);
    let resolution = fixture.resolve_local(&["alpha", "beta"]);
    let before = fixture.snapshot();
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["alpha".into(), "beta".into()],
        agent_ids: vec!["codex".into(), "cursor".into()],
        replace_conflicts: false,
    })
    .unwrap();
    assert_eq!(fixture.snapshot(), before);
    assert_eq!(
        plan.targets
            .iter()
            .map(|row| row.target_id.as_str())
            .collect::<Vec<_>>(),
        vec!["agents-user"]
    );
    assert_eq!(
        plan.targets[0].affected_agent_ids,
        vec!["codex", "cursor", "gemini"]
    );

    commit_install(plan.confirmation()).unwrap();
    assert!(fixture.central("alpha").join("SKILL.md").exists());
    assert_managed_link(
        fixture.agent_target("agents-user", "alpha"),
        fixture.central("alpha"),
    );
    assert!(!fixture.agent_target("cursor-user", "alpha").exists());
}

#[test]
fn stale_plan_and_high_risk_without_bound_confirmation_are_rejected() {
    let fixture = SkillsFixture::installed_agents(&["claude-code"]);
    let plan = fixture.plan_risky_install();
    fixture.change_target_after_plan();
    let stale_result = commit_install(plan.confirmation());
    assert!(
        matches!(stale_result, Err(SkillError::PlanStale { .. })),
        "unexpected stale commit result: {stale_result:?}"
    );

    fixture.create_real_target("claude-user", "risky");
    std::fs::remove_dir_all(fixture.target("claude-user", "risky")).unwrap();
    let fresh = fixture.plan_risky_install();
    assert!(matches!(
        commit_install(fresh.confirmation()),
        Err(SkillError::ConfirmationRequired { .. })
    ));
    assert!(commit_install(fresh.high_risk_confirmation()).is_ok());
}

#[test]
fn install_with_no_agents_is_central_only_and_duplicate_selections_are_rejected() {
    let fixture = SkillsFixture::installed_agents(&[]);
    let resolution = fixture.resolve_local(&["central-only"]);
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["central-only".into()],
        agent_ids: Vec::new(),
        replace_conflicts: false,
    })
    .unwrap();
    assert!(plan.targets.is_empty());
    let transient_backup = SkillsPaths::from_env()
        .unwrap()
        .backups_skills_dir()
        .join(format!("{}-central-central-only", plan.operation_id));
    assert!(transient_backup.parent().unwrap().is_dir());
    assert!(!transient_backup.exists());
    commit_install(plan.confirmation()).unwrap();
    assert!(fixture.central("central-only").join("SKILL.md").exists());
    assert!(!transient_backup.exists());
    assert!(!load_settings()
        .skill_assignments
        .as_ref()
        .is_some_and(|assignments| assignments.contains_key("central-only")));

    let duplicate_skills = fixture.resolve_local(&["duplicate-skill"]);
    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: duplicate_skills.operation_id,
            skill_names: vec!["duplicate-skill".into(), "duplicate-skill".into()],
            agent_ids: Vec::new(),
            replace_conflicts: false,
        }),
        Err(SkillError::InvalidSource { .. })
    ));

    fs::create_dir_all(fixture.home.home.join(".codex")).unwrap();
    let duplicate_agents = fixture.resolve_local(&["duplicate-agent"]);
    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: duplicate_agents.operation_id,
            skill_names: vec!["duplicate-agent".into()],
            agent_ids: vec!["codex".into(), "codex".into()],
            replace_conflicts: false,
        }),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn replacement_install_preserves_prior_managed_links_and_consumption() {
    let fixture = SkillsFixture::managed_on_targets("replace-safe", &["cursor-user"]);
    let old_hash = hash_tree(&fixture.central("replace-safe")).unwrap();
    let old_source = load_settings().managed_skills.unwrap()["replace-safe"]
        .source
        .clone();
    let resolution = fixture.resolve_local(&["replace-safe"]);
    let old_link = fixture.target("cursor-user", "replace-safe");

    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: resolution.operation_id.clone(),
            skill_names: vec!["replace-safe".into()],
            agent_ids: Vec::new(),
            replace_conflicts: false,
        }),
        Err(SkillError::Conflict { .. })
    ));

    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["replace-safe".into()],
        agent_ids: Vec::new(),
        replace_conflicts: true,
    })
    .unwrap();
    assert_eq!(plan.skills[0].existing_source.as_ref(), Some(&old_source));
    assert_ne!(plan.skills[0].source, old_source);
    let wire = serde_json::to_value(&plan).unwrap();
    assert_eq!(
        wire["skills"][0]["existing_source"],
        serde_json::to_value(&plan.skills[0].existing_source).unwrap()
    );
    assert!(plan.targets.is_empty());
    let transient_backup = SkillsPaths::from_env()
        .unwrap()
        .backups_skills_dir()
        .join(format!("{}-central-replace-safe", plan.operation_id));
    assert!(transient_backup.parent().unwrap().is_dir());
    assert!(!transient_backup.exists());

    commit_install(plan.confirmation()).unwrap();
    assert!(transient_backup.exists());
    assert_eq!(hash_tree(&transient_backup).unwrap(), old_hash);
    assert_managed_link(old_link, fixture.central("replace-safe"));
    assert_eq!(load_settings().skill_assignments.unwrap()["replace-safe"],
        ["cursor-user".into()].into());
    assert!(fixture.central("replace-safe").join("SKILL.md").exists());
}

#[test]
fn central_only_reinstall_preserves_unrecorded_exact_target_link() {
    let fixture = SkillsFixture::managed("unrecorded-central");
    let cursor_link = fixture.target("cursor-user", "unrecorded-central");
    fs::create_dir_all(cursor_link.parent().unwrap()).unwrap();
    symlink(fixture.central("unrecorded-central"), &cursor_link).unwrap();
    let resolution = fixture.resolve_local(&["unrecorded-central"]);

    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["unrecorded-central".into()],
        agent_ids: Vec::new(),
        replace_conflicts: true,
    })
    .unwrap();

    assert!(plan.targets.is_empty());
    commit_install(plan.confirmation()).unwrap();
    assert_managed_link(cursor_link, fixture.central("unrecorded-central"));
}

#[test]
fn reinstall_adds_requested_target_without_removing_unrecorded_links() {
    let fixture = SkillsFixture::managed("unrecorded-normalized");
    let cursor_link = fixture.target("cursor-user", "unrecorded-normalized");
    fs::create_dir_all(cursor_link.parent().unwrap()).unwrap();
    symlink(fixture.central("unrecorded-normalized"), &cursor_link).unwrap();
    let resolution = fixture.resolve_local(&["unrecorded-normalized"]);

    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["unrecorded-normalized".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: true,
    })
    .unwrap();

    assert_eq!(
        plan.targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect::<Vec<_>>(),
        vec!["agents-user"]
    );
    commit_install(plan.confirmation()).unwrap();
    assert_managed_link(
        fixture.target("agents-user", "unrecorded-normalized"),
        fixture.central("unrecorded-normalized"),
    );
    assert_managed_link(cursor_link, fixture.central("unrecorded-normalized"));
}

#[test]
fn central_replacement_preserves_enabled_and_disabled_consumers() {
    let name = "replace-consumers";
    let fixture = SkillsFixture::managed_on_targets(name, &["claude-user", "cursor-user"]);
    let disabled = fixture.target("cursor-user", name);
    fs::remove_file(&disabled).unwrap();
    mutate_settings(|settings| {
        for (target, enabled) in [("claude-user", true), ("cursor-user", false)] {
            settings.skill_consumptions.get_or_insert_default().entry(name.into()).or_default().insert(
                target.into(), arca_core::domain::assets::SkillConsumptionRecord {
                    name: name.into(), target_id: target.into(), enabled,
                });
        }
    }).unwrap();
    let before = load_settings();
    let resolution = fixture.resolve_local(&[name]);
    let plan = arca_core::skills::plan_asset_install(arca_core::skills::PlanSkillAssetInstallRequest {
        resolution_id: resolution.operation_id, skill_names: vec![name.into()], replace_conflicts: true,
    }).unwrap();
    assert!(plan.targets.is_empty());
    commit_install(plan.confirmation()).unwrap();
    let after = load_settings();
    assert_eq!(after.skill_assignments, before.skill_assignments);
    assert_eq!(after.skill_consumptions, before.skill_consumptions);
    assert_managed_link(fixture.target("claude-user", name), fixture.central(name));
    assert!(fs::symlink_metadata(disabled).is_err());
}

#[test]
fn settings_hash_ignores_unrelated_fields_but_rejects_skill_section_changes() {
    let fixture = SkillsFixture::installed_agents(&["codex"]);
    let resolution = fixture.resolve_local(&["unrelated"]);
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["unrelated".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: false,
    })
    .unwrap();
    mutate_settings(|settings| {
        settings
            .extra
            .insert("future".into(), json!({"kept": true}));
        settings.model_assignments = Some([("codex".into(), "profile".into())].into());
    })
    .unwrap();
    commit_install(plan.confirmation()).unwrap();
    assert_eq!(
        load_settings().extra.get("future"),
        Some(&json!({"kept": true}))
    );

    let resolution = fixture.resolve_local(&["skills-stale"]);
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["skills-stale".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: false,
    })
    .unwrap();
    mutate_settings(|settings| {
        settings.skill_update_checked_at = Some("2026-07-17T02:00:00Z".into());
    })
    .unwrap();
    assert!(matches!(
        commit_install(plan.confirmation()),
        Err(SkillError::PlanStale { .. })
    ));
}

#[test]
fn candidate_changes_and_forged_candidate_hashes_are_rejected_without_mutation() {
    let fixture = SkillsFixture::installed_agents(&["codex"]);
    let resolution = fixture.resolve_local(&["candidate-stale"]);
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["candidate-stale".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: false,
    })
    .unwrap();
    let before = fixture.snapshot();
    let staged = SkillsPaths::from_env()
        .unwrap()
        .staging_skills_dir()
        .join(&plan.operation_id)
        .join("candidates/candidate-stale/SKILL.md");
    fs::write(
        staged,
        "---\nname: candidate-stale\ndescription: changed after plan\n---\n",
    )
    .unwrap();
    assert!(matches!(
        commit_install(plan.confirmation()),
        Err(SkillError::PlanStale { .. })
    ));
    assert_eq!(fixture.snapshot(), before);

    let resolution = fixture.resolve_local(&["forged-hash"]);
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["forged-hash".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: false,
    })
    .unwrap();
    let mut request = plan.confirmation();
    request.candidate_hash = "0".repeat(64);
    assert!(matches!(
        commit_install(request),
        Err(SkillError::PlanStale { .. })
    ));
    assert!(!fixture.central("forged-hash").exists());
}

#[test]
fn commit_rejects_a_target_root_created_or_swapped_after_review() {
    let fixture = SkillsFixture::installed_agents(&["codex"]);
    let resolution = fixture.resolve_local(&["root-stale"]);
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["root-stale".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: false,
    })
    .unwrap();
    let target_root = fixture.home.home.join(".agents/skills");
    assert!(!target_root.exists());
    fs::create_dir_all(&target_root).unwrap();

    let result = commit_install(plan.confirmation());
    assert!(matches!(&result, Err(SkillError::PlanStale { .. })));
    let message = format!("{:?}", result.unwrap_err());
    assert!(!message.contains(fixture.home.home.to_string_lossy().as_ref()));
    assert!(!fixture.central("root-stale").exists());
    assert!(!target_root.join("root-stale").exists());
    assert!(!load_settings()
        .managed_skills
        .as_ref()
        .is_some_and(|skills| skills.contains_key("root-stale")));
}

#[test]
fn high_risk_confirmation_must_equal_the_canonical_findings_hash() {
    let fixture = SkillsFixture::installed_agents(&["claude-code"]);
    let plan = fixture.plan_risky_install();
    let mut forged = plan.confirmation();
    forged.findings_confirmation = Some("f".repeat(64));
    assert!(matches!(
        commit_install(forged),
        Err(SkillError::ConfirmationRequired { ref findings_hash, .. })
            if findings_hash == &plan.findings_hash
    ));
    commit_install(plan.high_risk_confirmation()).unwrap();
}

#[test]
fn real_directory_unknown_and_broken_targets_remain_hard_conflicts() {
    let fixture = SkillsFixture::installed_agents(&["cursor"]);

    let directory = fixture.resolve_local(&["directory-conflict"]);
    fixture.create_real_target("cursor-user", "directory-conflict");
    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: directory.operation_id.clone(),
            skill_names: vec!["directory-conflict".into()],
            agent_ids: vec!["cursor".into()],
            replace_conflicts: false,
        }),
        Err(SkillError::Conflict { .. })
    ));
    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: directory.operation_id,
            skill_names: vec!["directory-conflict".into()],
            agent_ids: vec!["cursor".into()],
            replace_conflicts: true,
        }),
        Err(SkillError::Conflict { .. })
    ));
    assert!(fixture.target("cursor-user", "directory-conflict").is_dir());

    let unknown = fixture.resolve_local(&["unknown-conflict"]);
    let outside = fixture.home.home.join("outside-unknown");
    fs::create_dir_all(&outside).unwrap();
    let unknown_target = fixture.target("cursor-user", "unknown-conflict");
    fs::create_dir_all(unknown_target.parent().unwrap()).unwrap();
    symlink(&outside, &unknown_target).unwrap();
    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: unknown.operation_id.clone(),
            skill_names: vec!["unknown-conflict".into()],
            agent_ids: vec!["cursor".into()],
            replace_conflicts: false,
        }),
        Err(SkillError::Conflict { .. })
    ));
    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: unknown.operation_id,
            skill_names: vec!["unknown-conflict".into()],
            agent_ids: vec!["cursor".into()],
            replace_conflicts: true,
        }),
        Err(SkillError::Conflict { .. })
    ));
    assert_eq!(fs::read_link(&unknown_target).unwrap(), outside);

    let broken = fixture.resolve_local(&["broken-conflict"]);
    let broken_destination = fixture.home.home.join("missing-link-target");
    let broken_target = fixture.target("cursor-user", "broken-conflict");
    symlink(&broken_destination, &broken_target).unwrap();
    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: broken.operation_id.clone(),
            skill_names: vec!["broken-conflict".into()],
            agent_ids: vec!["cursor".into()],
            replace_conflicts: false,
        }),
        Err(SkillError::Conflict { .. })
    ));
    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: broken.operation_id,
            skill_names: vec!["broken-conflict".into()],
            agent_ids: vec!["cursor".into()],
            replace_conflicts: true,
        }),
        Err(SkillError::Conflict { .. })
    ));
    assert_eq!(fs::read_link(broken_target).unwrap(), broken_destination);
}

#[test]
fn planning_is_blocked_by_pending_recovery_without_touching_reviewed_state() {
    let fixture = SkillsFixture::installed_agents(&["codex"]);
    let resolution = fixture.resolve_local(&["recovery-blocked"]);
    let before = fixture.snapshot();
    let paths = SkillsPaths::from_env().unwrap();
    fs::write(
        paths
            .journals_skills_dir()
            .join("20000000-0000-4000-8000-000000000007.json"),
        "pending",
    )
    .unwrap();

    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: resolution.operation_id,
            skill_names: vec!["recovery-blocked".into()],
            agent_ids: vec!["codex".into()],
            replace_conflicts: false,
        }),
        Err(SkillError::RecoveryRequired { .. })
    ));
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn staged_resolution_and_persisted_plan_reject_unknown_nested_fields() {
    let fixture = SkillsFixture::installed_agents(&["codex"]);
    let resolution = fixture.resolve_local(&["strict-resolution"]);
    let paths = SkillsPaths::from_env().unwrap();
    let resolution_path = paths
        .staging_skills_dir()
        .join(&resolution.operation_id)
        .join("resolution.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&resolution_path).unwrap()).unwrap();
    document["candidates"][0]["unexpected"] = json!(true);
    fs::write(&resolution_path, serde_json::to_vec(&document).unwrap()).unwrap();
    assert!(matches!(
        plan_install(PlanInstallRequest {
            resolution_id: resolution.operation_id,
            skill_names: vec!["strict-resolution".into()],
            agent_ids: vec!["codex".into()],
            replace_conflicts: false,
        }),
        Err(SkillError::InvalidSource { .. })
    ));

    let resolution = fixture.resolve_local(&["strict-plan"]);
    let plan = plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["strict-plan".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: false,
    })
    .unwrap();
    let plan_path = paths
        .staging_skills_dir()
        .join(&plan.operation_id)
        .join("plan.json");
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&plan_path).unwrap()).unwrap();
    document["plan"]["unexpected"] = json!(true);
    fs::write(&plan_path, serde_json::to_vec(&document).unwrap()).unwrap();
    assert!(matches!(
        commit_install(plan.confirmation()),
        Err(SkillError::InvalidSource { .. })
    ));
    assert!(!fixture.central("strict-plan").exists());
}

#[test]
fn staged_resolution_reuses_strict_source_component_and_path_validation() {
    let fixture = SkillsFixture::installed_agents(&[]);
    let assert_rejected = |operation_id: String, skill_name: &str, from: &str, to: &str| {
        let path = SkillsPaths::from_env()
            .unwrap()
            .staging_skills_dir()
            .join(&operation_id)
            .join("resolution.json");
        let document = fs::read_to_string(&path).unwrap();
        assert!(document.contains(from), "missing tamper source {from}");
        fs::write(&path, document.replacen(from, to, 1)).unwrap();
        assert!(matches!(
            plan_install(PlanInstallRequest {
                resolution_id: operation_id,
                skill_names: vec![skill_name.into()],
                agent_ids: Vec::new(),
                replace_conflicts: false,
            }),
            Err(SkillError::InvalidSource { .. }) | Err(SkillError::UnsafePath { .. })
        ));
    };

    let local = fixture.resolve_local(&["strict-local-path"]);
    let SkillSource::Local { path, .. } = &local.source else {
        unreachable!()
    };
    assert_rejected(
        local.operation_id,
        "strict-local-path",
        &format!("\"path\":{}", serde_json::to_string(path).unwrap()),
        "\"path\":\"../outside\"",
    );

    let local = fixture.resolve_local(&["strict-local-subpath"]);
    assert_rejected(
        local.operation_id,
        "strict-local-subpath",
        "\"subpath\":\"\"",
        "\"subpath\":\"../outside\"",
    );

    let server = MockGithub::start(&["strict-github"]);
    for (from, to) in [
        ("\"owner\":\"acme\"", "\"owner\":\"../acme\""),
        ("\"repo\":\"skills\"", "\"repo\":\"bad/repo\""),
        ("\"subpath\":\"\"", "\"subpath\":\"../outside\""),
        (
            "\"requested_ref\":\"main\"",
            "\"requested_ref\":\"../main\"",
        ),
    ] {
        let github = resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            server.endpoints(),
        )
        .unwrap();
        assert_rejected(github.operation_id, "strict-github", from, to);
    }

    let pinned = resolve_source(
        SkillSourceInput::Github {
            value: format!("https://github.com/acme/skills/tree/{FIXTURE_SHA}/catalog"),
        },
        server.endpoints(),
    )
    .unwrap();
    assert_rejected(
        pinned.operation_id,
        "strict-github",
        &format!("\"resolved_revision\":\"{FIXTURE_SHA}\""),
        "\"resolved_revision\":\"1123456789abcdef0123456789abcdef01234567\"",
    );

    let pinned = resolve_source(
        SkillSourceInput::Github {
            value: format!("https://github.com/acme/skills/tree/{FIXTURE_SHA}/catalog"),
        },
        server.endpoints(),
    )
    .unwrap();
    assert_rejected(
        pinned.operation_id,
        "strict-github",
        "\"pinned\":true",
        "\"pinned\":false",
    );
}
