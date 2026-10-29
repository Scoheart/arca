use super::*;
use crate::testenv::TestHome;

const PAYLOAD: &[u8] = b"#!/bin/sh\nprintf executed > \"$HOME/crush-executed-marker\"\n";

fn fixture(home: &TestHome) -> (Contract, PathBuf) {
    let path = home.home.join("bin/crush");
    fs::write(&path, PAYLOAD).unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    (Contract {
        version: "fixture-version".into(),
        binary_bytes: 58,
        binary_sha256: "2b7ee410899914a2d44f29d182ce135ce840fbc12756ba5742f3829f3874ebd7".into(),
    }, path)
}

// Catches replacing complete-file identity with a header/version-string probe.
#[test]
fn selected_artifact_supplies_version_without_executing_its_bytes() {
    let home = TestHome::new("crush-passive-identity");
    let (contract, path) = fixture(&home);
    assert_eq!(version_for(&contract, &path).as_deref(), Some("fixture-version"));
    assert!(!home.home.join("crush-executed-marker").exists());

    let mut changed = PAYLOAD.to_vec();
    changed[PAYLOAD.len() - 2] = b'X';
    fs::write(&path, &changed).unwrap();
    assert_eq!(version_for(&contract, &path), None);
    fs::write(&path, [PAYLOAD, b"x"].concat()).unwrap();
    assert_eq!(version_for(&contract, &path), None);
    fs::write(&path, &PAYLOAD[..PAYLOAD.len() - 1]).unwrap();
    assert_eq!(version_for(&contract, &path), None);
    assert!(!home.home.join("crush-executed-marker").exists());
}

// Catches accidentally sending an unrecognized Crush installation to cli_version.
#[test]
fn unknown_selected_runtime_never_executes_or_borrows_a_sibling_installation() {
    let home = TestHome::new("crush-passive-unknown");
    let (contract, selected) = fixture(&home);
    let sibling = home.home.join(".local/bin/crush");
    fs::create_dir_all(sibling.parent().unwrap()).unwrap();
    fs::write(&sibling, PAYLOAD).unwrap();
    fs::write(&selected, [PAYLOAD, b"# unknown selected runtime\n"].concat()).unwrap();
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sibling, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(version_for(&contract, &sibling).as_deref(), Some("fixture-version"));
    assert_eq!(version_for(&contract, &selected), None);
    assert_eq!(runtime_version(&selected), None);
    assert_eq!(super::super::passive_runtime_version("crush", &selected), Some(None));
    assert_eq!(crate::application::agent_launch::runtime_version("crush").unwrap(), None);
    assert!(!home.home.join("crush-executed-marker").exists());
}

// Catches sharing the private-metadata read allowance or omitting the hash bound.
#[test]
fn complete_artifact_hashing_uses_a_separate_finite_budget() {
    let home = TestHome::new("crush-passive-budget");
    let (contract, path) = fixture(&home);
    let mut budget = ScanBudget { bytes: MAX_READ_BYTES, ..ScanBudget::default() };
    assert_eq!(verified_runtime(&contract, &path, &mut budget).unwrap(), path);
    assert_eq!(budget.bytes, MAX_READ_BYTES);
    assert_eq!(budget.verified_bytes, 58);

    let mut exhausted = ScanBudget { verified_bytes: MAX_VERIFIED_BYTES, ..ScanBudget::default() };
    assert!(verified_runtime(&contract, &path, &mut exhausted).is_err());
    let mut oversized = contract;
    oversized.binary_bytes = MAX_VERIFIED_BYTES as u64 + 1;
    assert_eq!(version_for(&oversized, &path), None);
    assert!(!home.home.join("crush-executed-marker").exists());
}

