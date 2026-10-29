use super::*;
use crate::testenv::TestHome;
use serde_json::json;

fn host() -> &'static Host { hosts().iter().find(|host|host.agent_id=="vscode").unwrap() }
fn write(root: &Path, name: &str, content: &[u8]) {
    let path=root.join(name);fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,content).unwrap();
}
fn fixture() -> (TestHome,PathBuf,PathBuf,Contract) {
    let home=TestHome::new("vscode-native");
    let app=crate::paths::system_probe_path("/Applications/Visual Studio Code.app");
    write(&app,"Contents/Info.plist",br#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.microsoft.VSCode</string><key>CFBundleName</key><string>Code</string><key>CFBundleShortVersionString</key><string>1.106.3</string></dict></plist>"#);
    let resources=app.join("Contents/Resources/app");
    write(&resources,"product.json",br#"{"nameShort":"Code","dataFolderName":".vscode"}"#);
    write(&resources,"package.json",br#"{"version":"1.106.3"}"#);
    let mut c:Contract=serde_json::from_str(include_str!("../../../../../data/native-plugin-vscode.json")).unwrap();
    for guard in [&mut c.environment,&mut c.workbench] {
        let content=guard.path.as_bytes();write(&resources,&guard.path,content);
        guard.bytes=content.len() as u64;guard.sha256=hex::encode(Sha256::digest(content));
    }
    let users=home.home.join(".vscode/extensions");fs::create_dir_all(&users).unwrap();
    (home,resources,users,c)
}
fn install(root:&Path,folder:&str,name:&str,version:&str,contributes:Value)->PathBuf {
    let p=root.join(folder);write(&p,"package.json",json!({"publisher":"example","name":name,"version":version,"displayName":"%title%","main":"./extension","contributes":contributes}).to_string().as_bytes());
    write(&p,"package.nls.json",br#"{"title":"Example extension","unused":"PRIVATE_LOCALIZATION"}"#);
    write(&p,"extension.js",b"\xffPRIVATE_EXECUTABLE_BODY");p
}
fn registration(root:&Path,folder:&str,id:&str,version:&str)->Value {
    json!({"identifier":{"id":id},"version":version,"relativeLocation":folder,
        "location":{"path":root.join(folder),"scheme":"file"},"metadata":{"private":"PRIVATE_INSTALL_METADATA"}})
}
fn ledger(root:&Path,rows:Value) {write(root,"extensions.json",rows.to_string().as_bytes());}
fn scan(c:&Contract)->PluginScan {let mut r=PluginScan::default();if observe_contract(host(),c,&mut r).is_err(){r.unavailable=true;}normalize(&mut r);r}

#[test]
fn bundled_and_registered_versions_keep_distinct_sources_without_guessing_activation() {
    let (_home,resources,users,c)=fixture();install(&resources.join("extensions"),"builtin","same","1",json!({"commands":[{"command":"ordinary.ide.command"}]}));
    install(&users,"same-v1","same","1",json!({}));install(&users,"same-v2","same","2",json!({}));
    ledger(&users,json!([registration(&users,"same-v1","example.same","1"),registration(&users,"same-v2","example.same","2")]));
    let r=scan(&c);assert!(!r.unavailable);assert_eq!(r.plugins.len(),3);assert!(r.mcps.is_empty());
    assert_eq!(r.plugins.iter().filter(|p|p.presence==PluginPresence::Installed).count(),2);
    assert!(r.plugins.iter().all(|p|p.plugin_id=="example.same"&&p.enabled.is_none()));
    assert!(r.plugins.iter().all(|p|!p.components.iter().any(|c|c.kind==PluginComponentKind::Command)));
    let serialized=serde_json::to_string(&r.plugins).unwrap();assert!(!serialized.contains("PRIVATE_"));
}

#[test]
fn provider_definitions_remain_references_without_fabricating_mcp_servers() {
    let (_home,resources,users,c)=fixture();ledger(&users,json!([]));
    install(&resources.join("extensions"),"provider","provider","1",json!({"mcpServerDefinitionProviders":[{"id":"python.provider","label":"Python MCP","when":"host.feature"}],"languageModelTools":[{"name":"inspect","displayName":"Inspect","modelDescription":"PRIVATE_TOOL_DESCRIPTION","inputSchema":{"private":"PRIVATE_SCHEMA"}}]}));
    let r=scan(&c);assert!(!r.unavailable);assert!(r.mcps.is_empty());let p=&r.plugins[0];
    assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.native_id.as_deref()==Some("python.provider")));
    assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::Tool&&c.native_id.as_deref()==Some("inspect")));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn unrelated_global_budget_exhaustion_does_not_truncate_this_physical_hosts_metadata() {
    let (_home,resources,users,c)=fixture();ledger(&users,json!([]));
    install(&resources.join("extensions"),"provider","provider","1",json!({"mcpServerDefinitionProviders":[{"id":"x","label":"X"}]}));
    let mut result=PluginScan::default();result.budget.visited=MAX_SCAN_ENTRIES;result.budget.bytes=MAX_READ_BYTES;result.budget.verified_bytes=MAX_VERIFIED_BYTES;
    observe_isolated(host(),&c,&mut result);normalize(&mut result);
    assert!(!result.unavailable);assert_eq!(result.plugins.len(),1);assert!(result.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::McpReference));
    assert_eq!(result.budget.visited,MAX_SCAN_ENTRIES);assert_eq!(result.budget.bytes,MAX_READ_BYTES);assert_eq!(result.budget.verified_bytes,MAX_VERIFIED_BYTES);
}

