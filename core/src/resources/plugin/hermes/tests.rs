use super::*;
use crate::testenv::TestHome;
use serde_json::json;

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}
fn host() -> Host { serde_json::from_value(json!({"agent_id":"hermes","adapter":"hermes","docs_url":"https://hermes-agent.nousresearch.com/","manifest_paths":[],"default_mcp_files":[],"mcp_id_prefix":"hermes-","roots":[]})).unwrap() }
fn fixture(home: &TestHome) -> (Contract, runtime::BoundSource) {
    let mut c: Contract = serde_json::from_str(include_str!("../../../../../data/native-plugin-hermes.json")).unwrap();
    let root = home.home.join("public-source");
    let site = root.join("venv/lib/python3.11/site-packages");
    fs::create_dir_all(&site).unwrap();
    let init = b"# public provider fixture, never imported\n";
    let yaml = b"name: different-manifest-name\nversion: 1.0.0\n";
    let path = "plugins/memory/directory-name";
    write(&root.join(path).join("__init__.py"), init);
    write(&root.join(path).join("plugin.yaml"), yaml);
    c.memory_providers = vec![MemoryProvider { name: "directory-name".into(), path: path.into(), version: "1.0.0".into(), guards: vec![
        Guard { path: format!("{path}/__init__.py"), bytes: init.len() as u64, sha256: hex::encode(Sha256::digest(init)) },
        Guard { path: format!("{path}/plugin.yaml"), bytes: yaml.len() as u64, sha256: hex::encode(Sha256::digest(yaml)) },
    ] }];
    (c, runtime::BoundSource { root, site_packages: site, source_version: "0.8.0".into(), packaging_version: "0.7.0".into() })
}
fn scan(home: &TestHome, c: &Contract, bound: &runtime::BoundSource) -> PluginScan {
    let mut result = PluginScan::default();
    observe_bound(&host(), c, bound, &home.home, None, &mut result);
    normalize(&mut result);
    result
}
fn user_plugins(result: &PluginScan) -> Vec<&NativePluginObservation> {
    result.plugins.iter().filter(|p| p.plugin_id.starts_with("@user/")).collect()
}

#[test]
fn yaml_has_priority_and_invalid_yaml_never_borrows_a_valid_yml() {
    let home = TestHome::new("hermes-yaml"); let (c, bound) = fixture(&home);
    let root = home.home.join(".hermes/plugins");
    write(&root.join("a/plugin.yaml"), b"name: yaml-choice\n");
    write(&root.join("a/plugin.yml"), b"name: yml-wrong\n");
    write(&root.join("a/__init__.py"), b"PRIVATE_PYTHON_BODY");
    write(&root.join("bad/plugin.yaml"), b"name: [broken\n");
    write(&root.join("bad/plugin.yml"), b"name: must-not-fallback\n");
    write(&root.join("good/plugin.yml"), b"name: valid-sibling\n");
    write(&root.join("good/__init__.py"), b"PRIVATE_PYTHON_BODY");
    let result = scan(&home, &c, &bound); let names = user_plugins(&result).iter().map(|p| p.name.as_str()).collect::<Vec<_>>();
    assert!(names.contains(&"yaml-choice") && names.contains(&"valid-sibling"));
    assert!(!names.contains(&"yml-wrong") && !names.contains(&"must-not-fallback"));
    assert!(result.unavailable);
}

#[test]
fn fallback_directory_names_and_duplicate_declared_names_keep_physical_identities() {
    let home = TestHome::new("hermes-names"); let (c, bound) = fixture(&home);
    let root = home.home.join(".hermes/plugins");
    for (dir, yaml) in [("fallback", "version: 1.0.0\n"), ("a", "name: same\n"), ("b", "name: same\n"), (".hidden", "name: hidden\n")] {
        write(&root.join(dir).join("plugin.yaml"), yaml.as_bytes()); write(&root.join(dir).join("__init__.py"), b"PRIVATE");
    }
    let result = scan(&home, &c, &bound); let users = user_plugins(&result);
    assert_eq!(users.len(), 4); assert_eq!(users.iter().filter(|p| p.name == "same").count(), 2);
    assert_eq!(users.iter().map(|p| &p.plugin_id).collect::<BTreeSet<_>>().len(), 4);
    assert!(users.iter().any(|p| p.name == "fallback") && users.iter().any(|p| p.name == "hidden"));
}

