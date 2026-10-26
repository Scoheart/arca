#![cfg(unix)]

mod support;

use arca_core::settings::{mutate_settings, AgentConfigPathOverride};
use arca_core::skills::{
    get_skill_detail, hash_tree, list_inventory, list_skill_agents, normalize_agent_selection,
    recover_pending_with_paths, InventoryState, JournalPhase, SkillError, SkillsPaths,
};
use arca_core::testenv::TestHome;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use support::skills::{has_state, managed_record, write_skill, TransactionFixture};

#[test]
fn inventory_surfaces_pending_recovery_without_mutating_it_and_clears_after_recovery() {
    let fixture = TransactionFixture::crashed_at(JournalPhase::Prepared);
    let journal_root = fixture.paths.journals_skills_dir();

    let pending = list_inventory().unwrap();

    assert_eq!(
        pending.recovery_error.as_deref(),
        Some("A pending Skills operation requires recovery.")
    );
    assert!(
        journal_root.exists(),
        "inventory unexpectedly recovered a journal"
    );
    let rendered = serde_json::to_string(&pending).unwrap();
    assert!(!rendered.contains(fixture.home.home.to_string_lossy().as_ref()));

    recover_pending_with_paths(&fixture.paths).unwrap();
    assert_eq!(list_inventory().unwrap().recovery_error, None);
}

#[test]
fn inventory_turns_recovery_required_root_evidence_into_a_path_free_status() {
    let th = TestHome::new("inventory-recovery-status");
    fs::create_dir_all(th.home.join(".arca/journals")).unwrap();
    fs::write(th.home.join(".arca/journals/skills"), "not a directory").unwrap();

    let inventory = list_inventory().unwrap();

    assert_eq!(
        inventory.recovery_error.as_deref(),
        Some("A pending Skills operation requires recovery.")
    );
    let rendered = serde_json::to_string(&inventory).unwrap();
    assert!(!rendered.contains(th.home.to_string_lossy().as_ref()));
    assert!(th.home.join(".arca/journals/skills").is_file());
}

#[test]
fn inventory_reports_journal_inspection_io_as_recovery_status_after_inventory_succeeds() {
    let th = TestHome::new("inventory-journal-inspection-io");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    let paths = SkillsPaths::from_env().unwrap();
    let journals = paths.journals_skills_dir();
    fs::set_permissions(&journals, fs::Permissions::from_mode(0o000)).unwrap();

    let result = list_inventory();

    fs::set_permissions(&journals, fs::Permissions::from_mode(0o700)).unwrap();
    let inventory = result.expect("journal inspection must not hide a readable inventory");
    assert_eq!(
        inventory.recovery_error.as_deref(),
        Some("A pending Skills operation requires recovery.")
    );
    let rendered = serde_json::to_string(&inventory).unwrap();
    assert!(!rendered.contains(th.home.to_string_lossy().as_ref()));
}

fn install_cursor(home: &std::path::Path) {
    fs::create_dir_all(home.join("Library/Application Support/Cursor")).unwrap();
}

#[test]
fn only_installed_verified_agents_are_assignable_and_aliases_expand_impact() {
    let th = TestHome::new("skills-targets");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    install_cursor(&th.home);
    fs::create_dir_all(th.home.join(".config/opencode")).unwrap();

    let agents = list_skill_agents().unwrap();
    assert_eq!(
        agents.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        vec!["codex", "cursor", "opencode"]
    );
    let codex = agents.iter().find(|row| row.id == "codex").unwrap();
    assert_eq!(codex.target_id, "agents-user");
    assert_eq!(
        codex.affected_agent_ids,
        vec!["codex", "cursor", "opencode"]
    );

    assert_eq!(
        normalize_agent_selection(&["codex".into(), "cursor".into()]).unwrap(),
        vec!["agents-user"]
    );
}

