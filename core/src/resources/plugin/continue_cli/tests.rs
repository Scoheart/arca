use super::*;
use crate::testenv::TestHome;
use serde_json::json;

const WRAPPER: &[u8] = b"#!/usr/bin/env node\nimport { runCli } from './index.js';\nawait runCli();\n";
const BUNDLE: &[u8] = b"export async function runCli() { throw new Error('must never execute'); }\n";
const EXECUTION_MARKER: &[u8] = b"#!/bin/sh\nprintf executed > \"$HOME/continue-cli-executed-marker\"\n";

fn write(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn fixture(home: &TestHome) -> (Contract, PathBuf) {
    let root = home.home.join("npm/node_modules/@continuedev/cli");
    let package = br#"{"name":"@continuedev/cli","version":"1.5.47","type":"module","bin":{"cn":"dist/cn.js"}}"#;
    let sources = [("package.json", package.as_slice()), ("dist/cn.js", WRAPPER), ("dist/index.js", BUNDLE)]
        .into_iter().map(|(path, body)| {
            write(&root.join(path), body);
            SourceFile { path: path.into(), bytes: body.len() as u64, sha256: hex::encode(Sha256::digest(body)) }
        }).collect();
    (Contract {
        package_name: "@continuedev/cli".into(), version: "1.5.47".into(), module_type: "module".into(),
        command: "cn".into(), entrypoint: "dist/cn.js".into(), entry_import: "./index.js".into(),
        entry_export: "runCli".into(), sources,
    }, root.join("dist/cn.js"))
}

fn replace_public(c: &mut Contract, selected: &Path, relative: &str, body: &[u8]) {
    let root = selected.parent().unwrap().parent().unwrap();
    write(&root.join(relative), body);
    let source = c.sources.iter_mut().find(|s| s.path == relative).unwrap();
    source.bytes = body.len() as u64;
    source.sha256 = hex::encode(Sha256::digest(body));
}

// Removing any identity/entry check must not let a differently shaped package
// borrow the audited version, even when a test contract hashes its exact bytes.
#[test]
fn source_bound_public_package_supplies_version_without_loading_private_configuration() {
    let home = TestHome::new("cn-public");
    let (c, selected) = fixture(&home);
    write(&home.home.join(".continue/config.yaml"), b"PRIVATE_CONFIG_BODY");
    write(&home.home.join(".continue/auth.json"), b"PRIVATE_CREDENTIAL_BODY");
    write(&home.home.join(".continue/settings.json"), EXECUTION_MARKER);
    write(&home.home.join(".claude/skills/private/SKILL.md"), b"PRIVATE_SKILL_BODY");
    assert_eq!(version_for(&c, &selected).as_deref(), Some("1.5.47"));
    assert!(!home.home.join("continue-cli-executed-marker").exists());
}

#[cfg(unix)]
#[test]
fn npm_user_alias_resolves_only_to_its_selected_physical_wrapper() {
    use std::os::unix::fs::symlink;
    let home = TestHome::new("cn-alias");
    let (c, entry) = fixture(&home);
    let selected = home.home.join("bin/cn");
    symlink(&entry, &selected).unwrap();
    assert_eq!(version_for(&c, &selected).as_deref(), Some("1.5.47"));
}

#[test]
fn wrong_name_version_module_type_or_bin_cannot_borrow_the_runtime_identity() {
    let home = TestHome::new("cn-identity");
    for (field, value) in [("name", json!("@other/cli")), ("version", json!("9.9.9")),
        ("type", json!("commonjs")), ("bin", json!({"cn":"dist/other.js"})),
        ("bin", json!({"other":"dist/cn.js"}))] {
        let (mut c, selected) = fixture(&home);
        let mut manifest = json!({"name":"@continuedev/cli","version":"1.5.47","type":"module","bin":{"cn":"dist/cn.js"}});
        manifest[field] = value;
        replace_public(&mut c, &selected, "package.json", &serde_json::to_vec(&manifest).unwrap());
        assert_eq!(version_for(&c, &selected), None, "{field}");
    }
}

#[test]
fn malformed_public_manifest_returns_unknown_after_its_digest_matches() {
    let home = TestHome::new("cn-json");
    let (mut c, selected) = fixture(&home);
    replace_public(&mut c, &selected, "package.json", b"{ malformed");
    assert_eq!(version_for(&c, &selected), None);
}

#[test]
fn matched_script_digest_does_not_replace_the_actual_esm_entry_shape() {
    let home = TestHome::new("cn-loader");
    let (mut c, selected) = fixture(&home);
    for body in [EXECUTION_MARKER, b"#!/usr/bin/env node\nimport '../private.js';\n".as_slice(),
        b"#!/usr/bin/env node\nimport { other } from './index.js';\nawait other();\n".as_slice()] {
        replace_public(&mut c, &selected, "dist/cn.js", body);
        assert_eq!(version_for(&c, &selected), None);
    }
    assert!(!home.home.join("continue-cli-executed-marker").exists());
}

#[test]
fn full_bundle_digest_rejects_an_equal_length_change_and_missing_bundle() {
    let home = TestHome::new("cn-bundle");
    let (c, selected) = fixture(&home);
    let bundle = selected.parent().unwrap().join("index.js");
    let mut changed = BUNDLE.to_vec();
    changed[0] = b'X';
    write(&bundle, &changed);
    assert_eq!(version_for(&c, &selected), None);
    fs::remove_file(&bundle).unwrap();
    assert_eq!(version_for(&c, &selected), None);
}

#[test]
fn unsafe_or_incomplete_contract_paths_fail_before_any_extra_member_read() {
    let home = TestHome::new("cn-paths");
    for replacement in ["../private.json", "/private.json", "dist/../private.json", "dist\\private.js", ""] {
        let (mut c, selected) = fixture(&home);
        c.sources[0].path = replacement.into();
        assert_eq!(version_for(&c, &selected), None);
    }
    let (mut c, selected) = fixture(&home);
    c.sources.pop();
    assert_eq!(version_for(&c, &selected), None);
    let (mut c, selected) = fixture(&home);
    c.sources.push(SourceFile { path: "unrelated.json".into(), bytes: 1, sha256: "0".repeat(64) });
    assert_eq!(version_for(&c, &selected), None);
}

#[test]
fn independent_identity_budget_rejects_oversized_guards_and_does_not_leak_between_calls() {
    let home = TestHome::new("cn-budget");
    let (mut c, selected) = fixture(&home);
    c.sources.iter_mut().find(|s| s.path == "dist/index.js").unwrap().bytes = MAX_RUNTIME_BYTES as u64 + 1;
    assert_eq!(version_for(&c, &selected), None);
    let (c, selected) = fixture(&home);
    let mut exhausted = ScanBudget { verified_bytes: MAX_VERIFIED_BYTES, ..ScanBudget::default() };
    assert!(VerifiedRuntime::capture(&c, &selected, &mut exhausted).is_err());
    assert_eq!(version_for(&c, &selected).as_deref(), Some("1.5.47"));
}

#[cfg(unix)]
#[test]
fn linked_or_special_public_members_are_rejected_without_following_or_blocking() {
    use std::os::unix::fs::symlink;
    let home = TestHome::new("cn-special");
    for member in ["package.json", "dist/index.js"] {
        let (c, selected) = fixture(&home);
        let root = selected.parent().unwrap().parent().unwrap();
        let original = root.join(member);
        let outside = home.home.join("private-outside");
        fs::rename(&original, &outside).unwrap();
        symlink(&outside, &original).unwrap();
        assert_eq!(version_for(&c, &selected), None);
        fs::remove_file(&original).unwrap();
    }
    let (c, selected) = fixture(&home);
    let bundle = selected.parent().unwrap().join("index.js");
    fs::remove_file(&bundle).unwrap();
    rustix::fs::mkfifoat(rustix::fs::CWD, &bundle, rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR).unwrap();
    assert_eq!(version_for(&c, &selected), None);
}

#[cfg(unix)]
#[test]
fn retargeted_selected_symlink_invalidates_an_already_captured_runtime() {
    use std::os::unix::fs::symlink;
    let home = TestHome::new("cn-retarget");
    let (c, entry) = fixture(&home);
    let selected = home.home.join("bin/cn");
    symlink(&entry, &selected).unwrap();
    let identity = VerifiedRuntime::capture(&c, &selected, &mut ScanBudget::default()).unwrap();
    let other = home.home.join("other/dist/cn.js");
    write(&other, WRAPPER);
    fs::remove_file(&selected).unwrap();
    symlink(&other, &selected).unwrap();
    assert!(identity.revalidate(&selected).is_err());
}

#[test]
fn atomic_replacement_of_any_guard_invalidates_captured_descriptors() {
    let home = TestHome::new("cn-cas");
    for member in ["package.json", "dist/cn.js", "dist/index.js"] {
        let (c, selected) = fixture(&home);
        let identity = VerifiedRuntime::capture(&c, &selected, &mut ScanBudget::default()).unwrap();
        let path = selected.parent().unwrap().parent().unwrap().join(member);
        let replacement = path.with_extension("replacement");
        write(&replacement, &fs::read(&path).unwrap());
        fs::rename(&replacement, &path).unwrap();
        assert!(identity.revalidate(&selected).is_err(), "{member}");
    }
}

#[test]
fn relative_or_parent_traversing_selection_is_unknown() {
    let home = TestHome::new("cn-selection");
    let (c, selected) = fixture(&home);
    assert_eq!(version_for(&c, Path::new("dist/cn.js")), None);
    let unsafe_selected = selected.parent().unwrap().join("../dist/cn.js");
    assert_eq!(version_for(&c, &unsafe_selected), None);
}

#[test]
fn declarative_runtime_contract_rejects_unknown_fields() {
    let home = TestHome::new("cn-schema");
    let mut value: Value = serde_json::from_str(include_str!("../../../../../data/native-runtime-continue-cli.json")).unwrap();
    value["invented_plugin_root"] = json!("PRIVATE_PATH");
    assert!(serde_json::from_value::<Contract>(value).is_err());
    assert!(!home.home.join("continue-cli-executed-marker").exists());
}

#[test]
fn unsupported_continue_selection_blocks_active_version_fallback_without_a_plugin_host() {
    let home = TestHome::new("cn-passive");
    let selected = home.home.join("bin/cn");
    write(&selected, EXECUTION_MARKER);
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&selected, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(runtime_version(&selected), None);
    assert_eq!(super::super::passive_runtime_version("continue-cli", &selected), Some(None));
    assert_eq!(crate::application::agent_launch::runtime_version("continue-cli").unwrap(), None);
    assert!(!home.home.join("continue-cli-executed-marker").exists());
}

#[cfg(unix)]
#[test]
fn default_launcher_discovers_cn_independently_of_the_continue_ide_host() {
    use std::os::unix::fs::PermissionsExt;
    let home = TestHome::new("cn-default-launcher");
    let command = home.home.join("bin/cn");
    write(&command, EXECUTION_MARKER);
    fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
    let cli = crate::application::agent_launch::info("continue-cli").unwrap();
    assert!(cli.available);
    assert_eq!(cli.installed, Some(true));
    let Some(crate::domain::agents::LaunchTarget::Cli { command: resolved, args, env }) = cli.resolved_target else {
        panic!("Continue CLI must resolve its own official cn entry");
    };
    assert_eq!(Path::new(&resolved), command.as_path());
    assert!(args.is_empty() && env.is_empty());
    let ide = crate::application::agent_launch::info("continue").unwrap();
    assert_eq!(ide.kind.as_deref(), Some("app"));
    assert!(ide.resolved_target.is_none());
    assert_eq!(crate::application::agent_launch::runtime_version("continue-cli").unwrap(), None);
    assert!(!home.home.join("continue-cli-executed-marker").exists());
}

