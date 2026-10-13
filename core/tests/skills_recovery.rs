#![cfg(unix)]

mod support;

use arca_core::skills::{
    crash_transaction_before_phase_for_test, execute_transaction,
    execute_transaction_with_failpoint, hash_tree, recover_pending_with_paths, CrashPoint,
    DirectoryMutation, Failpoint, JournalPhase, LinkState, SkillError, SkillSource,
    TransactionOrder,
};
use serde_json::json;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use support::skills::{TransactionFixture, TRANSACTION_OPERATION_ID};

#[test]
fn runtime_failure_restores_content_links_and_skill_settings() {
    let fixture = TransactionFixture::managed("rollback");
    arca_core::settings::mutate_settings(|settings| {
        settings
            .extra
            .insert("future".into(), json!({"kept": true}));
    })
    .unwrap();
    let before = fixture.snapshot();
    let error =
        execute_transaction_with_failpoint(fixture.update_spec(), Some(Failpoint::AfterFirstLink))
            .unwrap_err();
    assert!(matches!(error, SkillError::Io { .. }));
    assert_eq!(fixture.snapshot(), before);
    assert!(!fixture.paths.journals_dir().exists());
    assert!(!fixture
        .paths
        .staging_skills_dir()
        .join(TRANSACTION_OPERATION_ID)
        .exists());
}

#[test]
fn runtime_failure_restores_a_reviewed_real_directory_link() {
    let fixture = TransactionFixture::managed("directory-link");
    let link = fixture.spec.link_mutations[0].path.clone();
    fs::remove_file(&link).unwrap();
    fs::create_dir(&link).unwrap();
    fs::write(link.join("local.txt"), b"preserve me").unwrap();
    let tree_hash = hash_tree(&link).unwrap();
    let backup = fixture
        .paths
        .backups_skills_dir()
        .join(TRANSACTION_OPERATION_ID)
        .join("agent-directory");
    let before = fixture.snapshot();
    let mut spec = fixture.update_spec();
    spec.link_mutations[0].expected = LinkState::Directory { tree_hash };
    spec.link_mutations[0].backup = Some(backup.clone());

    let error =
        execute_transaction_with_failpoint(spec, Some(Failpoint::AfterFirstLink)).unwrap_err();

    assert!(matches!(error, SkillError::Io { .. }));
    assert_eq!(fixture.snapshot(), before);
    assert!(!backup.exists());
    assert!(!fixture.paths.journals_dir().exists());
}

#[test]
fn runtime_failure_restores_a_relative_managed_link_byte_for_byte() {
    let fixture = TransactionFixture::managed("relative-managed");
    let link = fixture.spec.link_mutations[0].path.clone();
    let raw_target = std::path::PathBuf::from("../../.arca/assets/skills/items/relative-managed");
    fs::remove_file(&link).unwrap();
    symlink(&raw_target, &link).unwrap();
    assert_eq!(
        fs::canonicalize(&link).unwrap(),
        fs::canonicalize(fixture.paths.central_skill("relative-managed")).unwrap()
    );
    let before = fixture.snapshot();
    let mut spec = fixture.update_spec();
    spec.link_mutations[0].expected = LinkState::ManagedSymlink {
        target: raw_target.clone(),
    };

    let error =
        execute_transaction_with_failpoint(spec, Some(Failpoint::AfterFirstLink)).unwrap_err();

    assert!(matches!(error, SkillError::Io { .. }));
    assert_eq!(fixture.snapshot(), before);
    assert_eq!(fs::read_link(link).unwrap(), raw_target);
}

#[test]
fn relative_managed_link_can_be_safely_disabled() {
    let fixture = TransactionFixture::managed("relative-disable");
    let link = fixture.spec.link_mutations[0].path.clone();
    let raw_target = std::path::PathBuf::from("../../.arca/assets/skills/items/relative-disable");
    fs::remove_file(&link).unwrap();
    symlink(&raw_target, &link).unwrap();
    let mut spec = fixture.update_spec();
    spec.directory_mutations.clear();
    spec.link_mutations[0].expected = LinkState::ManagedSymlink { target: raw_target };
    spec.link_mutations[0].desired_target = None;
    spec.settings_after.skill_assignments = None;

    execute_transaction(spec).unwrap();

    assert!(matches!(
        fs::symlink_metadata(&link),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    ));
    assert!(fixture.paths.central_skill("relative-disable").exists());
    assert!(arca_core::settings::load_settings()
        .skill_assignments
        .is_none());
}

