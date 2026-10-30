//! Meaningful isolation regressions retained unrun in the authorized fast mode.
#![cfg(all(target_os="macos",target_arch="aarch64"))]
use super::*;
use crate::testenv::TestHome;
use serde_json::json;
use std::os::unix::fs::{symlink,PermissionsExt};

struct Env(Vec<(&'static str,Option<std::ffi::OsString>)>);
impl Env{fn clear()->Self{Self(["NODE_OPTIONS","NODE_PATH","BUN_OPTIONS","KILO_BIN_PATH","KILO_TEST_HOME","KILO_CONFIG_DIR","KILO_CONFIG","KILO_CONFIG_CONTENT"].into_iter().map(|key|{
    let old=std::env::var_os(key);std::env::remove_var(key);(key,old)
}).collect())}}
impl Drop for Env{fn drop(&mut self){for(key,value)in &self.0{match value{Some(v)=>std::env::set_var(key,v),None=>std::env::remove_var(key)}}}}
fn put(path:&Path,bytes:&[u8]){fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,bytes).unwrap();}
struct Fixture{_env:Env,home:TestHome,c:Contract,host:Host,alias:PathBuf}
fn fixture()->Fixture{
    let home=TestHome::new("kilo-cli-native");let env=Env::clear();
    let mut c:Contract=serde_json::from_str(include_str!("../../../../../data/native-plugin-kilo-cli.json")).unwrap();
    c.runtime.version="1.2.3".into();c.runtime.compiled_version_offset=32;
    let marker="var Bf=\"1.2.3\",";c.runtime.compiled_version_binding_sha256=hex::encode(Sha256::digest(marker));
    let mut bytes=vec![0u8;1152];bytes[..4].copy_from_slice(&0xfeedfacfu32.to_le_bytes());bytes[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes());
    let raw=marker.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>();bytes[32..32+raw.len()].copy_from_slice(&raw);
    c.bun.section_offset=128;c.bun.section_bytes=1024;c.bun.graph_bytes=1016;c.bun.table_offset=512;c.bun.table_bytes=104;
    bytes[128..136].copy_from_slice(&1016u64.to_le_bytes());
    c.bun.entry=Member{index:0,name:"/$bunfs/root/kilo".into(),offset:256,bytes:64,encoding:2};
    c.bun.builtin_skill.member=Member{index:1,name:"/$bunfs/root/kilo-config-fixture.md".into(),offset:400,bytes:17,encoding:2};
    for(member,name_offset)in [(&c.bun.entry,136usize),(&c.bun.builtin_skill.member,176usize)]{
        bytes[name_offset..name_offset+member.name.len()].copy_from_slice(member.name.as_bytes());
        let row=512+member.index*52;
        for(at,value)in [(0,(name_offset-136)as u32),(4,member.name.len()as u32),(8,(member.offset-136)as u32),(12,member.bytes as u32)]{bytes[row+at..row+at+4].copy_from_slice(&value.to_le_bytes());}
        bytes[row+48]=member.encoding;
    }
    bytes[400..417].copy_from_slice(b"UNREAD_SKILL_BODY");
    c.bun.table_sha256=hex::encode(Sha256::digest(&bytes[512..616]));
    bytes[1104..1112].copy_from_slice(&968u64.to_le_bytes());bytes[1112..1116].copy_from_slice(&376u32.to_le_bytes());bytes[1116..1120].copy_from_slice(&104u32.to_le_bytes());
    bytes[1136..1152].copy_from_slice(b"\n---- Bun! ----\n");
    c.sdk.offset=360;c.sdk.bytes=14;bytes[360..374].copy_from_slice(b"INERT_SDK_LIST");c.sdk.sha256=hex::encode(Sha256::digest(&bytes[360..374]));
    c.runtime.carrier.bytes=bytes.len()as u64;c.runtime.carrier.sha256=hex::encode(Sha256::digest(&bytes));
    let alias=home.home.join("bin/kilo");put(&alias,&bytes);fs::set_permissions(&alias,fs::Permissions::from_mode(0o755)).unwrap();
    let host=Host{agent_id:"kilo-code".into(),adapter:"kilo_cli".into(),docs_url:"https://kilo.ai/docs/customize/plugins".into(),manifest_paths:vec![],default_mcp_files:vec![],mcp_id_prefix:String::new(),resource_groups:vec![],
        roots:vec![Root{path:"~/.config/kilo".into(),layout:"kilo_cli_global".into(),presence:PluginPresence::Local,complete_marker:None}]};
    Fixture{_env:env,home,c,host,alias}
}
fn read(f:&Fixture)->PluginScan{let mut result=PluginScan::default();observe_contract(&f.host,&f.c,&mut result);normalize(&mut result);result}
fn config(f:&Fixture,file:&str,value:Value){put(&f.home.home.join(".config/kilo").join(file),&serde_json::to_vec(&value).unwrap());}
fn sdk<'a>(r:&'a PluginScan,label:&str)->&'a NativePluginObservation{r.plugins.iter().find(|v|v.name==label).unwrap()}

