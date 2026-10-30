//! Isolated regression source retained without executing tests in fast mode.
use super::*;
use crate::testenv::TestHome;
use serde_json::json;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};

struct Env(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Env {
    fn clear() -> Self {
        Self(["NODE_OPTIONS", "NODE_PATH", "CODEBUDDY_CONFIG_DIR", "ACC_PRODUCT_CONFIG_PATH",
            "ACC_PRODUCT_CONFIG_V3", "ACC_PRODUCT_CONFIG_V2", "ACC_PRODUCT_CONFIG"].into_iter().map(|k| {
                let previous = std::env::var_os(k); std::env::remove_var(k); (k, previous)
            }).collect())
    }
}
impl Drop for Env {
    fn drop(&mut self) { for (key, value) in &self.0 { match value { Some(v) => std::env::set_var(key, v), None => std::env::remove_var(key) } } }
}
struct Fixture { _env: Env, home: TestHome, c: Contract, host: Host, root: PathBuf, alias: PathBuf, native: PathBuf }
fn put(path: &Path, bytes: &[u8]) { fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, bytes).unwrap(); }
fn fixture() -> Fixture {
    let home = TestHome::new("codebuddy-cli-native"); let env = Env::clear();
    let mut c: Contract = serde_json::from_str(include_str!("../../../../../data/native-plugin-codebuddy-cli.json")).unwrap();
    let root = home.home.join("public-package"); let alias = home.home.join("bin/codebuddy");
    for g in &mut c.guards {
        let bytes = match g.path.as_str() {
            "package.json" => serde_json::to_vec(&json!({"name":c.package_name,"version":c.version,"bin":{"codebuddy":"./bin/codebuddy"},"publishConfig":{"access":"public"}})).unwrap(),
            "product.json" => br#"{"platform":"CLI"}"#.to_vec(),
            _ => format!("INERT PUBLIC MEMBER:{}", g.path).into_bytes(),
        };
        put(&root.join(&g.path), &bytes); g.bytes = bytes.len() as u64; g.sha256 = hex::encode(Sha256::digest(&bytes));
    }
    #[cfg(unix)] {
        fs::set_permissions(root.join("bin/codebuddy"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir_all(alias.parent().unwrap()).unwrap(); symlink(root.join("bin/codebuddy"), &alias).unwrap();
    }
    put(&root.join("builtin/skills/mods-create/SKILL.md"), b"PRIVATE_SKILL_BODY_NOT_OPENED");
    let native = home.home.join(".codebuddy");
    let host = Host { agent_id:"codebuddy-code".into(), adapter:"codebuddy_cli".into(), docs_url:"https://cnb.cool/codebuddy/codebuddy-code".into(),
        manifest_paths:vec![], default_mcp_files:vec![], mcp_id_prefix:String::new(), resource_groups:vec![],
        roots:vec![Root { path:"~/.codebuddy/plugins/installed_plugins.json".into(), layout:"codebuddy_installed_registry".into(), presence:PluginPresence::Installed, complete_marker:None }] };
    Fixture { _env:env, home, c, host, root, alias, native }
}
fn read(f: &Fixture) -> PluginScan {
    let mut result = PluginScan::default();
    if observe_contract(&f.host, &f.c, &Mutex::default(), &mut result).is_err() { result.unavailable = true; }
    normalize(&mut result); result
}
fn registry(f: &Fixture, value: Value) { put(&f.native.join("plugins/installed_plugins.json"), &serde_json::to_vec(&value).unwrap()); }
fn plugin(f: &Fixture, name: &str, market: &str, version: &str) -> PathBuf {
    let root = f.native.join("plugins/cache").join(sanitized_name(market)).join(sanitized_name(name)).join(version);
    put(&root.join(".codebuddy-plugin/plugin.json"), &serde_json::to_vec(&json!({"name":name,"version":version})).unwrap()); root
}
fn register(f: &Fixture, root: &Path) { registry(f, json!({"version":2,"plugins":{"native@market":[{"scope":"user","installPath":root,"version":"1.0.0"}]}})); }
fn named<'a>(r: &'a PluginScan, id: &str) -> &'a NativePluginObservation { r.plugins.iter().find(|p| p.plugin_id == id).unwrap() }