#[test]
fn changing_only_managed_link_raw_bytes_after_review_is_stale() {
    let fixture = TransactionFixture::managed("relative-stale");
    let link = fixture.spec.link_mutations[0].path.clone();
    let reviewed_raw = std::path::PathBuf::from("../../.arca/assets/skills/items/relative-stale");
    fs::remove_file(&link).unwrap();
    symlink(&reviewed_raw, &link).unwrap();
    let mut spec = fixture.update_spec();
    spec.directory_mutations.clear();
    spec.link_mutations[0].expected = LinkState::ManagedSymlink {
        target: reviewed_raw,
    };
    spec.link_mutations[0].desired_target = None;

    fs::remove_file(&link).unwrap();
    let replacement_raw = fixture.paths.central_skill("relative-stale");
    symlink(&replacement_raw, &link).unwrap();

    assert!(matches!(
        execute_transaction(spec),
        Err(SkillError::PlanStale { .. })
    ));
    assert_eq!(fs::read_link(&link).unwrap(), replacement_raw);
    assert_eq!(journal_count(&fixture), 0);
}

#[test]
fn runtime_failure_restores_missing_broken_and_unknown_link_states() {
    for (name, state) in [
        ("missing-link", "missing"),
        ("broken-link", "broken"),
        ("unknown-link", "unknown"),
    ] {
        let fixture = TransactionFixture::managed(name);
        let link = fixture.spec.link_mutations[0].path.clone();
        fs::remove_file(&link).unwrap();
        let expected = match state {
            "missing" => LinkState::Missing,
            "broken" => {
                let target = fixture.home.home.join("missing-target");
                symlink(&target, &link).unwrap();
                LinkState::BrokenSymlink { target }
            }
            "unknown" => {
                let target = fixture.home.home.join("external-target");
                fs::create_dir(&target).unwrap();
                symlink(&target, &link).unwrap();
                LinkState::UnknownSymlink { target }
            }
            _ => unreachable!(),
        };
        let before = fixture.snapshot();
        let mut spec = fixture.update_spec();
        spec.link_mutations[0].expected = expected;

        let error =
            execute_transaction_with_failpoint(spec, Some(Failpoint::AfterFirstLink)).unwrap_err();

        assert!(matches!(error, SkillError::Io { .. }), "failed for {state}");
        assert_eq!(fixture.snapshot(), before, "failed for {state}");
        assert!(!fixture.paths.journals_dir().exists());
    }
}

#[test]
fn recovery_uses_disk_evidence_when_crash_precedes_phase_persistence() {
    for point in [
        CrashPoint::AfterContentBeforePhase,
        CrashPoint::AfterLinksBeforePhase,
        CrashPoint::AfterSettingsBeforePhase,
    ] {
        let fixture = TransactionFixture::managed("phase-seam");
        crash_transaction_before_phase_for_test(fixture.update_spec(), point).unwrap();
        recover_pending_with_paths(&fixture.paths).unwrap();
        assert_eq!(
            fixture.snapshot(),
            fixture.before_snapshot,
            "failed at {point:?}"
        );
    }
}

#[test]
fn recovery_removes_a_partial_same_parent_copy_left_by_a_crash() {
    let fixture = TransactionFixture::crashed_at(JournalPhase::Prepared);
    let temporary = fixture
        .paths
        .skills_dir()
        .join(format!(".arca-transaction-{TRANSACTION_OPERATION_ID}-0.tmp"));
    fs::create_dir(&temporary).unwrap();
    fs::write(temporary.join("partial"), b"incomplete copy").unwrap();

    recover_pending_with_paths(&fixture.paths).unwrap();

    assert!(!temporary.exists());
    assert_eq!(fixture.snapshot(), fixture.before_snapshot);
}

#[test]
fn startup_recovery_is_idempotent_at_every_durable_phase() {
    for phase in [
        JournalPhase::Prepared,
        JournalPhase::ContentSwapped,
        JournalPhase::LinksSwapped,
        JournalPhase::SettingsWritten,
    ] {
        let fixture = TransactionFixture::crashed_at(phase);
        recover_pending_with_paths(&fixture.paths).unwrap();
        let once = fixture.snapshot();
        recover_pending_with_paths(&fixture.paths).unwrap();
        assert_eq!(fixture.snapshot(), once);
        assert_eq!(once, fixture.before_snapshot);
    }
}

