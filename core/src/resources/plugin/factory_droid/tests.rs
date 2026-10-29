use super::*;
use crate::testenv::TestHome;
use serde_json::json;

const PROGRAM:&[u8]=b"#!/bin/sh\nprintf executed > \"$HOME/droid-executed-marker\"\n";
fn host() -> Host {serde_json::from_value(json!({
    "agent_id":"factory-droid","adapter":"factory_droid","docs_url":"https://docs.factory.com/harness/plugins",
    "manifest_paths":[],"default_mcp_files":["mcp.json"],"mcp_id_prefix":"",
    "roots":[{"path":"~/.factory/plugins","layout":"factory_installed_registry","presence":"installed"}]
})).unwrap()}
fn write(path:&Path,body:&[u8]) {fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,body).unwrap();}
fn fixture(home:&TestHome) -> (Contract,PathBuf,PathBuf) {
    let selected=home.home.join("bin/droid");write(&selected,PROGRAM);
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&selected,fs::Permissions::from_mode(0o755)).unwrap();}
    (Contract{agent_id:"factory-droid".into(),command:"droid".into(),version:"fixture-native".into(),
        binary_bytes:PROGRAM.len() as u64,binary_sha256:hex::encode(Sha256::digest(PROGRAM))},selected,home.home.join(".factory"))
}
fn entry(path:&Path,scope:&str,version:&str) -> Value {json!({"scope":scope,"installPath":path,"version":version,"installedAt":"metadata","lastUpdated":"metadata","source":"PRIVATE_SOURCE_URL"})}
fn legacy(base:&Path,value:Value) {write(&base.join("plugins/installed_plugins.json"),serde_json::to_vec(&value).unwrap().as_slice());}
fn one(base:&Path,file:&str,id:&str,value:Value) {write(&base.join("plugins/installed_plugins").join(file),serde_json::to_vec(&json!({"pluginId":id,"entry":value})).unwrap().as_slice());}
fn scan(c:&Contract,selected:&Path,base:&Path) -> PluginScan {
    let mut result=PluginScan::default();observe_at(&host(),c,selected,base,&mut result);normalize(&mut result);result
}
fn printed(result:&PluginScan) -> String {serde_json::to_string(&(&result.plugins,&result.mcps)).unwrap()}

#[test]
fn whole_source_identity_is_passive_and_changed_images_never_fall_back_to_execution() {
    let home=TestHome::new("droid-source");let(c,selected,base)=fixture(&home);
    assert_eq!(version_for(&c,&selected),Some("fixture-native".into()));
    fs::write(&selected,b"changed").unwrap();assert_eq!(version_for(&c,&selected),None);
    let out=scan(&c,&selected,&base);assert!(out.unavailable&&out.plugins.is_empty());
    assert_eq!(passive_runtime_version("factory-droid",&selected),Some(None));
    assert_eq!(crate::application::agent_launch::runtime_version("factory-droid").unwrap(),None);
    assert!(!home.home.join("droid-executed-marker").exists());
}