#[test]
fn application_skill_and_fifteen_sdk_callbacks_are_distinct_readonly_source_groups(){
    let f=fixture();let r=read(&f);assert!(!r.unavailable);assert_eq!(r.plugins.len(),2);assert!(r.mcps.is_empty());
    let skills=skill_observations(&r.plugins);assert_eq!(skills.len(),1);assert_eq!(skills[0].plugin_id,"@builtin/skills");
    assert_eq!(sdk(&r,"Kilo built-in SDK integrations").components.len(),15);
    assert!(r.plugins.iter().all(|v|v.enabled.is_none()&&v.presence==PluginPresence::Bundled));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("UNREAD_SKILL_BODY"));
}
#[test]
fn catalog_union_does_not_disappear_when_later_server_array_replaces_earlier(){
    let f=fixture();config(&f,"config.json",json!({"plugin":["older"]}));config(&f,"opencode.jsonc",json!({"plugin":["newer"]}));config(&f,"tui.json",json!({"plugin":["terminal"]}));
    let r=read(&f);assert!(!r.unavailable);
    assert!(sdk(&r,"older").revision.contains("observed-global-server-array:false"));assert!(sdk(&r,"newer").revision.contains("observed-global-server-array:true"));
    assert!(sdk(&r,"terminal").revision.contains("native-catalog"));assert!(r.plugins.iter().all(|v|v.enabled.is_none()));
}
#[test]
fn only_current_references_join_cache_and_cache_never_means_installed(){
    let f=fixture();config(&f,"kilo.jsonc",json!({"plugin":["current"]}));
    put(&f.home.home.join(".cache/kilo/packages/current@latest/node_modules/current/package.json"),br#"{"name":"current","version":"9.8.7","privateOption":"PRIVATE_TOKEN"}"#);
    put(&f.home.home.join(".cache/kilo/packages/orphan@latest/node_modules/orphan/package.json"),br#"{"name":"orphan","version":"1.0.0"}"#);
    let r=read(&f);let row=sdk(&r,"current");assert_eq!(row.version,"9.8.7");assert_eq!(row.presence,PluginPresence::Local);
    let text=serde_json::to_string(&r.plugins).unwrap();assert!(!text.contains("PRIVATE_TOKEN")&&!text.contains("orphan"));
}
#[test]
fn auto_discovered_javascript_is_stat_only_and_has_no_exports_or_native_id(){
    let f=fixture();put(&f.home.home.join(".config/kilo/plugins/native.js"),br#"{"plugin":["PRIVATE_MODULE_BODY"]}"#);
    let proof=runtime::Proof::capture(&f.alias,&f.c.runtime).unwrap();let mut snapshot=metadata::Snapshot::default();
    let rows=declarations::collect(&f.host,&proof,&mut snapshot).unwrap();assert_eq!(rows.len(),1);assert_eq!(snapshot.metadata_bytes_read(),0);
    assert!(rows[0].components.iter().all(|v|v.native_id.is_none()));assert!(!serde_json::to_string(&rows).unwrap().contains("PRIVATE_MODULE_BODY"));
}
#[test]
fn custom_skill_or_script_override_is_never_opened_as_json_metadata(){
    let f=fixture();let path=f.home.home.join(".kilo/skills/demo/SKILL.md");put(&path,br#"{"plugin":["PRIVATE_SKILL_BODY"]}"#);std::env::set_var("KILO_CONFIG",&path);
    let proof=runtime::Proof::capture(&f.alias,&f.c.runtime).unwrap();let mut snapshot=metadata::Snapshot::default();
    let rows=declarations::collect(&f.host,&proof,&mut snapshot).unwrap();assert!(snapshot.incomplete);assert_eq!(snapshot.metadata_bytes_read(),0);assert!(rows.is_empty());
    assert!(snapshot.json(&path,DocumentFormat::Jsonc).is_err());assert_eq!(snapshot.metadata_bytes_read(),0);
}
#[test]
fn normal_metadata_and_auto_leaf_work_with_home_and_root_fd_anchors(){
    let f=fixture();let p=f.home.home.join(".config/kilo/kilo.jsonc");put(&p,b"// comment\n{\"plugin\":[\"registered\",],}");
    let leaf=f.home.home.join(".config/kilo/plugin/own.js");put(&leaf,b"PRIVATE_HELPER_BODY");let mut snapshot=metadata::Snapshot::default();
    assert!(snapshot.json(&p,DocumentFormat::Jsonc).unwrap().is_some());assert!(snapshot.stat(&leaf).unwrap().unwrap().file);
    assert_eq!(snapshot.entries(leaf.parent().unwrap()).unwrap().len(),1);snapshot.revalidate().unwrap();
    assert!(!snapshot.watches.contains(Path::new("/")));assert!(!snapshot.watches.contains(&f.home.home));
}
#[test]
fn credential_urls_and_options_are_not_display_metadata(){
    let f=fixture();config(&f,"kilo.json",json!({"plugin":[["https://user:PRIVATE_URL_TOKEN@example.invalid/repo",{"token":"PRIVATE_OPTION"}]]}));
    let r=read(&f);let text=serde_json::to_string(&r.plugins).unwrap();assert!(!text.contains("PRIVATE_URL_TOKEN")&&!text.contains("PRIVATE_OPTION"));
    assert!(r.plugins.iter().any(|v|v.name=="Unresolved Kilo SDK plugin"));
}
#[test]
fn missing_cached_npm_carrier_does_not_guess_node_architecture_or_run_fallback(){
    let mut f=fixture();let native=fs::read(&f.alias).unwrap();let root=f.home.home.join("npm-package");
    for guard in &mut f.c.runtime.wrapper_guards{let value=format!("INERT WRAPPER:{}",guard.path);put(&root.join(&guard.path),value.as_bytes());guard.bytes=value.len()as u64;guard.sha256=hex::encode(Sha256::digest(value.as_bytes()));}
    fs::set_permissions(root.join("bin/kilo"),fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_file(&f.alias).unwrap();symlink(root.join("bin/kilo"),&f.alias).unwrap();
    put(&root.join("node_modules/@kilocode/cli-darwin-arm64/bin/kilo"),&native);
    assert!(runtime::Proof::capture(&f.alias,&f.c.runtime).is_err());
    put(&root.join("bin/.kilo"),&native);fs::set_permissions(root.join("bin/.kilo"),fs::Permissions::from_mode(0o755)).unwrap();
    assert!(runtime::Proof::capture(&f.alias,&f.c.runtime).is_ok());
}
#[test]
fn changed_carrier_and_changed_alias_invalidate_a_cached_passive_proof(){
    let f=fixture();let proof=runtime::Proof::capture(&f.alias,&f.c.runtime).unwrap();let mut bytes=fs::read(&f.alias).unwrap();bytes[100]^=1;put(&f.alias,&bytes);
    assert!(proof.revalidate().is_err());assert!(runtime::Proof::capture(&f.alias,&f.c.runtime).is_err());
}
#[test]
fn deep_declared_paths_fail_before_unbounded_ancestor_descent(){
    let f=fixture();let path=(0..100).fold(f.home.home.clone(),|p,_|p.join("x")).join("index.js");let mut snapshot=metadata::Snapshot::default();
    assert!(snapshot.stat(&path).is_err());assert_eq!(snapshot.metadata_bytes_read(),0);assert!(snapshot.watches.is_empty());
}
#[test]
fn malformed_independent_config_does_not_mask_other_registered_sources(){
    let f=fixture();put(&f.home.home.join(".config/kilo/kilo.json"),b"{ invalid");config(&f,"tui.jsonc",json!({"plugin":["independent"]}));
    let r=read(&f);assert!(r.unavailable);assert!(r.plugins.iter().any(|v|v.name=="independent"));
}
#[test]
fn observing_config_never_initializes_schema_or_installs_dependencies(){
    let f=fixture();let p=f.home.home.join(".config/kilo/kilo.jsonc");put(&p,br#"{"plugin":["not-downloaded"]}"#);let before=fs::read(&p).unwrap();
    let r=read(&f);assert_eq!(fs::read(&p).unwrap(),before);assert!(!f.home.home.join(".cache/kilo/packages").exists());
    assert_eq!(sdk(&r,"not-downloaded").presence,PluginPresence::Declared);assert!(r.mcps.is_empty());
}
