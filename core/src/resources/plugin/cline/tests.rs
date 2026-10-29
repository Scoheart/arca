use super::*;
use crate::testenv::TestHome;
use serde_json::json;

const CODE:&[u8]=b"#!/bin/sh\nprintf executed > \"$HOME/cline-executed-marker\"\n";
fn write(path:&Path,body:&[u8]) {fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,body).unwrap();}
fn host(id:&str) -> Host {serde_json::from_value(json!({"agent_id":id,"adapter":"cline","docs_url":"https://docs.cline.bot/usage/cline-desktop","manifest_paths":["plugin.json"],"default_mcp_files":["mcp.json"],"mcp_id_prefix":"","roots":[]})).unwrap()}
fn paths(home:&TestHome) -> GlobalPaths {paths_from(&home.home,None,None).unwrap()}
fn package(root:&Path,name:&str,plugin_name:&str) -> PathBuf {
    let dir=root.join(name);write(&dir.join("plugin.json"),serde_json::to_vec(&json!({"$schema":PLUGIN_SCHEMA,"name":plugin_name})).unwrap().as_slice());dir
}
fn mcp(package:&Path,servers:Value) {write(&package.join("mcp.json"),serde_json::to_vec(&json!({"$schema":MCP_SCHEMA,"mcpServers":servers})).unwrap().as_slice());}
fn scan(id:&str,p:&GlobalPaths) -> PluginScan {let mut r=PluginScan::default();observe_sources(&host(id),p,&mut r);normalize(&mut r);r}
fn guard(path:&str,bytes:&[u8]) -> Value {json!({"path":path,"bytes":bytes.len(),"sha256":hex::encode(Sha256::digest(bytes))})}
fn cli_contract() -> CliContract {serde_json::from_value(json!({
    "agent_id":"cline-cli","command":"cline","version":"3.0.74","binary_bytes":CODE.len(),"binary_sha256":hex::encode(Sha256::digest(CODE)),
    "wrapper_guards":[guard("bin/cline",b"fixture wrapper"),guard("bin/ca-certs.cjs",b"fixture helper"),guard("package.json",b"fixture manifest")]
})).unwrap()}
fn wrapper(home:&TestHome,c:&CliContract) -> (PathBuf,PathBuf) {
    let root=home.home.join("npm/cline");write(&root.join("bin/cline"),b"fixture wrapper");write(&root.join("bin/ca-certs.cjs"),b"fixture helper");write(&root.join("package.json"),b"fixture manifest");
    let target=root.join("node_modules/@cline/cli-darwin-arm64/bin/cline");write(&target,CODE);assert_eq!(c.version,"3.0.74");(root.join("bin/cline"),target)
}

#[test]
fn strict_v1_metadata_keeps_native_dotted_ids_and_never_exports_configuration() {
    let home=TestHome::new("cline-v1");let p=paths(&home);let dir=package(&p.packages,"physical-folder","tools.one");
    write(&dir.join("skills/example/SKILL.md"),b"PRIVATE_COMPONENT_BODY");
    mcp(&dir,json!({"raw-key":{"type":"stdio","command":"PRIVATE_COMMAND","args":["PRIVATE_ARG"],"env":{"TOKEN":"PRIVATE_ENV"}},"remote":{"type":"sse","url":"https://example.test/PRIVATE_URL","headers":{"Authorization":"PRIVATE_HEADER"}}}));
    let r=scan("cline-cli",&p);assert!(!r.unavailable);assert_eq!(r.plugins.len(),1);assert_eq!(r.mcps.len(),2);
    assert_eq!(r.plugins[0].plugin_id,"tools.one");assert_eq!(r.plugins[0].version,"unknown");assert!(r.plugins[0].enabled.is_none());
    assert!(r.mcps.iter().any(|m|m.server_id=="tools.one.raw-key"));
    assert!(r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill&&c.native_id.is_none()));
    assert!(!serde_json::to_string(&(&r.plugins,&r.mcps)).unwrap().contains("PRIVATE_"));assert!(!home.home.join("cline-executed-marker").exists());
}

#[test]
fn manifest_schema_and_author_are_strict_but_unknown_fields_and_extensions_are_not_fatal() {
    assert!(manifest(&json!({"$schema":PLUGIN_SCHEMA,"name":"a.b","unknown":"ignored","extensions":42})).is_ok());
    for v in [json!({"name":"a"}),json!({"$schema":PLUGIN_SCHEMA,"name":"a--b"}),json!({"$schema":PLUGIN_SCHEMA,"name":"a..b"}),
        json!({"$schema":PLUGIN_SCHEMA,"name":"a","author":{"extra":"no"}}),json!({"$schema":PLUGIN_SCHEMA,"name":"a","version":4})] {assert!(manifest(&v).is_err());}
    let home=TestHome::new("cline-manifest-json");let p=paths(&home);let dir=package(&p.packages,"bad","x");write(&dir.join("plugin.json"),b"\xef\xbb\xbf{\"name\":\"x\"}");
    let r=scan("cline-cli",&p);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);assert!(r.mcps.is_empty());assert_eq!(r.plugins[0].presence,PluginPresence::Declared);
}

