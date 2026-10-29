use super::*;
use crate::testenv::TestHome;
use serde_json::json;

const PROGRAM: &[u8] = b"#!/bin/sh\nprintf executed > \"$HOME/auggie-executed-marker\"\n";

fn host() -> Host {
    serde_json::from_value(json!({
        "agent_id":"augment","adapter":"augment","docs_url":"https://docs.augmentcode.com/cli/plugins",
        "manifest_paths":[".augment-plugin/plugin.json",".claude-plugin/plugin.json"],
        "default_mcp_files":[".mcp.json"],"mcp_id_prefix":"",
        "roots":[{"path":"~/.augment/plugins/marketplaces","layout":"marketplace_checkouts","presence":"local"}]
    })).unwrap()
}
fn write(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path,body).unwrap();
}
fn fixture(home: &TestHome) -> (Contract,PathBuf) {
    let selected=home.home.join("bin/auggie"); let package=selected.parent().unwrap();
    write(&selected,PROGRAM);write(&package.join("package.json"),b"{}");
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&selected,fs::Permissions::from_mode(0o755)).unwrap();}
    (Contract{agent_id:"augment".into(),command:"auggie".into(),entrypoint:"auggie".into(),
        version:"fixture-package".into(),runtime_version:"fixture-native-version".into(),
        sources:vec![SourceFile{path:"auggie".into(),bytes:PROGRAM.len() as u64,sha256:hex::encode(Sha256::digest(PROGRAM))},
            SourceFile{path:"package.json".into(),bytes:2,sha256:hex::encode(Sha256::digest(b"{}"))}]},selected)
}
fn market(home:&TestHome,folder:&str,name:&str,entries:Value) -> PathBuf {
    let root=home.home.join(".augment/plugins/marketplaces").join(folder);
    write(&root.join(".augment-plugin/marketplace.json"),serde_json::to_string(&json!({"name":name,"plugins":entries})).unwrap().as_bytes());root
}
fn scan(c:&Contract) -> PluginScan {
    let mut out=PluginScan::default();observe_contract(&host(),c,&mut out);normalize(&mut out);out
}
fn printed(out:&PluginScan)->String {serde_json::to_string(&(&out.plugins,&out.mcps)).unwrap()}

#[test]
fn verified_source_is_passive_and_changed_source_disables_facts() {
    let home=TestHome::new("auggie-source");let(c,selected)=fixture(&home);
    assert_eq!(version_for(&c,&selected),Some("fixture-native-version".into()));
    fs::write(selected.parent().unwrap().join("package.json"),b"changed").unwrap();
    assert_eq!(version_for(&c,&selected),None);let out=scan(&c);
    assert!(out.unavailable&&out.plugins.is_empty()&&out.mcps.is_empty());
    assert_eq!(passive_runtime_version("augment",&selected),Some(None));
    assert_eq!(crate::application::agent_launch::runtime_version("augment").unwrap(),None);
    assert!(!home.home.join("auggie-executed-marker").exists());
}

#[test]
fn json5_markets_fixed_components_and_native_mcp_names_do_not_leak_bodies() {
    let home=TestHome::new("auggie-json5");let(c,_)=fixture(&home);
    let root=home.home.join(".augment/plugins/marketplaces/official");
    write(&root.join(".augment-plugin/marketplace.json"),b"{name:'market',metadata:{pluginRoot:'plugins'},plugins:[{name:'entry',source:'demo'}],}");
    let p=root.join("plugins/demo");write(&p.join(".augment-plugin/plugin.json"),b"{name:'different-label',version:'1.0.0',commands:'ignored-directory'}");
    write(&p.join(".mcp.json"),b"{mcpServers:{web:{type:'http',url:'PRIVATE_URL',headers:{Authorization:'PRIVATE_HEADER'}},shell:{command:'PRIVATE_COMMAND',args:['PRIVATE_ARGUMENT'],env:{TOKEN:'PRIVATE_ENV'}}}}");
    for (path,body) in [("commands/nested/task.txt","PRIVATE_COMMAND_BODY"),("agents/helper.md","PRIVATE_AGENT_BODY"),
        ("rules/rule.txt","PRIVATE_RULE_BODY"),("skills/custom-skill.md","PRIVATE_SKILL_BODY"),("hooks/pre.json","PRIVATE_HOOK_BODY"),
        ("hooks/ignored.js","PRIVATE_SCRIPT_BODY"),("commands/.hidden.md","PRIVATE_HIDDEN_BODY"),("ignored-directory/ignored.md","PRIVATE_IGNORED_BODY")] {write(&p.join(path),body.as_bytes());}
    let out=scan(&c);assert_eq!(out.plugins.len(),1);let plugin=&out.plugins[0];
    assert_eq!(plugin.plugin_id,"entry@market");assert_eq!(plugin.name,"entry");assert_eq!(plugin.version,"1.0.0");assert!(plugin.enabled.is_none());
    assert_eq!(out.mcps.iter().map(|m|m.server_id.as_str()).collect::<BTreeSet<_>>(),BTreeSet::from(["entry@market/shell","entry@market/web"]));
    for kind in [PluginComponentKind::Command,PluginComponentKind::Agent,PluginComponentKind::Rule,PluginComponentKind::Skill,PluginComponentKind::Hook] {assert!(plugin.components.iter().any(|c|c.kind==kind));}
    assert!(!plugin.components.iter().any(|c|c.source_path.ends_with("ignored.js")||c.source_path.ends_with(".hidden.md")||c.source_path.contains("ignored-directory")));
    assert!(!printed(&out).contains("PRIVATE_"));assert!(!home.home.join("auggie-executed-marker").exists());
}