#[test]
fn recovery_finishes_a_commit_if_cleanup_already_removed_its_rollback_backup() {
    let fixture = TransactionFixture::crashed_at(JournalPhase::SettingsWritten);
    fs::remove_dir_all(
        fixture
            .paths
            .staging_skills_dir()
            .join(TRANSACTION_OPERATION_ID),
    )
    .unwrap();
    fs::remove_dir_all(&fixture.spec.directory_mutations[0].backup).unwrap();
    let committed = fixture.snapshot();
    assert_ne!(committed, fixture.before_snapshot);

    recover_pending_with_paths(&fixture.paths).unwrap();

    assert_eq!(fixture.snapshot(), committed);
    assert!(!fixture.paths.journals_dir().exists());
}

#[test]
fn stale_directory_link_and_skill_settings_preconditions_do_not_mutate() {
    let fixture = TransactionFixture::managed("stale");
    let before = fixture.snapshot();

    fs::write(
        fixture.paths.central_skill("stale").join("SKILL.md"),
        "---\nname: stale\ndescription: Concurrent edit\n---\n",
    )
    .unwrap();
    assert!(matches!(
        execute_transaction(fixture.update_spec()),
        Err(SkillError::PlanStale { .. })
    ));
    assert_eq!(journal_count(&fixture), 0);

    fs::remove_dir_all(fixture.paths.central_skill("stale")).unwrap();
    support::skills::write_skill(
        &fixture.paths.central_skill("stale"),
        "stale",
        "Managed fixture",
    );
    let link = &fixture.spec.link_mutations[0].path;
    fs::remove_file(link).unwrap();
    symlink(fixture.home.home.join("unknown"), link).unwrap();
    assert!(matches!(
        execute_transaction(fixture.update_spec()),
        Err(SkillError::PlanStale { .. })
    ));
    assert_eq!(journal_count(&fixture), 0);

    fs::remove_file(link).unwrap();
    symlink(fixture.paths.central_skill("stale"), link).unwrap();
    arca_core::settings::mutate_settings(|settings| {
        settings.skill_update_checked_at = Some("concurrent".into());
    })
    .unwrap();
    assert!(matches!(
        execute_transaction(fixture.update_spec()),
        Err(SkillError::PlanStale { .. })
    ));
    assert_eq!(journal_count(&fixture), 0);
    assert_ne!(fixture.snapshot(), before);
}

#[test]
fn removal_uses_links_then_content_and_cleans_consumed_state() {
    let fixture = TransactionFixture::managed("remove");
    let mut spec = fixture.update_spec();
    spec.order = TransactionOrder::LinksThenContent;
    spec.directory_mutations[0].replacement = None;
    spec.link_mutations[0].desired_target = None;
    spec.settings_after.managed_skills = None;
    spec.settings_after.skill_assignments = None;

    execute_transaction(spec).unwrap();

    assert!(!fixture.paths.central_skill("remove").exists());
    assert!(!fixture.spec.link_mutations[0].path.exists());
    assert!(!fixture.paths.journals_dir().exists());
    assert!(!fixture
        .paths
        .staging_skills_dir()
        .join(TRANSACTION_OPERATION_ID)
        .exists());
    assert!(!fixture.spec.directory_mutations[0].backup.exists());
}

#[test]
fn links_then_content_removal_recovers_after_the_content_move() {
    let fixture = TransactionFixture::managed("remove-recovery");
    let mut spec = fixture.update_spec();
    spec.order = TransactionOrder::LinksThenContent;
    spec.directory_mutations[0].replacement = None;
    spec.link_mutations[0].desired_target = None;
    spec.settings_after.managed_skills = None;
    spec.settings_after.skill_assignments = None;

    crash_transaction_before_phase_for_test(spec, CrashPoint::AfterContentBeforePhase).unwrap();
    recover_pending_with_paths(&fixture.paths).unwrap();

    assert_eq!(fixture.snapshot(), fixture.before_snapshot);
}