#[test]
fn inventory_distinguishes_external_broken_conflicting_and_modified() {
    let th = TestHome::new("inventory-states");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    install_cursor(&th.home);
    let central = th.home.join(".arca/assets/skills/items/managed");
    write_skill(&central, "managed", "Managed fixture");
    let original_hash = hash_tree(&central).unwrap();
    mutate_settings(|settings| {
        settings
            .managed_skills
            .get_or_insert_default()
            .insert("managed".into(), managed_record("managed", &original_hash));
    })
    .unwrap();
    fs::write(
        central.join("SKILL.md"),
        "---\nname: managed\ndescription: Changed fixture\n---\n",
    )
    .unwrap();

    write_skill(
        &th.home.join(".agents/skills/external"),
        "external",
        "External fixture",
    );
    fs::create_dir_all(th.home.join(".cursor/skills")).unwrap();
    symlink(
        th.home.join("missing/broken"),
        th.home.join(".cursor/skills/broken"),
    )
    .unwrap();
    let wrong = th.home.join("wrong/conflict");
    write_skill(&wrong, "conflict", "Wrong target fixture");
    symlink(&wrong, th.home.join(".cursor/skills/conflict")).unwrap();

    let inventory = list_inventory().unwrap();
    assert!(has_state(&inventory, "external", InventoryState::External));
    assert!(has_state(&inventory, "broken", InventoryState::BrokenLink));
    assert!(has_state(
        &inventory,
        "conflict",
        InventoryState::ConflictingLink
    ));
    assert!(has_state(
        &inventory,
        "managed",
        InventoryState::LocallyModified
    ));
}

#[test]
fn assigned_target_remains_visible_after_its_agent_probe_disappears() {
    let th = TestHome::new("inventory-orphaned-target");
    let central = th.home.join(".arca/assets/skills/items/safe");
    write_skill(&central, "safe", "Managed fixture");
    let hash = hash_tree(&central).unwrap();
    mutate_settings(|settings| {
        settings
            .managed_skills
            .get_or_insert_default()
            .insert("safe".into(), managed_record("safe", &hash));
        settings
            .skill_assignments
            .get_or_insert_default()
            .insert("safe".into(), ["cursor-user".into()].into_iter().collect());
    })
    .unwrap();
    let target = th.home.join(".cursor/skills/safe");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    symlink(&central, &target).unwrap();

    let inventory = list_inventory().unwrap();
    assert!(inventory.agents.iter().all(|agent| agent.id != "cursor"));
    assert!(has_state(&inventory, "safe", InventoryState::Assigned));
    assert!(
        !inventory
            .targets
            .iter()
            .find(|row| row.target_id == "cursor-user")
            .unwrap()
            .assignable
    );
}