#[test]
fn registered_paths_versions_and_raw_mcp_names_do_not_depend_on_plugin_manifests() {
    let home=TestHome::new("droid-native");let(c,selected,base)=fixture(&home);let p=home.home.join("registered/demo");
    legacy(&base,json!({"schemaVersion":1,"plugins":{"entry@market":[entry(&p,"user","recorded-commit")]}}));
    write(&p.join(".factory-plugin/plugin.json"),b"PRIVATE_MALFORMED_MANIFEST");
    write(&p.join("mcp.json"),br#"{"mcpServers":{"raw":{"command":"PRIVATE_COMMAND","args":["PRIVATE_ARG"],"env":{"TOKEN":"PRIVATE_ENV"}},"web":{"type":"http","url":"PRIVATE_URL","headers":{"Authorization":"PRIVATE_HEADER"}}}}"#);
    for file in ["skills/team/SKILL.md","droids/helper.md","output-styles/style.md","commands/nested/run.sh","hooks/hooks.json"] {write(&p.join(file),b"PRIVATE_COMPONENT_BODY");}
    let out=scan(&c,&selected,&base);assert_eq!(out.plugins.len(),1);let plugin=&out.plugins[0];
    assert_eq!(plugin.version,"recorded-commit");assert!(plugin.enabled.is_none());assert_eq!(plugin.presence,PluginPresence::Installed);
    assert_eq!(out.mcps.iter().map(|m|m.server_id.as_str()).collect::<BTreeSet<_>>(),BTreeSet::from(["raw","web"]));
    for kind in [PluginComponentKind::Skill,PluginComponentKind::Agent,PluginComponentKind::OutputStyle,PluginComponentKind::Command,PluginComponentKind::Hook] {assert!(plugin.components.iter().any(|v|v.kind==kind));}
    assert!(!printed(&out).contains("PRIVATE_"));assert!(!home.home.join("droid-executed-marker").exists());
}

#[test]
fn sorted_entry_files_replace_only_the_matching_scope_and_ignore_filename_identity() {
    let home=TestHome::new("droid-overlay");let(c,selected,base)=fixture(&home);
    let old=home.home.join("old");let first=home.home.join("first");let last=home.home.join("last");let project=home.home.join("project");
    for p in [&old,&first,&last,&project]{write(&p.join("commands/task.md"),b"PRIVATE_BODY");}
    legacy(&base,json!({"plugins":{"@scope/plugin@market@pin":[entry(&old,"user","old"),entry(&project,"project","excluded")]}}));
    one(&base,"10-not-a-hash.json","@scope/plugin@market@pin",entry(&first,"user","first"));
    one(&base,"2-not-a-hash.json","@scope/plugin@market@pin",entry(&last,"user","last"));
    let out=scan(&c,&selected,&base);assert_eq!(out.plugins.len(),1);assert_eq!(out.plugins[0].name,"@scope/plugin");
    assert_eq!(out.plugins[0].version,"last");assert!(out.plugins[0].source_paths.iter().any(|s|s.ends_with("2-not-a-hash.json")));
    assert!(!printed(&out).contains("excluded"));
}

#[test]
fn duplicate_scope_snapshots_and_global_server_key_collisions_never_choose_a_winner() {
    let home=TestHome::new("droid-collisions");let(c,selected,base)=fixture(&home);
    let a=home.home.join("a");let b=home.home.join("b");let d=home.home.join("d");
    for p in [&a,&b,&d]{write(&p.join("mcp.json"),br#"{"mcpServers":{"same":{"command":"PRIVATE_COMMAND"}}}"#);}
    legacy(&base,json!({"schemaVersion":1,"plugins":{"entry@market":[entry(&a,"user","a"),entry(&b,"org","b")],"other@market":[entry(&d,"user","d")]}}));
    let out=scan(&c,&selected,&base);assert!(out.unavailable&&out.mcps.is_empty());assert_eq!(out.plugins.len(),3);
    assert!(out.plugins.iter().all(|p|p.components.iter().any(|v|v.kind==PluginComponentKind::McpReference&&v.native_id.is_none())));
}

#[test]
fn missing_packages_remain_declarations_and_cache_history_is_not_an_installation() {
    let home=TestHome::new("droid-cache");let(c,selected,base)=fixture(&home);let missing=home.home.join("missing");
    legacy(&base,json!({"plugins":{"missing@market":[entry(&missing,"user","record")]}}));
    write(&base.join("plugins/cache/unregistered/99/mcp.json"),br#"{"mcpServers":{"PRIVATE_UNREGISTERED":{"command":"PRIVATE_COMMAND"}}}"#);
    let out=scan(&c,&selected,&base);assert_eq!(out.plugins.len(),1);assert_eq!(out.plugins[0].presence,PluginPresence::Declared);
    assert!(out.mcps.is_empty()&&!printed(&out).contains("PRIVATE_"));
}

#[test]
fn portable_records_exclude_mcp_and_output_style_bodies_but_retain_other_candidates() {
    let home=TestHome::new("droid-portable");let(c,selected,base)=fixture(&home);let p=home.home.join("portable");let mut e=entry(&p,"user","1");e["portableOnly"]=json!(true);
    one(&base,"portable.json","portable@market",e);
    for file in ["skills/team/SKILL.md","commands/run.sh","hooks/hooks.json","mcp.json","output-styles/style.md"]{write(&p.join(file),b"PRIVATE_INVALID_BODY");}
    let out=scan(&c,&selected,&base);assert!(out.mcps.is_empty());let plugin=&out.plugins[0];
    assert!(plugin.components.iter().any(|v|v.kind==PluginComponentKind::Skill));assert!(plugin.components.iter().any(|v|v.kind==PluginComponentKind::Command));
    assert!(!plugin.components.iter().any(|v|v.kind==PluginComponentKind::OutputStyle));assert!(plugin.components.iter().any(|v|v.kind==PluginComponentKind::McpReference));
    assert!(!printed(&out).contains("PRIVATE_"));
}

#[test]
fn native_profile_and_home_overrides_are_pure_and_watch_only_registered_global_inputs() {
    use std::ffi::OsStr;
    let home=TestHome::new("droid-home");let(c,selected,base)=fixture(&home);let alternate=home.home.join("alternate");
    assert_eq!(base_from_env(Some(home.home.as_os_str()),None,None,None).unwrap(),base);
    assert_eq!(base_from_env(Some(home.home.as_os_str()),None,None,Some(OsStr::new("development"))).unwrap(),home.home.join(".factory-dev"));
    assert_eq!(base_from_env(Some(home.home.as_os_str()),None,Some(alternate.as_os_str()),None).unwrap(),alternate.join(".factory"));
    assert_eq!(base_from_env(Some(home.home.as_os_str()),None,Some(OsStr::new("")),None).unwrap(),base);
    assert!(base_from_env(Some(home.home.as_os_str()),None,None,Some(OsStr::new(""))).is_err());
    assert!(base_from_env(Some(home.home.as_os_str()),None,Some(OsStr::new("relative")),None).is_err());
    assert!(base_from_env(Some(OsStr::new("")),Some(&home.home),None,None).is_err());
    let paths=watch_at(&host(),&c,&base);assert!(paths.contains(&selected));
    assert!(paths.contains(&base.join("plugins/installed_plugins.json")));assert!(paths.contains(&base.join("plugins/installed_plugins")));
    assert!(!paths.iter().any(|p|p==&base.join("plugins/cache")||p==&base.join("plugins/marketplaces")));
}

#[test]
fn registries_accept_one_bom_without_loosening_native_mcp_json() {
    let home=TestHome::new("droid-bom");let(c,selected,base)=fixture(&home);let p=home.home.join("package");
    let body=json!({"plugins":{"entry@market":[entry(&p,"user","1")]}});let bytes=[b"\xef\xbb\xbf".as_slice(),serde_json::to_vec(&body).unwrap().as_slice()].concat();
    write(&base.join("plugins/installed_plugins.json"),&bytes);write(&p.join("commands/task.md"),b"PRIVATE_BODY");
    assert_eq!(scan(&c,&selected,&base).plugins.len(),1);
    write(&p.join("mcp.json"),b"\xef\xbb\xbf{\"mcpServers\":{\"bad\":{\"command\":\"PRIVATE_COMMAND\"}}}");
    let out=scan(&c,&selected,&base);assert!(out.unavailable&&out.mcps.is_empty());
}

#[test]
fn one_invalid_server_demotes_the_entire_native_mcp_map() {
    let home=TestHome::new("droid-mcp-map");let(c,selected,base)=fixture(&home);let p=home.home.join("package");
    one(&base,"entry.json","entry@market",entry(&p,"user","1"));
    write(&p.join("mcp.json"),br#"{"mcpServers":{"valid":{"command":"PRIVATE_COMMAND"},"invalid":{"type":"http","url":"PRIVATE_URL","timeout":0}}}"#);
    let out=scan(&c,&selected,&base);assert!(out.unavailable&&out.mcps.is_empty());
    assert!(out.plugins[0].components.iter().all(|v|v.kind!=PluginComponentKind::Mcp));assert!(!printed(&out).contains("PRIVATE_"));
}

#[test]
fn native_oauth_shapes_validate_combinations_without_returning_credentials_or_urls() {
    let home=TestHome::new("droid-oauth");let(c,selected,base)=fixture(&home);let p=home.home.join("package");
    one(&base,"entry.json","entry@market",entry(&p,"user","1"));
    write(&p.join("mcp.json"),br#"{"mcpServers":{"web":{"type":"http","url":"PRIVATE_URL","oauth":{"clientId":"PRIVATE_CLIENT","clientSecret":"PRIVATE_SECRET","authorizationServerIssuer":"https://example.test","callbackPort":54321,"tokenEndpointAuthMethod":"client_secret_post","scopes":["PRIVATE_SCOPE"]}},"public":{"type":"sse","url":"PRIVATE_URL","oauth":{"clientMetadataUrl":"https://example.test/client.json","tokenEndpointAuthMethod":"none"}}}}"#);
    let out=scan(&c,&selected,&base);assert_eq!(out.mcps.len(),2);assert!(!printed(&out).contains("PRIVATE_")&&!printed(&out).contains("example.test"));
    write(&p.join("mcp.json"),br#"{"mcpServers":{"bad":{"type":"http","url":"PRIVATE_URL","oauth":{"clientId":"PRIVATE_CLIENT"}}}}"#);
    assert!(scan(&c,&selected,&base).mcps.is_empty());
}

#[test]
fn agent_plugins_schema_only_selects_the_native_mcp_fallback_and_does_not_translate_layouts() {
    let home=TestHome::new("droid-interop");let(c,selected,base)=fixture(&home);let p=home.home.join("package");
    one(&base,"entry.json","entry@market",entry(&p,"user","1"));
    write(&p.join("mcp.json"),br#"{"$schema":"https://agent-plugins.org/schemas/future-version/mcp.schema.json","mcpServers":{}}"#);
    write(&p.join(".mcp.json"),br#"{"fallback":{"command":"PRIVATE_COMMAND"}}"#);
    write(&p.join("agents/not-a-droid.md"),b"PRIVATE_BODY");write(&p.join("plugin.json"),b"PRIVATE_INVALID_STANDARD_MANIFEST");
    let out=scan(&c,&selected,&base);assert_eq!(out.mcps[0].server_id,"fallback");assert!(!out.plugins[0].components.iter().any(|v|v.kind==PluginComponentKind::Agent));
    fs::remove_file(p.join("mcp.json")).unwrap();assert!(scan(&c,&selected,&base).mcps.is_empty());
}

#[test]
fn skill_recursion_stops_at_native_skill_roots_and_hidden_command_files_remain_candidates() {
    let home=TestHome::new("droid-walk");let(c,selected,base)=fixture(&home);let p=home.home.join("package");
    one(&base,".registered.json","entry@market",entry(&p,"user","1"));
    for f in ["skills/group/team/SKILL.md","skills/group/team/deeper/SKILL.md","skills/.hidden/SKILL.md","commands/.hidden.txt","commands/nested/any.data"]{write(&p.join(f),b"PRIVATE_BODY");}
    let out=scan(&c,&selected,&base);let components=&out.plugins[0].components;
    assert_eq!(components.iter().filter(|v|v.kind==PluginComponentKind::Skill).count(),2);
    assert!(components.iter().any(|v|v.kind==PluginComponentKind::Command&&v.source_path.ends_with(".hidden.txt")));
    assert!(components.iter().any(|v|v.kind==PluginComponentKind::Command&&v.source_path.ends_with("any.data")));
    assert!(!printed(&out).contains("PRIVATE_"));
}

#[cfg(unix)]
#[test]
fn linked_registered_roots_and_component_paths_never_expose_outside_bodies() {
    use std::os::unix::fs::symlink;
    let home=TestHome::new("droid-links");let(c,selected,base)=fixture(&home);let outside=home.home.join("outside");let p=home.home.join("linked");
    write(&outside.join("mcp.json"),br#"{"mcpServers":{"PRIVATE_OUTSIDE":{"command":"PRIVATE_COMMAND"}}}"#);symlink(&outside,&p).unwrap();
    one(&base,"entry.json","entry@market",entry(&p,"user","1"));
    let out=scan(&c,&selected,&base);assert!(out.unavailable&&out.mcps.is_empty());assert_eq!(out.plugins[0].presence,PluginPresence::Declared);
    assert!(!printed(&out).contains("PRIVATE_"));
}

#[cfg(unix)]
#[test]
fn unverified_registration_overlays_keep_physical_metadata_without_asserting_current_mcp_bindings() {
    use std::os::unix::fs::symlink;
    let home=TestHome::new("droid-partial");let(c,selected,base)=fixture(&home);let p=home.home.join("package");
    legacy(&base,json!({"plugins":{"entry@market":[entry(&p,"user","1")]}}));
    write(&p.join("commands/task.md"),b"PRIVATE_BODY");write(&p.join("mcp.json"),br#"{"mcpServers":{"known":{"command":"PRIVATE_COMMAND"}}}"#);
    let outside=home.home.join("outside.json");write(&outside,b"PRIVATE_UNREAD_OVERLAY");
    let directory=base.join("plugins/installed_plugins");fs::create_dir_all(&directory).unwrap();symlink(&outside,directory.join("overlay.json")).unwrap();
    let out=scan(&c,&selected,&base);assert!(out.unavailable&&out.mcps.is_empty());
    let known=out.plugins.iter().find(|p|p.plugin_id=="entry@market").unwrap();assert_eq!(known.presence,PluginPresence::Declared);
    assert!(known.components.iter().any(|v|v.kind==PluginComponentKind::Command));
    assert!(known.components.iter().any(|v|v.kind==PluginComponentKind::McpReference&&v.name=="known"));
    assert!(!printed(&out).contains("PRIVATE_"));
}
