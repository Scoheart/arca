use arca_core::adapter::{get_agent_adapter_for, Adapter};
use arca_core::testenv::TestHome;
use arca_core::types::{HttpConfig, McpConfig, StdioConfig};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const MIXED: &str = r#"# Keep the user's Goose configuration
provider: fixture-provider
extensions:
  paused:
    name: Paused remote
    type: streamable_http
    uri: https://example.test/mcp
    enabled: false # native pause
    timeout: 42
    policy: {allow: [read], keep: false}
  active:
    name: Active local
    type: stdio
    cmd: fixture-command
    args: []
    enabled: true
  developer:
    name: developer
    type: builtin
    enabled: true
    display_name: Developer
  platform:
    name: platform
    type: platform
    enabled: false
  future:
    name: future
    type: future_extension
    cmd: this-field-does-not-make-it-an-mcp
    enabled: true
  opaque: private-reference
"#;

fn adapter(id: &str) -> Box<dyn Adapter> {
    get_agent_adapter_for(&arca_core::agents::builtin_agents()[id], id)
}

fn local() -> McpConfig {
    McpConfig::Stdio(StdioConfig {
        command: "fixture-command".into(), args: Some(vec![]), env: None, cwd: None,
    })
}

fn value(path: &Path) -> Value {
    serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn goose_and_desktop_keep_native_paused_state_without_counting_it_as_active() {
    let home = TestHome::new("goose-native-state");
    let path = home.home.join("config.yaml");
    fs::write(&path, MIXED).unwrap();
    for id in ["goose", "goose-desktop"] {
        let adapter = adapter(id);
        let observed = adapter.read_with_enabled(&path);
        assert_eq!(observed.keys().cloned().collect::<BTreeSet<_>>(), BTreeSet::from(["active".into(), "paused".into()]), "{id}");
        assert!(!observed["paused"].1, "{id}: false is a native pause, not an enabled connection");
        assert!(observed["active"].1, "{id}");
        assert!(!adapter.read(&path).contains_key("paused"), "{id}");
        assert!(adapter.supports_native_enabled(), "{id}");
    }
    assert_eq!(fs::read_to_string(path).unwrap(), MIXED);
}

#[test]
fn new_goose_stdio_and_http_entries_have_the_required_explicit_enabled_flag() {
    let home = TestHome::new("goose-required-enabled");
    for id in ["goose", "goose-desktop"] {
        let path = home.home.join(format!("{id}.yaml"));
        let adapter = adapter(id);
        adapter.upsert(&path, "local", &local()).unwrap();
        adapter.upsert(&path, "remote", &McpConfig::Http(HttpConfig {
            kind: "streamable-http".into(), url: "https://example.test/mcp".into(), headers: None,
        })).unwrap();
        let parsed = value(&path);
        assert_eq!(parsed["extensions"]["local"]["enabled"], true, "{id}");
        assert_eq!(parsed["extensions"]["remote"]["enabled"], true, "{id}");
        assert_eq!(adapter.read_with_enabled(&path).len(), 2, "{id}");
    }
}

#[test]
fn goose_native_toggle_is_entry_bound_and_keeps_policy_comments_siblings_and_inode() {
    let home = TestHome::new("goose-native-toggle");
    let path = home.home.join("config.yaml");
    fs::write(&path, MIXED).unwrap();
    let adapter = adapter("goose");
    #[cfg(unix)]
    let inode = { use std::os::unix::fs::MetadataExt; fs::metadata(&path).unwrap().ino() };
    let before = value(&path);
    let paused = adapter.snapshot(&path, "paused").unwrap().unwrap();
    adapter.set_enabled(&path, "paused", true, &paused).unwrap();
    let mut expected = before.clone();
    expected["extensions"]["paused"]["enabled"] = json!(true);
    assert_eq!(value(&path), expected);
    assert!(fs::read_to_string(&path).unwrap().contains("# native pause"));
    assert!(fs::read_to_string(&path).unwrap().contains("# Keep the user's Goose configuration"));
    assert!(adapter.read(&path).contains_key("paused"));
    let after = fs::read(&path).unwrap();
    assert!(adapter.set_enabled(&path, "paused", false, &paused).is_err());
    assert_eq!(fs::read(&path).unwrap(), after);
    let current = adapter.snapshot(&path, "paused").unwrap().unwrap();
    adapter.set_enabled(&path, "paused", false, &current).unwrap();
    assert_eq!(value(&path), before);
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
    }
}

