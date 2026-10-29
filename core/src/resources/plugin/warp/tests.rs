use super::*;
use crate::testenv::TestHome;

const GENERAL: [&str; 14] = ["add-mcp-server", "change-keybinding", "claude-api", "create-skill",
    "create-tab-config", "factory-files", "factory-mcp", "modify-settings", "oz-platform",
    "pr-comments", "tab-configs", "tui-migrate-setup", "update-tab-config", "warpctrl"];
const FIGMA: [&str; 8] = ["edit-figma-design", "figma-code-connect-components",
    "figma-create-design-system-rules", "figma-create-new-file", "figma-generate-design",
    "figma-generate-library", "figma-implement-design", "figma-use"];

fn write(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn root(path: &Path) -> Root {
    Root { path: path.to_str().unwrap().into(), layout: "warp_app".into(),
        presence: PluginPresence::Bundled, complete_marker: None }
}

fn seal(c: &mut Contract, app: &Path, which: &str, body: &[u8]) {
    let guard = match which { "info" => &mut c.info, "main" => &mut c.main,
        "version" => &mut c.bundled_version, _ => panic!("fixture guard") };
    write(&app.join(&guard.path), body);
    guard.bytes = body.len() as u64;
    guard.sha256 = hex::encode(Sha256::digest(body));
}

fn info(c: &Contract, changed: Option<(&str, &str)>) -> Vec<u8> {
    let mut fields = plist::Dictionary::new();
    for (key, value) in [("CFBundleIdentifier", c.bundle_id.as_str()), ("CFBundleName", c.bundle_name.as_str()),
        ("CFBundleExecutable", c.executable.as_str()), ("CFBundleShortVersionString", c.version.as_str()),
        ("CFBundleVersion", c.version.as_str()), ("WarpVersion", c.warp_version.as_str())] {
        fields.insert(key.into(), plist::Value::String(value.into()));
    }
    if let Some((key, value)) = changed { fields.insert(key.into(), plist::Value::String(value.into())); }
    let mut body = Vec::new();
    plist::Value::Dictionary(fields).to_writer_xml(&mut body).unwrap();
    body
}

fn fixture(home: &TestHome) -> (Contract, Host, PathBuf) {
    let mut c: Contract = serde_json::from_str(include_str!("../../../../../data/native-plugin-warp.json")).unwrap();
    let app = home.home.join("Applications/Warp.app");
    let body = info(&c, None); seal(&mut c, &app, "info", &body);
    let body = serde_json::to_vec(&serde_json::json!({"warp_version": c.warp_version.clone()})).unwrap();
    seal(&mut c, &app, "version", &body);
    let mut main = Vec::new();
    for word in [0xfeedfacfu32, 0x0100000c, 0, 2, 1, 8, 0, 0, 0, 8] { main.extend(word.to_le_bytes()); }
    main.extend(b"Never execute this fixture or its target helpers.");
    seal(&mut c, &app, "main", &main);
    for (group, names) in [(&c.groups[0], GENERAL.as_slice()), (&c.groups[1], FIGMA.as_slice())] {
        for name in names { write(&app.join(&group.root).join(name).join("SKILL.md"), b"\0Not parsed Skill body"); }
    }
    let host = Host { agent_id: "warp".into(), adapter: "warp".into(), docs_url: "https://docs.warp.dev/".into(),
        manifest_paths: Vec::new(), default_mcp_files: Vec::new(), mcp_id_prefix: String::new(),
        roots: vec![root(&app)], resource_groups: Vec::new() };
    (c, host, app)
}

fn scan(c: &Contract, host: &Host, cache: &Mutex<DigestCache>) -> PluginScan {
    let mut result = PluginScan::default();
    if observe_contract(host, c, cache, &mut result).is_err() { result.unavailable = true; }
    result
}

#[test]
fn app_owned_groups_expose_twenty_two_skill_paths_without_mcp_or_activation_claims() {
    let home = TestHome::new("warp-groups"); let (c, host, app) = fixture(&home);
    write(&home.home.join(".warp/skills/private/SKILL.md"), b"PRIVATE_USER_SKILL");
    write(&home.home.join(".warp/.mcp.json"), b"PRIVATE_MCP_CONFIG");
    write(&app.join("Contents/PlugIns/WarpDockTilePlugin.docktileplugin/SKILL.md"), b"NOT_AN_AGENT_PLUGIN");
    let result = scan(&c, &host, &Mutex::default());
    assert!(!result.unavailable); assert!(result.mcps.is_empty()); assert_eq!(result.plugins.len(), 2);
    assert_eq!(result.plugins.iter().map(|item| item.components.len()).collect::<Vec<_>>(), [14, 8]);
    for item in &result.plugins {
        assert!(item.plugin_id.starts_with("@builtin/")); assert_eq!(item.enabled, None);
        assert_eq!(item.presence, PluginPresence::Bundled); assert_eq!(item.version, c.version);
        assert!(item.components.iter().all(|part| part.kind == PluginComponentKind::Skill && part.native_id.as_deref() == Some(part.name.as_str())));
    }
    let serialized = serde_json::to_string(&result.plugins).unwrap();
    assert!(!serialized.contains("PRIVATE_")); assert!(!serialized.contains("WarpDockTilePlugin"));
}

#[cfg(unix)]
#[test]
fn unreadable_skill_bodies_still_supply_stat_only_availability() {
    use std::os::unix::fs::PermissionsExt;
    let home = TestHome::new("warp-stat"); let (c, host, app) = fixture(&home);
    let leaf = app.join(&c.groups[0].root).join(GENERAL[0]).join("SKILL.md");
    fs::set_permissions(&leaf, fs::Permissions::from_mode(0o000)).unwrap();
    let result = scan(&c, &host, &Mutex::default());
    fs::set_permissions(leaf, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!result.unavailable); assert_eq!(result.plugins[0].components.len(), 14);
}

#[test]
fn same_names_in_two_builtin_roots_retain_separate_physical_provenance() {
    let home = TestHome::new("warp-names"); let (c, host, app) = fixture(&home);
    write(&app.join(&c.groups[1].root).join(GENERAL[0]).join("SKILL.md"), b"not parsed");
    let result = scan(&c, &host, &Mutex::default());
    assert!(!result.unavailable);
    let matches = result.plugins.iter().flat_map(|item| &item.components).filter(|part| part.name == GENERAL[0]).collect::<Vec<_>>();
    assert_eq!(matches.len(), 2); assert_ne!(matches[0].source_path, matches[1].source_path);
}

#[test]
fn unrelated_global_scan_exhaustion_does_not_consume_warps_identity_or_leaf_budget() {
    let home = TestHome::new("warp-budget"); let (c, host, _) = fixture(&home);
    let mut result = PluginScan::default();
    result.budget.visited = MAX_SCAN_ENTRIES; result.budget.bytes = MAX_READ_BYTES;
    result.budget.verified_bytes = MAX_VERIFIED_BYTES;
    observe_contract(&host, &c, &Mutex::default(), &mut result).unwrap();
    assert_eq!(result.plugins.len(), 2); assert_eq!(result.budget.visited, MAX_SCAN_ENTRIES);
    assert_eq!(result.budget.bytes, MAX_READ_BYTES); assert_eq!(result.budget.verified_bytes, MAX_VERIFIED_BYTES);
}

#[test]
fn unchanged_fd_identity_reuses_the_digest_without_a_second_full_binary_read() {
    let home = TestHome::new("warp-cache"); let (c, host, _) = fixture(&home); let cache = Mutex::default();
    let mut cold = IdentityBudget::default();
    let first = VerifiedApp::capture(&host, &c, &cache, &mut cold).unwrap().unwrap(); first.revalidate(&host).unwrap();
    assert_eq!(cold.hashed_bytes, c.main.bytes);
    let mut warm = IdentityBudget::default();
    let second = VerifiedApp::capture(&host, &c, &cache, &mut warm).unwrap().unwrap(); second.revalidate(&host).unwrap();
    assert_eq!(warm.hashed_bytes, 0); assert!(warm.metadata_bytes > 0);
}

#[cfg(unix)]
#[test]
fn a_same_length_change_with_restored_mtime_cannot_reuse_a_cached_digest() {
    let home = TestHome::new("warp-ctime"); let (c, host, app) = fixture(&home); let cache = Mutex::default();
    VerifiedApp::capture(&host, &c, &cache, &mut IdentityBudget::default()).unwrap().unwrap();
    let path = app.join(&c.main.path); let before = fs::metadata(&path).unwrap();
    let mut body = fs::read(&path).unwrap(); *body.last_mut().unwrap() ^= 1; fs::write(&path, body).unwrap();
    fs::OpenOptions::new().write(true).open(&path).unwrap()
        .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap())).unwrap();
    let mut budget = IdentityBudget::default();
    assert!(VerifiedApp::capture(&host, &c, &cache, &mut budget).is_err());
    assert_eq!(budget.hashed_bytes, c.main.bytes);
}

