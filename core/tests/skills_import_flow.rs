#![cfg(unix)]

mod support;

use arca_core::settings::load_settings;
use arca_core::skills::{
    cancel_operation, commit_assignment, commit_import, hash_tree, list_inventory,
    list_migration_candidates, plan_assignment, plan_import, recover_pending,
    PlanAssignmentRequest, PlanImportRequest, PlanInstallRequest, SkillError, SkillSource,
    SkillsPaths,
};
use serde_json::json;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use support::skills::{assert_managed_link, write_skill, SkillsFixture};
use uuid::Uuid;

#[test]
fn import_does_not_move_external_copy_until_commit() {
    let fixture = SkillsFixture::external_skill("legacy", "claude-user");
    let original = fixture.read_external("legacy");
    let plan = plan_import(fixture.import_request("legacy")).unwrap();
    let SkillSource::Imported { backup_path, .. } = &plan.skills[0].source else {
        panic!("import plan did not use Imported provenance")
    };
    let backup = SkillsPaths::from_env()
        .unwrap()
        .expand_user(backup_path)
        .unwrap();
    let reviewed_hash = plan.skills[0].content_hash.clone();
    assert_eq!(fixture.read_external("legacy"), original);
    commit_import(plan.confirmation()).unwrap();
    assert!(fs::symlink_metadata(&backup).unwrap().is_dir());
    assert_eq!(hash_tree(&backup).unwrap(), reviewed_hash);
    assert_eq!(fs::read(backup.join("SKILL.md")).unwrap(), original);
    assert_managed_link(fixture.external_path("legacy"), fixture.central("legacy"));
}

#[test]
fn import_from_alias_only_target_keeps_one_discovery_link() {
    let fixture = SkillsFixture::installed_agents(&["cursor"]);
    let name = "alias-import";
    let source = fixture.target("agents-user", name);
    write_skill(&source, name, "Alias-only external fixture");
    let original = fs::read(source.join("SKILL.md")).unwrap();

    let inventory = list_inventory().unwrap();
    let item = inventory
        .items
        .iter()
        .find(|item| {
            item.name == name
                && item
                    .states
                    .contains(&arca_core::skills::InventoryState::External)
                && matches!(
                    &item.location,
                    arca_core::skills::SkillLocation::AgentTarget { target_id, .. }
                        if target_id == "agents-user"
                )
        })
        .unwrap();
    assert_eq!(item.affected_agent_ids, vec!["cursor"]);

    let plan = plan_import(PlanImportRequest {
        identity: item.identity.clone(),
        agent_ids: item.affected_agent_ids.clone(),
        replace_conflicts: false,
    })
    .unwrap();
    assert_eq!(
        plan.targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect::<Vec<_>>(),
        vec!["agents-user"]
    );
    assert_eq!(plan.targets[0].affected_agent_ids, vec!["cursor"]);
    let SkillSource::Imported { original_path, .. } = &plan.skills[0].source else {
        panic!("import plan did not preserve Imported provenance")
    };
    assert_eq!(original_path, "~/.agents/skills/alias-import");

    commit_import(plan.confirmation()).unwrap();

    assert_managed_link(source, fixture.central(name));
    assert!(fs::symlink_metadata(fixture.target("cursor-user", name)).is_err());
    assert_eq!(fixture.read_backup(name), original);
    assert_eq!(
        load_settings().skill_assignments.unwrap()[name],
        ["agents-user".to_owned()].into_iter().collect()
    );
}

