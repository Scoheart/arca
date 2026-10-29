use super::*;
use super::super::artifact_runtime::{RuntimeSelection, verified_runtime, version_for};
use super::super::{MAX_READ_BYTES, MAX_VERIFIED_BYTES, ScanBudget, passive_runtime_version, verified_artifact};
use crate::testenv::TestHome;
use std::{fs, path::PathBuf};

const PAYLOAD: &[u8] = b"#!/bin/sh\nprintf executed > \"$HOME/traecli-executed-marker\"\n";

fn fixture(home: &TestHome) -> (Contract, PathBuf) {
    let path = home.home.join("runtime/traex");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, PAYLOAD).unwrap();
    executable(&path);
    (Contract {
        version: "fixture-release".into(),
        binary_bytes: 60,
        binary_sha256: "77ed74c225799feb561b5e5bde526ef22135cbc5b6ec2d4cf05ad7d70f10fac1".into(),
    }, path)
}

fn executable(path: &Path) {
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))] { let _ = path; }
}

// Catches both a missing official launcher and an unsafe trae-agent alias.
#[test]
fn default_launcher_finds_traecli_without_running_or_borrowing_trae_agent() {
    let home = TestHome::new("trae-launcher");
    let other_product = home.home.join("bin/trae-agent");
    fs::write(&other_product, PAYLOAD).unwrap();
    executable(&other_product);
    let absent = crate::application::agent_launch::info("trae-cli").unwrap();
    assert!(!absent.available);
    assert_eq!(absent.installed, Some(false));
    assert_eq!(crate::application::agent_launch::runtime_version("trae-cli").unwrap(), None);

    let selected = home.home.join("bin/traecli");
    fs::write(&selected, PAYLOAD).unwrap();
    executable(&selected);
    let found = crate::application::agent_launch::info("trae-cli").unwrap();
    assert_eq!(found.available, cfg!(target_os = "macos"));
    assert_eq!(found.installed, Some(true));
    assert!(matches!(
        found.resolved_target,
        Some(crate::application::agent_launch::LaunchTarget::Cli { command, .. })
            if command == selected.to_string_lossy().into_owned()
    ));
    assert_eq!(crate::application::agent_launch::runtime_version("trae-cli").unwrap(), None);
    assert!(!home.home.join("traecli-executed-marker").exists());
}

// Catches replacing whole-artifact identity with filename or version-text probes.
#[test]
fn whole_artifact_fixture_labels_without_execution_and_rejects_changes() {
    let home = TestHome::new("trae-digest");
    let (contract, path) = fixture(&home);
    assert_eq!(version_for(&contract, &path).as_deref(), Some("fixture-release"));

    let mut changed = PAYLOAD.to_vec();
    changed[PAYLOAD.len() - 2] = b'X';
    fs::write(&path, changed).unwrap();
    assert_eq!(version_for(&contract, &path), None);
    fs::write(&path, [PAYLOAD, b"x"].concat()).unwrap();
    assert_eq!(version_for(&contract, &path), None);
    fs::write(&path, &PAYLOAD[..PAYLOAD.len() - 1]).unwrap();
    assert_eq!(version_for(&contract, &path), None);
    assert!(!home.home.join("traecli-executed-marker").exists());
}

// Catches unknown-image fallback and borrowing another installation's version.
#[test]
fn unknown_selection_cannot_borrow_a_release_or_mutable_metadata() {
    let home = TestHome::new("trae-unknown");
    let (contract, sibling) = fixture(&home);
    let selected = home.home.join("bin/traecli");
    fs::write(&selected, [PAYLOAD, b"# unknown image\n"].concat()).unwrap();
    executable(&selected);
    fs::write(home.home.join("bin/current_version"), "0.212.1\n").unwrap();
    fs::write(
        home.home.join("bin/trae-package.json"),
        r#"{"layoutVersion":1,"version":"0.212.1","entrypoint":"traecli"}"#,
    ).unwrap();
    assert_eq!(version_for(&contract, &sibling).as_deref(), Some("fixture-release"));
    assert_eq!(version_for(&contract, &selected), None);
    assert_eq!(runtime_version(&selected), None);
    assert_eq!(passive_runtime_version("trae-cli", &selected), Some(None));
    assert!(!home.home.join("traecli-executed-marker").exists());
}