#[test]
fn missing_init_and_unsafe_manifest_shapes_are_references_without_hiding_siblings() {
    let home = TestHome::new("hermes-shapes"); let (c, bound) = fixture(&home);
    let root = home.home.join(".hermes/plugins");
    for (dir, yaml) in [("missing", "name: no-init\n"), ("wrong", "name: [not, a, string]\n"), ("good", "name: good\n")] {
        write(&root.join(dir).join("plugin.yaml"), yaml.as_bytes());
    }
    write(&root.join("good/__init__.py"), b"PRIVATE_BODY");
    let result = scan(&home, &c, &bound); let users = user_plugins(&result);
    assert!(result.unavailable); assert!(users.iter().any(|p| p.name == "good" && p.presence == PluginPresence::Installed));
    let missing = users.iter().find(|p| p.name == "no-init").unwrap();
    assert_eq!(missing.presence, PluginPresence::Declared);
    assert!(missing.components.iter().any(|c| c.kind == PluginComponentKind::PluginReference));
}

#[test]
fn declarations_never_fabricate_mcp_or_plugin_owned_skills() {
    let home = TestHome::new("hermes-declarations"); let (c, bound) = fixture(&home);
    let plugin = home.home.join(".hermes/plugins/bridge");
    write(&plugin.join("plugin.yaml"), b"name: bridge\nprovides_tools: [mcp_bridge]\nprovides_hooks: [pre_tool_call]\nmcp: {command: PRIVATE_COMMAND}\n");
    for file in ["__init__.py", "mcp.json", "SKILL.md", "hooks/handler.py"] { write(&plugin.join(file), b"PRIVATE_COMPONENT_BODY"); }
    let result = scan(&home, &c, &bound); let users = user_plugins(&result);
    assert!(result.mcps.is_empty());
    assert!(users[0].components.iter().any(|c| c.kind == PluginComponentKind::Tool && c.name.contains("Declared tool")));
    assert!(users[0].components.iter().any(|c| c.kind == PluginComponentKind::Hook));
    assert!(!users[0].components.iter().any(|c| matches!(c.kind, PluginComponentKind::Mcp | PluginComponentKind::Skill)));
    assert!(users[0].components.iter().all(|c| c.native_id.is_none()));
}

#[test]
fn unknown_fields_and_python_markdown_config_bodies_never_escape_inventory() {
    let home = TestHome::new("hermes-private"); let (c, bound) = fixture(&home);
    let plugin = home.home.join(".hermes/plugins/p");
    write(&plugin.join("plugin.yaml"), b"name: p\ndescription: PRIVATE_DESCRIPTION\nauthor: PRIVATE_AUTHOR\nenv: {TOKEN: PRIVATE_TOKEN}\n");
    write(&plugin.join("__init__.py"), b"PRIVATE_PYTHON_BODY");
    write(&home.home.join(".hermes/config.yaml"), b"mcp_servers: {secret: {url: PRIVATE_URL, headers: {Authorization: PRIVATE_TOKEN}}}\nskills: {external_dirs: [PRIVATE_EXTERNAL]}\n");
    write(&home.home.join(".hermes/skills/a/SKILL.md"), b"PRIVATE_MARKDOWN_BODY");
    let result = scan(&home, &c, &bound); let output = serde_json::to_string(&result.plugins).unwrap();
    assert!(!output.contains("PRIVATE_")); assert!(result.plugins.iter().all(|p| p.enabled.is_none())); assert!(result.mcps.is_empty());
}

#[test]
fn sticky_named_profile_overrides_environment_but_default_or_empty_keeps_it() {
    let home = TestHome::new("hermes-profiles");
    let named = home.home.join(".hermes/profiles/coder"); fs::create_dir_all(&named).unwrap();
    let custom = home.home.join("custom");
    for (value, expected) in [("coder\n", named.clone()), ("default\n", custom.clone()), ("\n", custom.clone())] {
        write(&home.home.join(".hermes/active_profile"), value.as_bytes());
        let paths = paths_from(&home.home, Some(custom.as_os_str()), &mut Context::default()).unwrap();
        assert_eq!(paths.root, expected);
    }
}