#[test]
fn command_probes_require_executable_regular_files_in_disposable_roots() {
    let th = TestHome::new("inventory-command-probes");
    let fallback = th.home.join("opt/homebrew/bin");
    fs::create_dir_all(&fallback).unwrap();
    let gemini = fallback.join("gemini");
    fs::write(&gemini, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&gemini, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir(fallback.join("claude")).unwrap();
    fs::write(fallback.join("copilot"), "not executable").unwrap();

    let agents = list_skill_agents().unwrap();
    assert!(agents.iter().any(|agent| agent.id == "gemini"));
    assert!(agents.iter().all(|agent| agent.id != "claude-code"));
    assert!(agents.iter().all(|agent| agent.id != "copilot-cli"));

    let executable = th.home.join("fixture-copilot");
    fs::write(&executable, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let usr_local = th.home.join("usr/local/bin");
    fs::create_dir_all(&usr_local).unwrap();
    symlink(&executable, usr_local.join("copilot")).unwrap();
    assert!(list_skill_agents()
        .unwrap()
        .iter()
        .any(|agent| agent.id == "copilot-cli"));
}

#[test]
fn installed_alias_only_target_is_scanned_but_not_assignable() {
    let th = TestHome::new("inventory-alias-only-target");
    install_cursor(&th.home);
    write_skill(
        &th.home.join(".agents/skills/shared"),
        "shared",
        "Shared alias fixture",
    );

    let inventory = list_inventory().unwrap();
    assert_eq!(
        inventory
            .agents
            .iter()
            .map(|agent| agent.id.as_str())
            .collect::<Vec<_>>(),
        vec!["cursor"]
    );
    let target = inventory
        .targets
        .iter()
        .find(|target| target.target_id == "agents-user")
        .unwrap();
    assert_eq!(target.affected_agent_ids, vec!["cursor"]);
    assert!(!target.assignable);
    assert!(has_state(&inventory, "shared", InventoryState::External));
}

#[test]
fn canonical_target_graph_merges_two_ids_resolving_to_one_path() {
    let th = TestHome::new("inventory-target-collision");
    fs::create_dir_all(th.home.join(".agents/skills")).unwrap();
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    install_cursor(&th.home);
    symlink(th.home.join(".agents"), th.home.join(".cursor")).unwrap();

    let agents = list_skill_agents().unwrap();
    let cursor = agents.iter().find(|agent| agent.id == "cursor").unwrap();
    assert_eq!(cursor.target_id, "agents-user");
    assert_eq!(cursor.global_dir, "~/.agents/skills");
    assert_eq!(cursor.affected_agent_ids, vec!["codex", "cursor"]);

    let inventory = list_inventory().unwrap();
    assert_eq!(inventory.targets.len(), 1);
    assert_eq!(inventory.targets[0].target_id, "agents-user");
    assert_eq!(
        inventory.targets[0].primary_agent_ids,
        vec![
            "codex",
            "cursor",
            "dirac",
            "docker-agent",
            "goose",
            "poolside",
            "theiaai-theiaide",
            "vt-code",
            "warp",
            "zed",
            "zencoder"
        ]
    );
}

#[test]
fn skills_path_override_joins_an_existing_physical_target() {
    let th = TestHome::new("inventory-target-override-join");
    fs::create_dir_all(th.home.join(".agents/skills")).unwrap();
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    install_cursor(&th.home);
    mutate_settings(|settings| {
        settings.agent_config_paths = Some(
            [(
                "cursor".into(),
                AgentConfigPathOverride {
                    skills_global_dir: Some("~/.agents/skills".into()),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
        );
    })
    .unwrap();

    let agents = list_skill_agents().unwrap();
    let cursor = agents.iter().find(|agent| agent.id == "cursor").unwrap();
    assert_eq!(cursor.target_id, "agents-user");
    assert_eq!(cursor.global_dir, "~/.agents/skills");
    assert_eq!(cursor.affected_agent_ids, vec!["codex", "cursor"]);
}

#[test]
fn canonical_target_graph_rejects_a_non_directory_existing_parent() {
    let th = TestHome::new("inventory-target-file-parent");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    fs::create_dir_all(th.home.join(".agents")).unwrap();
    fs::write(th.home.join(".agents/skills"), "not a directory").unwrap();

    assert!(matches!(
        list_skill_agents(),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn selection_rejects_duplicates_unknown_ids_and_agents_without_current_evidence() {
    let th = TestHome::new("inventory-invalid-selection");
    fs::create_dir_all(th.home.join(".codex")).unwrap();

    assert!(normalize_agent_selection(&["codex".into(), "codex".into()]).is_err());
    assert!(normalize_agent_selection(&["not-an-agent".into()]).is_err());
    assert!(normalize_agent_selection(&["cursor".into()]).is_err());
}

#[test]
fn detail_uses_opaque_identity_and_truncates_skill_md_at_a_utf8_boundary() {
    let th = TestHome::new("inventory-detail-bound");
    let central = th.home.join(".arca/assets/skills/items/large");
    fs::create_dir_all(&central).unwrap();
    let mut skill_md = "---\nname: large\ndescription: Large fixture\n---\n\n".to_string();
    skill_md.push_str(&"界".repeat(400_000));
    fs::write(central.join("SKILL.md"), skill_md).unwrap();
    let hash = hash_tree(&central).unwrap();
    mutate_settings(|settings| {
        settings
            .managed_skills
            .get_or_insert_default()
            .insert("large".into(), managed_record("large", &hash));
    })
    .unwrap();

    let inventory = list_inventory().unwrap();
    let item = inventory
        .items
        .iter()
        .find(|item| item.name == "large")
        .unwrap();
    assert_eq!(item.identity, "central:large");
    let detail = get_skill_detail(&item.identity).unwrap();
    assert!(detail.skill_md.len() <= 1024 * 1024);
    assert!(detail.skill_md_truncated);
    assert!(!detail.files.is_empty());
    assert!(std::str::from_utf8(detail.skill_md.as_bytes()).is_ok());

    for invalid in [
        "central:../large",
        "central:/tmp/large",
        "target:not-known:large",
        "target:cursor-user:/tmp/large",
    ] {
        assert!(get_skill_detail(invalid).is_err(), "accepted {invalid}");
    }
}

#[test]
fn strict_settings_errors_do_not_expose_private_paths() {
    let th = TestHome::new("inventory-corrupt-settings");
    fs::create_dir_all(th.home.join(".arca")).unwrap();
    fs::write(
        th.home.join(".arca/settings.json"),
        r#"{"managed_skills": ["#,
    )
    .unwrap();

    let error = list_inventory().unwrap_err();
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(!rendered.contains(th.home.to_string_lossy().as_ref()));
    assert!(!rendered.contains("settings.json"));
}

#[test]
fn read_only_inventory_does_not_create_arca_skill_roots() {
    let th = TestHome::new("inventory-read-only-roots");
    let arca_home = th.home.join(".arca");

    let inventory = list_inventory().unwrap();

    assert!(inventory.items.is_empty());
    assert!(!arca_home.exists(), "a list API created the Arca home");
}

#[test]
fn missing_installed_target_roots_remain_absent_after_listing() {
    let th = TestHome::new("inventory-missing-target-roots");
    install_cursor(&th.home);
    let target = th.home.join(".cursor/skills");

    let inventory = list_inventory().unwrap();

    assert!(inventory.items.is_empty());
    assert!(
        !target.exists(),
        "a list API created a missing Agent target"
    );
    assert!(!th.home.join(".arca").exists());
}

#[test]
fn invalid_managed_tree_is_visible_but_is_not_marked_managed() {
    let th = TestHome::new("inventory-corrupt-managed-tree");
    let central = th.home.join(".arca/assets/skills/items/corrupt");
    write_skill(&central, "wrong-name", "Mismatched manifest fixture");
    let hash = hash_tree(&central).unwrap();
    mutate_settings(|settings| {
        settings
            .managed_skills
            .get_or_insert_default()
            .insert("corrupt".into(), managed_record("corrupt", &hash));
    })
    .unwrap();

    let inventory = list_inventory().unwrap();
    let item = inventory
        .items
        .iter()
        .find(|item| item.identity == "central:corrupt")
        .unwrap();
    assert!(item.states.contains(&InventoryState::LocallyModified));
    assert!(!item.states.contains(&InventoryState::Managed));
    assert_eq!(item.content_hash.as_deref(), Some(hash.as_str()));
}

#[test]
fn managed_central_link_failures_are_broken_and_never_managed() {
    let th = TestHome::new("inventory-managed-central-links");
    let central_root = th.home.join(".arca/assets/skills/items");
    fs::create_dir_all(&central_root).unwrap();
    symlink(
        th.home.join("missing/dangling"),
        central_root.join("dangling"),
    )
    .unwrap();
    symlink("loop", central_root.join("loop")).unwrap();
    let existing = th.home.join("existing-link-target");
    fs::create_dir(&existing).unwrap();
    symlink(&existing, central_root.join("resolving")).unwrap();
    mutate_settings(|settings| {
        let records = settings.managed_skills.get_or_insert_default();
        records.insert("dangling".into(), managed_record("dangling", "recorded"));
        records.insert("loop".into(), managed_record("loop", "recorded"));
        records.insert("resolving".into(), managed_record("resolving", "recorded"));
    })
    .unwrap();

    let inventory = list_inventory().unwrap();
    for name in ["dangling", "loop"] {
        let item = inventory
            .items
            .iter()
            .find(|item| item.identity == format!("central:{name}"))
            .unwrap();
        assert!(item.states.contains(&InventoryState::BrokenLink));
        assert!(!item.states.contains(&InventoryState::Managed));
    }
    let resolving = inventory
        .items
        .iter()
        .find(|item| item.identity == "central:resolving")
        .unwrap();
    assert!(resolving.states.contains(&InventoryState::ConflictingLink));
    assert!(!resolving.states.contains(&InventoryState::Managed));
}

#[test]
fn target_self_loop_is_reported_as_broken_instead_of_aborting_inventory() {
    let th = TestHome::new("inventory-target-loop");
    install_cursor(&th.home);
    let target_root = th.home.join(".cursor/skills");
    fs::create_dir_all(&target_root).unwrap();
    symlink("loop", target_root.join("loop")).unwrap();

    let inventory = list_inventory().unwrap();
    let item = inventory
        .items
        .iter()
        .find(|item| item.identity == "target:cursor-user:loop")
        .unwrap();
    assert!(item.states.contains(&InventoryState::BrokenLink));
}

#[test]
fn inventory_root_errors_do_not_expose_the_private_home() {
    let th = TestHome::new("inventory-private-root-error");
    fs::create_dir_all(th.home.join(".arca/assets/skills")).unwrap();
    fs::write(th.home.join(".arca/assets/skills/items"), "not a directory").unwrap();

    let error = list_inventory().unwrap_err();
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(!rendered.contains(th.home.to_string_lossy().as_ref()));
    assert!(!rendered.contains("skills"));
}

#[test]
fn read_api_path_resolution_errors_are_path_free() {
    let th = TestHome::new("inventory-path-resolution-error");
    std::env::set_var("ARCA_HOME", "relative-arca-home");

    let error = list_inventory().unwrap_err();
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(matches!(error, SkillError::InvalidSource { .. }));
    assert!(!rendered.contains(th.home.to_string_lossy().as_ref()));
    assert!(!rendered.contains("settings.json"));
}

#[test]
fn external_list_scan_reads_only_the_bounded_manifest_but_detail_walks_the_tree() {
    let th = TestHome::new("inventory-minimal-external-scan");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    let root = th.home.join(".agents/skills/external");
    write_skill(&root, "external", "Bounded external summary");
    symlink("../../../../outside", root.join("escape")).unwrap();

    let inventory = list_inventory().unwrap();
    let item = inventory
        .items
        .iter()
        .find(|item| item.identity == "target:agents-user:external")
        .unwrap();
    assert_eq!(item.description, "Bounded external summary");
    assert!(item.content_hash.is_none());
    assert!(item.consumer_agent_ids.contains(&"codex".to_string()));

    let error = get_skill_detail(&item.identity).unwrap_err();
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(!rendered.contains(th.home.to_string_lossy().as_ref()));
}

#[test]
fn malformed_external_targets_never_claim_consumers() {
    let th = TestHome::new("inventory-malformed-external-consumers");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    let root = th.home.join(".agents/skills/invalid");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("SKILL.md"), "No manifest").unwrap();
    mutate_settings(|settings| {
        settings.managed_skills.get_or_insert_default()
            .insert("invalid".into(), managed_record("invalid", "recorded"));
    }).unwrap();
    let inventory = list_inventory().unwrap();
    let item = inventory.items.iter()
        .find(|item| item.identity == "target:agents-user:invalid").unwrap();
    assert!(item.consumer_agent_ids.is_empty());
}

#[test]
fn unreadable_target_is_a_hard_path_free_error_not_a_missing_inventory() {
    let th = TestHome::new("inventory-unreadable-target");
    install_cursor(&th.home);
    let target = th.home.join(".cursor/skills");
    fs::create_dir_all(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o000)).unwrap();

    let result = list_inventory();

    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    let error = result.expect_err("an unreadable target must not be treated as empty");
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(matches!(
        error,
        SkillError::Io { .. } | SkillError::Conflict { .. }
    ));
    assert!(!rendered.contains(th.home.to_string_lossy().as_ref()));
}

#[test]
fn stable_declared_target_symlink_supports_scan_and_detail() {
    let th = TestHome::new("inventory-symlinked-target");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    let physical_root = th.home.join("managed/agent-skills");
    write_skill(
        &physical_root.join("shared"),
        "shared",
        "Stable symlink target",
    );
    fs::create_dir_all(th.home.join(".agents")).unwrap();
    symlink(&physical_root, th.home.join(".agents/skills")).unwrap();

    let inventory = list_inventory().unwrap();
    let item = inventory
        .items
        .iter()
        .find(|item| item.identity == "target:agents-user:shared")
        .unwrap();
    assert!(item.states.contains(&InventoryState::External));

    let detail = get_skill_detail(&item.identity).unwrap();
    assert!(detail.skill_md.contains("Stable symlink target"));
    assert!(detail.files.iter().any(|file| file.path == "SKILL.md"));
}

#[test]
fn detail_recomputes_external_content_kind_from_the_full_safe_tree() {
    let th = TestHome::new("inventory-detail-content-kind");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    let target = th.home.join(".agents/skills");

    let automation = target.join("automation");
    write_skill(&automation, "automation", "Automation fixture");
    fs::write(automation.join("run-tool"), "#!/bin/sh\n").unwrap();
    fs::set_permissions(
        automation.join("run-tool"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::create_dir(automation.join("assets")).unwrap();
    fs::write(automation.join("assets/icon.bin"), [0_u8]).unwrap();
    fs::create_dir(automation.join("references")).unwrap();
    fs::write(automation.join("references/guide.md"), "guide").unwrap();

    let assets = target.join("assets-kind");
    write_skill(&assets, "assets-kind", "Assets fixture");
    fs::create_dir(assets.join("assets")).unwrap();
    fs::write(assets.join("assets/icon.bin"), [0_u8, 1, 2]).unwrap();
    fs::create_dir(assets.join("references")).unwrap();
    fs::write(assets.join("references/guide.md"), "guide").unwrap();

    let reference = target.join("reference-kind");
    write_skill(&reference, "reference-kind", "Reference fixture");
    fs::create_dir(reference.join("references")).unwrap();
    fs::write(reference.join("references/guide.md"), "guide").unwrap();

    let instructions = target.join("instructions");
    write_skill(&instructions, "instructions", "Instructions fixture");

    for (name, expected) in [
        ("automation", arca_core::skills::SkillContentKind::Automation),
        ("assets-kind", arca_core::skills::SkillContentKind::Assets),
        (
            "reference-kind",
            arca_core::skills::SkillContentKind::Reference,
        ),
        (
            "instructions",
            arca_core::skills::SkillContentKind::Instructions,
        ),
    ] {
        let identity = format!("target:agents-user:{name}");
        let list_item = list_inventory()
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.identity == identity)
            .unwrap();
        assert_eq!(
            list_item.content_kind,
            arca_core::skills::SkillContentKind::Instructions
        );
        assert_eq!(
            get_skill_detail(&identity).unwrap().item.content_kind,
            expected
        );
    }
}

#[test]
fn modified_central_content_and_its_exact_links_expose_the_observed_fingerprint() {
    let th = TestHome::new("inventory-observed-central");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    let central = th.home.join(".arca/assets/skills/items/observed");
    write_skill(&central, "observed", "Original description");
    let original = hash_tree(&central).unwrap();
    mutate_settings(|settings| {
        settings.managed_skills.get_or_insert_default().insert("observed".into(), managed_record("observed", &original));
        settings.skill_assignments.get_or_insert_default().insert("observed".into(), ["agents-user".into()].into());
    }).unwrap();
    fs::create_dir_all(th.home.join(".agents/skills")).unwrap();
    symlink(&central, th.home.join(".agents/skills/observed")).unwrap();
    write_skill(&central, "observed", "Observed description");
    let observed = hash_tree(&central).unwrap();
    assert_ne!(original, observed);
    let inventory = list_inventory().unwrap();
    for identity in ["central:observed", "target:agents-user:observed"] {
        let item = inventory.items.iter().find(|item| item.identity == identity).unwrap();
        assert!(item.states.contains(&InventoryState::LocallyModified));
        assert_eq!(item.content_hash.as_deref(), Some(observed.as_str()));
        assert_eq!(item.description, "Observed description");
        assert!(item.risk.is_none(), "the installed audit must not certify changed content");
    }
    let rows = arca_core::assets::list_consumption_inventory().unwrap();
    let row = rows.consumptions.iter().find(|row| row.agent_id == "codex").unwrap();
    assert_eq!(row.status, arca_core::domain::assets::ConsumptionStatus::ExternalChanged);
    assert_eq!(row.reason.as_deref(), Some("skill_local_modification"));
}

#[test]
fn invalid_central_bytes_cannot_reuse_the_installed_hash_for_adoption() {
    let th = TestHome::new("inventory-invalid-central-fingerprint");
    let central = th.home.join(".arca/assets/skills/items/invalid");
    write_skill(&central, "invalid", "Valid before editing");
    let original = hash_tree(&central).unwrap();
    mutate_settings(|settings| {
        settings.managed_skills.get_or_insert_default().insert("invalid".into(), managed_record("invalid", &original));
    }).unwrap();
    for bytes in [b"broken manifest".as_slice(), &[0xff, 0xfe][..]] {
        fs::write(central.join("SKILL.md"), bytes).unwrap();
        let inventory = list_inventory().unwrap();
        let item = inventory.items.iter().find(|item| item.identity == "central:invalid").unwrap();
        assert!(item.states.contains(&InventoryState::LocallyModified));
        assert!(item.content_hash.is_none());
        assert!(item.risk.is_none());
    }
}

#[test]
fn same_name_external_directory_never_inherits_central_provenance_or_hash() {
    let th = TestHome::new("inventory-external-provenance");
    fs::create_dir_all(th.home.join(".codex")).unwrap();
    let central = th.home.join(".arca/assets/skills/items/same-name");
    write_skill(&central, "same-name", "Central description");
    let original = hash_tree(&central).unwrap();
    mutate_settings(|settings| {
        settings.managed_skills.get_or_insert_default().insert("same-name".into(), managed_record("same-name", &original));
    }).unwrap();
    write_skill(&th.home.join(".agents/skills/same-name"), "same-name", "External description");
    let inventory = list_inventory().unwrap();
    let item = inventory.items.iter().find(|item| item.identity == "target:agents-user:same-name").unwrap();
    assert_eq!(item.description, "External description");
    assert!(item.source.is_none() && item.content_hash.is_none() && item.risk.is_none());
    assert!(item.states.contains(&InventoryState::External));
}
