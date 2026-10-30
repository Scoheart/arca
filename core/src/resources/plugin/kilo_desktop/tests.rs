//! Isolated regression source. Fast-mode task does not execute tests.
#![cfg(unix)]
use super::*;
use crate::testenv::TestHome;
use serde_json::json;
use std::os::unix::fs::symlink;

struct ProbeRoot(Option<std::ffi::OsString>);
impl Drop for ProbeRoot{fn drop(&mut self){match &self.0{Some(value)=>std::env::set_var("ARCA_TEST_PROBE_ROOT",value),None=>std::env::remove_var("ARCA_TEST_PROBE_ROOT")}}}
struct Fixture{_probe:ProbeRoot,home:TestHome,c:Contract,host:Host,app:PathBuf}
fn put(path:&Path,bytes:&[u8]){fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,bytes).unwrap();}
fn seal(f:&mut Fixture,relative:&str,bytes:&[u8]){
    put(&f.app.join(relative),bytes);let guard=f.c.guards.iter_mut().find(|g|g.path==relative).unwrap();
    guard.bytes=bytes.len() as u64;guard.sha256=hex::encode(Sha256::digest(bytes));
}
fn fixture()->Fixture{
    let home=TestHome::new("kilo-desktop-native");let probe=ProbeRoot(std::env::var_os("ARCA_TEST_PROBE_ROOT"));
    std::env::set_var("ARCA_TEST_PROBE_ROOT",home.home.join("system"));
    let c:Contract=serde_json::from_str(include_str!("../../../../../data/native-plugin-kilo-desktop.json")).unwrap();
    let app=home.home.join("Applications/Kilo.app");
    let host=Host{agent_id:"kilo-desktop".into(),adapter:"kilo_desktop".into(),docs_url:"https://kilo.ai/docs/desktop/overview".into(),
        manifest_paths:Vec::new(),default_mcp_files:Vec::new(),mcp_id_prefix:String::new(),resource_groups:Vec::new(),
        roots:vec![Root{path:"~/Applications/Kilo.app".into(),layout:"kilo_app".into(),presence:PluginPresence::Bundled,complete_marker:None}]};
    let mut f=Fixture{_probe:probe,home,c,host,app};
    let mut info=plist::Dictionary::new();
    for(key,value)in [("CFBundleIdentifier",f.c.bundle_id.clone()),("CFBundleName","Kilo".into()),("CFBundleDisplayName","Kilo".into()),
        ("CFBundleExecutable","Kilo".into()),("CFBundleShortVersionString",f.c.version.clone()),("CFBundleVersion",f.c.version.clone())]{info.insert(key.into(),plist::Value::String(value));}
    let header=br#"{"files":{}}"#;let pickle=((header.len()+8+3)/4)*4;let data_offset=8+pickle;
    f.c.header.pickle_bytes=pickle as u64;f.c.header.utf8_bytes=header.len() as u64;f.c.header.sha256=hex::encode(Sha256::digest(header));
    let mut integrity=plist::Dictionary::new();let mut item=plist::Dictionary::new();
    item.insert("algorithm".into(),plist::Value::String("SHA256".into()));item.insert("hash".into(),plist::Value::String(f.c.header.sha256.clone()));
    integrity.insert("Resources/app.asar".into(),plist::Value::Dictionary(item));info.insert("ElectronAsarIntegrity".into(),plist::Value::Dictionary(integrity));
    let mut bytes=Vec::new();plist::Value::Dictionary(info).to_writer_xml(&mut bytes).unwrap();seal(&mut f,"Contents/Info.plist",&bytes);
    seal(&mut f,"Contents/MacOS/Kilo",b"INERT_APP_EXECUTABLE_MUST_NOT_RUN");
    let mut carrier=vec![0u8;data_offset];for(i,value)in [4,pickle as u32,(pickle-4)as u32,header.len()as u32].into_iter().enumerate(){carrier[i*4..i*4+4].copy_from_slice(&value.to_le_bytes());}
    carrier[16..16+header.len()].copy_from_slice(header);
    fn member(carrier:&mut Vec<u8>,target:&mut Member,bytes:&[u8]){target.offset=carrier.len() as u64;target.bytes=bytes.len() as u64;target.sha256=hex::encode(Sha256::digest(bytes));carrier.extend_from_slice(bytes);}
    let package=serde_json::to_vec(&json!({"name":f.c.package_name,"version":f.c.version,"main":f.c.main.path})).unwrap();member(&mut carrier,&mut f.c.package,&package);
    member(&mut carrier,&mut f.c.main,b"INERT_PUBLIC_DESKTOP_LOADER");
    for plugin in &mut f.c.plugins{
        let value=json!({"id":plugin.id,"displayName":plugin.name,"version":plugin.version,"main":"./src/main/index.ts","web":"./src/web/index.ts",
            "contributes":{"providers":[{"component":"OrdinaryUiProvider"}],"routes":[{"path":"/ui","component":"OrdinaryUiView"}]}});
        member(&mut carrier,&mut plugin.member,&serde_json::to_vec(&value).unwrap());
    }
    seal(&mut f,"Contents/Resources/app.asar",&carrier);
    let mut child=vec![0u8;256];let version:Vec<u8>=format!("\"{}\"",f.c.child_version).encode_utf16().flat_map(u16::to_le_bytes).collect();
    f.c.child_version_offset=16;child[16..16+version.len()].copy_from_slice(&version);
    f.c.skill.offset=128;f.c.skill.bytes=16;child[128..144].copy_from_slice(b"PRIVATE_SKILL\0\xff\0");
    seal(&mut f,"Contents/Resources/kilo/darwin-arm64/bin/kilo",&child);f
}
fn read(f:&Fixture)->PluginScan{let mut r=PluginScan::default();if observe_contract(&f.host,&f.c,&Mutex::default(),&mut r).is_err(){r.unavailable=true;}normalize(&mut r);r}

