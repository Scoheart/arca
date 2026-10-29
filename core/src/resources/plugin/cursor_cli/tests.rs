use super::*;
use crate::testenv::TestHome;
use serde_json::json;

const LAUNCHER: &[u8] = b"#!/bin/sh\nprintf executed > \"$HOME/cursor-cli-executed-marker\"\n";

fn host() -> Host {
    serde_json::from_value(json!({
        "agent_id":"cursor-cli","adapter":"cursor_cli","docs_url":"https://cursor.com/docs/plugins",
        "manifest_paths":[".cursor-plugin/plugin.json",".claude-plugin/plugin.json","plugin.json"],
        "default_mcp_files":[".mcp.json","mcp.json"],"mcp_id_prefix":"plugin-",
        "roots":[{"path":"~/.cursor/plugins/cache","layout":"marketplace_cache","presence":"cache_only","complete_marker":".cache-complete"},
            {"path":"~/.cursor/plugins/local","layout":"flat","presence":"local"}]
    })).unwrap()
}

fn write(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path,body).unwrap();
}

fn fixture(home: &TestHome, mode: Mode) -> (Contract, PathBuf) {
    let root=home.home.join("bin"); let selected=root.join("cursor-agent");
    let mut sources=Vec::new();
    for (path,body) in [("cursor-agent",LAUNCHER),("index.js",b"public cli source".as_slice()),
        ("package.json",b"{}".as_slice()),("3863.index.js",b"public computer-use declaration".as_slice())] {
        write(&root.join(path),body);
        sources.push(SourceFile{path:path.into(),bytes:body.len() as u64,sha256:hex::encode(Sha256::digest(body))});
    }
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&selected,fs::Permissions::from_mode(0o755)).unwrap();}
    (Contract{agent_id:"cursor-cli".into(),command:"cursor-agent".into(),
        profiles:vec![Profile{version:"fixture-version".into(),mode,sources}],
        builtin_mcp:Builtin{id:"computer-use".into(),source_file:"3863.index.js".into(),
            source_module:"../computer-use-mac/dist/index.js".into(),registration_module:"./src/shared/resources.ts".into()}},selected)
}

fn scan(c: &Contract) -> PluginScan {
    let mut out=PluginScan::default();observe_contract(&host(),c,&mut out);normalize(&mut out);out
}
fn printed(scan: &PluginScan) -> String {serde_json::to_string(&(&scan.plugins,&scan.mcps)).unwrap()}