#[test]
fn invalid_server_is_a_reference_while_other_servers_and_filename_skills_survive() {
    let home=TestHome::new("cline-server-rejection");let p=paths(&home);let dir=package(&p.packages,"p","p");
    write(&dir.join("skills/kept/SKILL.md"),b"PRIVATE_BODY");mcp(&dir,json!({
        "valid":{"type":"stdio","command":"tool"},"unknown-field":{"type":"stdio","command":"tool","disabled":true},
        "reserved":{"type":"stdio","command":"tool","env":{"PLUGIN_ROOT":"forbidden"}},"missing-type":{"command":"tool"},
        "unsafe-http":{"type":"streamable-http","url":"http://example.test"},"headers":{"type":"sse","url":"https://example.test","headers":{"A":"x","a":"y"}}
    }));
    let r=scan("cline-cli",&p);assert!(r.unavailable);assert_eq!(r.mcps.len(),1);assert_eq!(r.mcps[0].server_id,"p.valid");
    assert_eq!(r.plugins[0].components.iter().filter(|c|c.kind==PluginComponentKind::McpReference).count(),5);
    assert!(r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill));
}

#[test]
fn missing_relative_commands_are_valid_candidates_but_escapes_and_wrong_types_are_not() {
    let home=TestHome::new("cline-relative-commands");let p=paths(&home);let dir=package(&p.packages,"p","p");fs::create_dir_all(dir.join("directory")).unwrap();
    mcp(&dir,json!({"future":{"type":"stdio","command":"./future/tool","cwd":"${PLUGIN_DATA}/future"},
        "escape":{"type":"stdio","command":"./../../escape"},"directory":{"type":"stdio","command":"./directory"},
        "bare":{"type":"stdio","command":"two tokens"},"absolute-cwd":{"type":"stdio","command":"tool","cwd":"/tmp"},
        "loopback":{"type":"streamable-http","url":"http://127.0.0.1:1/mcp"}}));
    let r=scan("cline-cli",&p);assert_eq!(r.mcps.iter().map(|m|m.server_id.as_str()).collect::<BTreeSet<_>>(),BTreeSet::from(["p.future","p.loopback"]));
    assert!(!p.data.exists());
}

#[test]
fn duplicate_names_and_dotted_registration_collisions_never_choose_a_guessed_winner() {
    let home=TestHome::new("cline-duplicates");let p=paths(&home);
    for (folder,name,key) in [("one","same","one"),("two","same","two"),("three","a.b","c"),("four","a","b.c")] {
        let dir=package(&p.packages,folder,name);mcp(&dir,json!({key:{"type":"stdio","command":"tool"}}));
    }
    let r=scan("cline-cli",&p);assert!(r.unavailable);assert_eq!(r.plugins.len(),4);assert!(r.mcps.is_empty());
    assert!(r.plugins.iter().all(|p|p.components.iter().all(|c|c.native_id.is_none())));
}

#[test]
fn module_files_remain_physical_references_and_v1_roots_mask_module_discovery() {
    let home=TestHome::new("cline-module-references");let p=paths(&home);let root=&p.modules[0];
    write(&root.join("plain.ts"),CODE);write(&root.join(".hidden.js"),CODE);write(&root.join("node_modules/x/index.js"),CODE);
    write(&root.join("package/plugin.json"),b"invalid marker still masks native module discovery");write(&root.join("package/index.js"),CODE);
    let r=scan("cline-cli",&p);assert_eq!(r.plugins.len(),1);assert!(r.mcps.is_empty());assert!(r.plugins[0].plugin_id.starts_with("@module/"));
    assert!(r.plugins[0].components.iter().all(|c|c.native_id.is_none()));assert!(!home.home.join("cline-executed-marker").exists());
}