#[test]
fn local_roots_normalize_containment_and_fall_back_only_when_first_missing() {
    let home=TestHome::new("auggie-local");let(c,_)=fixture(&home);
    let root=market(&home,"one","market",json!([{"name":"entry","source":"demo"}]));
    write(&root.join(".augment-plugin/marketplace.json"),br#"{"name":"market","metadata":{"pluginRoot":"plugins"},"plugins":[{"name":"entry","source":"demo"}]}"#);
    write(&root.join("plugins/demo/.augment-plugin/plugin.json"),b"{version:'first'}");
    write(&root.join("demo/.augment-plugin/plugin.json"),b"{version:'fallback'}");
    assert_eq!(scan(&c).plugins[0].version,"first");fs::remove_dir_all(root.join("plugins/demo")).unwrap();
    assert_eq!(scan(&c).plugins[0].version,"fallback");
    write(&root.join(".augment-plugin/marketplace.json"),br#"{"name":"market","metadata":{"pluginRoot":"plugins"},"plugins":[{"name":"entry","source":"../demo"}]}"#);
    assert_eq!(scan(&c).plugins[0].version,"fallback");
}

#[test]
fn uncached_urls_stay_references_and_cached_namespaces_are_known() {
    let home=TestHome::new("auggie-cache");let(c,_)=fixture(&home);
    let root=market(&home,"one","market",json!([{"name":"folder/name","source":{"source":"url","url":"PRIVATE_REMOTE_URL"}}]));
    let missing=scan(&c);assert!(missing.mcps.is_empty());assert_eq!(missing.plugins[0].presence,PluginPresence::Declared);
    let p=root.join(".plugin-cache/foldername");write(&p.join(".git"),b"PRIVATE_GIT_REFERENCE");
    write(&p.join(".mcp.json"),b"{mcpServers:{web:{type:'sse',url:'PRIVATE_URL'}}}");
    let out=scan(&c);assert_eq!(out.plugins[0].presence,PluginPresence::CacheOnly);
    assert_eq!(out.mcps[0].server_id,"folder/name@market/web");assert!(!printed(&out).contains("PRIVATE_"));
}

#[test]
fn duplicate_ids_retain_physical_snapshots_without_choosing_a_runtime_winner() {
    let home=TestHome::new("auggie-dups");let(c,_)=fixture(&home);
    for folder in ["one","two"] {
        let root=market(&home,folder,"same",json!([{"name":"entry","source":"demo"}]));
        write(&root.join("demo/.mcp.json"),b"{mcpServers:{web:{command:'PRIVATE_COMMAND'}}}");
    }
    let out=scan(&c);assert!(out.unavailable&&out.mcps.is_empty());assert_eq!(out.plugins.len(),2);
    assert_ne!(out.plugins[0].revision,out.plugins[1].revision);
    assert!(out.plugins.iter().all(|p|p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.native_id.is_none())));
}

#[test]
fn manifest_precedence_and_manifestless_fixed_components_follow_the_native_loader() {
    let home=TestHome::new("auggie-priority");let(c,_)=fixture(&home);
    let root=market(&home,"one","preferred",json!([{"name":"entry","source":"demo"}]));
    write(&root.join(".claude-plugin/marketplace.json"),b"{name:'ignored',plugins:[]}");
    let p=root.join("demo");write(&p.join(".augment-plugin/plugin.json"),b"{version:'first'}");
    write(&p.join(".claude-plugin/plugin.json"),b"{version:'second'}");write(&p.join("commands/task.md"),b"PRIVATE_BODY");
    let out=scan(&c);assert_eq!(out.plugins[0].plugin_id,"entry@preferred");assert_eq!(out.plugins[0].version,"first");
    fs::remove_file(p.join(".augment-plugin/plugin.json")).unwrap();assert_eq!(scan(&c).plugins[0].version,"second");
    fs::remove_file(p.join(".claude-plugin/plugin.json")).unwrap();let out=scan(&c);
    assert_eq!(out.plugins[0].version,"unknown");assert!(out.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Command));
}