// Catches accepting an ambiguous selection or opening directories/special files.
#[test]
fn invalid_paths_and_nonregular_selections_fail_closed() {
    let home = TestHome::new("crush-passive-paths");
    let (contract, selected) = fixture(&home);
    fs::write(home.home.join("bin/crush\n"), PAYLOAD).unwrap();
    for path in [
        PathBuf::from("bin/crush"),
        home.home.join("missing"),
        home.home.join("bin"),
        home.home.join("bin/../bin/crush"),
        home.home.join("bin/crush\n"),
    ] {
        assert_eq!(version_for(&contract, &path), None, "{}", path.display());
    }
    assert_eq!(runtime_version(&selected), None);
    assert!(!home.home.join("crush-executed-marker").exists());
}

#[cfg(unix)]
#[test]
fn selected_symlink_is_allowed_but_retargeting_and_same_path_replacement_are_rejected() {
    use std::os::unix::fs::symlink;
    let home = TestHome::new("crush-passive-cas");
    let (contract, selected) = fixture(&home);
    let physical = home.home.join("runtime/crush");
    fs::create_dir_all(physical.parent().unwrap()).unwrap();
    fs::rename(&selected, &physical).unwrap();
    symlink(&physical, &selected).unwrap();
    assert_eq!(version_for(&contract, &selected).as_deref(), Some("fixture-version"));

    // A new alias inode pointing to the same physical file is still a changed
    // caller selection, even though canonicalization alone would accept it.
    let selection = RuntimeSelection::capture(&selected).unwrap();
    let file = verified_artifact(&selection.physical, 58, &contract.binary_sha256, &mut ScanBudget::default()).unwrap();
    let alias_replacement = home.home.join("bin/replacement-alias");
    symlink(&physical, &alias_replacement).unwrap();
    fs::rename(&alias_replacement, &selected).unwrap();
    assert!(selection.revalidate(&selected, &file).is_err());

    let selection = RuntimeSelection::capture(&selected).unwrap();
    let file = verified_artifact(&selection.physical, 58, &contract.binary_sha256, &mut ScanBudget::default()).unwrap();
    let other = home.home.join("runtime/other");
    fs::write(&other, PAYLOAD).unwrap();
    fs::remove_file(&selected).unwrap();
    symlink(&other, &selected).unwrap();
    assert!(selection.revalidate(&selected, &file).is_err());

    let selection = RuntimeSelection::capture(&selected).unwrap();
    let file = verified_artifact(&selection.physical, 58, &contract.binary_sha256, &mut ScanBudget::default()).unwrap();
    let replacement = home.home.join("runtime/replacement");
    fs::write(&replacement, PAYLOAD).unwrap();
    fs::rename(&replacement, &other).unwrap();
    assert!(selection.revalidate(&selected, &file).is_err());
    assert!(!home.home.join("crush-executed-marker").exists());
}

#[cfg(unix)]
#[test]
fn fifo_dangling_symlink_and_non_utf8_selection_are_rejected_without_reading() {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;
    let home = TestHome::new("crush-passive-special");
    let (contract, _) = fixture(&home);
    let fifo = home.home.join("bin/fifo");
    rustix::fs::mkfifoat(rustix::fs::CWD, &fifo, rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR).unwrap();
    let fifo_alias = home.home.join("bin/fifo-alias");
    symlink(&fifo, &fifo_alias).unwrap();
    let dangling = home.home.join("bin/dangling");
    symlink(home.home.join("missing"), &dangling).unwrap();
    let invalid = home.home.join(std::ffi::OsString::from_vec(vec![0xff]));
    fs::write(&invalid, PAYLOAD).unwrap();
    for path in [fifo, fifo_alias, dangling, invalid] {
        assert_eq!(version_for(&contract, &path), None);
    }
    assert!(!home.home.join("crush-executed-marker").exists());
}

#[test]
fn runtime_contract_rejects_unrecognized_fields() {
    let _home = TestHome::new("crush-passive-schema");
    let mut value: Value = serde_json::from_str(include_str!("../../../../../data/native-runtime-crush.json")).unwrap();
    value.as_object_mut().unwrap().insert("fallback_command".into(), Value::String("crush --version".into()));
    assert!(serde_json::from_value::<Contract>(value).is_err());
}