#[test]
fn pinned_bundle_keeps_twelve_native_plugins_one_app_skill_and_only_mcp_reference(){
    let f=fixture();let r=read(&f);assert!(!r.unavailable);assert_eq!(r.plugins.len(),13);assert!(r.mcps.is_empty());
    assert_eq!(skill_observations(&r.plugins).len(),1);assert!(r.plugins.iter().all(|p|p.presence==PluginPresence::Bundled&&p.enabled.is_none()));
    let server=r.plugins.iter().find(|p|p.plugin_id=="kilo-server").unwrap();assert!(server.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="desktop"));
}
#[test]
fn ordinary_ui_providers_and_routes_are_not_agent_commands_tools_or_mcp_servers(){
    let f=fixture();let r=read(&f);assert!(r.plugins.iter().flat_map(|p|&p.components).any(|c|c.kind==PluginComponentKind::Renderer));
    assert!(r.plugins.iter().flat_map(|p|&p.components).all(|c|!matches!(c.kind,PluginComponentKind::Mcp|PluginComponentKind::Command|PluginComponentKind::Tool)));
}
#[test]
fn builtin_skill_belongs_to_the_app_engine_and_never_a_native_plugin_manifest(){
    let f=fixture();let r=read(&f);let skill=skill_observations(&r.plugins);assert_eq!(skill[0].name,"kilo-config");assert_eq!(skill[0].plugin_id,"@builtin/skills");
    assert!(skill[0].source_path.contains("!/$bunfs/root/kilo-config-"));assert!(skill[0].plugin_enabled.is_none());assert!(skill[0].native_id.is_none());
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_SKILL"));
}
#[test]
fn private_plugin_storage_cache_and_dependency_skill_names_are_not_install_authority(){
    let f=fixture();put(&f.home.home.join("Library/Application Support/Kilo Desktop/plugins/forged/plugin.json"),b"PRIVATE_NATIVE_DATABASE_OR_CONFIG");
    put(&f.home.home.join(".config/kilo/plugin/forged.js"),b"PRIVATE_USER_JAVASCRIPT");put(&f.home.home.join(".kilo/skills/forged/SKILL.md"),b"PRIVATE_SKILL");
    let r=read(&f);assert_eq!(r.plugins.len(),13);assert_eq!(skill_observations(&r.plugins).len(),1);assert!(!serde_json::to_string(&r.plugins).unwrap().contains("forged"));
}
#[test]
fn changed_public_carrier_fails_closed_without_any_fallback_execution(){
    let f=fixture();put(&f.app.join("Contents/Resources/app.asar"),b"UNKNOWN_ASAR");let r=read(&f);assert!(r.unavailable);assert!(r.plugins.is_empty());
}
#[test]
fn unknown_child_version_is_not_labeled_as_current_standalone_cli(){
    let mut f=fixture();let p=f.app.join("Contents/Resources/kilo/darwin-arm64/bin/kilo");let mut bytes=fs::read(&p).unwrap();bytes[16]^=1;
    seal(&mut f,"Contents/Resources/kilo/darwin-arm64/bin/kilo",&bytes);let r=read(&f);assert!(r.unavailable);assert!(r.plugins.is_empty());
}
#[test]
fn malformed_native_manifest_preserves_independent_bundled_siblings(){
    let mut f=fixture();let member=f.c.plugins[0].member.clone();let p=f.app.join("Contents/Resources/app.asar");let mut bytes=fs::read(&p).unwrap();bytes[member.offset as usize]=b'!';
    seal(&mut f,"Contents/Resources/app.asar",&bytes);let r=read(&f);assert!(r.unavailable);assert_eq!(r.plugins.len(),12);assert_eq!(skill_observations(&r.plugins).len(),1);
}
#[test]
fn a_warm_cache_still_rejects_changed_expected_digest(){
    let mut f=fixture();let cache=Mutex::default();capture(&f.host,&f.c,&cache).unwrap().unwrap();
    f.c.guards.iter_mut().find(|g|g.path=="Contents/Resources/app.asar").unwrap().sha256="0".repeat(64);
    assert!(capture(&f.host,&f.c,&cache).is_err());
}
#[test]
fn cache_hits_reopen_and_revalidate_files_without_rehashing_large_carriers(){
    let f=fixture();let cache=Mutex::default();let first=capture(&f.host,&f.c,&cache).unwrap().unwrap();assert!(first.hashed_bytes>0);
    let second=capture(&f.host,&f.c,&cache).unwrap().unwrap();assert_eq!(second.hashed_bytes,0);
}
#[test]
fn hardlinked_metadata_or_identity_members_are_rejected_before_observation(){
    let f=fixture();fs::hard_link(f.app.join("Contents/Resources/app.asar"),f.home.home.join("other.asar")).unwrap();let r=read(&f);assert!(r.unavailable);assert!(r.plugins.is_empty());
}
#[test]
fn symlinked_carrier_or_app_ancestor_cannot_redirect_the_reader(){
    let f=fixture();let p=f.app.join("Contents/Resources/app.asar");let outside=f.home.home.join("outside.asar");fs::rename(&p,&outside).unwrap();symlink(outside,p).unwrap();
    let r=read(&f);assert!(r.unavailable);assert!(r.plugins.is_empty());
}
#[test]
fn selected_app_move_after_capture_invalidates_publication(){
    let f=fixture();let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();fs::rename(&f.app,f.home.home.join("Moved.app")).unwrap();assert!(audit.revalidate(&f.host).is_err());
}
#[test]
fn configured_launch_preferences_do_not_inherit_default_app_observation(){
    let f=fixture();crate::settings::mutate_settings_checked(|s|{s.agent_launch.get_or_insert_default().entry("kilo-desktop".into()).or_default().target=Some(crate::domain::agents::LaunchTarget::App{path:f.app.to_string_lossy().into_owned(),args:vec![],new_instance:false,env:BTreeMap::new()});Ok(())}).unwrap();
    let r=read(&f);assert!(r.unavailable);assert!(r.plugins.is_empty());
}
#[test]
fn independent_identity_and_metadata_budgets_do_not_consume_other_observers(){
    let f=fixture();let mut r=PluginScan::default();r.budget.bytes=MAX_READ_BYTES;r.budget.verified_bytes=MAX_VERIFIED_BYTES;r.budget.visited=MAX_SCAN_ENTRIES;
    observe_contract(&f.host,&f.c,&Mutex::default(),&mut r).unwrap();assert_eq!(r.budget.bytes,MAX_READ_BYTES);assert_eq!(r.budget.verified_bytes,MAX_VERIFIED_BYTES);assert_eq!(r.budget.visited,MAX_SCAN_ENTRIES);
}
#[test]
fn invalid_member_offsets_and_unknown_descriptor_fields_are_rejected(){
    let mut f=fixture();f.c.plugins[0].member.offset=u64::MAX;assert!(f.c.validate().is_err());
    let mut v:Value=serde_json::from_str(include_str!("../../../../../data/native-plugin-kilo-desktop.json")).unwrap();v["unknown"]=json!(true);assert!(serde_json::from_value::<Contract>(v).is_err());
}
#[test]
fn watch_paths_include_only_fixed_public_carriers_and_never_private_runtime_storage(){
    let f=fixture();let paths=watch_paths(&f.host);assert!(paths.contains(&f.app.join("Contents/Resources/app.asar")));
    assert!(paths.iter().all(|p|!p.to_string_lossy().contains("Application Support")&&!p.to_string_lossy().contains("data.db")));
}