#[test]
fn old_source_can_supply_a_passive_version_without_inheriting_new_plugin_facts() {
    let home=TestHome::new("cursor-cli-old");let(c,selected)=fixture(&home,Mode::PassiveVersionOnly);
    write(&home.home.join(".cursor/plugins/local/demo/.cursor-plugin/plugin.json"),br#"{"name":"demo","mcpServers":{"secret":{"command":"PRIVATE_COMMAND_VALUE"}}}"#);
    let package=verified_package(&c,&selected,&mut ScanBudget::default()).unwrap();assert_eq!(package.profile.version,"fixture-version");
    let out=scan(&c);assert!(out.plugins.is_empty()&&out.mcps.is_empty()&&out.unavailable);assert_eq!(out.budget.bytes,0);
    assert!(builtin_items(&c,&package).is_empty());assert!(!home.home.join("cursor-cli-executed-marker").exists());
}

#[test]
fn compiled_builtin_is_a_declaration_and_never_materializes_or_starts_its_helper() {
    let home=TestHome::new("cursor-cli-builtin");let(c,selected)=fixture(&home,Mode::NativeMetadata);
    let package=verified_package(&c,&selected,&mut ScanBudget::default()).unwrap();let items=builtin_items(&c,&package);
    assert_eq!(items.len(),1);assert_eq!(items[0].server_id,"computer-use");assert_eq!(items[0].agent_id,"cursor-cli");
    assert!(items[0].manifest_path.ends_with("3863.index.js!../computer-use-mac/dist/index.js"));
    assert!(items[0].source_path.ends_with("3863.index.js!./src/shared/resources.ts"));
    assert!(!home.home.join(".cursor/agent-helper").exists());assert!(!home.home.join("cursor-cli-executed-marker").exists());
}

#[test]
fn cache_snapshots_keep_market_identity_and_all_versions_without_claiming_runtime_aliases() {
    let home=TestHome::new("cursor-cli-cache");let(c,_)=fixture(&home,Mode::NativeMetadata);let root=home.home.join(".cursor/plugins/cache");
    for (market,version) in [("one","v1"),("one","v2"),("two","v1")] {
        let p=root.join(market).join("opaque-id").join(version);
        write(&p.join(".cache-complete"),b"");write(&p.join(".cursor-plugin/plugin.json"),br#"{"name":"same-name","version":"1.0.0"}"#);
        write(&p.join(".mcp.json"),br#"{"mcpServers":{"web":{"url":"PRIVATE_URL_VALUE","headers":{"Authorization":"PRIVATE_HEADER_VALUE"}}}}"#);
    }
    let out=scan(&c);assert_eq!(out.plugins.len(),3);assert!(out.mcps.is_empty());
    assert!(out.plugins.iter().all(|p|p.presence==PluginPresence::CacheOnly&&p.enabled.is_none()
        &&p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="web"&&c.native_id.is_none())));
    assert!(out.plugins.iter().any(|p|p.plugin_id=="@cache/one/opaque-id"&&p.revision=="v2"));
    assert!(out.plugins.iter().any(|p|p.plugin_id=="@cache/two/opaque-id"));assert!(!printed(&out).contains("PRIVATE_"));
}

#[test]
fn local_metadata_accepts_jsonc_direct_maps_and_uses_the_entry_folder_for_declared_ids() {
    let home=TestHome::new("cursor-cli-local");let(c,_)=fixture(&home,Mode::NativeMetadata);
    let p=home.home.join(".cursor/plugins/local/actual-folder");
    write(&p.join(".cursor-plugin/plugin.json"),br#"{"name":"manifest-label","hooks":{"hooks":{"preToolUse":[{"command":"PRIVATE_HOOK_VALUE"}]}},"mcpServers":["extra.json",{"inline":{"command":"PRIVATE_INLINE_VALUE"}}]}"#);
    write(&p.join(".mcp.json"),b"// JSONC direct map\n{\"direct\":{\"command\":\"PRIVATE_COMMAND_VALUE\"},\"author\":{\"name\":\"PRIVATE_AUTHOR_VALUE\"},}");
    write(&p.join("extra.json"),br#"{"mcpServers":{"extra":{"url":"PRIVATE_URL_VALUE"}}}"#);
    write(&p.join("skills/example/SKILL.md"),b"PRIVATE_MARKDOWN_BODY");
    let out=scan(&c);assert_eq!(out.plugins.len(),1);assert_eq!(out.plugins[0].plugin_id,"@local/actual-folder");
    let ids=out.mcps.iter().map(|m|m.server_id.as_str()).collect::<BTreeSet<_>>();
    assert_eq!(ids,BTreeSet::from(["plugin-actual-folder-direct","plugin-actual-folder-extra","plugin-actual-folder-inline"]));
    assert!(out.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Hook));
    assert!(out.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill));assert!(!printed(&out).contains("PRIVATE_"));
}

#[test]
fn plugin_roots_do_not_follow_general_cursor_configuration_overrides() {
    let home=TestHome::new("cursor-cli-homes");let(c,_)=fixture(&home,Mode::NativeMetadata);
    std::env::set_var("CURSOR_CONFIG_DIR",home.home.join("other"));std::env::set_var("XDG_CONFIG_HOME",home.home.join("other-xdg"));
    write(&home.home.join("other/plugins/local/wrong/plugin.json"),br#"{"name":"wrong"}"#);
    write(&home.home.join(".cursor/plugins/local/right/plugin.json"),br#"{"name":"right"}"#);
    let out=scan(&c);assert_eq!(out.plugins.len(),1);assert_eq!(out.plugins[0].plugin_id,"@local/right");
}

#[test]
fn manifest_priority_is_independent_from_its_schema_label() {
    let home=TestHome::new("cursor-cli-precedence");let(c,_)=fixture(&home,Mode::NativeMetadata);let p=home.home.join(".cursor/plugins/local/entry");
    write(&p.join(".cursor-plugin/plugin.json"),br#"{"name":"chosen","$schema":"https://example.invalid/future.json"}"#);
    write(&p.join(".claude-plugin/plugin.json"),br#"{"name":"lower"}"#);write(&p.join("plugin.json"),br#"{"name":"lowest"}"#);
    let out=scan(&c);assert_eq!(out.plugins.len(),1);assert_eq!(out.plugins[0].name,"chosen");
}

#[cfg(unix)]
#[test]
fn unsafe_entries_and_incomplete_caches_remain_references_without_following_private_bodies() {
    use std::os::unix::fs::symlink;
    let home=TestHome::new("cursor-cli-paths");let(c,_)=fixture(&home,Mode::NativeMetadata);
    let local=home.home.join(".cursor/plugins/local");fs::create_dir_all(&local).unwrap();
    let outside=home.home.join("outside");write(&outside.join("plugin.json"),br#"{"name":"PRIVATE_OUTSIDE_BODY"}"#);symlink(&outside,local.join("linked")).unwrap();
    let incomplete=home.home.join(".cursor/plugins/cache/market/id/revision");write(&incomplete.join("plugin.json"),br#"{"name":"PRIVATE_INCOMPLETE_BODY"}"#);
    let out=scan(&c);assert!(out.unavailable&&out.mcps.is_empty());assert_eq!(out.plugins.len(),2);
    assert!(out.plugins.iter().all(|p|p.presence==PluginPresence::Declared&&p.components[0].kind==PluginComponentKind::PluginReference));
    assert!(!printed(&out).contains("PRIVATE_"));
}

#[test]
fn changed_package_inputs_never_fall_back_to_executing_either_cursor_or_bare_agent() {
    let home=TestHome::new("cursor-cli-passive");let(c,selected)=fixture(&home,Mode::NativeMetadata);
    write(&home.home.join("bin/agent"),LAUNCHER);fs::write(selected.parent().unwrap().join("3863.index.js"),b"changed").unwrap();
    assert!(verified_package(&c,&selected,&mut ScanBudget::default()).is_err());
    assert_eq!(passive_runtime_version("cursor-cli",&selected),Some(None));
    assert_eq!(crate::application::agent_launch::runtime_version("cursor-cli").unwrap(),None);
    let out=scan(&c);assert!(out.unavailable&&out.plugins.is_empty());assert!(!home.home.join("cursor-cli-executed-marker").exists());
}

#[test]
fn watches_cover_selected_alias_package_guards_and_native_cache_marker() {
    let home=TestHome::new("cursor-cli-watch");let(c,selected)=fixture(&home,Mode::NativeMetadata);let p=home.home.join(".cursor/plugins/cache/market/id/rev");
    write(&p.join(".cache-complete"),b"");let paths=watch_contract(&host(),&c);
    for want in [selected,home.home.join("bin/index.js"),home.home.join("bin/3863.index.js"),
        home.home.join(".cursor/plugins/local"),p.clone(),p.join(".cache-complete")] {assert!(paths.contains(&want),"{}",want.display());}
    assert!(builtin_observe(&BTreeMap::new()).items.is_empty());assert!(builtin_watch_paths(&BTreeMap::new()).is_empty());
}