#[test]
fn contributed_agent_files_use_native_names_descriptions_and_confined_markers_only() {
    let (_home,resources,users,c)=fixture();ledger(&users,json!([]));
    let p=install(&resources.join("extensions"),"agent-files","files","1",json!({"chatPromptFiles":[{"name":"review","path":"prompts/review.md","description":"PRIVATE_DESCRIPTION"}],"chatInstructions":[{"name":"style","path":"rules/style.md","description":"PRIVATE_DESCRIPTION"}],"chatAgents":[{"name":"helper","path":"agents/helper.md","description":"PRIVATE_DESCRIPTION"}],"chatSkills":[{"name":"not-supported","path":"skills/unbound/SKILL.md"}]}));
    for name in ["prompts/review.md","rules/style.md","agents/helper.md","skills/unbound/SKILL.md"]{write(&p,name,b"PRIVATE_COMPONENT_BODY");}
    let r=scan(&c);assert!(!r.unavailable);let kinds=r.plugins[0].components.iter().map(|c|c.kind.clone()).collect::<BTreeSet<_>>();
    assert!(kinds.contains(&PluginComponentKind::Command)&&kinds.contains(&PluginComponentKind::Rule)&&kinds.contains(&PluginComponentKind::Agent));
    assert!(!kinds.contains(&PluginComponentKind::Skill));assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn invalid_native_provider_array_does_not_publish_a_partial_provider_set() {
    let (_home,resources,users,c)=fixture();ledger(&users,json!([]));
    install(&resources.join("extensions"),"provider","provider","1",json!({"mcpServerDefinitionProviders":[{"id":"valid","label":"Valid"},{"id":"invalid","label":"Invalid","when":false}]}));
    let r=scan(&c);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);assert!(!r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::McpReference));
}

#[test]
fn missing_descriptions_unsafe_names_and_escaping_files_remain_unavailable() {
    let (home,resources,users,c)=fixture();ledger(&users,json!([]));write(&home.home,"outside.md",b"PRIVATE_OUTSIDE");
    install(&resources.join("extensions"),"files","files","1",json!({"chatAgents":[{"name":"bad name","path":"agent.md","description":"x"},{"name":"missing-description","path":"agent.md"},{"name":"escape","path":"../../../../outside.md","description":"x"}]}));
    let r=scan(&c);assert!(r.unavailable);assert!(!r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Agent));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn unregistered_history_and_other_profile_directories_are_not_installations() {
    let (_home,resources,users,c)=fixture();ledger(&users,json!([]));install(&users,"history","history","0",json!({}));
    install(&resources.join("extensions"),"builtin","builtin","1",json!({}));
    let r=scan(&c);assert!(!r.unavailable);assert_eq!(r.plugins.len(),1);assert_eq!(r.plugins[0].presence,PluginPresence::Bundled);
}

#[test]
fn one_rejected_agent_file_does_not_hide_other_native_valid_declarations() {
    let (_home,resources,users,c)=fixture();ledger(&users,json!([]));
    let p=install(&resources.join("extensions"),"agents","agents","1",json!({"chatAgents":[{"name":"invalid","path":"missing.md"},{"name":"valid","path":"valid.md","description":"x"}]}));
    write(&p,"valid.md",b"PRIVATE_BODY");let r=scan(&c);assert!(r.unavailable);
    let agents=r.plugins[0].components.iter().filter(|c|c.kind==PluginComponentKind::Agent).collect::<Vec<_>>();assert_eq!(agents.len(),1);assert_eq!(agents[0].name,"valid");
}

#[test]
fn missing_registration_manifest_stays_declared_and_version_mismatch_does_not_select_a_winner() {
    let (_home,_resources,users,c)=fixture();install(&users,"mismatch","mismatch","2",json!({}));
    ledger(&users,json!([registration(&users,"missing","example.missing","1"),registration(&users,"mismatch","example.mismatch","1")]));
    let r=scan(&c);assert!(r.unavailable);assert_eq!(r.plugins.len(),2);assert!(r.plugins.iter().all(|p|p.presence==PluginPresence::Declared&&p.components.is_empty()&&p.version=="1"));
}