#[test]
fn import_merges_identical_external_directories_across_targets() {
    let fixture = SkillsFixture::installed_agents(&["claude-code", "cursor"]);
    let name = "shared-history";
    write_skill(
        &fixture.target("claude-user", name),
        name,
        "Shared historical fixture",
    );
    write_skill(
        &fixture.target("cursor-user", name),
        name,
        "Shared historical fixture",
    );
    let migration = list_migration_candidates().unwrap();
    let matching: Vec<_> = migration.iter().filter(|item| item.name == name).collect();
    assert_eq!(matching.len(), 2);
    assert!(matching.iter().all(|item| item.content_hash.is_some()));
    assert_eq!(matching[0].content_hash, matching[1].content_hash);
    assert!(matching.iter().all(|item| item.risk.is_some()));
    let inventory = list_inventory().unwrap();
    let source = inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap();

    let plan = plan_import(PlanImportRequest {
        identity: source.identity.clone(),
        agent_ids: vec!["claude-code".into(), "cursor".into()],
        replace_conflicts: false,
    })
    .unwrap();
    commit_import(plan.confirmation()).unwrap();

    assert_managed_link(fixture.target("claude-user", name), fixture.central(name));
    assert_managed_link(fixture.target("cursor-user", name), fixture.central(name));
    assert_eq!(
        load_settings().skill_assignments.unwrap()[name],
        ["claude-user".to_owned(), "cursor-user".to_owned()]
            .into_iter()
            .collect()
    );
}

#[test]
fn assignment_never_overwrites_a_real_directory_or_unknown_link() {
    let fixture = SkillsFixture::managed("safe");
    fixture.create_real_target("cursor-user", "safe");
    let error = plan_assignment(PlanAssignmentRequest {
        skill_name: "safe".into(),
        agent_ids: vec!["cursor".into()],
        enabled: true,
    })
    .unwrap_err();
    assert!(matches!(error, SkillError::Conflict { .. }));

    fs::remove_dir_all(fixture.target("cursor-user", "safe")).unwrap();
    let outside = fixture.home.home.join("unknown-assignment");
    fs::create_dir_all(&outside).unwrap();
    let target = fixture.target("cursor-user", "safe");
    symlink(&outside, &target).unwrap();
    let error = plan_assignment(PlanAssignmentRequest {
        skill_name: "safe".into(),
        agent_ids: vec!["cursor".into()],
        enabled: true,
    })
    .unwrap_err();
    assert!(matches!(error, SkillError::Conflict { .. }));
    assert_eq!(fs::read_link(target).unwrap(), outside);
}

#[test]
fn import_plan_is_read_only_and_persists_imported_source_with_timestamped_backup() {
    let fixture = SkillsFixture::external_skill("legacy-source", "claude-user");
    let before = fixture.snapshot();
    let plan = plan_import(fixture.import_request("legacy-source")).unwrap();
    assert_eq!(fixture.snapshot(), before);
    let SkillSource::Imported {
        original_path,
        backup_path,
    } = &plan.skills[0].source
    else {
        panic!("import plan did not use Imported provenance")
    };
    assert_eq!(original_path, "~/.claude/skills/legacy-source");
    assert!(backup_path.starts_with("~/.arca/backups/skills/import-"));
    assert!(backup_path.ends_with("/legacy-source"));

    commit_import(plan.confirmation()).unwrap();
    let record = &load_settings().managed_skills.unwrap()["legacy-source"];
    assert_eq!(record.source, plan.skills[0].source);
    assert!(fs::symlink_metadata(fixture.latest_backup("legacy-source"))
        .unwrap()
        .is_dir());
}

#[test]
fn import_revalidates_the_external_directory_before_commit() {
    let fixture = SkillsFixture::external_skill("legacy-stale", "claude-user");
    let plan = plan_import(fixture.import_request("legacy-stale")).unwrap();
    fs::write(
        fixture.external_path("legacy-stale").join("SKILL.md"),
        "---\nname: legacy-stale\ndescription: changed after plan\n---\n",
    )
    .unwrap();
    assert!(matches!(
        commit_import(plan.confirmation()),
        Err(SkillError::PlanStale { .. })
    ));
    assert!(!fixture.central("legacy-stale").exists());
    assert!(fs::symlink_metadata(fixture.external_path("legacy-stale"))
        .unwrap()
        .is_dir());
}

