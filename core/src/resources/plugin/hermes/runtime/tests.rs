use super::*;
use crate::testenv::TestHome;

const METADATA: &[u8] = b"Metadata-Version: 2.4\nName: hermes-agent\nVersion: 0.7.0\n";
const ENTRY_POINTS: &[u8] = b"[console_scripts]\nhermes = hermes_cli.main:main\n";
const SOURCE_VERSION: &[u8] = b"__version__ = \"0.8.0\"\n";
const PROJECT: &[u8] = b"[project]\nname = \"hermes-agent\"\nversion = \"0.8.0\"\n";
const MAIN: &[u8] = b"# reviewed public main fixture; never imported\n";

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn guard(path: &str, bytes: &[u8]) -> Guard {
    Guard { path: path.into(), bytes: bytes.len() as u64, sha256: hex::encode(Sha256::digest(bytes)) }
}

struct Fixture {
    c: Contract,
    root: PathBuf,
    site: PathBuf,
    entry: PathBuf,
}

fn fixture(home: &TestHome, name: &str) -> Fixture {
    let mut c: Contract = serde_json::from_str(include_str!("../../../../../../data/native-plugin-hermes-runtime.json")).unwrap();
    c.metadata = guard("hermes_agent-0.7.0.dist-info/METADATA", METADATA);
    c.entry_points = guard("hermes_agent-0.7.0.dist-info/entry_points.txt", ENTRY_POINTS);
    c.guards = vec![guard("hermes_cli/__init__.py", SOURCE_VERSION), guard("pyproject.toml", PROJECT), guard("hermes_cli/main.py", MAIN)];
    let root = home.home.join(name);
    let venv = root.join("venv");
    let site = venv.join("lib/python3.11/site-packages");
    let entry = venv.join("bin/hermes");
    let entry_text = format!("#!{}/bin/python3\n# -*- coding: utf-8 -*-\nimport sys\nfrom hermes_cli.main import main\nif __name__ == \"__main__\":\n    if sys.argv[0].endswith(\"-script.pyw\"):\n        sys.argv[0] = sys.argv[0][:-11]\n    elif sys.argv[0].endswith(\".exe\"):\n        sys.argv[0] = sys.argv[0][:-4]\n    sys.exit(main())\n", venv.display());
    write(&entry, entry_text.as_bytes());
    write(&site.join(&c.metadata.path), METADATA);
    write(&site.join(&c.entry_points.path), ENTRY_POINTS);
    // Literal wire shape captured from the reviewed editable-install metadata;
    // do not serialize this fixture from the contract or a generic JSON value.
    write(&site.join(&c.direct_url_path), format!("{{\"url\":\"file://{}\",\"dir_info\":{{\"editable\":true}}}}", root.display()).as_bytes());
    write(&site.join(&c.pth_path), b"import __editable___hermes_agent_0_7_0_finder; __editable___hermes_agent_0_7_0_finder.install()");
    let finder = c.finder.text.replace("${REGISTERED_SOURCE_ROOT}", root.to_str().unwrap())
        .replace("${FINDER_PLACEHOLDER_PREFIX}", "__editable__.hermes_agent-0.7.0.finder");
    write(&site.join(&c.finder_path), finder.as_bytes());
    for (path, bytes) in [("hermes_cli/__init__.py", SOURCE_VERSION), ("pyproject.toml", PROJECT), ("hermes_cli/main.py", MAIN)] {
        write(&root.join(path), bytes);
    }
    Fixture { c, root, site, entry }
}

#[test]
fn reviewed_registration_labels_source_and_packaging_separately_without_starting_python() {
    let home = TestHome::new("hermes-binding");
    let f = fixture(&home, "source");
    let marker = home.home.join("interpreter-was-executed");
    write(&f.root.join("venv/bin/python3"), format!("#!/bin/sh\ntouch '{}'\n", marker.display()).as_bytes());
    write(&f.site.join("unrelated-private.pth"), b"PRIVATE_STARTUP_CODE");
    write(&f.root.join(".env"), b"PRIVATE_ENV");
    write(&f.root.join("config.yaml"), b"PRIVATE_CONFIG");
    let bound = bind_with_contract(&f.entry, &f.c).unwrap();
    assert_eq!(bound.root, f.root);
    assert_eq!(bound.site_packages, f.site);
    assert_eq!(bound.source_version, "0.8.0");
    assert_eq!(bound.packaging_version, "0.7.0");
    assert!(!marker.exists());
}

#[test]
fn unchanged_mapping_cannot_hide_changed_executable_finder_or_pth_code() {
    let home = TestHome::new("hermes-finder");
    let f = fixture(&home, "source");
    for path in [f.site.join(&f.c.finder_path), f.site.join(&f.c.pth_path), f.entry.clone()] {
        let old = fs::read(&path).unwrap();
        let mut changed = old.clone();
        changed.extend_from_slice(b"\nraise RuntimeError('must never run')\n");
        write(&path, &changed);
        assert!(bind_with_contract(&f.entry, &f.c).is_err());
        write(&path, &old);
    }
    assert!(bind_with_contract(&f.entry, &f.c).is_ok());
}

