//! Isolated regression source; fast mode never executes these cases.
#![cfg(unix)]
use super::*;
use crate::testenv::TestHome;
use serde_json::json;
use std::os::unix::fs::symlink;

struct EnvRestore(Vec<(&'static str,Option<std::ffi::OsString>)>);
impl Drop for EnvRestore{fn drop(&mut self){for(k,v)in &self.0{match v{Some(v)=>std::env::set_var(k,v),None=>std::env::remove_var(k)}}}}
struct Fixture{_env:EnvRestore,home:TestHome,c:Contract,host:Host,app:PathBuf}
fn put(path:&Path,bytes:&[u8]){fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,bytes).unwrap();}
fn fixture()->Fixture{
    let home=TestHome::new("coco-desktop-native");
    let env=EnvRestore(["SNOWFLAKE_HOME","ARCA_TEST_PROBE_ROOT"].into_iter().map(|k|(k,std::env::var_os(k))).collect());
    std::env::remove_var("SNOWFLAKE_HOME");std::env::set_var("ARCA_TEST_PROBE_ROOT",home.home.join("system"));
    let c:Contract=serde_json::from_str(include_str!("../../../../../data/native-plugin-coco-desktop.json")).unwrap();
    let app=home.home.join("Applications/Cortex Code.app");
    let host=Host{agent_id:c.agent_id.clone(),adapter:"coco_desktop".into(),docs_url:"https://docs.snowflake.com/en/user-guide/cortex-code/cortex-code-desktop/plugins".into(),
        manifest_paths:Vec::new(),default_mcp_files:Vec::new(),mcp_id_prefix:String::new(),resource_groups:Vec::new(),
        roots:vec![Root{path:"~/Applications/Cortex Code.app".into(),layout:"coco_app".into(),presence:PluginPresence::Bundled,complete_marker:None}]};
    let mut f=Fixture{_env:env,home,c,host,app};let mut info=plist::Dictionary::new();
    for(k,v)in [("CFBundleIdentifier",f.c.bundle_id.clone()),("CFBundleName","Cortex Code".into()),("CFBundleDisplayName","Cortex Code".into()),("CFBundleExecutable","Cortex Code".into()),("CFBundleVersion",f.c.version.clone()),("CFBundleShortVersionString",f.c.version.clone())]{info.insert(k.into(),plist::Value::String(v));}
    let mut plist=Vec::new();plist::Value::Dictionary(info).to_writer_xml(&mut plist).unwrap();
    for guard in f.c.guards.clone(){let b=if guard.path=="Contents/Info.plist"{plist.clone()}
        else if guard.path==PACKAGE{serde_json::to_vec(&json!({"name":f.c.package_name,"version":f.c.version,"main":f.c.package_main})).unwrap()}
        else if guard.path==PRODUCT{serde_json::to_vec(&json!({"appVersion":f.c.version,"version":f.c.engine_version,"applicationName":f.c.product_application_name,"dataFolderName":f.c.product_data_folder,"commit":f.c.product_commit,"darwinBundleIdentifier":f.c.bundle_id,"nameShort":"Cortex Code"})).unwrap()}
        else{format!("INERT_PUBLIC_GUARD_{}",guard.path).into_bytes()};seal(&mut f,&guard.path,&b);}
    for skill in &mut f.c.skills{let b=b"PRIVATE_SKILL_BODY\xff";skill.bytes=b.len()as u64;put(&f.app.join(&skill.path),b);}
    f
}
fn seal(f:&mut Fixture,path:&str,bytes:&[u8]){put(&f.app.join(path),bytes);let g=f.c.guards.iter_mut().find(|g|g.path==path).unwrap();g.bytes=bytes.len()as u64;g.sha256=hex::encode(Sha256::digest(bytes));}
fn snowflake(f:&Fixture)->PathBuf{f.home.home.join(".snowflake")}
fn managed(f:&Fixture)->PathBuf{snowflake(f).join(&f.c.plugins_directory)}
fn plugin(f:&Fixture,dir:&str,value:Value)->PathBuf{let root=managed(f).join(dir);put(&root.join(&f.c.manifest_paths[0]),&serde_json::to_vec(&value).unwrap());root}
fn registry(f:&Fixture,value:Value){put(&snowflake(f).join(&f.c.registry_file),&serde_json::to_vec(&value).unwrap());}
fn read(f:&Fixture)->PluginScan{let mut r=PluginScan::default();observe_contract(&f.host,&f.c,&Mutex::default(),&mut r);normalize(&mut r);r}
fn native<'a>(r:&'a PluginScan,name:&str)->&'a NativePluginObservation{r.plugins.iter().find(|p|p.name==name&&p.presence==PluginPresence::Local).unwrap()}