#[test]
fn import_central_collision_requires_opt_in_and_retains_the_displaced_tree() {
    let fixture = SkillsFixture::managed("central-collision");
    let central = fixture.central("central-collision");
    let old_hash = hash_tree(&central).unwrap();
    fixture.create_real_target("claude-user", "central-collision");
    let request = fixture.import_request("central-collision");

    assert!(matches!(
        plan_import(request.clone()),
        Err(SkillError::Conflict { .. })
    ));
    let plan = plan_import(PlanImportRequest {
        replace_conflicts: true,
        ..request
    })
    .unwrap();
    let retained = SkillsPaths::from_env()
        .unwrap()
        .backups_skills_dir()
        .join(format!("{}-central-central-collision", plan.operation_id));

    commit_import(plan.confirmation()).unwrap();

    assert!(retained.is_dir());
    assert_eq!(hash_tree(&retained).unwrap(), old_hash);
    assert_ne!(hash_tree(&central).unwrap(), old_hash);
}

#[test]
fn replacing_import_preserves_other_enabled_and_disabled_consumers() {
    let name = "import-preserve-consumers";
    let fixture = SkillsFixture::managed_on_targets(name, &["agents-user", "cursor-user"]);
    let disabled = fixture.target("cursor-user", name);
    fs::remove_file(&disabled).unwrap();
    arca_core::settings::mutate_settings(|settings| {
        for (target, enabled) in [("agents-user", true), ("cursor-user", false)] {
            settings.skill_consumptions.get_or_insert_default().entry(name.into()).or_default().insert(
                target.into(), arca_core::domain::assets::SkillConsumptionRecord {
                    name: name.into(), target_id: target.into(), enabled,
                });
        }
    }).unwrap();
    let before = load_settings().skill_consumptions.unwrap()[name].clone();
    fixture.create_real_target("claude-user", name);
    let request = fixture.import_request(name);
    let plan = arca_core::skills::plan_asset_import(arca_core::skills::PlanSkillAssetImportRequest {
        identity: request.identity, replace_conflicts: true,
    }).unwrap();
    assert_eq!(plan.targets.iter().map(|target| target.target_id.as_str()).collect::<Vec<_>>(), vec!["claude-user"]);
    commit_import(plan.confirmation()).unwrap();
    let after = load_settings();
    assert_eq!(after.skill_assignments.unwrap()[name],
        ["agents-user".into(), "cursor-user".into(), "claude-user".into()].into());
    let consumers = &after.skill_consumptions.unwrap()[name];
    for (target, record) in before { assert_eq!(consumers[&target], record); }
    assert_managed_link(fixture.target("agents-user", name), fixture.central(name));
    assert_managed_link(fixture.target("claude-user", name), fixture.central(name));
    assert!(fs::symlink_metadata(disabled).is_err());
}

#[test]
fn import_replacement_flag_never_overwrites_other_agent_targets() {
    for kind in ["directory", "unknown", "broken"] {
        let name = format!("import-{kind}-conflict");
        let fixture = SkillsFixture::external_skill(&name, "claude-user");
        fs::create_dir_all(fixture.home.home.join("Library/Application Support/Cursor")).unwrap();
        let inventory = list_inventory().unwrap();
        let source = inventory
            .items
            .iter()
            .find(|item| {
                item.name == name
                    && matches!(
                        &item.location,
                        arca_core::skills::SkillLocation::AgentTarget { target_id, .. }
                            if target_id == "claude-user"
                    )
            })
            .unwrap();
        let conflict = fixture.target("cursor-user", &name);
        fs::create_dir_all(conflict.parent().unwrap()).unwrap();
        match kind {
            "directory" => fixture.create_real_target("cursor-user", &name),
            "unknown" => {
                let outside = fixture.home.home.join(format!("outside-{name}"));
                fs::create_dir_all(&outside).unwrap();
                symlink(outside, &conflict).unwrap();
            }
            "broken" => {
                symlink(fixture.home.home.join(format!("missing-{name}")), &conflict).unwrap();
            }
            _ => unreachable!(),
        }

        assert!(matches!(
            plan_import(PlanImportRequest {
                identity: source.identity.clone(),
                agent_ids: vec!["cursor".into()],
                replace_conflicts: true,
            }),
            Err(SkillError::Conflict { .. })
        ));
        assert!(fs::symlink_metadata(conflict).is_ok());
    }
}