#[test]
fn a_different_inode_or_selected_app_path_requires_a_new_full_digest() {
    let home = TestHome::new("warp-inode"); let (c, mut host, app) = fixture(&home); let cache = Mutex::default();
    VerifiedApp::capture(&host, &c, &cache, &mut IdentityBudget::default()).unwrap().unwrap();
    let main = app.join(&c.main.path); let replacement = main.with_file_name("replacement");
    fs::write(&replacement, fs::read(&main).unwrap()).unwrap(); fs::rename(&replacement, &main).unwrap();
    let mut budget = IdentityBudget::default(); VerifiedApp::capture(&host, &c, &cache, &mut budget).unwrap().unwrap();
    assert_eq!(budget.hashed_bytes, c.main.bytes);
    let moved = home.home.join("Other/Warp.app"); fs::create_dir_all(moved.parent().unwrap()).unwrap(); fs::rename(app, &moved).unwrap();
    host.roots = vec![root(&moved)]; let mut moved_budget = IdentityBudget::default();
    VerifiedApp::capture(&host, &c, &cache, &mut moved_budget).unwrap().unwrap(); assert_eq!(moved_budget.hashed_bytes, c.main.bytes);
}

#[test]
fn path_replacement_after_fd_capture_invalidates_publication_even_with_identical_bytes() {
    let home = TestHome::new("warp-fd"); let (c, host, app) = fixture(&home);
    let proof = VerifiedApp::capture(&host, &c, &Mutex::default(), &mut IdentityBudget::default()).unwrap().unwrap();
    let main = app.join(&c.main.path); let replacement = main.with_file_name("replacement");
    fs::write(&replacement, fs::read(&main).unwrap()).unwrap(); fs::rename(replacement, main).unwrap();
    assert!(proof.revalidate(&host).is_err());
}