#[test]
fn unsupported_sources_and_nonstandard_mcp_shapes_remain_explicit_references() {
    let home=TestHome::new("auggie-shapes");let(c,_)=fixture(&home);
    let root=market(&home,"one","market",json!([{"name":"unverified","source":{"source":"unknown"}},{"name":"entry","source":"demo"}]));
    write(&root.join("demo/.mcp.json"),b"{mcpServers:{bad:{command:'PRIVATE_COMMAND',args:'PRIVATE_NON_ARRAY'}}}");
    let out=scan(&c);assert!(out.unavailable&&out.mcps.is_empty());assert_eq!(out.plugins.len(),2);
    assert!(out.plugins.iter().all(|p|p.components.iter().any(|c|c.kind==PluginComponentKind::PluginReference||c.kind==PluginComponentKind::McpReference)));
    assert!(!printed(&out).contains("PRIVATE_"));
}

#[test]
fn native_home_priority_and_recursive_watch_roots_are_explicit() {
    use std::ffi::OsStr;
    let home=TestHome::new("auggie-homes");let(c,selected)=fixture(&home);
    let alternate=home.home.join("alternate");
    assert_eq!(home_from_env(Some(home.home.as_os_str()),Some(alternate.as_os_str()),None).unwrap(),home.home);
    assert_eq!(home_from_env(Some(OsStr::new("")),Some(alternate.as_os_str()),None).unwrap(),alternate);
    assert!(home_from_env(Some(OsStr::new("relative")),Some(alternate.as_os_str()),Some(&home.home)).is_err());
    assert!(home_from_env(Some(OsStr::new("")),None,Some(&home.home)).is_err());
    std::env::set_var("XDG_CONFIG_HOME",&alternate);let paths=watch_contract(&host(),&c);
    assert!(paths.contains(&home.home.join(".augment/plugins/marketplaces")));
    assert!(paths.contains(&selected)&&paths.contains(&selected.parent().unwrap().join("package.json")));
    assert!(!paths.iter().any(|p|p.starts_with(&alternate)));
}

#[test]
fn json5_reader_preserves_other_formats_and_bounds_nesting_without_parsing_component_bodies() {
    let home=TestHome::new("auggie-format");let root=&home.home;let mut budget=ScanBudget::default();
    write(&root.join("metadata.json"),"{unquoted:'[', trailing:.5, hex:+0x10, ignored:Infinity, /* } */ list:[1,], // ]\u{2028} last:'ok'}".as_bytes());
    let value=optional_document(root,"metadata.json",DocumentFormat::Json5,&mut budget).unwrap().unwrap();
    assert_eq!(value["last"],"ok");assert!(optional_document(root,"metadata.json",DocumentFormat::Jsonc,&mut budget).is_err());
    write(&root.join("deep.json"),format!("{}0{}","[".repeat(129),"]".repeat(129)).as_bytes());
    assert!(optional_document(root,"deep.json",DocumentFormat::Json5,&mut budget).is_err());
}

#[cfg(unix)]
#[test]
fn links_do_not_expose_outside_bodies_or_force_a_fallback_past_the_selected_native_root() {
    use std::os::unix::fs::symlink;
    let home=TestHome::new("auggie-links");let(c,_)=fixture(&home);
    let root=market(&home,"one","market",json!([{"name":"entry","source":"demo"}]));
    write(&root.join(".augment-plugin/marketplace.json"),br#"{"name":"market","metadata":{"pluginRoot":"plugins"},"plugins":[{"name":"entry","source":"demo"}]}"#);
    let outside=home.home.join("outside");write(&outside.join(".mcp.json"),b"{mcpServers:{PRIVATE_OUTSIDE:{command:'PRIVATE_COMMAND'}}}");
    fs::create_dir_all(root.join("plugins")).unwrap();symlink(&outside,root.join("plugins/demo")).unwrap();
    write(&root.join("demo/.mcp.json"),b"{mcpServers:{wrongFallback:{command:'PRIVATE_COMMAND'}}}");
    let out=scan(&c);assert!(out.unavailable&&out.mcps.is_empty());assert_eq!(out.plugins.len(),1);
    assert_eq!(out.plugins[0].components[0].kind,PluginComponentKind::PluginReference);assert!(!printed(&out).contains("PRIVATE_"));
}