#[test]
fn invalid_or_missing_named_profiles_fail_without_default_fallback() {
    let home = TestHome::new("hermes-invalid-profile");
    for profile in ["../private", "missing", "UPPER", "x/y"] {
        write(&home.home.join(".hermes/active_profile"), profile.as_bytes());
        assert!(paths_from(&home.home, None, &mut Context::default()).is_err());
    }
    write(&home.home.join(".hermes/active_profile"), &vec![b'a'; 257]);
    assert!(paths_from(&home.home, None, &mut Context::default()).is_err());
}

#[test]
fn present_empty_relative_or_unexpanded_environment_homes_are_unknown() {
    let home = TestHome::new("hermes-relative");
    for value in ["", "relative", "~/.hermes", "/tmp/${HOME}"] {
        assert!(paths_from(&home.home, Some(std::ffi::OsStr::new(value)), &mut Context::default()).is_err());
    }
    assert_eq!(paths_from(&home.home, None, &mut Context::default()).unwrap().root, home.home.join(".hermes"));
}

#[test]
fn entrypoint_name_conflicts_preserve_distribution_sources_without_exporting_targets() {
    let home = TestHome::new("hermes-entrypoints"); let (c, bound) = fixture(&home);
    for distribution in ["first-1.0.dist-info", "second-2.0.dist-info"] {
        write(&bound.site_packages.join(distribution).join("entry_points.txt"), b"[hermes_agent.plugins]\nsame = secret_target.module:Plugin\n[other]\nsecret = PRIVATE_OTHER_VALUE\n");
    }
    let result = scan(&home, &c, &bound); let eps = result.plugins.iter().filter(|p| p.plugin_id.starts_with("@entrypoint/")).collect::<Vec<_>>();
    assert_eq!(eps.len(), 2); assert!(eps.iter().all(|p| p.name == "same" && p.enabled.is_none()));
    assert_ne!(eps[0].plugin_id, eps[1].plugin_id);
    let output = serde_json::to_string(&result.plugins).unwrap(); assert!(!output.contains("secret_target") && !output.contains("PRIVATE_OTHER_VALUE"));
}

#[test]
fn malformed_entrypoint_registration_does_not_hide_a_valid_distribution() {
    let home = TestHome::new("hermes-entrypoint-shapes"); let (c, bound) = fixture(&home);
    write(&bound.site_packages.join("bad.dist-info/entry_points.txt"), b"[hermes_agent.plugins]\nplugin = not a python target!\n");
    write(&bound.site_packages.join("good.dist-info/entry_points.txt"), b"[hermes_agent.plugins]\nvalid = public.module\n");
    let result = scan(&home, &c, &bound); assert!(result.unavailable);
    assert!(result.plugins.iter().any(|p| p.plugin_id.starts_with("@entrypoint/") && p.name == "valid"));
}