#[cfg(unix)]
#[test]
fn symlinks_never_open_private_component_bodies_or_create_native_ids() {
    use std::os::unix::fs::symlink;
    let home=TestHome::new("cline-links");let p=paths(&home);let dir=package(&p.packages,"p","p");
    let outside=home.home.join("outside");write(&outside,b"PRIVATE_BODY");symlink(&outside,dir.join("mcp.json")).unwrap();
    write(&dir.join("skills/s/SKILL.md"),b"PRIVATE_BODY");fs::remove_file(dir.join("skills/s/SKILL.md")).unwrap();symlink(&outside,dir.join("skills/s/SKILL.md")).unwrap();
    let r=scan("cline-cli",&p);assert!(r.unavailable);assert!(r.mcps.is_empty());assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn desktop_and_cli_keep_distinct_host_ids_for_the_same_physical_packages() {
    let home=TestHome::new("cline-shared-packages");let p=paths(&home);let dir=package(&p.packages,"p","p");mcp(&dir,json!({"m":{"type":"stdio","command":"tool"}}));
    let mut r=scan("cline-cli",&p);observe_sources(&host("cline-desktop"),&p,&mut r);normalize(&mut r);
    assert_eq!(r.mcps.len(),2);assert_eq!(r.mcps.iter().map(|m|m.agent_id.as_str()).collect::<BTreeSet<_>>(),BTreeSet::from(["cline-cli","cline-desktop"]));
}

#[test]
fn native_paths_keep_agents_packages_independent_from_cline_and_data_overrides() {
    let home=TestHome::new("cline-paths");let c=home.home.join("custom");let d=home.home.join("custom-data");
    let p=paths_from(&home.home,Some(c.as_os_str()),Some(d.as_os_str())).unwrap();
    assert_eq!(p.packages,home.home.join(".agents/plugins"));assert_eq!(p.modules[0],c.join("plugins"));assert_eq!(p.data,d.join("agent-plugins"));
    for invalid in ["relative","~/unresolved","/tmp/../escape","/tmp/line\nbreak"] {assert!(paths_from(&home.home,Some(std::ffi::OsStr::new(invalid)),None).is_err());}
}

#[test]
fn passive_cli_follows_override_then_cache_then_native_package_without_running_anything() {
    let home=TestHome::new("cline-passive-wrapper");let c=cli_contract();let (selected,native)=wrapper(&home,&c);
    assert_eq!(verify_cli(&c,&selected,None).unwrap(),native);let cache=selected.parent().unwrap().join(".cline");write(&cache,b"unverified cache");
    assert!(verify_cli(&c,&selected,None).is_err());assert_eq!(verify_cli(&c,&selected,Some(native.as_os_str())).unwrap(),native);
    let unknown=home.home.join("unknown");write(&unknown,CODE);assert_eq!(verify_cli(&c,&unknown,None).unwrap(),unknown);
    write(&unknown,b"unknown executable");assert!(verify_cli(&c,&selected,Some(unknown.as_os_str())).is_err());
    assert!(!home.home.join("cline-executed-marker").exists());
}

#[test]
fn unsupported_cli_always_suppresses_the_execution_fallback() {
    let home=TestHome::new("cline-unknown-version");let path=home.home.join("cline");write(&path,CODE);
    assert_eq!(super::super::passive_runtime_version("cline-cli",&path),Some(None));assert!(!home.home.join("cline-executed-marker").exists());
}

fn desktop_fixture(home:&TestHome) -> DesktopContract {
    let app=home.home.join("Applications/Cline.app");fs::create_dir_all(app.join("Contents")).unwrap();
    let mut info=plist::Dictionary::new();
    for (key,value) in [("CFBundleIdentifier","bot.cline.app"),("CFBundleName","Cline"),("CFBundleShortVersionString","0.0.49"),("CFBundleExecutable","cline-app")] {info.insert(key.into(),plist::Value::String(value.into()));}
    plist::Value::Dictionary(info).to_file_xml(app.join("Contents/Info.plist")).unwrap();
    for member in &APP_MEMBERS[1..]{write(&app.join(member),CODE);}
    serde_json::from_value(json!({"agent_id":"cline-desktop","version":"0.0.49","bundle_id":"bot.cline.app","product_name":"Cline",
        "app_paths":[app],"guards":APP_MEMBERS.iter().map(|member|guard(member,&fs::read(app.join(member)).unwrap())).collect::<Vec<_>>()
    })).unwrap()
}

#[test]
fn desktop_binds_info_launcher_bundled_cli_and_backend_independently() {
    let home=TestHome::new("cline-desktop-gates");let c=desktop_fixture(&home);assert_eq!(verify_desktop(&c),Ok(true));
    let app=home.home.join("Applications/Cline.app");
    for member in APP_MEMBERS {
        let original=fs::read(app.join(member)).unwrap();write(&app.join(member),b"changed public member");
        assert!(verify_desktop(&c).is_err());write(&app.join(member),&original);
    }
    assert!(!home.home.join("cline-executed-marker").exists());
}

#[test]
fn malformed_mcp_component_preserves_the_package_and_skill_file_candidate() {
    let home=TestHome::new("cline-component-schema");let p=paths(&home);let dir=package(&p.packages,"p","p");write(&dir.join("skills/s/SKILL.md"),b"PRIVATE_BODY");
    write(&dir.join("mcp.json"),serde_json::to_vec(&json!({"$schema":MCP_SCHEMA,"mcpServers":{"m":{"type":"stdio","command":"tool"}},"unknown":"invalid-root-field"})).unwrap().as_slice());
    let r=scan("cline-cli",&p);assert_eq!(r.plugins.len(),1);assert!(r.mcps.is_empty());assert!(r.unavailable);
    assert!(r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_BODY"));
}