#[test]
fn selected_official_package_has_one_application_owned_stat_only_skill() {
    let f = fixture(); let r = read(&f); assert!(!r.unavailable); assert!(r.mcps.is_empty());
    let p = named(&r, "@builtin/skills"); assert_eq!(p.components.len(), 1);
    assert_eq!(p.components[0].name, "mods-create"); assert!(p.components[0].native_id.is_none());
    assert_eq!(p.presence, PluginPresence::Bundled); assert!(p.enabled.is_none());
    assert_eq!(skill_observations(&r.plugins).len(), 1);
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_SKILL_BODY"));
}
#[test]
fn templates_are_not_default_installed_plugins_or_bundled_skills() {
    let f = fixture(); put(&f.root.join("builtin/mod-templates/host/skills/example/SKILL.md"), b"PRIVATE_TEMPLATE");
    let r = read(&f); assert_eq!(r.plugins.len(), 1); assert_eq!(named(&r,"@builtin/skills").components.len(), 1);
}
#[test]
fn passive_version_never_reads_native_registration_or_account_data() {
    let f = fixture(); put(&f.native.join("plugins/installed_plugins.json"), b"INVALID_PRIVATE_REGISTRY");
    let proof = Verified::capture(&f.alias, &f.c, &Mutex::default()).unwrap(); proof.revalidate().unwrap();
    assert!(read(&f).unavailable);
}
#[test]
fn cache_history_without_current_registration_is_never_installed() {
    let f = fixture(); plugin(&f,"stale","market","9.0.0"); assert_eq!(read(&f).plugins.len(), 1);
}
#[test]
fn current_registry_selects_materialized_cache_and_keeps_unknown_enablement() {
    let f = fixture(); let root = plugin(&f,"native","market","1.0.0"); register(&f,&root);
    let r = read(&f); assert!(!r.unavailable); let p = named(&r,"native@market");
    assert_eq!(p.presence,PluginPresence::Installed); assert_eq!(p.version,"1.0.0"); assert!(p.enabled.is_none());
}
#[test]
fn current_reader_array_and_record_aliases_preserve_native_identity() {
    let f = fixture(); let root = plugin(&f,"native","market","1.0.0");
    registry(&f,json!([{"name":"native","marketplaceName":"market","scope":"user","path":root}]));
    let r = read(&f); assert!(!r.unavailable); assert_eq!(named(&r,"native@market").version,"unknown");
}
#[test]
fn explicit_invalid_plugin_id_does_not_silently_recover_the_registry_key() {
    assert!(record(Some("native@market"),&json!({"pluginId":42,"installPath":"/unused"})).is_err());
    let row=record(Some("wrong@key"),&json!({"pluginId":42,"name":"native","marketplaceName":"market","installPath":"/unused"})).unwrap().unwrap();
    assert_eq!(row.id,"native@market");
}
#[test]
fn empty_preferred_install_path_is_ignored_without_using_the_other_path_alias() {
    let f=fixture();let root=plugin(&f,"ghost","market","1.0.0");
    registry(&f,json!({"version":2,"plugins":{"ghost@market":[{"scope":"user","installPath":"","path":root}]}}));
    let r=read(&f);assert!(!r.unavailable);assert!(!r.plugins.iter().any(|p|p.plugin_id=="ghost@market"));
}
#[test]
fn registered_versions_and_global_scopes_are_not_collapsed_to_the_host_version() {
    let f = fixture(); let a = plugin(&f,"native","market","1.0.0"); let b = plugin(&f,"native","market","2.0.0");
    registry(&f,json!({"version":2,"plugins":{"native@market":[{"scope":"user","installPath":a,"version":"1.0.0"},{"scope":"managed","installPath":b,"version":"2.0.0"}]}}));
    let r = read(&f); assert_eq!(r.plugins.iter().filter(|p|p.plugin_id=="native@market").count(),2);
    assert!(r.plugins.iter().filter(|p|p.plugin_id=="native@market").all(|p|p.version!=f.c.version&&p.enabled.is_none()));
}
#[test]
fn project_local_records_and_arbitrary_sources_are_never_followed() {
    let f = fixture(); let outside = f.home.home.join("PRIVATE_OUTSIDE"); put(&outside.join(".codebuddy-plugin/plugin.json"),br#"{"name":"native"}"#);
    registry(&f,json!({"version":2,"plugins":{"project@market":[{"scope":"project","installPath":outside}],"native@market":[{"scope":"user","installPath":outside}]}}));
    let r = read(&f); assert!(r.unavailable); assert!(!r.plugins.iter().any(|p|p.plugin_id=="project@market"));
    let p = named(&r,"native@market"); assert_eq!(p.presence,PluginPresence::Declared); assert!(p.components.is_empty());
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_OUTSIDE"));
}
#[test]
fn a_missing_registered_cache_stays_declared_without_manufactured_capabilities() {
    let f = fixture(); let root = f.native.join("plugins/cache/market/native/1.0.0"); register(&f,&root);
    let r = read(&f); assert!(r.unavailable); assert!(named(&r,"native@market").components.is_empty());
}
#[test]
fn metadata_directory_precedence_does_not_fall_through_malformed_preferred_json() {
    let f = fixture(); let root = plugin(&f,"native","market","1.0.0"); register(&f,&root);
    put(&root.join(".workbuddy-plugin/plugin.json"),br#"{"name":"native"}"#);
    put(&root.join(".codebuddy-plugin/plugin.json"),b"NOT_JSON");
    assert_eq!(named(&read(&f),"native@market").presence,PluginPresence::Declared);
}
#[test]
fn cli_admits_workbuddy_metadata_directory_without_using_desktop_contracts() {
    let f = fixture(); let root = plugin(&f,"native","market","1.0.0"); register(&f,&root);
    fs::remove_file(root.join(".codebuddy-plugin/plugin.json")).unwrap(); put(&root.join(".workbuddy-plugin/plugin.json"),br#"{"name":"native"}"#);
    assert_eq!(named(&read(&f),"native@market").presence,PluginPresence::Installed);
}
#[test]
fn skill_command_and_hook_bodies_never_enter_observations() {
    let f = fixture(); let root = plugin(&f,"native","market","1.0.0"); register(&f,&root);
    put(&root.join("skills/review/SKILL.md"),b"PRIVATE_SKILL"); put(&root.join("commands/review.md"),b"PRIVATE_COMMAND");
    put(&root.join("hooks/hooks.json"),b"NOT_JSON_PRIVATE_HOOK");
    let r = read(&f); let p = named(&r,"native@market"); assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::Skill&&c.name=="review"));
    assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::Hook)); assert!(p.components.iter().all(|c|c.native_id.is_none()));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}