#[test]
fn bundled_candidates_are_app_owned_and_never_fabricate_native_plugins_or_host_mcp(){
    let f=fixture();let r=read(&f);assert!(!r.unavailable);assert_eq!(r.plugins.len(),1);assert_eq!(skill_observations(&r.plugins).len(),96);assert!(r.mcps.is_empty());
    assert!(r.plugins[0].enabled.is_none());assert!(r.plugins[0].components.iter().all(|c|c.native_id.is_none()));assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_SKILL"));
}
#[test]
fn ordinary_ide_extension_manifests_are_not_native_agent_plugins(){
    let f=fixture();put(&f.app.join("Contents/Resources/app/extensions/forged/package.json"),br#"{"name":"forged","publisher":"ide"}"#);let r=read(&f);assert_eq!(r.plugins.len(),1);
}
#[test]
fn unregistered_physical_user_manifest_is_local_availability(){
    let f=fixture();plugin(&f,"physical",json!({"name":"My_Plugin","version":"2.0.0"}));let r=read(&f);let p=native(&r,"My_Plugin");assert_eq!(p.version,"2.0.0");assert!(p.enabled.is_none());assert!(p.revision.contains("runtime=unknown"));
}
#[test]
fn primary_manifest_wins_without_merging_the_second_format(){
    let f=fixture();let root=plugin(&f,"same",json!({"name":"primary"}));put(&root.join(".claude-plugin/plugin.json"),br#"{"name":"secondary"}"#);let r=read(&f);assert!(r.plugins.iter().any(|p|p.name=="primary"));assert!(!r.plugins.iter().any(|p|p.name=="secondary"));
}
#[test]
fn selected_invalid_namespace_does_not_fall_through_to_secondary(){
    let f=fixture();let root=plugin(&f,"bad",json!({"name":"bad:name"}));put(&root.join(".claude-plugin/plugin.json"),br#"{"name":"secondary"}"#);let r=read(&f);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);
}
#[test]
fn typed_registry_active_is_a_setting_not_loading_proof(){
    let f=fixture();plugin(&f,"physical",json!({"name":"demo"}));plugin(&f,"requested",json!({"name":"requested"}));
    registry(&f,json!({"demo":{"source":"github:org/repo","installedAt":"date","active":false},"requested":{"source":"github:org/requested","installedAt":"date","active":true},"bad":{"source":4,"installedAt":"date","active":true}}));
    let r=read(&f);assert!(r.unavailable);assert!(native(&r,"demo").enabled.is_none());assert!(native(&r,"requested").enabled.is_none());assert!(!r.plugins.iter().any(|p|p.name=="bad"));
    assert!(native(&r,"demo").revision.contains("registry-active-request=false"));assert!(native(&r,"requested").revision.contains("registry-active-request=true"));assert!(native(&r,"demo").revision.contains("runtime=unknown"));
}
#[test]
fn registry_only_remote_source_stays_opaque_and_does_not_leak_url_values(){
    let f=fixture();registry(&f,json!({"remote":{"source":"https://user:PRIVATE_PASS@example.com/repo","installedAt":"date","active":true},"paused":{"source":"https://example.com/paused","installedAt":"date","active":false}}));
    let r=read(&f);let p=r.plugins.iter().find(|p|p.name=="remote"&&p.presence==PluginPresence::Declared).unwrap();assert!(p.enabled.is_none());assert!(p.revision.contains("registry-active-request=true"));
    let p=r.plugins.iter().find(|p|p.name=="paused").unwrap();assert!(p.enabled.is_none());assert!(p.revision.contains("registry-active-request=false"));assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_PASS"));
}
#[test]
fn invalid_optional_registry_descriptors_do_not_reject_a_native_source(){
    let f=fixture();let mut records=serde_json::Map::new();
    for(name,location)in [("catalog",json!("catalog")),("unknown-enum",json!("unknown")),("wrong-type",json!(4))]{
        let root=f.home.home.join("explicit").join(name);put(&root.join(&f.c.manifest_paths[0]),&serde_json::to_vec(&json!({"name":name})).unwrap());
        records.insert(name.into(),json!({"source":root,"installedAt":"","active":true,"location":location,"installKind":false,"github":{"owner":4},"git":["bad"],"stage":{"stagePath":"stage","cachePath":f.home.home.join("outside-stage-cache")}}));
    }
    registry(&f,Value::Object(records));let r=read(&f);
    for name in ["catalog","unknown-enum","wrong-type"]{let p=native(&r,name);assert!(p.enabled.is_none());assert!(p.revision.contains("registry-active-request=true"));}
}
#[test]
fn user_scan_does_not_discover_its_root_manifest_without_an_explicit_source(){
    let f=fixture();let root=managed(&f);put(&root.join(&f.c.manifest_paths[0]),br#"{"name":"root-plugin"}"#);
    assert!(!read(&f).plugins.iter().any(|p|p.name=="root-plugin"));
    registry(&f,json!({"root-plugin":{"source":root,"installedAt":"date","active":true}}));let r=read(&f);let p=native(&r,"root-plugin");
    assert!(p.enabled.is_none());assert!(p.revision.contains("registry-active-request=true"));
}
#[test]
fn inline_mcp_names_are_references_without_endpoints_or_credentials(){
    let f=fixture();plugin(&f,"p",json!({"name":"mcp","mcpServers":{"api":{"url":"https://private.invalid","env":{"API_KEY":"PRIVATE_VALUE"},"oauth":{"clientSecret":"PRIVATE_SECRET"}}}}));
    let r=read(&f);let p=native(&r,"mcp");assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="api"));assert!(r.mcps.is_empty());assert!(!serde_json::to_string(p).unwrap().contains("PRIVATE_"));
}
#[test]
fn empty_mcp_string_is_exclusive_while_null_uses_default(){
    let f=fixture();let a=plugin(&f,"a",json!({"name":"empty","mcpServers":""}));let b=plugin(&f,"b",json!({"name":"fallback","mcpServers":null}));
    for p in [a,b]{put(&p.join(".mcp.json"),br#"{"mcpServers":{"default":{}}}"#);}let r=read(&f);
    assert!(!native(&r,"empty").components.iter().any(|c|c.kind==PluginComponentKind::McpReference));assert!(native(&r,"fallback").components.iter().any(|c|c.name=="default"));
}
#[test]
fn bad_mcp_source_preserves_a_separate_safe_source(){
    let f=fixture();let p=plugin(&f,"p",json!({"name":"sources","mcpServers":["../escape.json","safe.json"]}));put(&p.join("safe.json"),br#"{"good":{"command":"private-executable"}}"#);
    let r=read(&f);assert!(r.unavailable);assert!(native(&r,"sources").components.iter().any(|c|c.name=="good"));
}
#[test]
fn mcp_references_cannot_reclassify_hook_or_skill_bodies_as_json_metadata(){
    let f=fixture();let p=plugin(&f,"p",json!({"name":"opaque","hooks":"payload.json","mcpServers":["hooks/hooks.json","payload.json","skills/demo/SKILL.md"]}));
    for path in ["hooks/hooks.json","payload.json","skills/demo/SKILL.md"]{put(&p.join(path),b"PRIVATE_COMPONENT_BODY\xff");}
    let r=read(&f);assert!(r.unavailable);assert!(!native(&r,"opaque").components.iter().any(|c|c.kind==PluginComponentKind::McpReference));assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_COMPONENT"));
}
#[test]
fn nested_hook_path_declarations_stay_opaque_to_mcp_metadata_reads(){
    let f=fixture();let p=plugin(&f,"p",json!({"name":"nested","hooks":{"PreToolUse":[{"file":"payload.json"}]},"mcpServers":"payload.json"}));
    put(&p.join("payload.json"),b"PRIVATE_NESTED_HOOK\xff");let r=read(&f);assert!(r.unavailable);assert!(!native(&r,"nested").components.iter().any(|c|c.kind==PluginComponentKind::McpReference));
}
#[test]
fn absolute_hook_body_path_inside_plugin_cannot_be_read_through_mcp(){
    let f=fixture();let root=managed(&f).join("p");let payload=root.join("payload.json");let p=plugin(&f,"p",json!({"name":"absolute-hook","hooks":{"PreToolUse":[{"path":payload.to_string_lossy()}]},"mcpServers":"payload.json"}));
    put(&p.join("payload.json"),b"PRIVATE_ABSOLUTE_HOOK\xff");let r=read(&f);assert!(r.unavailable);assert!(!native(&r,"absolute-hook").components.iter().any(|c|c.kind==PluginComponentKind::McpReference));
}
#[test]
fn equal_native_names_keep_distinct_physical_sources_after_normalization(){
    let f=fixture();plugin(&f,"one",json!({"name":"same","version":"1.0.0"}));plugin(&f,"two",json!({"name":"same","version":"1.0.0"}));
    let r=read(&f);assert_eq!(r.plugins.iter().filter(|p|p.name=="same").count(),2);
}
#[test]
fn credential_urls_in_untrusted_display_fields_are_not_emitted(){
    let f=fixture();plugin(&f,"p",json!({"name":"labels","version":"https://user:PRIVATE_PASS@example.com","mcpServers":{"https://user:PRIVATE_PASS@example.com":{}}}));
    let r=read(&f);assert!(r.unavailable);assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_PASS"));
}
#[test]
fn native_skill_scan_stops_at_first_skill_file_and_keeps_manifest_ownership(){
    let f=fixture();let p=plugin(&f,"p",json!({"name":"owner"}));put(&p.join("skills/demo/SKILL.md"),b"PRIVATE_FRONTMATTER\xff");put(&p.join("skills/demo/nested/SKILL.md"),b"PRIVATE_NESTED\xff");
    let r=read(&f);let skills=native(&r,"owner").components.iter().filter(|c|c.kind==PluginComponentKind::Skill).collect::<Vec<_>>();assert_eq!(skills.len(),1);assert_eq!(skills[0].name,"demo");assert!(skills[0].native_id.is_none());
}
#[test]
fn explicit_skill_roots_replace_default_and_agents_hooks_are_stat_only(){
    let f=fixture();let p=plugin(&f,"p",json!({"name":"paths","skills":["custom"]}));put(&p.join("custom/selected/SKILL.md"),b"PRIVATE\xff");put(&p.join("skills/ignored/SKILL.md"),b"PRIVATE\xff");put(&p.join("agents/one.md"),b"PRIVATE_AGENT\xff");put(&p.join("hooks/hooks.json"),b"PRIVATE_HOOK\xff");
    let r=read(&f);let p=native(&r,"paths");assert!(p.components.iter().any(|c|c.name=="selected"));assert!(!p.components.iter().any(|c|c.name=="ignored"));assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::Agent));assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::Hook));
}
#[test]
fn old_enabled_setting_never_calls_registry_migration_or_creates_files(){
    let f=fixture();let path=snowflake(&f).join(&f.c.settings_file);put(&path,br#"{"snowflake.plugins.enabledPlugins":["old"]}"#);read(&f);assert!(!snowflake(&f).join(&f.c.registry_file).exists());
}
#[test]
fn symlinked_user_manifest_keeps_independent_safe_source(){
    let f=fixture();plugin(&f,"safe",json!({"name":"safe"}));let p=plugin(&f,"unsafe",json!({"name":"unsafe"}));let m=p.join(&f.c.manifest_paths[0]);let outside=f.home.home.join("outside.json");fs::rename(&m,&outside).unwrap();symlink(outside,m).unwrap();
    let r=read(&f);assert!(r.unavailable);assert!(r.plugins.iter().any(|p|p.name=="safe"));assert!(!r.plugins.iter().any(|p|p.name=="unsafe"));
}
#[test]
fn hardlinked_registry_is_rejected_before_metadata_read(){
    let f=fixture();registry(&f,json!({"x":{"source":"ref","installedAt":"date","active":true}}));fs::hard_link(snowflake(&f).join(&f.c.registry_file),f.home.home.join("other.json")).unwrap();let r=read(&f);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);
}
#[test]
fn unsafe_snowflake_home_does_not_hide_independent_bundled_candidates(){
    let f=fixture();std::env::set_var("SNOWFLAKE_HOME","https://user:PRIVATE@example.com");let r=read(&f);assert!(r.unavailable);assert_eq!(skill_observations(&r.plugins).len(),96);assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE@example"));
}
#[test]
fn unknown_public_guard_never_gets_a_version_or_inventory_fallback(){
    let f=fixture();put(&f.app.join(WORKBENCH),b"UNKNOWN_PUBLIC_LOADER");let r=read(&f);assert!(r.unavailable);assert!(r.plugins.is_empty());
}
#[test]
fn warm_identity_cache_revalidates_files_and_expected_digest(){
    let mut f=fixture();let cache=Mutex::default();let mut b=Budget::default();Proof::capture(&f.host,&f.c,&cache,&mut b).unwrap().unwrap();assert!(b.hashed>0);
    let mut b=Budget::default();Proof::capture(&f.host,&f.c,&cache,&mut b).unwrap().unwrap();assert_eq!(b.hashed,0);f.c.guards[0].sha256="0".repeat(64);assert!(Proof::capture(&f.host,&f.c,&cache,&mut Budget::default()).is_err());
}
#[test]
fn stat_leaf_replacement_fails_final_snapshot_cas(){
    let f=fixture();let mut s=Snapshot::default();let p=f.app.join(&f.c.skills[0].path);s.stat(&p).unwrap();fs::rename(&p,p.with_extension("old")).unwrap();put(&p,b"CHANGED");assert!(s.revalidate().is_err());
}
#[test]
fn replaced_bundled_leaf_changes_metadata_fingerprint_without_reading_its_body(){
    let f=fixture();let before=read(&f).plugins[0].source_fingerprint.clone();let p=f.app.join(&f.c.skills[0].path);
    fs::rename(&p,p.with_extension("old")).unwrap();put(&p,b"PRIVATE_SKILL_BODY\xfe");let after=read(&f);assert_ne!(before,after.plugins[0].source_fingerprint);
}
#[test]
fn metadata_anchors_support_absolute_paths_and_bound_depth_before_descent(){
    let f=fixture();let p=snowflake(&f).join("normal.json");put(&p,b"{}");let mut s=Snapshot::default();assert!(s.json(&p).unwrap().is_some());
    let deep=f.home.home.join(std::iter::repeat_n("x",100).collect::<Vec<_>>().join("/"));assert!(s.stat(&deep).is_err());
}
#[test]
fn watch_set_contains_sources_and_never_consistency_ancestors(){
    let f=fixture();let p=plugin(&f,"p",json!({"name":"watch"}));let paths=watch_contract(&f.host,&f.c,&Mutex::default());assert!(paths.contains(&p.join(&f.c.manifest_paths[0])));assert!(!paths.contains(&f.home.home));assert!(!paths.contains(&PathBuf::from("/")));
}
#[test]
fn independent_budgets_leave_existing_observer_allowances_unchanged(){
    let f=fixture();let mut r=PluginScan::default();r.budget.bytes=MAX_READ_BYTES;r.budget.verified_bytes=MAX_VERIFIED_BYTES;r.budget.visited=MAX_SCAN_ENTRIES;observe_contract(&f.host,&f.c,&Mutex::default(),&mut r);assert_eq!(r.budget.bytes,MAX_READ_BYTES);assert_eq!(r.budget.verified_bytes,MAX_VERIFIED_BYTES);assert_eq!(r.budget.visited,MAX_SCAN_ENTRIES);
}
#[test]
fn descriptor_unknown_fields_and_escape_paths_are_rejected(){
    let mut f=fixture();f.c.skills[0].path="../escape/SKILL.md".into();assert!(f.c.validate().is_err());let mut v:Value=serde_json::from_str(include_str!("../../../../../data/native-plugin-coco-desktop.json")).unwrap();v["unknown"]=json!(true);assert!(serde_json::from_value::<Contract>(v).is_err());
}
#[test]
fn custom_launch_target_does_not_inherit_default_app_identity(){
    let f=fixture();crate::settings::mutate_settings_checked(|s|{s.agent_launch.get_or_insert_default().entry(f.c.agent_id.clone()).or_default().target=Some(crate::domain::agents::LaunchTarget::App{path:f.app.to_string_lossy().into_owned(),args:vec![],new_instance:false,env:BTreeMap::new()});Ok(())}).unwrap();let r=read(&f);assert!(r.unavailable);assert!(r.plugins.is_empty());
}