#[test]
fn assignment_disable_normalizes_shared_targets_and_removes_only_managed_links() {
    let fixture = SkillsFixture::managed_on_targets("shared-safe", &["agents-user"]);
    let plan = plan_assignment(PlanAssignmentRequest {
        skill_name: "shared-safe".into(),
        agent_ids: vec!["cursor".into()],
        enabled: false,
    })
    .unwrap();
    assert_eq!(
        plan.targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect::<Vec<_>>(),
        vec!["agents-user"]
    );
    assert!(plan.targets[0]
        .affected_agent_ids
        .iter()
        .any(|agent| agent == "codex"));
    assert!(plan.targets[0]
        .affected_agent_ids
        .iter()
        .any(|agent| agent == "cursor"));
    commit_assignment(plan.confirmation()).unwrap();
    assert!(fs::symlink_metadata(fixture.target("agents-user", "shared-safe")).is_err());
    assert!(fixture.central("shared-safe").exists());
    drop(fixture);

    let broken = SkillsFixture::broken_managed_link("broken-safe", "cursor-user");
    let before_target = fs::read_link(broken.target("cursor-user", "broken-safe")).unwrap();
    assert!(matches!(
        plan_assignment(PlanAssignmentRequest {
            skill_name: "broken-safe".into(),
            agent_ids: vec!["cursor".into()],
            enabled: false,
        }),
        Err(SkillError::Conflict { .. })
    ));
    assert_eq!(
        fs::read_link(broken.target("cursor-user", "broken-safe")).unwrap(),
        before_target
    );
}