#[test]
fn arbitrary_markdown_and_unproven_rule_fields_do_not_become_native_skills_or_rules() {
    let f=fixture();let root=plugin(&f,"native","market","1.0.0");register(&f,&root);
    put(&root.join(".codebuddy-plugin/plugin.json"),br#"{"name":"native","skills":"README.md","rules":"rules"}"#);
    put(&root.join("README.md"),b"PRIVATE_README");put(&root.join("rules/example.md"),b"PRIVATE_RULE");
    let r=read(&f);let p=named(&r,"native@market");assert!(!p.components.iter().any(|c|matches!(c.kind,PluginComponentKind::Skill|PluginComponentKind::Rule)));
}
#[test]
fn mcp_and_lsp_declarations_are_references_without_transport_values() {
    let f = fixture(); let root = plugin(&f,"native","market","1.0.0"); register(&f,&root);
    put(&root.join(".mcp.json"),br#"{"mcpServers":{"same":{"command":"PRIVATE_COMMAND","env":{"KEY":"PRIVATE_TOKEN"}}}}"#);
    put(&root.join("mcp/remote.json"),br#"{"mcpServers":{"same":{"url":"https://PRIVATE_URL","headers":{"Authorization":"PRIVATE_SECRET"}}}}"#);
    put(&root.join(".lsp.json"),br#"{"lspServers":{"language":{"command":"PRIVATE_LSP"}}}"#);
    let r = read(&f); assert!(r.mcps.is_empty()); let p = named(&r,"native@market");
    assert_eq!(p.components.iter().filter(|c|c.kind==PluginComponentKind::McpReference).count(),2);
    assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::Lsp)); assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}
#[test]
fn skill_bodies_cannot_be_reclassified_as_json_metadata_reads() {
    let f = fixture(); let root = plugin(&f,"native","market","1.0.0"); register(&f,&root);
    put(&root.join(".codebuddy-plugin/plugin.json"),br#"{"name":"native","mcpServers":"skills/private/SKILL.md"}"#);
    put(&root.join("skills/private/SKILL.md"),b"PRIVATE_NOT_JSON"); let r = read(&f); assert!(r.unavailable);
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_NOT_JSON"));
}
#[test]
fn credential_store_overrides_stay_filename_references_without_opening_payloads() {
    let f=fixture();let root=plugin(&f,"native","market","1.0.0");register(&f,&root);
    put(&root.join(".codebuddy-plugin/plugin.json"),br#"{"name":"native","mcpServers":"oauth-tokens.json"}"#);
    put(&root.join("oauth-tokens.json"),br#"{"PRIVATE_CREDENTIAL_SERVER":{"token":"PRIVATE_TOKEN_VALUE"}}"#);
    let r=read(&f);assert!(r.unavailable);let p=named(&r,"native@market");
    assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="oauth-tokens.json"));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}
#[test]
fn every_sensitive_or_hook_payload_path_stays_opaque_without_hiding_valid_siblings() {
    let f=fixture();let root=plugin(&f,"native","market","1.0.0");register(&f,&root);
    for path in ["hooks/hooks.json","credentials/data.json","oauth/session.json","lifecycle/custom.json"] {
        let manifest=json!({"name":"native","mcpServers":path,"hooks":"lifecycle/custom.json"});
        put(&root.join(".codebuddy-plugin/plugin.json"),&serde_json::to_vec(&manifest).unwrap());
        put(&root.join(path),br#"{"PRIVATE_BODY_SERVER":{"command":"PRIVATE_BODY_COMMAND"}}"#);
        put(&root.join("mcp/a-invalid.json"),b"NOT_JSON");
        put(&root.join("mcp/b-valid.json"),br#"{"mcpServers":{"public-server":{"command":"not-executed"}}}"#);
        let r=read(&f);assert!(r.unavailable);let p=named(&r,"native@market");
        assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="public-server"));
        assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_BODY_"));
    }
}
#[test]
fn product_overrides_do_not_redirect_to_private_global_sources() {
    let f = fixture(); std::env::set_var("ACC_PRODUCT_CONFIG", "PRIVATE_PRODUCT");
    let r = read(&f); assert!(r.unavailable); assert_eq!(r.plugins.len(),1); assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_PRODUCT"));
}
#[test]
fn absolute_native_home_override_is_confined_and_relative_override_is_unknown() {
    let f = fixture(); let p = f.home.home.join("other-home"); std::env::set_var("CODEBUDDY_CONFIG_DIR",&p); assert_eq!(native_home().unwrap(),p);
    std::env::set_var("CODEBUDDY_CONFIG_DIR","relative"); assert!(native_home().is_err());
}
#[test]
fn unknown_node_preloads_or_public_guard_mutation_never_execute_a_fallback() {
    let f = fixture(); std::env::set_var("NODE_OPTIONS","--require PRIVATE_MODULE"); assert!(Verified::capture(&f.alias,&f.c,&Mutex::default()).is_err());
    std::env::remove_var("NODE_OPTIONS"); put(&f.root.join("dist/codebuddy.js"),b"CHANGED"); assert!(Verified::capture(&f.alias,&f.c,&Mutex::default()).is_err());
}
#[test]
fn private_identity_budget_does_not_consume_preexisting_host_allowances() {
    let f = fixture(); let mut r = PluginScan::default(); r.budget.bytes=MAX_READ_BYTES; r.budget.verified_bytes=MAX_VERIFIED_BYTES; r.budget.visited=MAX_SCAN_ENTRIES;
    observe_contract(&f.host,&f.c,&Mutex::default(),&mut r).unwrap(); assert_eq!(r.budget.bytes,MAX_READ_BYTES); assert_eq!(r.budget.verified_bytes,MAX_VERIFIED_BYTES); assert_eq!(r.budget.visited,MAX_SCAN_ENTRIES);
}
#[test]
fn changed_guard_expectation_cannot_reuse_a_previous_digest_entry() {
    let mut f = fixture(); let cache=Mutex::default(); Verified::capture(&f.alias,&f.c,&cache).unwrap();
    f.c.guards.iter_mut().find(|g|g.path=="dist/codebuddy.js").unwrap().sha256="0".repeat(64); assert!(Verified::capture(&f.alias,&f.c,&cache).is_err());
}
#[test]
fn registry_replacement_and_alias_switch_invalidate_final_publication() {
    let f = fixture(); let root = plugin(&f,"native","market","1.0.0"); register(&f,&root);
    let audit = capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap(); let p=f.native.join("plugins/installed_plugins.json");
    let bytes=fs::read(&p).unwrap(); fs::rename(&p,p.with_extension("old")).unwrap(); put(&p,&bytes); assert!(audit.revalidate().is_err());
}
#[cfg(unix)]
#[test]
fn symlinked_cache_and_skill_markers_never_follow_external_payloads() {
    let f = fixture(); let root = plugin(&f,"native","market","1.0.0"); register(&f,&root);
    let outside=f.home.home.join("outside.md"); put(&outside,b"PRIVATE_BODY"); fs::create_dir_all(root.join("skills/private")).unwrap();
    symlink(&outside,root.join("skills/private/SKILL.md")).unwrap(); let r=read(&f); assert!(r.unavailable);
    assert!(!named(&r,"native@market").components.iter().any(|c|c.kind==PluginComponentKind::Skill));
}
#[test]
fn native_registry_limits_reject_oversized_arrays_before_allocation_or_scanning() {
    let values=vec![Value::Null;MAX_PLUGINS+1]; assert!(records(&Value::Array(values)).is_err());
}
#[test]
fn watch_paths_exclude_unknown_project_and_source_locations() {
    let f = fixture(); let root=plugin(&f,"native","market","1.0.0"); register(&f,&root); put(&root.join("skills/review/SKILL.md"),b"PRIVATE_BODY");
    let a=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();
    assert!(a.snapshot.watches.contains(&root.join("skills/review/SKILL.md"))); assert!(a.snapshot.watches.iter().all(|p|!p.to_string_lossy().contains("PRIVATE_SOURCE")));
}
#[test]
fn credential_urls_cannot_become_component_source_paths() {
    let f=fixture();assert!(relative_member(&f.root,"https://PRIVATE_USER:PRIVATE_PASSWORD@example.test/oauth.json").is_err());
}