#[test]
fn relative_registration_precedes_a_stale_absolute_location() {
    let (_home,_resources,users,c)=fixture();install(&users,"current","current","1",json!({}));
    let mut row=registration(&users,"current","example.current","1");row["location"]["path"]=json!("/unrelated/private/location");ledger(&users,json!([row]));
    let r=scan(&c);assert!(!r.unavailable);assert_eq!(r.plugins[0].presence,PluginPresence::Installed);
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("/unrelated/private"));
}

#[test]
fn malformed_registry_is_not_treated_as_a_partial_success() {
    let (_home,resources,users,c)=fixture();install(&users,"valid","valid","1",json!({}));install(&resources.join("extensions"),"builtin","builtin","1",json!({}));
    ledger(&users,json!([registration(&users,"valid","example.valid","1"),{"identifier":{"id":"broken"}}]));
    let r=scan(&c);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);assert_eq!(r.plugins[0].presence,PluginPresence::Bundled);
}

#[test]
fn changed_host_guards_keep_inert_bundled_metadata_without_native_component_claims() {
    let (_home,resources,users,c)=fixture();install(&resources.join("extensions"),"provider","provider","1",json!({"mcpServerDefinitionProviders":[{"id":"x","label":"X"}]}));
    install(&users,"user","user","1",json!({}));ledger(&users,json!([registration(&users,"user","example.user","1")]));
    write(&resources,&c.workbench.path,b"changed workbench");write(&resources,&c.environment.path,b"changed environment");
    let r=scan(&c);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);assert_eq!(r.plugins[0].presence,PluginPresence::Bundled);
    assert!(!r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::McpReference));
}

#[test]
fn explicit_absolute_extensions_root_precedes_portable_and_relative_roots_fail_closed() {
    let (home,_resources,_users,c)=fixture();let selected=home.home.join("selected");fs::create_dir_all(&selected).unwrap();
    install(&selected,"selected","selected","1",json!({}));ledger(&selected,json!([registration(&selected,"selected","example.selected","1")]));
    std::env::set_var("VSCODE_EXTENSIONS",&selected);std::env::set_var("VSCODE_PORTABLE",home.home.join("portable"));
    assert_eq!(scan(&c).plugins[0].plugin_id,"example.selected");
    std::env::set_var("VSCODE_EXTENSIONS","relative-root");let r=scan(&c);assert!(r.unavailable);assert!(r.plugins.is_empty());
}

#[test]
fn unsafe_external_registration_does_not_read_or_attribute_another_directory() {
    let (home,_resources,users,c)=fixture();let outside=home.home.join("outside");install(&outside,"private","private","1",json!({}));
    let row=json!({"identifier":{"id":"example.private"},"version":"1","location":{"scheme":"file","path":outside.join("private")}});ledger(&users,json!([row]));
    let r=scan(&c);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);assert_eq!(r.plugins[0].presence,PluginPresence::Declared);assert!(r.plugins[0].components.is_empty());
    assert!(!r.plugins[0].source_paths.iter().any(|p|p.contains("outside/private")));
}

#[cfg(unix)]
#[test]
fn symlinked_registrations_and_contributed_file_aliases_do_not_escape_metadata_scope() {
    use std::os::unix::fs::symlink;
    let (home,resources,users,c)=fixture();let outside=install(&home.home,"outside","secret","1",json!({}));
    symlink(outside,users.join("alias")).unwrap();ledger(&users,json!([registration(&users,"alias","example.alias","1")]));
    let p=install(&resources.join("extensions"),"agent","agent","1",json!({"chatAgents":[{"name":"alias","path":"agent.md","description":"x"}]}));
    let body=home.home.join("private.md");fs::write(&body,b"PRIVATE_BODY").unwrap();symlink(body,p.join("agent.md")).unwrap();
    let r=scan(&c);assert!(r.unavailable);assert_eq!(r.plugins.len(),2);
    let alias=r.plugins.iter().find(|p|p.plugin_id=="example.alias").unwrap();assert_eq!(alias.presence,PluginPresence::Declared);assert!(alias.components.is_empty());
    assert!(r.plugins.iter().all(|p|!p.components.iter().any(|c|c.kind==PluginComponentKind::Agent)));
}

#[test]
fn watched_paths_include_guards_registrations_and_contributed_files_without_private_state() {
    let (_home,resources,users,_c)=fixture();ledger(&users,json!([]));
    let p=install(&resources.join("extensions"),"agent","agent","1",json!({"chatAgents":[{"name":"review","path":"agents/review.md","description":"x"}]}));write(&p,"agents/review.md",b"PRIVATE_BODY");
    let paths=watch_paths(host());assert!(paths.contains(&users.join("extensions.json")));assert!(paths.contains(&p.join("package.json")));assert!(paths.contains(&p.join("agents/review.md")));
    assert!(paths.contains(&resources.join("out/main.js")));assert!(paths.iter().all(|p|!p.to_string_lossy().contains("state.vscdb")));
}