#[test]
fn global_skills_are_filename_candidates_with_no_implicit_agents_fallback() {
    let home = TestHome::new("hermes-global-skills"); let (c, bound) = fixture(&home);
    write(&home.home.join(".hermes/skills/category/local/SKILL.md"), b"PRIVATE_FRONTMATTER_AND_BODY");
    write(&home.home.join(".agents/skills/hidden/SKILL.md"), b"PRIVATE_OTHER_AGENT");
    let result = scan(&home, &c, &bound); let global = result.plugins.iter().find(|p| p.plugin_id == "@global/resources").unwrap();
    assert_eq!(global.components.iter().filter(|c| c.kind == PluginComponentKind::Skill).count(), 1);
    assert!(global.components.iter().any(|c| c.kind == PluginComponentKind::Skill && c.name.contains("local")));
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn memory_provider_uses_directory_identity_and_public_member_guards() {
    let home = TestHome::new("hermes-memory"); let (c, bound) = fixture(&home);
    let result = scan(&home, &c, &bound); let provider = result.plugins.iter().find(|p| p.plugin_id == "@memory/directory-name").unwrap();
    assert_eq!(provider.name, "directory-name"); assert_eq!(provider.presence, PluginPresence::Bundled); assert!(provider.enabled.is_none());
    write(&bound.root.join("plugins/memory/directory-name/__init__.py"), b"changed public member");
    let changed = scan(&home, &c, &bound); assert!(changed.unavailable);
    assert_eq!(changed.plugins.iter().find(|p| p.plugin_id == "@memory/directory-name").unwrap().presence, PluginPresence::Declared);
}

#[cfg(unix)]
#[test]
fn user_component_and_profile_symlinks_are_never_followed() {
    use std::os::unix::fs::symlink;
    let home = TestHome::new("hermes-links"); let (c, bound) = fixture(&home);
    let private = home.home.join("private"); write(&private.join("plugin.yaml"), b"name: PRIVATE_LINK_TARGET\n");
    fs::create_dir_all(home.home.join(".hermes/plugins")).unwrap();
    symlink(&private, home.home.join(".hermes/plugins/link")).unwrap();
    let plugin = home.home.join(".hermes/plugins/manifest-link"); fs::create_dir_all(&plugin).unwrap();
    symlink(private.join("plugin.yaml"), plugin.join("plugin.yaml")).unwrap();
    write(&plugin.join("plugin.yml"), b"name: must-not-borrow-yml\n"); write(&plugin.join("__init__.py"), b"PRIVATE_BODY");
    write(&private.join("entry_points.txt"), b"[hermes_agent.plugins]\nPRIVATE_ENTRY_TARGET = module:Plugin\n");
    let distribution = bound.site_packages.join("linked.dist-info"); fs::create_dir_all(&distribution).unwrap();
    symlink(private.join("entry_points.txt"), distribution.join("entry_points.txt")).unwrap();
    let result = scan(&home, &c, &bound); assert!(result.unavailable);
    let output = serde_json::to_string(&result.plugins).unwrap();
    assert!(!output.contains("PRIVATE_LINK_TARGET") && !output.contains("PRIVATE_ENTRY_TARGET") && !output.contains("must-not-borrow-yml"));
    write(&private.join("profile"), b"default");
    symlink(private.join("profile"), home.home.join(".hermes/active_profile")).unwrap();
    assert!(paths_from(&home.home, None, &mut Context::default()).is_err());
}

#[test]
fn watchers_cover_only_declared_metadata_and_resource_scopes() {
    let home = TestHome::new("hermes-watches"); let (c, bound) = fixture(&home);
    write(&home.home.join(".hermes/plugins/p/plugin.yaml"), b"name: p\n");
    write(&home.home.join(".hermes/plugins/p/__init__.py"), b"PRIVATE_BODY");
    write(&home.home.join(".hermes/skills/s/SKILL.md"), b"PRIVATE_BODY");
    write(&bound.site_packages.join("p.dist-info/entry_points.txt"), b"[hermes_agent.plugins]\np = module:Plugin\n");
    let watches = observe_bound(&host(), &c, &bound, &home.home, None, &mut PluginScan::default());
    for path in [".hermes/config.yaml", ".hermes/active_profile", ".hermes/plugins/p/plugin.yaml", ".hermes/skills/s/SKILL.md"] { assert!(watches.contains(&home.home.join(path))); }
    assert!(!watches.contains(&home.home.join(".hermes")) && !watches.contains(&bound.root) && !watches.contains(&bound.site_packages));
    assert!(!watches.iter().any(|p| p.starts_with(home.home.join(".hermes/auth.json")) || p.starts_with(home.home.join(".hermes/sessions"))));
}

#[test]
fn exhausted_other_agent_budget_cannot_change_hermes_observations() {
    let home = TestHome::new("hermes-budget"); let (c, bound) = fixture(&home);
    write(&home.home.join(".hermes/plugins/p/plugin.yaml"), b"name: p\n"); write(&home.home.join(".hermes/plugins/p/__init__.py"), b"PRIVATE_BODY");
    let expected = scan(&home, &c, &bound);
    let mut combined = PluginScan { budget: ScanBudget { visited: MAX_SCAN_ENTRIES, references: MAX_REFERENCES, bytes: MAX_READ_BYTES, verified_bytes: MAX_VERIFIED_BYTES, ..Default::default() }, ..Default::default() };
    observe_bound(&host(), &c, &bound, &home.home, None, &mut combined); normalize(&mut combined);
    assert_eq!(combined.plugins, expected.plugins); assert_eq!(combined.unavailable, expected.unavailable);
    assert_eq!(combined.budget.bytes, MAX_READ_BYTES); assert_eq!(combined.budget.verified_bytes, MAX_VERIFIED_BYTES);
}
