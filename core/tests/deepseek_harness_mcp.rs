use arca_core::{adapter::Adapter, resources::{deepseek_harness::{guard_profile, home_dir, profile_path, PatchFile}, mcp::deepseek_harness_adapter::DeepseekHarnessAdapter}, testenv::TestHome, types::{McpConfig, StdioConfig, HttpConfig}};
use serde_json::{json, Value};
use std::{ffi::OsString, fs, path::PathBuf};

struct Fixture { home: TestHome, previous: Option<OsString>, path: PathBuf }
impl Fixture {
    fn new(profile: &str) -> Self {
        let home = TestHome::new("deepseek-harness-mcp");
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", home.home.join(".dsh"));
        let path = home_dir().join("profiles").join(profile).join("cordis.patch.yml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path.parent().unwrap().join("package.json"), json!({"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app"]}}}).to_string()).unwrap();
        Self { home, previous, path }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { if let Some(value) = &self.previous { std::env::set_var("DSH_HOME", value); } else { std::env::remove_var("DSH_HOME"); } }
}
fn local() -> McpConfig { McpConfig::Stdio(StdioConfig { command: "fixture-command".into(), args: Some(vec!["changed".into()]), env: None, cwd: None }) }
const NATIVE: &str = include_str!("fixtures/deepseek-harness-profile.yaml");

#[cfg(unix)]
struct CarrierPath(Option<OsString>);
#[cfg(unix)]
impl CarrierPath {
    fn select(home: &std::path::Path, version: &str) -> Self {
        use std::os::unix::{fs::symlink, fs::PermissionsExt};
        let root = home.join("carrier/node_modules/@deepseek-ai/dsh");
        fs::create_dir_all(root.join("lib")).unwrap();
        fs::write(root.join("lib/bin.js"), "// inert fixture: must never execute\n").unwrap();
        fs::set_permissions(root.join("lib/bin.js"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("package.json"), json!({"name":"@deepseek-ai/dsh","version":version,"bin":{"dsh":"lib/bin.js"},"repository":{"url":"git+https://github.com/deepseek-ai/deepseek-harness.git","directory":"apps/cli"}}).to_string()).unwrap();
        let bin = home.join("selected-bin"); fs::create_dir_all(&bin).unwrap();
        symlink(root.join("lib/bin.js"), bin.join("dsh")).unwrap();
        let previous = std::env::var_os("PATH"); std::env::set_var("PATH", bin);
        Self(previous)
    }
}
#[cfg(unix)]
impl Drop for CarrierPath {
    fn drop(&mut self) { if let Some(path) = &self.0 { std::env::set_var("PATH", path); } else { std::env::remove_var("PATH"); } }
}

#[cfg(unix)]
#[test]
fn old_unknown_or_incomplete_carrier_cannot_create_an_inherited_manager_override() {
    for version in ["0.2.1-rc.3", "9.0.0", "0.2.2-rc.1"] {
        let f = Fixture::new("web");
        let _carrier = CarrierPath::select(&f.home.home, version);
        // Even the audited CLI version without its physical bundle/dependency
        // closure cannot authorize a manager override merely from its label.
        let original = "# profile metadata, no installed manager\n[]\n";
        fs::write(&f.path, original).unwrap();
        let error = DeepseekHarnessAdapter.upsert(&f.path, "safe", &local()).unwrap_err();
        assert!(error.contains("deepseek_harness_mcp_manager_unverified"));
        assert_eq!(fs::read_to_string(&f.path).unwrap(), original);
        assert!(!f.home.home.join(".dsh/.credentials.yaml").exists());
    }
}

#[cfg(unix)]
#[test]
fn an_existing_complete_manager_preserves_its_writer_without_new_carrier_authority() {
    let f = Fixture::new("web");
    let _carrier = CarrierPath::select(&f.home.home, "0.2.1-rc.3");
    fs::write(&f.path, NATIVE).unwrap();
    DeepseekHarnessAdapter.upsert(&f.path, "safe", &local()).unwrap();
    assert!(DeepseekHarnessAdapter.read(&f.path).contains_key("safe"));
    assert!(fs::read_to_string(&f.path).unwrap().contains("# keep model comment"));
}

#[test]
fn native_web_and_desktop_patch_keeps_paused_state_policy_comments_and_unrelated_js() {
    for profile in ["web", "desktop"] {
        let f = Fixture::new(profile);
        fs::write(&f.path, NATIVE).unwrap();
        let adapter = DeepseekHarnessAdapter;
        let observed = adapter.read_with_enabled(&f.path);
        assert!(!observed["paused"].1);
        assert!(!adapter.read(&f.path).contains_key("paused"));
        #[cfg(unix)] let inode = { use std::os::unix::fs::MetadataExt; fs::metadata(&f.path).unwrap().ino() };
        adapter.upsert(&f.path, "paused", &local()).unwrap();
        let config = PatchFile::read(&f.path).unwrap().config("mcp-manager").unwrap().unwrap();
        assert_eq!(config["servers"]["paused"]["enabled"], false);
        assert_eq!(config["servers"]["paused"]["startupTimeoutMs"], 42000);
        assert_eq!(config["servers"]["paused"]["futureServerPolicy"], json!({"keep":true}));
        assert_eq!(config["futureManagerPolicy"], json!({"keep":true}));
        let rendered = fs::read_to_string(&f.path).unwrap();
        for text in ["!!js dshHomePath('unrelated')", "# keep command comment", "# native pause", "# keep model comment"] { assert!(rendered.contains(text), "{text}"); }
        #[cfg(unix)] { use std::os::unix::fs::MetadataExt; assert_eq!(fs::metadata(&f.path).unwrap().ino(), inode); }
    }
}

#[test]
fn native_toggle_and_snapshot_removal_are_bound_to_the_complete_entry() {
    let f = Fixture::new("web"); fs::write(&f.path, NATIVE).unwrap();
    let adapter = DeepseekHarnessAdapter;
    let before = adapter.snapshot(&f.path, "paused").unwrap().unwrap();
    adapter.set_enabled(&f.path, "paused", true, &before).unwrap();
    assert!(adapter.read(&f.path).contains_key("paused"));
    let bytes = fs::read(&f.path).unwrap();
    assert!(adapter.remove_snapshot(&f.path, "paused", &before).is_err());
    assert_eq!(fs::read(&f.path).unwrap(), bytes);
    let current = adapter.snapshot(&f.path, "paused").unwrap().unwrap();
    adapter.remove_snapshot(&f.path, "paused", &current).unwrap();
    adapter.restore(&f.path, "paused", &current).unwrap();
    assert!(adapter.restore(&f.path, "paused", &current).is_err());
    assert_eq!(adapter.snapshot(&f.path, "paused").unwrap(), Some(current));
}

#[test]
fn clear_keeps_explicit_servers_map_and_never_touches_model_or_expression_rows() {
    let f = Fixture::new("desktop"); fs::write(&f.path, NATIVE).unwrap();
    let adapter = DeepseekHarnessAdapter;
    let models = PatchFile::read(&f.path).unwrap().config("llm-pi-ai").unwrap();
    adapter.clear(&f.path).unwrap();
    let file = PatchFile::read(&f.path).unwrap();
    assert_eq!(file.config("mcp-manager").unwrap().unwrap()["servers"], json!({}));
    assert_eq!(file.config("llm-pi-ai").unwrap(), models);
    assert!(!adapter.has_entries(&f.path).unwrap());
    assert!(fs::read_to_string(&f.path).unwrap().contains("!!js dshHomePath('unrelated')"));
}

#[test]
fn home_override_custom_inheritance_disabled_or_duplicate_target_never_grants_a_writer() {
    let f = Fixture::new("web"); let adapter = DeepseekHarnessAdapter;
    fs::write(home_dir().join("cordis.patch.yml"), "- id: mcp-manager\n  config: {servers: {}}\n").unwrap();
    assert!(adapter.upsert(&f.path, "safe", &local()).is_err());
    assert!(!f.path.exists());
    fs::remove_file(home_dir().join("cordis.patch.yml")).unwrap();
    fs::write(f.path.parent().unwrap().join("package.json"), r#"{"dsh":{"profile":{"bundles":["@fixture/custom"]}}}"#).unwrap();
    assert!(adapter.upsert(&f.path, "safe", &local()).is_err());
    for text in ["- id: mcp-manager\n  disabled: true\n  config: {servers: {}}\n", "- id: mcp-manager\n  config: {}\n- id: mcp-manager\n  config: {}\n", "- id: mcp-manager\n  config: {servers: {}, servers: {}}\n", "- id: mcp-manager\n  config: !!js computeServers()\n"] {
        fs::write(&f.path, text).unwrap();
        assert!(adapter.upsert(&f.path, "safe", &local()).is_err());
        assert_eq!(fs::read_to_string(&f.path).unwrap(), text);
    }
}

#[test]
fn existing_empty_or_whitespace_patch_is_not_repaired_or_overwritten() {
    let f = Fixture::new("web"); let adapter = DeepseekHarnessAdapter;
    for text in ["", "  \n\t", "# no patch sequence\n"] {
        fs::write(&f.path, text).unwrap();
        assert!(PatchFile::read(&f.path).is_err());
        assert!(adapter.upsert(&f.path, "safe", &local()).is_err());
        assert_eq!(fs::read_to_string(&f.path).unwrap(), text);
    }
    fs::remove_file(&f.path).unwrap();
    assert_eq!(PatchFile::read(&f.path).unwrap().original(), None);
}

#[test]
fn server_identity_transport_cross_fields_and_credential_templates_are_native() {
    let f = Fixture::new("web"); let adapter = DeepseekHarnessAdapter;
    for name in ["", "has.dot", "has space", "../escape", "012345678901234567890123456789012"] { assert!(adapter.upsert(&f.path, name, &local()).is_err()); }
    let sse = McpConfig::Http(HttpConfig { kind: "sse".into(), url: "https://example.test/mcp".into(), headers: None });
    assert!(adapter.upsert(&f.path, "safe", &sse).is_err());
    adapter.upsert(&f.path, "Upper_OK-1", &local()).unwrap();
    let text = "- id: mcp-manager\n  config:\n    servers:\n      unsafe:\n        transport: streamable-http\n        url: https://example.test/mcp\n        env: {KEEP: value}\n        cwd: /fixture\n";
    fs::write(&f.path, text).unwrap();
    assert!(adapter.upsert(&f.path, "unsafe", &local()).is_err());
    assert_eq!(fs::read_to_string(&f.path).unwrap(), text);
    let mut invalid = local(); if let McpConfig::Stdio(ref mut config) = invalid { config.args = Some(vec!["${OTHER}".into()]); }
    assert!(adapter.upsert(&f.path, "safe", &invalid).is_err());
}

#[test]
fn shared_leaf_array_edit_and_whole_config_removal_preserve_sibling_comments() {
    let f = Fixture::new("web"); fs::write(&f.path, NATIVE).unwrap();
    let mut file = PatchFile::read(&f.path).unwrap();
    file.set_config_path("llm-pi-ai", &["providers","external","models","0","contextWindow"], &json!(24000)).unwrap();
    let text = file.render().unwrap();
    assert!(text.contains("# keep model comment"));
    assert_eq!(file.config("llm-pi-ai").unwrap().unwrap()["providers"]["external"]["models"][0]["policy"], json!({"keep":true}));
    file.set_config_field("agent-default-model", "provider", &json!("fixture-provider")).unwrap();
    file.set_config_field("agent-default-model", "model", &json!("fixture-model")).unwrap();
    file.remove_config_path("agent-default-model", &[]).unwrap();
    assert!(file.config("agent-default-model").unwrap().is_none());
    assert!(file.has_row("agent-default-model").unwrap());
    assert!(file.render().unwrap().contains("!!js dshHomePath('unrelated')"));
}

#[test]
fn readonly_declared_inspection_does_not_inherit_writer_policy_or_evaluate_js() {
    let _f = Fixture::new("web"); let adapter = DeepseekHarnessAdapter;
    let text = "- id: unrelated\n  config: !!js doNotRun()\n- id: mcp-manager\n  disabled: true\n  config: {servers: {docs: {transport: future, token: fixture}}}\n";
    assert_eq!(adapter.declaration_names(text).unwrap(), ["docs"]);
    assert_eq!(adapter.inspect_text(text, "docs").unwrap().unwrap()["transport"], "future");
}

#[cfg(unix)]
#[test]
fn shared_reader_and_writer_reject_symlink_parents_and_alias_targets() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("web");
    let outside = f.home.home.join("outside.yaml"); fs::write(&outside, NATIVE).unwrap();
    symlink(&outside, &f.path).unwrap();
    assert!(PatchFile::read(&f.path).is_err());
    assert!(DeepseekHarnessAdapter.upsert(&f.path, "safe", &local()).is_err());
    assert_eq!(fs::read_to_string(&outside).unwrap(), NATIVE);
    assert!(PatchFile::from_text("- id: mcp-manager\n  config: &config {servers: {}}\n- id: other\n  config: *config\n").is_err());
    assert!(profile_path("web").ends_with("/profiles/web/cordis.patch.yml"));
    assert!(guard_profile(&f.path, &["mcp-manager"]).is_err());
    let _ = Value::Null;
}