#[test]
fn unrelated_settings_fields_survive_commit_and_rollback() {
    let fixture = TransactionFixture::managed("settings-extra");
    arca_core::settings::mutate_settings(|settings| {
        settings
            .extra
            .insert("future".into(), json!({"kept": true}));
    })
    .unwrap();

    execute_transaction(fixture.update_spec()).unwrap();

    assert_eq!(
        arca_core::settings::load_settings().extra.get("future"),
        Some(&json!({"kept": true}))
    );
}

#[test]
fn successful_import_keeps_the_backup_referenced_by_settings() {
    let fixture = TransactionFixture::managed("import-backup");
    let mut spec = fixture.update_spec();
    let backup = spec.directory_mutations[0].backup.clone();
    let before_hash = spec.directory_mutations[0]
        .expected_before_hash
        .clone()
        .unwrap();
    spec.settings_after
        .managed_skills
        .as_mut()
        .unwrap()
        .get_mut("import-backup")
        .unwrap()
        .source = SkillSource::Imported {
        original_path: "~/.agents/skills/import-backup".into(),
        backup_path: backup.to_string_lossy().into_owned(),
    };

    execute_transaction(spec).unwrap();

    assert_eq!(hash_tree(&backup).unwrap(), before_hash);
    assert!(!fixture.paths.journals_dir().exists());
}

#[test]
fn successful_transaction_keeps_an_explicitly_retained_backup() {
    let fixture = TransactionFixture::managed("explicit-retained-backup");
    let mut spec = fixture.update_spec();
    let mutation = &mut spec.directory_mutations[0];
    mutation.retain_backup = true;
    let backup = mutation.backup.clone();
    let before_hash = mutation.expected_before_hash.clone().unwrap();

    execute_transaction(spec).unwrap();

    assert_eq!(hash_tree(&backup).unwrap(), before_hash);
    assert!(!fixture.paths.journals_dir().exists());
}

#[test]
fn malformed_and_out_of_root_journals_never_mutate_the_named_path() {
    let fixture = TransactionFixture::managed("journal-bounds");
    let outside = fixture.home.home.join("outside");
    fs::write(&outside, b"untouched").unwrap();
    let journals = fixture.paths.journals_dir();
    fs::create_dir_all(&journals).unwrap();
    let malformed = journals.join("malformed.json");
    fs::write(&malformed, b"not-json").unwrap();
    fs::set_permissions(&malformed, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        recover_pending_with_paths(&fixture.paths),
        Err(SkillError::RecoveryRequired { .. })
    ));
    assert_eq!(fs::read(&outside).unwrap(), b"untouched");

    fs::remove_file(journals.join("malformed.json")).unwrap();
    let mut spec = fixture.update_spec();
    spec.directory_mutations[0].destination = outside.clone();
    let out_of_root = journals.join(format!("{TRANSACTION_OPERATION_ID}.json"));
    fs::write(
        &out_of_root,
        serde_json::to_vec(&json!({"spec": spec, "phase": "prepared"})).unwrap(),
    )
    .unwrap();
    fs::set_permissions(&out_of_root, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        recover_pending_with_paths(&fixture.paths),
        Err(SkillError::RecoveryRequired { .. })
    ));
    assert_eq!(fs::read(&outside).unwrap(), b"untouched");
}

#[test]
fn recovery_never_follows_a_journal_file_symlink() {
    let fixture = TransactionFixture::managed("journal-symlink");
    let before = fixture.snapshot();
    let external = fixture.home.home.join("external-journal");
    fs::write(&external, b"external bytes").unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o600)).unwrap();
    let journal = fixture
        .paths
        .journals_dir()
        .join(format!("{TRANSACTION_OPERATION_ID}.json"));
    symlink(&external, journal).unwrap();

    assert!(matches!(
        recover_pending_with_paths(&fixture.paths),
        Err(SkillError::RecoveryRequired { .. })
    ));
    assert_eq!(fs::read(external).unwrap(), b"external bytes");
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn recovery_validates_every_journal_before_rolling_back_the_first() {
    let fixture = TransactionFixture::crashed_at(JournalPhase::ContentSwapped);
    let crashed = fixture.snapshot();
    assert_ne!(crashed, fixture.before_snapshot);
    let later = fixture
        .paths
        .journals_dir()
        .join("ffffffff-ffff-4fff-8fff-ffffffffffff.json");
    fs::write(&later, b"not-json").unwrap();
    fs::set_permissions(&later, fs::Permissions::from_mode(0o600)).unwrap();

    assert!(matches!(
        recover_pending_with_paths(&fixture.paths),
        Err(SkillError::RecoveryRequired { .. })
    ));
    assert_eq!(fixture.snapshot(), crashed);

    fs::remove_file(later).unwrap();
    recover_pending_with_paths(&fixture.paths).unwrap();
    assert_eq!(fixture.snapshot(), fixture.before_snapshot);
}