#[test]
fn first_existing_app_masks_a_lower_priority_valid_app_and_selection_is_rechecked() {
    let home = TestHome::new("warp-select"); let (c, mut host, _) = fixture(&home);
    let first = home.home.join("First/Warp.app"); host.roots.insert(0, root(&first));
    let proof = VerifiedApp::capture(&host, &c, &Mutex::default(), &mut IdentityBudget::default()).unwrap().unwrap();
    fs::create_dir_all(&first).unwrap(); assert!(proof.revalidate(&host).is_err());
    let result = scan(&c, &host, &Mutex::default()); assert!(result.unavailable); assert!(result.plugins.is_empty());
}

#[test]
fn semantic_app_and_bundled_versions_remain_required_after_exact_guard_hashes_match() {
    let home = TestHome::new("warp-fields");
    for (field, value) in [("CFBundleIdentifier", "other.app"), ("CFBundleName", "Other"),
        ("CFBundleExecutable", "helper"), ("CFBundleShortVersionString", "9"), ("CFBundleVersion", "9"), ("WarpVersion", "other")] {
        let (mut c, host, app) = fixture(&home); let body = info(&c, Some((field, value))); seal(&mut c, &app, "info", &body);
        let result = scan(&c, &host, &Mutex::default()); assert!(result.unavailable, "{field}"); assert!(result.plugins.is_empty());
    }
    let (mut c, host, app) = fixture(&home); seal(&mut c, &app, "version", br#"{"warp_version":"other"}"#);
    assert!(scan(&c, &host, &Mutex::default()).unavailable);
}

#[test]
fn changed_guard_malformed_plist_or_non_macho_main_is_not_a_partial_observation() {
    let home = TestHome::new("warp-guards");
    for which in ["info", "version", "main"] {
        let (c, host, app) = fixture(&home); let guard = match which { "info" => &c.info, "version" => &c.bundled_version, _ => &c.main };
        let mut body = fs::read(app.join(&guard.path)).unwrap(); *body.last_mut().unwrap() ^= 1; write(&app.join(&guard.path), &body);
        let result = scan(&c, &host, &Mutex::default()); assert!(result.unavailable); assert!(result.plugins.is_empty());
    }
    let (mut c, host, app) = fixture(&home); seal(&mut c, &app, "info", b"not a plist"); assert!(scan(&c, &host, &Mutex::default()).unavailable);
    let (mut c, host, app) = fixture(&home); seal(&mut c, &app, "main", b"#!/bin/sh\nexit 1\n"); assert!(scan(&c, &host, &Mutex::default()).unavailable);
}

#[cfg(unix)]
#[test]
fn symlinked_app_main_root_and_skill_leaves_are_rejected_without_following() {
    use std::os::unix::fs::symlink;
    let home = TestHome::new("warp-links");
    for which in ["main", "root", "leaf", "app"] {
        let (c, host, app) = fixture(&home);
        let path = match which { "main" => app.join(&c.main.path), "root" => app.join(&c.groups[0].root),
            "leaf" => app.join(&c.groups[0].root).join(GENERAL[0]).join("SKILL.md"), _ => app.clone() };
        let outside = home.home.join(format!("outside-{which}")); fs::rename(&path, &outside).unwrap(); symlink(&outside, &path).unwrap();
        let result = scan(&c, &host, &Mutex::default()); assert!(result.unavailable, "{which}"); assert!(result.plugins.is_empty());
        fs::remove_file(path).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn special_skill_nodes_are_never_opened_as_bodies() {
    use std::os::unix::net::UnixListener;
    let home = TestHome::new("warp-node"); let (c, host, app) = fixture(&home);
    let path = app.join(&c.groups[0].root).join(GENERAL[0]).join("SKILL.md"); fs::remove_file(&path).unwrap();
    // Bind at a short path first: macOS limits sockaddr_un paths to 104 bytes.
    let socket_path = home.home.join("node"); let _socket = UnixListener::bind(&socket_path).unwrap();
    fs::rename(socket_path, path).unwrap(); let result = scan(&c, &host, &Mutex::default());
    assert!(result.unavailable); assert!(result.plugins.is_empty());
}

#[test]
fn direct_leaf_and_entry_limits_do_not_recurse_into_supporting_files_or_publish_partial_groups() {
    let home = TestHome::new("warp-limit"); let (c, host, app) = fixture(&home);
    write(&app.join(&c.groups[0].root).join(GENERAL[0]).join("support/deep/SKILL.md"), b"not another skill");
    assert_eq!(scan(&c, &host, &Mutex::default()).plugins[0].components.len(), 14);
    for index in 0..MAX_SKILL_LEAVES { write(&app.join(&c.groups[0].root).join(format!("extra-{index}")).join("SKILL.md"), b"metadata only"); }
    let result = scan(&c, &host, &Mutex::default()); assert!(result.unavailable); assert!(result.plugins.is_empty());
    let mut budget = ResourceBudget { entries: MAX_RESOURCE_ENTRIES, ..ResourceBudget::default() };
    assert!(budget.visit().is_err());
}

#[test]
fn leaf_and_guard_replacement_after_inventory_capture_invalidates_the_snapshot() {
    let home = TestHome::new("warp-cas"); let (c, host, app) = fixture(&home);
    let proof = VerifiedApp::capture(&host, &c, &Mutex::default(), &mut IdentityBudget::default()).unwrap().unwrap();
    let snapshot = ResourceSnapshot::capture(&proof, &host, &c, &mut ResourceBudget::default()).unwrap();
    let leaf = app.join(&c.groups[0].root).join(GENERAL[0]).join("SKILL.md"); fs::write(leaf, b"changed bytes").unwrap();
    assert!(snapshot.revalidate().is_err());
    let mut body = fs::read(app.join(&c.info.path)).unwrap(); *body.last_mut().unwrap() ^= 1; fs::write(app.join(&c.info.path), body).unwrap();
    assert!(proof.revalidate(&host).is_err());
}

#[cfg(unix)]
#[test]
fn a_replaced_leaf_parent_cannot_redirect_the_metadata_snapshot() {
    use std::os::unix::fs::symlink;
    let home = TestHome::new("warp-parent"); let (c, host, app) = fixture(&home);
    let proof = VerifiedApp::capture(&host, &c, &Mutex::default(), &mut IdentityBudget::default()).unwrap().unwrap();
    let snapshot = ResourceSnapshot::capture(&proof, &host, &c, &mut ResourceBudget::default()).unwrap();
    let directory = app.join(&c.groups[0].root).join(GENERAL[0]);
    let outside = home.home.join("changed-parent"); fs::rename(&directory, &outside).unwrap();
    symlink(&outside, &directory).unwrap(); assert!(snapshot.revalidate().is_err());
}

#[test]
fn audited_contract_rejects_unknown_fields_and_escaping_or_additional_group_roots() {
    let home = TestHome::new("warp-schema"); let (mut c, host, _) = fixture(&home);
    let mut value: Value = serde_json::from_str(include_str!("../../../../../data/native-plugin-warp.json")).unwrap();
    value["unexpected"] = Value::Bool(true); assert!(serde_json::from_value::<Contract>(value).is_err());
    c.groups[0].root = "../../private".into(); assert!(scan(&c, &host, &Mutex::default()).unavailable);
    let (mut c, host, _) = fixture(&home); c.groups.push(c.groups[0].clone()); assert!(scan(&c, &host, &Mutex::default()).unavailable);
    let (mut c, host, _) = fixture(&home); c.main.bytes = MAX_IDENTITY_BYTES + 1;
    assert!(scan(&c, &host, &Mutex::default()).unavailable);
}

#[test]
fn cache_is_finite_and_watch_paths_never_include_global_private_state() {
    let home = TestHome::new("warp-watch"); let (c, mut host, app) = fixture(&home); let cache = Mutex::default();
    for index in 0..MAX_DIGEST_CACHE_ENTRIES + 2 {
        let moved = home.home.join(format!("App-{index}/Warp.app")); fs::create_dir_all(moved.parent().unwrap()).unwrap();
        let previous = if index == 0 { app.clone() } else { home.home.join(format!("App-{}/Warp.app", index - 1)) };
        fs::rename(previous, &moved).unwrap(); host.roots = vec![root(&moved)];
        VerifiedApp::capture(&host, &c, &cache, &mut IdentityBudget::default()).unwrap().unwrap();
    }
    assert_eq!(cache.lock().unwrap().entries.len(), MAX_DIGEST_CACHE_ENTRIES);
    let app = PathBuf::from(&host.roots[0].path); let paths = watch_paths(&host);
    assert!(!paths.is_empty()); assert!(paths.iter().all(|path| path.starts_with(&app)));
    assert!(!paths.iter().any(|path| path.starts_with(home.home.join(".warp"))));
    let redirected = app.join(&c.groups[0].root); let outside = home.home.join("private-watch-target");
    fs::rename(&redirected, &outside).unwrap(); std::os::unix::fs::symlink(outside, &redirected).unwrap();
    assert!(!watch_paths(&host).contains(&redirected));
}