#[test]
fn assignment_enable_creates_one_managed_link_and_rejects_duplicate_agents() {
    let fixture = SkillsFixture::managed("assign-safe");
    let plan = plan_assignment(PlanAssignmentRequest {
        skill_name: "assign-safe".into(),
        agent_ids: vec!["cursor".into()],
        enabled: true,
    })
    .unwrap();
    assert_eq!(plan.targets[0].target_id, "cursor-user");
    commit_assignment(plan.confirmation()).unwrap();
    assert_managed_link(
        fixture.target("cursor-user", "assign-safe"),
        fixture.central("assign-safe"),
    );

    assert!(matches!(
        plan_assignment(PlanAssignmentRequest {
            skill_name: "assign-safe".into(),
            agent_ids: vec!["cursor".into(), "cursor".into()],
            enabled: true,
        }),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn incremental_assignment_enable_recomputes_one_minimal_physical_graph() {
    let fixture = SkillsFixture::managed("incremental-safe");
    let cursor = plan_assignment(PlanAssignmentRequest {
        skill_name: "incremental-safe".into(),
        agent_ids: vec!["cursor".into()],
        enabled: true,
    })
    .unwrap();
    commit_assignment(cursor.confirmation()).unwrap();
    assert_managed_link(
        fixture.target("cursor-user", "incremental-safe"),
        fixture.central("incremental-safe"),
    );

    let codex = plan_assignment(PlanAssignmentRequest {
        skill_name: "incremental-safe".into(),
        agent_ids: vec!["codex".into()],
        enabled: true,
    })
    .unwrap();
    commit_assignment(codex.confirmation()).unwrap();

    assert_managed_link(
        fixture.target("agents-user", "incremental-safe"),
        fixture.central("incremental-safe"),
    );
    assert!(fs::symlink_metadata(fixture.target("cursor-user", "incremental-safe")).is_err());
    assert_eq!(
        load_settings().skill_assignments.unwrap()["incremental-safe"],
        ["agents-user".to_owned()].into_iter().collect()
    );
}

#[test]
fn assignment_disable_uses_saved_verified_targets_after_agent_probe_disappears() {
    let fixture = SkillsFixture::managed_on_targets("offline-safe", &["cursor-user"]);
    let saved_target = fixture.target("cursor-user", "offline-safe");
    fs::remove_dir_all(fixture.home.home.join("Library/Application Support/Cursor")).unwrap();

    let plan = plan_assignment(PlanAssignmentRequest {
        skill_name: "offline-safe".into(),
        agent_ids: vec!["cursor".into()],
        enabled: false,
    })
    .unwrap();
    commit_assignment(plan.confirmation()).unwrap();

    assert!(fs::symlink_metadata(saved_target).is_err());
    assert!(!load_settings()
        .skill_assignments
        .as_ref()
        .is_some_and(|assignments| assignments.contains_key("offline-safe")));
}

#[test]
fn cancellation_requires_canonical_uuid_never_follows_symlinks_and_refuses_a_journal() {
    let fixture = SkillsFixture::installed_agents(&[]);
    let resolution = fixture.resolve_local(&["cancel-me"]);
    let paths = SkillsPaths::from_env().unwrap();
    let operation = paths.staging_skills_dir().join(&resolution.operation_id);
    cancel_operation(&resolution.operation_id).unwrap();
    assert!(!operation.exists());

    let outside = fixture.home.home.join("outside-cancel");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("marker"), "keep").unwrap();
    for invalid in [
        "../outside-cancel",
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
        "not-a-uuid",
    ] {
        assert!(matches!(
            cancel_operation(invalid),
            Err(SkillError::InvalidSource { .. })
        ));
    }
    assert!(outside.join("marker").exists());

    let linked_id = "10000000-0000-4000-8000-000000000007";
    symlink(&outside, paths.staging_skills_dir().join(linked_id)).unwrap();
    assert!(matches!(
        cancel_operation(linked_id),
        Err(SkillError::RecoveryRequired { .. })
    ));
    assert!(outside.join("marker").exists());

    let journaled_id = "10000000-0000-4000-8000-000000000008";
    let journaled = paths.staging_skills_dir().join(journaled_id);
    fs::create_dir(&journaled).unwrap();
    fs::write(
        paths
            .journals_skills_dir()
            .join(format!("{journaled_id}.json")),
        "active",
    )
    .unwrap();
    assert!(matches!(
        cancel_operation(journaled_id),
        Err(SkillError::RecoveryRequired { .. })
    ));
    assert!(journaled.exists());
}

#[test]
fn operation_ids_and_private_plan_files_are_canonical_and_wire_requests_roundtrip() {
    let fixture = SkillsFixture::installed_agents(&["codex"]);
    let resolution = fixture.resolve_local(&["private-plan"]);
    let plan = arca_core::skills::plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["private-plan".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: false,
    })
    .unwrap();
    assert_eq!(
        Uuid::parse_str(&plan.operation_id).unwrap().to_string(),
        plan.operation_id
    );
    let operation = SkillsPaths::from_env()
        .unwrap()
        .staging_skills_dir()
        .join(&plan.operation_id);
    for file in [
        operation.join("resolution.json"),
        operation.join("plan.json"),
    ] {
        assert_eq!(
            fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let install = PlanInstallRequest {
        resolution_id: plan.operation_id.clone(),
        skill_names: vec!["private-plan".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: true,
    };
    let import = PlanImportRequest {
        identity: "target:agents-user:private-plan".into(),
        agent_ids: vec!["codex".into()],
        replace_conflicts: false,
    };
    let assignment = PlanAssignmentRequest {
        skill_name: "private-plan".into(),
        agent_ids: vec!["codex".into()],
        enabled: true,
    };
    assert_eq!(
        serde_json::from_str::<PlanInstallRequest>(&serde_json::to_string(&install).unwrap())
            .unwrap(),
        install
    );
    assert_eq!(
        serde_json::from_str::<PlanImportRequest>(&serde_json::to_string(&import).unwrap())
            .unwrap(),
        import
    );
    assert_eq!(
        serde_json::from_str::<PlanAssignmentRequest>(&serde_json::to_string(&assignment).unwrap())
            .unwrap(),
        assignment
    );
}

#[test]
fn every_task7_operation_root_has_private_cleanup_metadata() {
    let assert_metadata = |operation_id: &str| {
        let path = SkillsPaths::from_env()
            .unwrap()
            .staging_skills_dir()
            .join(operation_id)
            .join("metadata.json");
        let bytes = fs::read(&path).unwrap();
        let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(document["operation_id"], operation_id);
        assert!(document["created_at"]
            .as_str()
            .is_some_and(|value| chrono::DateTime::parse_from_rfc3339(value).is_ok()));
        assert_eq!(
            bytes,
            format!(
                "{{\"operation_id\":\"{operation_id}\",\"created_at\":\"{}\"}}",
                document["created_at"].as_str().unwrap()
            )
            .into_bytes()
        );
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    };

    let fixture = SkillsFixture::installed_agents(&["codex"]);
    let resolution = fixture.resolve_local(&["metadata-install"]);
    assert_metadata(&resolution.operation_id);
    let install = arca_core::skills::plan_install(PlanInstallRequest {
        resolution_id: resolution.operation_id,
        skill_names: vec!["metadata-install".into()],
        agent_ids: vec!["codex".into()],
        replace_conflicts: false,
    })
    .unwrap();
    assert_metadata(&install.operation_id);
    drop(fixture);

    let fixture = SkillsFixture::managed("metadata-assignment");
    let assignment = plan_assignment(PlanAssignmentRequest {
        skill_name: "metadata-assignment".into(),
        agent_ids: vec!["codex".into()],
        enabled: true,
    })
    .unwrap();
    assert_metadata(&assignment.operation_id);
    drop(fixture);

    let fixture = SkillsFixture::external_skill("metadata-import", "claude-user");
    let import = plan_import(fixture.import_request("metadata-import")).unwrap();
    assert_metadata(&import.operation_id);
}

#[test]
fn task7_metadata_makes_only_old_unjournaled_operations_cleanup_eligible() {
    let fixture = SkillsFixture::installed_agents(&[]);
    let stale = fixture.resolve_local(&["stale-operation"]);
    let fresh = fixture.resolve_local(&["fresh-operation"]);
    let paths = SkillsPaths::from_env().unwrap();
    let stale_root = paths.staging_skills_dir().join(&stale.operation_id);
    fs::write(
        stale_root.join("metadata.json"),
        serde_json::to_vec(&json!({
            "operation_id": stale.operation_id,
            "created_at": "2000-01-01T00:00:00Z"
        }))
        .unwrap(),
    )
    .unwrap();

    recover_pending().unwrap();

    assert!(!stale_root.exists());
    assert!(paths.staging_skills_dir().join(fresh.operation_id).exists());
}

#[test]
fn cancellation_rejects_a_swapped_staging_parent_and_preserves_outside_content() {
    let fixture = SkillsFixture::installed_agents(&[]);
    let resolution = fixture.resolve_local(&["parent-swap"]);
    let paths = SkillsPaths::from_env().unwrap();
    let staging_parent = paths.arca_dir().join("staging");
    let retained = fixture.home.home.join("retained-staging-parent");
    fs::rename(&staging_parent, &retained).unwrap();

    let outside = fixture.home.home.join("outside-staging-parent");
    let outside_operation = outside.join("skills").join(&resolution.operation_id);
    fs::create_dir_all(&outside_operation).unwrap();
    fs::write(outside_operation.join("marker"), "keep").unwrap();
    symlink(&outside, &staging_parent).unwrap();

    let result = cancel_operation(&resolution.operation_id);
    assert!(matches!(
        result,
        Err(SkillError::UnsafePath { .. })
            | Err(SkillError::InvalidSource { .. })
            | Err(SkillError::RecoveryRequired { .. })
    ));
    assert_eq!(
        fs::read_to_string(outside_operation.join("marker")).unwrap(),
        "keep"
    );
}