#[test]
fn missing_or_non_boolean_goose_flags_do_not_acquire_native_state_or_write_authority() {
    let home = TestHome::new("goose-invalid-switch");
    let path = home.home.join("config.yaml");
    let adapter = adapter("goose");
    for flag in ["", "    enabled: null\n", "    enabled: \"true\"\n", "    enabled: 1\n"] {
        let source = format!("extensions:\n  invalid:\n    name: invalid\n    type: stdio\n    cmd: fixture\n    args: []\n{flag}");
        fs::write(&path, &source).unwrap();
        assert!(adapter.read_with_enabled(&path).is_empty());
        assert!(adapter.upsert(&path, "invalid", &local()).is_err());
        let snapshot = adapter.snapshot(&path, "invalid").unwrap().unwrap();
        assert!(adapter.set_enabled(&path, "invalid", true, &snapshot).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
    }
}

#[test]
fn goose_mcp_clear_preserves_builtins_platform_extensions_and_unknown_declarations() {
    let home = TestHome::new("goose-clear-boundary");
    let path = home.home.join("config.yaml");
    let adapter = adapter("goose-desktop");
    fs::write(&path, MIXED).unwrap();
    let mut expected = value(&path);
    assert!(adapter.has_entries(&path).unwrap());
    adapter.clear(&path).unwrap();
    expected["extensions"].as_object_mut().unwrap().remove("active");
    expected["extensions"].as_object_mut().unwrap().remove("paused");
    assert_eq!(value(&path), expected);
    assert!(!adapter.has_entries(&path).unwrap());
    assert!(adapter.read_with_enabled(&path).is_empty());
    let before = fs::read(&path).unwrap();
    adapter.clear(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn goose_non_mcp_extensions_reject_targeted_mutation_and_snapshot_deletion() {
    let home = TestHome::new("goose-builtin-boundary");
    let path = home.home.join("config.yaml");
    let adapter = adapter("goose");
    fs::write(&path, MIXED).unwrap();
    for name in ["developer", "platform", "future"] {
        let snapshot = value(&path)["extensions"][name].clone();
        assert!(adapter.snapshot(&path, name).is_err(), "{name}");
        assert!(adapter.remove(&path, &[name.into()]).is_err(), "{name}");
        assert!(adapter.remove_snapshot(&path, name, &snapshot).is_err(), "{name}");
        assert!(adapter.set_enabled(&path, name, false, &snapshot).is_err(), "{name}");
        assert!(adapter.upsert(&path, name, &local()).is_err(), "{name}");
        assert!(adapter.restore(&path, "copied-non-mcp", &snapshot).is_err(), "{name}");
        assert_eq!(fs::read_to_string(&path).unwrap(), MIXED, "{name}");
    }
}

#[test]
fn yaml_declaration_names_keep_non_mcp_and_unparseable_payloads_read_only() {
    let _home = TestHome::new("goose-declaration-names");
    let adapter = adapter("goose");
    let names = adapter.declaration_names(MIXED).unwrap().into_iter().collect::<BTreeSet<_>>();
    assert_eq!(names, ["paused", "active", "developer", "platform", "future", "opaque"].into_iter().map(str::to_string).collect());
    assert!(adapter.declaration_names("extensions:\n  duplicate: 1\n  duplicate: 2\n").is_err());
}

#[test]
fn goose_stdio_env_alias_is_effective_and_keeps_native_spelling_and_comments() {
    let home = TestHome::new("goose-env-alias");
    let path = home.home.join("config.yaml");
    for id in ["goose", "goose-desktop"] {
        let adapter = adapter(id);
        for field in ["env", "envs"] {
            let source = format!("extensions:\n  local:\n    type: stdio\n    cmd: fixture-command\n    args: []\n    enabled: false\n    {field}: # native environment spelling\n      GOOSE_FIXTURE: old # retain env comment\n    env_keys: [GOOSE_NATIVE_SECRET]\n    policy: {{keep: true}}\n");
            fs::write(&path, &source).unwrap();
            let (observed, enabled) = &adapter.read_with_enabled(&path)["local"];
            assert!(!enabled);
            let McpConfig::Stdio(observed) = observed else { panic!("expected stdio") };
            assert_eq!(observed.env.as_ref().unwrap()["GOOSE_FIXTURE"], "old", "{id}/{field}");
            let mut desired = local();
            let McpConfig::Stdio(ref mut stdio) = desired else { unreachable!() };
            stdio.env = Some([("GOOSE_FIXTURE".into(), "updated".into())].into_iter().collect());
            adapter.upsert(&path, "local", &desired).unwrap();
            let entry = &value(&path)["extensions"]["local"];
            assert_eq!(entry[field]["GOOSE_FIXTURE"], "updated");
            assert!(entry.get(if field == "env" { "envs" } else { "env" }).is_none());
            assert_eq!(entry["env_keys"], json!(["GOOSE_NATIVE_SECRET"]));
            assert_eq!(entry["enabled"], false);
            let text = fs::read_to_string(&path).unwrap();
            assert!(text.contains("# native environment spelling"));
            assert!(text.contains("# retain env comment"));
            adapter.upsert(&path, "local", &local()).unwrap();
            let entry = &value(&path)["extensions"]["local"];
            assert!(entry.get("env").is_none() && entry.get("envs").is_none());
            assert_eq!(entry["env_keys"], json!(["GOOSE_NATIVE_SECRET"]));
        }
    }
}

#[test]
fn goose_ambiguous_aliases_and_malformed_native_environments_do_not_acquire_write_authority() {
    let home = TestHome::new("goose-invalid-env");
    let path = home.home.join("config.yaml");
    let adapter = adapter("goose");
    for declaration in [
        "    env: {GOOSE_FIXTURE: same}\n    envs: {GOOSE_FIXTURE: same}\n",
        "    env: {GOOSE_FIXTURE: one}\n    envs: {GOOSE_FIXTURE: two}\n",
        "    env: null\n", "    envs: null\n", "    env: [bad]\n",
        "    envs: {GOOSE_FIXTURE: 1}\n", "    env: {GOOSE_FIXTURE: false}\n",
        "    env_keys: native-secret\n", "    env_keys: [1]\n",
    ] {
        let source = format!("extensions:\n  invalid:\n    type: stdio\n    cmd: fixture-command\n    args: []\n    enabled: true\n{declaration}");
        fs::write(&path, &source).unwrap();
        assert!(adapter.read_with_enabled(&path).is_empty(), "{declaration}");
        assert!(adapter.upsert(&path, "invalid", &local()).is_err(), "{declaration}");
        let snapshot = adapter.snapshot(&path, "invalid").unwrap().unwrap();
        assert!(adapter.set_enabled(&path, "invalid", false, &snapshot).is_err(), "{declaration}");
        assert_eq!(adapter.declaration_names(&source).unwrap(), ["invalid"]);
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
    }
    for declaration in ["    envs: null\n", "    envs: {GOOSE_FIXTURE: false}\n"] {
        let source = format!("extensions:\n  invalid:\n    type: streamable_http\n    uri: https://example.test/mcp\n    enabled: true\n{declaration}");
        fs::write(&path, &source).unwrap();
        let desired = McpConfig::Http(HttpConfig { kind: "streamable-http".into(), url: "https://example.test/new".into(), headers: None });
        assert!(adapter.read_with_enabled(&path).is_empty());
        assert!(adapter.upsert(&path, "invalid", &desired).is_err());
        let snapshot = adapter.snapshot(&path, "invalid").unwrap().unwrap();
        assert!(adapter.set_enabled(&path, "invalid", false, &snapshot).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
    }
}

#[test]
fn goose_http_updates_and_native_toggle_preserve_unmodelled_environment_context() {
    let home = TestHome::new("goose-http-env-policy");
    let path = home.home.join("config.yaml");
    let source = "# native HTTP configuration\nextensions:\n  remote:\n    name: Remote\n    description: \"\"\n    type: streamable_http\n    uri: https://example.test/old\n    enabled: false\n    envs: # keep native HTTP substitution\n      GOOSE_FIXTURE: value # keep native environment\n    env_keys: [GOOSE_NATIVE_SECRET]\n    env: {future: [keep, as-is]} # not an HTTP alias\n    headers: {X-Old: old}\n    client_secret_key: GOOSE_NATIVE_SECRET\n    policy: {keep: true}\n";
    let desired = McpConfig::Http(HttpConfig {
        kind: "streamable-http".into(), url: "https://example.test/new".into(),
        headers: Some([("X-New".into(), "$GOOSE_FIXTURE".into())].into_iter().collect()),
    });
    for id in ["goose", "goose-desktop"] {
        let adapter = adapter(id);
        fs::write(&path, source).unwrap();
        #[cfg(unix)]
        let inode = { use std::os::unix::fs::MetadataExt; fs::metadata(&path).unwrap().ino() };
        let mut expected = value(&path);
        expected["extensions"]["remote"]["uri"] = json!("https://example.test/new");
        expected["extensions"]["remote"]["headers"] = json!({"X-New": "$GOOSE_FIXTURE"});
        adapter.upsert(&path, "remote", &desired).unwrap();
        assert_eq!(value(&path), expected, "{id}");
        assert!(!adapter.read_with_enabled(&path)["remote"].1);
        let snapshot = adapter.snapshot(&path, "remote").unwrap().unwrap();
        adapter.set_enabled(&path, "remote", true, &snapshot).unwrap();
        expected["extensions"]["remote"]["enabled"] = json!(true);
        assert_eq!(value(&path), expected);
        let text = fs::read_to_string(&path).unwrap();
        for comment in ["# native HTTP configuration", "# keep native HTTP substitution", "# keep native environment", "# not an HTTP alias"] {
            assert!(text.contains(comment), "{comment}");
        }
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        }
    }
}

#[test]
fn goose_transport_changes_cannot_silently_transfer_or_erase_environment_context() {
    let home = TestHome::new("goose-transport-env-boundary");
    let path = home.home.join("config.yaml");
    let adapter = adapter("goose");
    for original_transport in ["stdio", "streamable_http"] {
        for context in ["    env: {GOOSE_FIXTURE: value}\n", "    envs: {GOOSE_FIXTURE: value}\n", "    envs: {}\n", "    env_keys: [GOOSE_NATIVE_SECRET]\n"] {
            let connection = if original_transport == "stdio" { "    cmd: fixture-command\n    args: []\n" } else { "    uri: https://example.test/mcp\n" };
            let source = format!("extensions:\n  target:\n    type: {original_transport}\n{connection}    enabled: true\n{context}");
            fs::write(&path, &source).unwrap();
            let desired = if original_transport == "stdio" {
                McpConfig::Http(HttpConfig { kind: "streamable-http".into(), url: "https://example.test/new".into(), headers: None })
            } else { local() };
            let error = adapter.upsert(&path, "target", &desired).unwrap_err();
            assert!(error.contains("environment"), "{error}");
            assert_eq!(fs::read_to_string(&path).unwrap(), source);
        }
    }
}

#[test]
fn goose_transport_changes_without_environment_context_keep_policy_and_remove_old_connection_fields() {
    let home = TestHome::new("goose-transport-without-env");
    let path = home.home.join("config.yaml");
    let adapter = adapter("goose");
    fs::write(&path, "extensions:\n  target:\n    type: stdio\n    cmd: fixture-command\n    args: []\n    cwd: /fixture\n    enabled: false\n    env_keys: []\n    policy: {keep: true}\n").unwrap();
    let desired = McpConfig::Http(HttpConfig { kind: "streamable-http".into(), url: "https://example.test/new".into(), headers: Some([("X-Test".into(), "value".into())].into_iter().collect()) });
    adapter.upsert(&path, "target", &desired).unwrap();
    let remote = value(&path)["extensions"]["target"].clone();
    assert_eq!(remote["type"], "streamable_http");
    assert_eq!(remote["uri"], "https://example.test/new");
    assert_eq!(remote["enabled"], false);
    assert_eq!(remote["env_keys"], json!([]));
    for field in ["cmd", "args", "cwd", "env", "envs"] { assert!(remote.get(field).is_none()); }
    adapter.upsert(&path, "target", &local()).unwrap();
    let local = value(&path)["extensions"]["target"].clone();
    assert_eq!(local["type"], "stdio");
    assert_eq!(local["cmd"], "fixture-command");
    assert_eq!(local["args"], json!([]));
    assert_eq!(local["enabled"], false);
    assert_eq!(local["policy"], json!({"keep": true}));
    assert!(local.get("uri").is_none() && local.get("headers").is_none());
}