#[test]
fn lexical_root_membership_cannot_hide_a_parent_symlink_escape() {
    let fixture = TransactionFixture::managed("path-escape");
    let outside = fixture.home.home.join("outside-tree");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"untouched").unwrap();
    let alias = fixture.paths.skills_dir().join("alias");
    symlink(&outside, &alias).unwrap();
    let mut spec = fixture.update_spec();
    spec.directory_mutations.push(DirectoryMutation {
        replacement: None,
        destination: alias.join("sentinel"),
        backup: fixture
            .paths
            .backups_skills_dir()
            .join(TRANSACTION_OPERATION_ID)
            .join("sentinel"),
        expected_before_hash: None,
        retain_backup: false,
    });

    assert!(matches!(
        execute_transaction(spec),
        Err(SkillError::UnsafePath { .. }) | Err(SkillError::InvalidSource { .. })
    ));
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"untouched");
}

#[test]
fn operation_ids_must_be_canonical_hyphenated_uuids() {
    let fixture = TransactionFixture::managed("uuid");
    for invalid in [
        "../../escape",
        "00000000000040008000000000000006",
        "00000000-0000-4000-8000-000000000006/child",
        "00000000-0000-4000-8000-000000000006.json",
    ] {
        let mut spec = fixture.update_spec();
        spec.operation_id = invalid.into();
        assert!(matches!(
            execute_transaction(spec),
            Err(SkillError::InvalidSource { .. })
        ));
    }
    assert!(!fixture.home.home.join("escape").exists());
}

#[test]
fn transaction_paths_reject_parent_components_even_when_they_stay_under_the_root() {
    let fixture = TransactionFixture::managed("path-traversal");
    let before = fixture.snapshot();
    let mut spec = fixture.update_spec();
    spec.directory_mutations[0].backup = fixture
        .paths
        .backups_skills_dir()
        .join(TRANSACTION_OPERATION_ID)
        .join("nested")
        .join("..")
        .join("path-traversal");

    assert!(matches!(
        execute_transaction(spec),
        Err(SkillError::UnsafePath { .. })
    ));
    assert_eq!(fixture.snapshot(), before);
    assert_eq!(journal_count(&fixture), 0);
}

#[test]
fn lock_and_journal_files_are_private() {
    let fixture = TransactionFixture::crashed_at(JournalPhase::Prepared);
    let lock_mode = fs::metadata(fixture.paths.skills_lock())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let journal = fixture
        .paths
        .journals_dir()
        .join(format!("{TRANSACTION_OPERATION_ID}.json"));
    let journal_mode = fs::metadata(&journal).unwrap().permissions().mode() & 0o777;
    assert_eq!(lock_mode, 0o600);
    assert_eq!(journal_mode, 0o600);
    let journal_bytes = fs::read(journal).unwrap();
    assert!(!journal_bytes
        .windows(b"Fixture body".len())
        .any(|window| window == b"Fixture body"));
}

#[test]
fn committed_content_preserves_private_and_executable_modes() {
    let fixture = TransactionFixture::managed("modes");
    execute_transaction(fixture.update_spec()).unwrap();
    let central = fixture.paths.central_skill("modes");
    assert_eq!(
        fs::metadata(central.join("SKILL.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(central.join("scripts/run.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o711
    );
}

#[test]
fn link_state_serialization_keeps_broken_and_unknown_distinct() {
    let broken = serde_json::to_value(LinkState::BrokenSymlink {
        target: "missing".into(),
    })
    .unwrap();
    let unknown = serde_json::to_value(LinkState::UnknownSymlink {
        target: "elsewhere".into(),
    })
    .unwrap();
    assert_ne!(broken, unknown);
}

fn journal_count(fixture: &TransactionFixture) -> usize {
    match fs::read_dir(fixture.paths.journals_dir()) {
        Ok(entries) => entries.count(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => panic!("read journal fixture: {error}"),
    }
}