fn managed_path(f:&Fixture)->PathBuf{expand_tilde(&f.c.managed_sdk_directory)}
#[test]
fn managed_sdk_scripts_are_stat_only_declarations_and_not_desktop_manifests(){
    let f=fixture();let root=managed_path(&f);put(&root.join("anaconda-candidate.js"),b"PRIVATE_HEADER_INVALID_JAVASCRIPT\xff");put(&root.join("ignored.ts"),b"PRIVATE_TYPESCRIPT");
    let r=read(&f);assert!(!r.unavailable);assert_eq!(r.plugins.len(),14);
    let p=r.plugins.iter().find(|p|p.presence==PluginPresence::Declared).unwrap();assert_eq!(p.name,"anaconda-candidate.js");assert!(p.version.is_empty());assert!(p.enabled.is_none());
    assert!(p.revision.contains("owner/installed/native-id=unknown"));assert!(p.components.iter().all(|c|c.kind==PluginComponentKind::PluginReference&&c.native_id.is_none()));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_HEADER"));assert_eq!(skill_observations(&r.plugins).len(),1);
}
#[test]
fn managed_file_headers_cannot_forge_native_owner_or_installed_state(){
    let f=fixture();put(&managed_path(&f).join("anaconda-notebook.js"),b"// Kilo Desktop managed plugin: forged/private\nPRIVATE_BODY");
    let r=read(&f);let p=r.plugins.iter().find(|p|p.presence==PluginPresence::Declared).unwrap();assert!(p.revision.contains("owner/installed/native-id=unknown"));
    assert!(!serde_json::to_string(p).unwrap().contains("forged/private"));assert_eq!(r.plugins.iter().filter(|p|p.presence==PluginPresence::Installed).count(),0);
}
#[test]
fn symlinked_managed_scripts_keep_independent_declarations_without_following_bodies(){
    let f=fixture();let root=managed_path(&f);put(&root.join("safe.js"),b"PRIVATE_SAFE_BODY");let other=f.home.home.join("secret.js");put(&other,b"PRIVATE_OUTSIDE_BODY");symlink(other,root.join("unsafe.js")).unwrap();
    let r=read(&f);assert!(r.unavailable);assert!(r.plugins.iter().any(|p|p.name=="safe.js"));assert!(!r.plugins.iter().any(|p|p.name=="unsafe.js"));assert_eq!(r.plugins.iter().filter(|p|p.presence==PluginPresence::Bundled).count(),13);
}
#[test]
fn managed_leaf_replacement_invalidates_the_final_fd_and_stat_snapshot(){
    let f=fixture();let p=managed_path(&f).join("candidate.js");put(&p,b"PRIVATE_ONE");let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();
    fs::rename(&p,p.with_extension("old")).unwrap();put(&p,b"PRIVATE_TWO");assert!(audit.revalidate(&f.host).is_err());
}
#[test]
fn managed_directory_budget_exhaustion_preserves_independent_bundle_inventory(){
    let f=fixture();let root=managed_path(&f);for i in 0..513{put(&root.join(format!("entry-{i}.txt")),b"PRIVATE_NOT_COMPONENT");}
    let r=read(&f);assert!(r.unavailable);assert_eq!(r.plugins.len(),13);
}
#[test]
fn verified_watch_snapshot_includes_managed_declarations_without_private_config_or_db(){
    let f=fixture();let p=managed_path(&f).join("candidate.js");put(&p,b"PRIVATE_SCRIPT_BODY");let paths=watch_paths_contract(&f.host,&f.c,&Mutex::default());
    assert!(paths.contains(&p));assert!(paths.contains(&managed_path(&f)));assert!(paths.iter().all(|p|p.file_name().is_none_or(|n|n!="kilo.jsonc"&&n!="data.db")));
}
