use super::*;
use crate::testenv::TestHome;
use serde_json::json;

fn put(root:&Path,name:&str,body:&[u8]){let p=root.join(name);fs::create_dir_all(p.parent().unwrap()).unwrap();fs::write(p,body).unwrap();}
fn fixture()->(TestHome,Host,PathBuf,Contract){
    let home=TestHome::new("zed-ext");let app=home.home.join("Applications/Zed.app");
    put(&app,"Contents/Info.plist",br#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>dev.zed.Zed</string><key>CFBundleName</key><string>Zed</string><key>CFBundleShortVersionString</key><string>1.25.3</string></dict></plist>"#);
    let c:Contract=serde_json::from_str(include_str!("../../../../../data/native-plugin-zed.json")).unwrap();let installed=expand_tilde(&c.installed_root);fs::create_dir_all(&installed).unwrap();
    let host=Host{agent_id:"zed".into(),adapter:"zed".into(),docs_url:"https://zed.dev".into(),manifest_paths:vec![],default_mcp_files:vec![],mcp_id_prefix:String::new(),roots:vec![Root{path:app.to_string_lossy().into_owned(),layout:"zed_app".into(),presence:PluginPresence::Bundled,complete_marker:None}],resource_groups:vec![]};
    (home,host,installed,c)
}
fn modern(root:&Path,name:&str,more:&str)->PathBuf{let p=root.join(name);put(&p,"extension.toml",format!("id='{name}'\nname='Example'\nversion='1'\nschema_version=1\n{more}").as_bytes());p}
fn read(host:&Host,c:&Contract)->PluginScan{let mut r=PluginScan::default();if scan(host,c,&mut r).is_err(){r.unavailable=true;}normalize(&mut r);r}

#[test]
fn metadata_only_extension_lsp_and_wasm_keep_unknown_activation(){
    let (_home,host,root,c)=fixture();let p=modern(&root,"html","languages=['languages/html']\n[lib]\nkind='Rust'\nversion='0.7.0'\n[language_servers.html]\nlanguage='HTML'\n[grammars.html]\nrepository='PRIVATE_REPOSITORY'\nrev='PRIVATE_REV'\n");
    put(&p,"languages/html/config.toml",b"PRIVATE_LANGUAGE_BODY");put(&p,"extension.wasm",b"\0asmPRIVATE_WASM_BODY");
    let r=read(&host,&c);assert!(!r.unavailable);assert_eq!(r.plugins.len(),1);assert!(r.mcps.is_empty());let x=&r.plugins[0];assert_eq!(x.presence,PluginPresence::Installed);assert_eq!(x.enabled,None);
    assert!(x.components.iter().any(|c|c.kind==PluginComponentKind::Lsp));assert!(x.components.iter().any(|c|c.kind==PluginComponentKind::Code));assert!(!serde_json::to_string(x).unwrap().contains("PRIVATE_"));
}
#[test]
fn native_factories_stay_references_and_undeclared_skill_folders_never_become_skills(){
    let (_home,host,root,c)=fixture();let p=modern(&root,"agent","[context_servers.example]\ncommand='PRIVATE_COMMAND'\n[slash_commands.review]\ndescription='PRIVATE_DESCRIPTION'\nrequires_argument=false\n[language_model_providers.custom]\nname='Provider'\n");put(&p,"skills/private/SKILL.md",b"PRIVATE_SKILL_BODY");
    let r=read(&host,&c);assert!(!r.unavailable);assert!(r.mcps.is_empty());let x=&r.plugins[0];assert!(x.components.iter().any(|c|c.kind==PluginComponentKind::McpReference));assert!(x.components.iter().any(|c|c.kind==PluginComponentKind::Command));assert!(!x.components.iter().any(|c|c.kind==PluginComponentKind::Skill));assert!(!serde_json::to_string(x).unwrap().contains("PRIVATE_"));
}
#[test]
fn malformed_toml_does_not_fall_back_to_legacy_json(){
    let (_home,host,root,c)=fixture();let p=root.join("bad");put(&p,"extension.toml",b"broken=[");put(&p,"extension.json",br#"{"name":"Fallback","version":"1"}"#);let r=read(&host,&c);assert!(r.unavailable);assert!(r.plugins.is_empty());
}
#[test]
fn legacy_conversion_uses_folder_identity_and_excludes_foreign_runtime_fields(){
    let (_home,host,root,c)=fixture();let p=root.join("old");put(&p,"extension.json",json!({"name":"Old","version":"1","languages":{"HTML":"languages/html"},"grammars":{"html":"grammar"},"context_servers":{"fake":{}},"slash_commands":{"fake":{}}}).to_string().as_bytes());put(&p,"languages/html/config.toml",b"PRIVATE_BODY");
    let r=read(&host,&c);assert!(!r.unavailable);assert_eq!(r.plugins[0].plugin_id,"old");assert!(!r.plugins[0].components.iter().any(|c|matches!(c.kind,PluginComponentKind::McpReference|PluginComponentKind::Command)));
}
#[test]
fn derived_index_work_and_staging_never_create_installed_rows(){
    let (_home,host,root,c)=fixture();modern(&root,"real","");let parent=root.parent().unwrap();put(parent,"index.json",br#"{"extensions":{"fake":{"manifest":{"id":"fake"}}}}"#);modern(&parent.join("work"),"fake","");modern(&parent.join("staging"),"fake","");let r=read(&host,&c);assert_eq!(r.plugins.len(),1);assert_eq!(r.plugins[0].plugin_id,"real");
}
#[test]
fn future_schema_preserves_basic_identity_and_mismatched_folder_is_rejected(){
    let (_home,host,root,c)=fixture();let p=modern(&root,"future","");put(&p,"extension.toml",b"id='future'\nname='Future'\nversion='2'\nschema_version=2\n[context_servers.fake]\n");let bad=modern(&root,"bad","");put(&bad,"extension.toml",b"id='elsewhere'\nname='Bad'\nversion='1'\nschema_version=1\n");
    let r=read(&host,&c);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);assert_eq!(r.plugins[0].components.len(),1);
}
#[test]
fn unknown_host_version_keeps_package_identity_without_reusing_native_field_interpretation(){
    let (_home,host,root,mut c)=fixture();modern(&root,"new","[context_servers.fake]\n");c.audited_version="other".into();let r=read(&host,&c);assert!(r.unavailable);assert_eq!(r.plugins[0].components.len(),1);
}
#[cfg(unix)]
#[test]
fn contributed_resource_symlinks_and_escaping_paths_do_not_get_read(){
    use std::os::unix::fs::symlink;let (home,host,root,c)=fixture();let p=modern(&root,"unsafe","themes=['../../private.json']\n");let external=home.home.join("private.json");fs::write(&external,b"PRIVATE_BODY").unwrap();fs::create_dir_all(p.join("themes")).unwrap();symlink(external,p.join("themes/private.json")).unwrap();let r=read(&host,&c);assert!(r.unavailable);assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_BODY"));
}
#[test]
fn observation_does_not_spend_an_unrelated_parents_exhausted_budget(){
    let (_home,host,root,_c)=fixture();modern(&root,"real","");let mut r=PluginScan::default();r.budget.bytes=MAX_READ_BYTES;r.budget.visited=MAX_SCAN_ENTRIES;observe(&host,&mut r);assert!(!r.unavailable);assert_eq!(r.plugins.len(),1);assert_eq!(r.budget.bytes,MAX_READ_BYTES);assert_eq!(r.budget.visited,MAX_SCAN_ENTRIES);
}
#[test]
fn watched_paths_cover_manifests_wasm_and_declared_resources_without_private_state(){
    let (_home,host,root,_c)=fixture();let p=modern(&root,"real","languages=['languages/custom']\nsnippets='snippets/custom.json'\n");let paths=watch_paths(&host);assert!(paths.contains(&p.join("extension.toml")));assert!(paths.contains(&p.join("extension.wasm")));assert!(paths.contains(&p.join("languages/custom")));assert!(paths.contains(&p.join("snippets/custom.json")));assert!(paths.iter().all(|p|!p.to_string_lossy().contains("settings.json")&&!p.to_string_lossy().contains("index.json")));
}