#[test]
fn another_source_url_or_nonlocal_editable_registration_cannot_borrow_a_venv() {
    let home = TestHome::new("hermes-url");
    let f = fixture(&home, "source");
    for value in [
        format!("{{\"url\":\"file://{}\",\"dir_info\":{{\"editable\":true}}}}", home.home.join("other").display()),
        "{\"url\":\"https://example.invalid/source\",\"dir_info\":{\"editable\":true}}".into(),
        format!("{{\"url\":\"file://{}\",\"dir_info\":{{\"editable\":false}}}}", f.root.display()),
    ] {
        write(&f.site.join(&f.c.direct_url_path), value.as_bytes());
        assert!(bind_with_contract(&f.entry, &f.c).is_err());
    }
}

#[test]
fn reviewed_direct_url_wire_bytes_bind_while_unreviewed_json_reserialization_is_rejected() {
    let home = TestHome::new("hermes-direct-url-wire");
    let f = fixture(&home, "source");
    assert_eq!(bind_with_contract(&f.entry, &f.c).unwrap().source_version, "0.8.0");
    // Same JSON fields, but this is the previously hand-invented wire shape.
    // Accepting it would broaden the reviewed byte contract rather than fix it.
    let rewritten = format!("{{\"dir_info\": {{\"editable\": true}}, \"url\": \"file://{}\"}}", f.root.display());
    write(&f.site.join(&f.c.direct_url_path), rewritten.as_bytes());
    assert!(bind_with_contract(&f.entry, &f.c).is_err());
}

#[test]
fn all_public_contract_members_gate_binding_including_source_version_declarations() {
    let home = TestHome::new("hermes-source");
    let f = fixture(&home, "source");
    for member in &f.c.guards {
        let path = f.root.join(&member.path);
        let old = fs::read(&path).unwrap();
        write(&path, b"changed public source");
        assert!(bind_with_contract(&f.entry, &f.c).is_err());
        write(&path, &old);
    }
}

#[test]
fn distribution_entrypoint_and_complete_template_digests_are_mandatory() {
    let home = TestHome::new("hermes-digests");
    let mut f = fixture(&home, "source");
    for member in [&f.c.metadata, &f.c.entry_points] {
        let path = f.site.join(&member.path);
        let old = fs::read(&path).unwrap();
        write(&path, b"unrelated distribution registration");
        assert!(bind_with_contract(&f.entry, &f.c).is_err());
        write(&path, &old);
    }
    f.c.finder.sha256 = "0".repeat(64);
    assert!(bind_with_contract(&f.entry, &f.c).is_err());
}

#[test]
fn unsafe_root_characters_are_not_interpolated_into_python_or_url_templates() {
    let home = TestHome::new("hermes-path");
    for name in ["quote'root", "double\"root", "back\\slash", "space root", "dollar$root"] {
        let f = fixture(&home, name);
        assert!(bind_with_contract(&f.entry, &f.c).is_err());
    }
}

#[cfg(unix)]
#[test]
fn only_the_selected_executable_link_is_followed_and_metadata_links_fail_closed() {
    use std::os::unix::fs::symlink;
    let home = TestHome::new("hermes-links");
    let f = fixture(&home, "source");
    let selected = home.home.join("bin/hermes");
    symlink(&f.entry, &selected).unwrap();
    assert_eq!(bind_with_contract(&selected, &f.c).unwrap().root, f.root);
    for path in [f.site.join(&f.c.metadata.path), f.site.join(&f.c.finder_path), f.root.join("hermes_cli/main.py")] {
        let old = fs::read(&path).unwrap();
        let private = home.home.join("private-target");
        write(&private, &old);
        fs::remove_file(&path).unwrap();
        symlink(&private, &path).unwrap();
        assert!(bind_with_contract(&selected, &f.c).is_err());
        fs::remove_file(&path).unwrap();
        write(&path, &old);
    }
    fs::remove_file(&selected).unwrap();
    symlink(&selected, &selected).unwrap();
    assert!(bind_with_contract(&selected, &f.c).is_err());
}

#[test]
fn changed_path_identity_after_read_invalidates_the_complete_binding_snapshot() {
    let home = TestHome::new("hermes-cas");
    let path = home.home.join("public-metadata");
    write(&path, b"safe");
    let mut snapshots = Vec::new();
    read_bounded(&path, 16, &mut ScanBudget::default(), &mut snapshots).unwrap();
    let replacement = home.home.join("replacement");
    write(&replacement, b"safe");
    fs::rename(&replacement, &path).unwrap();
    assert!(recheck(&snapshots).is_err());
}

#[test]
fn unsupported_entry_returns_unknown_without_executing_any_version_fallback() {
    let home = TestHome::new("hermes-version");
    let entry = home.home.join("bin/hermes");
    let marker = home.home.join("version-probe-ran");
    write(&entry, format!("#!/bin/sh\ntouch '{}'\n", marker.display()).as_bytes());
    assert!(runtime_version(&entry).is_none());
    assert!(!marker.exists());
}

#[test]
fn runtime_watches_verified_members_without_watching_source_or_site_roots() {
    let home = TestHome::new("hermes-runtime-watch");
    let f = fixture(&home, "source");
    let paths = watch_paths_with_contract(&f.entry, &f.c);
    assert!(paths.contains(&f.site.join(&f.c.pth_path)) && paths.contains(&f.site.join(&f.c.finder_path)));
    assert!(paths.contains(&f.root.join("hermes_cli/main.py")));
    assert!(!paths.contains(&f.root) && !paths.contains(&f.site));
    write(&f.site.join(&f.c.pth_path), b"changed registration");
    assert_eq!(watch_paths_with_contract(&f.entry, &f.c), vec![f.entry]);
}