// Catches treating the companion or unrelated TRAE Agent as the main artifact.
#[test]
fn sidecar_or_other_product_names_never_supply_the_main_version() {
    let home = TestHome::new("trae-identity");
    let (contract, main) = fixture(&home);
    let sidecar = main.with_file_name("traex-code-mode-host");
    let other_product = home.home.join("bin/trae-agent");
    for path in [&sidecar, &other_product] {
        fs::write(path, b"unrelated native image").unwrap();
        assert_eq!(version_for(&contract, path), None);
        assert_eq!(runtime_version(path), None);
    }
    // An unrelated companion does not change a known main's artifact label.
    assert_eq!(version_for(&contract, &main).as_deref(), Some("fixture-release"));
    assert!(!home.home.join("traecli-executed-marker").exists());
}

// Catches spending the resource-read allowance or removing the digest bound.
#[test]
fn artifact_digest_has_an_independent_finite_budget() {
    let home = TestHome::new("trae-budget");
    let (contract, path) = fixture(&home);
    let mut resource_exhausted = ScanBudget { bytes: MAX_READ_BYTES, ..ScanBudget::default() };
    assert_eq!(verified_runtime(&contract, &path, &mut resource_exhausted).unwrap(), path);
    assert_eq!(resource_exhausted.bytes, MAX_READ_BYTES);
    assert_eq!(resource_exhausted.verified_bytes, 60);

    let mut digest_exhausted = ScanBudget { verified_bytes: MAX_VERIFIED_BYTES, ..ScanBudget::default() };
    assert!(verified_runtime(&contract, &path, &mut digest_exhausted).is_err());
    assert!(!home.home.join("traecli-executed-marker").exists());
}

#[cfg(unix)]
#[test]
fn official_style_alias_allows_identity_but_replacement_fails_cas() {
    use std::os::unix::fs::symlink;
    let home = TestHome::new("trae-alias");
    let (contract, physical) = fixture(&home);
    let current = home.home.join("current");
    symlink(physical.parent().unwrap(), &current).unwrap();
    let main_alias = home.home.join("bin/traex");
    symlink(current.join("traex"), &main_alias).unwrap();
    let selected = home.home.join("bin/traecli");
    symlink(&main_alias, &selected).unwrap();
    assert_eq!(version_for(&contract, &selected).as_deref(), Some("fixture-release"));

    let selection = RuntimeSelection::capture(&selected).unwrap();
    let file = verified_artifact(
        &selection.physical, contract.binary_bytes, &contract.binary_sha256, &mut ScanBudget::default(),
    ).unwrap();
    let alias_replacement = home.home.join("bin/replacement-alias");
    symlink(&main_alias, &alias_replacement).unwrap();
    fs::rename(&alias_replacement, &selected).unwrap();
    assert!(selection.revalidate(&selected, &file).is_err());

    let selection = RuntimeSelection::capture(&selected).unwrap();
    let file = verified_artifact(
        &selection.physical, contract.binary_bytes, &contract.binary_sha256, &mut ScanBudget::default(),
    ).unwrap();
    let replacement = home.home.join("runtime/replacement");
    fs::write(&replacement, PAYLOAD).unwrap();
    fs::rename(&replacement, &physical).unwrap();
    assert!(selection.revalidate(&selected, &file).is_err());
    assert!(!home.home.join("traecli-executed-marker").exists());
}

// Catches malformed descriptors introducing an executable version fallback.
#[test]
fn descriptor_rejects_unknown_fields() {
    let _home = TestHome::new("trae-schema");
    let mut value: serde_json::Value = serde_json::from_str(
        include_str!("../../../../../data/native-runtime-trae-cli.json"),
    ).unwrap();
    value.as_object_mut().unwrap().insert(
        "fallback_command".into(), serde_json::Value::String("traecli --version".into()),
    );
    assert!(serde_json::from_value::<Contract>(value).is_err());
}
