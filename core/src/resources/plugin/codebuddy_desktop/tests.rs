//! Isolated regression source; intentionally not executed in this fast-mode task.
#![cfg(unix)]
use super::*;
use crate::testenv::TestHome;
use serde_json::json;
#[cfg(unix)]
use std::os::unix::fs::symlink;

struct EnvReset(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl EnvReset {
    fn clear(home:&Path) -> Self {
        let mut old=Vec::new();
        for key in ORDINARY_OVERRIDES {old.push((*key,std::env::var_os(key)));std::env::set_var(key,"");}
        old.push(("ARCA_TEST_PROBE_ROOT",std::env::var_os("ARCA_TEST_PROBE_ROOT")));
        // Keep the isolated system Applications distinct from ~/Applications.
        std::env::set_var("ARCA_TEST_PROBE_ROOT",home.join("system"));
        Self(old)
    }
}
impl Drop for EnvReset {fn drop(&mut self){for(key,value)in &self.0{match value{Some(value)=>std::env::set_var(key,value),None=>std::env::remove_var(key)}}}}
struct Fixture {
    _env:EnvReset, home:TestHome, c:Contract, host:Host, app:PathBuf,
    extensions:PathBuf, native:PathBuf, marketplaces:PathBuf,
}
fn put(path:&Path,bytes:&[u8]){fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,bytes).unwrap();}
fn json_file(path:&Path,value:Value){put(path,&serde_json::to_vec(&value).unwrap());}
fn seal(app:&Path,guard:&mut Guard,bytes:&[u8]){put(&app.join(&guard.path),bytes);guard.bytes=bytes.len() as u64;guard.sha256=hex::encode(Sha256::digest(bytes));}
fn fixture(agent:&str)->Fixture {
    let home=TestHome::new("codebuddy-desktop");let env=EnvReset::clear(&home.home);
    let mut c:Contract=serde_json::from_str(include_str!("../../../../../data/native-plugin-codebuddy-desktop.json")).unwrap();
    let profile=c.profiles.iter_mut().find(|p|p.agent_id==agent).unwrap();
    let app=home.home.join("Applications").join(&profile.app_name);
    for package in &mut profile.builtin_extensions {
        let (publisher,name)=package.id.split_once('.').unwrap();
        let mut value=json!({"name":name,"publisher":publisher,"version":package.version,
            "contributes":{"commands":[{"command":"ordinary.editor.command","title":"PRIVATE_COMMAND_BODY"}]}});
        if package.id=="Tencent-Cloud.coding-copilot" {
            value["main"]=json!("out/extension/index.js");value["activationEvents"]=json!(["*"]);
        }
        if package.id=="vscode.mermaid-chat-features" {
            value["contributes"]["languageModelTools"]=json!([{"name":"renderMermaidDiagram","when":"config.mermaid-chat.enabled","modelDescription":"PRIVATE_TOOL_BODY"}]);
        }
        seal(&app,&mut package.manifest,&serde_json::to_vec(&value).unwrap());
    }
    let mut info=plist::Dictionary::new();
    for(key,value)in [("CFBundleIdentifier",&profile.bundle_id),("CFBundleName",&profile.bundle_name),
        ("CFBundleDisplayName",&profile.bundle_name),("CFBundleExecutable",&c.executable),
        ("CFBundleShortVersionString",&c.version),("CFBundleVersion",&c.version)] {
        info.insert(key.into(),plist::Value::String(value.clone()));
    }
    let mut info_bytes=Vec::new();plist::Value::Dictionary(info).to_writer_xml(&mut info_bytes).unwrap();
    let product_requests:Vec<_>=c.declared_plugins.iter().map(|request|json!({"name":request.name,"marketplaceName":request.marketplace,"scope":"user","autoEnable":request.auto_enable})).collect();
    let inner=json!({"builtInMarketplaces":{"codebuddy-plugins-official":{"source":"zip","url":"https://example.invalid/official.zip"},"cb_teams_marketplace":{"source":"zip","url":"https://example.invalid/teams.zip"}},"builtInMarketPlugins":product_requests,"skillScanDirs":c.skill_scan_dirs});
    for guard in &mut profile.guards {
        let bytes=match guard.path.as_str(){
            "Contents/Info.plist"=>info_bytes.clone(),
            "Contents/Resources/app/package.json"=>serde_json::to_vec(&json!({"name":profile.bundle_name,"version":c.base_version,"main":"./out/main.js"})).unwrap(),
            "Contents/Resources/app/product.json"=>serde_json::to_vec(&json!({"nameShort":profile.bundle_name,"nameLong":profile.bundle_name,"applicationName":profile.application_name,"dataFolderName":profile.data_folder,"genieVersion":c.version,"version":c.base_version,"commit":c.commit})).unwrap(),
            "Contents/Resources/app/extensions/genie/package.json"=>fs::read(app.join(&guard.path)).unwrap(),
            "Contents/Resources/app/extensions/genie/product.json"=>serde_json::to_vec(&inner).unwrap(),
            _=>format!("INERT_PUBLIC_GUARD:{}",guard.path).into_bytes(),
        };seal(&app,guard,&bytes);
    }
    for group in &mut c.skill_groups {for skill in &mut group.skills {
        let bytes=b"\0PRIVATE_SKILL_BODY\xff";skill.bytes=bytes.len() as u64;
        put(&app.join(&group.root).join(&skill.name).join("SKILL.md"),bytes);
    }}
    let host=Host {agent_id:agent.into(),adapter:"codebuddy_desktop".into(),docs_url:"https://www.codebuddy.ai/docs/ide/Features/Skills".into(),
        manifest_paths:vec![],default_mcp_files:vec![],mcp_id_prefix:String::new(),resource_groups:vec![],
        roots:vec![Root{path:format!("~/Applications/{}",profile.app_name),layout:"codebuddy_app".into(),presence:PluginPresence::Bundled,complete_marker:None}]};
    let extensions=home.home.join(&profile.data_folder).join("extensions");let native=home.home.join(".codebuddy");let marketplaces=native.join("plugins/marketplaces");
    Fixture{_env:env,home,c,host,app,extensions,native,marketplaces}
}
fn read(f:&Fixture)->PluginScan {
    let mut result=PluginScan::default();if observe_contract(&f.host,&f.c,&Mutex::default(),&mut result).is_err(){result.unavailable=true;}normalize(&mut result);result
}
fn observed<'a>(r:&'a PluginScan,id:&str,presence:PluginPresence)->&'a NativePluginObservation {r.plugins.iter().find(|p|p.plugin_id==id&&p.presence==presence).unwrap()}
fn ordinary(f:&Fixture,folder:&str,name:&str,version:&str)->PathBuf {
    let root=f.extensions.join(folder);json_file(&root.join("package.json"),json!({"publisher":"example","name":name,"version":version}));root
}
fn registration(f:&Fixture,folder:&str,id:&str,version:&str)->Value {
    json!({"identifier":{"id":id},"version":version,"relativeLocation":folder,"location":{"scheme":"file","path":f.extensions.join(folder)},"metadata":{"unused":"PRIVATE_REGISTRY_BODY"}})
}
fn registry(f:&Fixture,rows:Value){json_file(&f.extensions.join("extensions.json"),rows);}
fn memberships(f:&Fixture,value:Value){json_file(&f.native.join("settings.json"),json!({"enabledPlugins":value,"privateUnused":"PRIVATE_SETTINGS_BODY"}));}
fn marketplace(f:&Fixture,storage:&str,name:&str,plugins:Value)->PathBuf {
    let root=f.marketplaces.join(storage);json_file(&root.join(".codebuddy-plugin/marketplace.json"),json!({"name":name,"plugins":plugins}));
    json_file(&f.native.join("plugins/known_marketplaces.json"),json!({(storage):{"source":{"source":"zip","url":"https://example.invalid/catalog.zip"},"type":"zip","installLocation":root}}));root
}
fn plugin(root:&Path,folder:&str,manifest:Value)->PathBuf {
    let path=root.join(folder);json_file(&path.join(".codebuddy-plugin/plugin.json"),manifest);path
}
fn configured(f:&Fixture,enabled:bool)->PathBuf {
    let root=marketplace(f,"stored-key","official",json!([{"name":"catalog-name","source":"./plugins/review"}]));
    let path=plugin(&root,"plugins/review",json!({"name":"review","version":"2.1","skills":["skills/review"]}));
    put(&path.join("skills/review/SKILL.md"),b"\0PRIVATE_PLUGIN_SKILL\xff");memberships(f,json!({"review@official":enabled}));path
}

#[test]
fn bundled_metadata_retains_skills_and_declared_requests_without_fake_commands_or_mcps(){
    let f=fixture("codebuddy-ide");let result=read(&f);assert!(!result.unavailable);assert_eq!(result.plugins.len(),111);assert!(result.mcps.is_empty());
    assert_eq!(result.plugins.iter().filter(|p|p.presence==PluginPresence::Bundled).count(),102);
    assert_eq!(result.plugins.iter().filter(|p|p.presence==PluginPresence::Declared).count(),9);
    assert_eq!(skill_observations(&result.plugins).len(),15);
    assert_eq!(result.plugins.iter().flat_map(|p|&p.components).filter(|p|p.kind==PluginComponentKind::Tool).count(),2);
    assert!(result.plugins.iter().all(|p|p.enabled.is_none()));
    assert!(result.plugins.iter().flat_map(|p|&p.components).all(|p|p.kind!=PluginComponentKind::Command));
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}
#[test]
fn cn_identity_keeps_outer_extension_root_separate_and_native_default_shared(){
    let f=fixture("codebuddy-cn-ide");let result=read(&f);assert!(!result.unavailable);assert_eq!(result.plugins.len(),111);
    let defaults=default_roots(f.c.profile(&f.host.agent_id).unwrap()).unwrap();
    assert_eq!(defaults.extensions,f.home.home.join(".codebuddycn/extensions"));assert_eq!(defaults.native,f.home.home.join(".codebuddy"));
    assert!(result.plugins.iter().all(|p|p.agent_id=="codebuddy-cn-ide"));
}
#[test]
fn host_cannot_select_the_other_regions_app(){
    let mut f=fixture("codebuddy-ide");f.host.agent_id="codebuddy-cn-ide".into();assert!(read(&f).unavailable);assert!(read(&f).plugins.is_empty());
}
#[test]
fn unknown_app_version_fails_even_when_other_public_fields_match(){
    let mut f=fixture("codebuddy-ide");let p=f.app.join("Contents/Info.plist");let mut value=plist::Value::from_reader(std::io::Cursor::new(fs::read(&p).unwrap())).unwrap();
    value.as_dictionary_mut().unwrap().insert("CFBundleShortVersionString".into(),plist::Value::String("99.0".into()));let mut bytes=Vec::new();value.to_writer_xml(&mut bytes).unwrap();
    seal(&f.app,&mut f.c.profiles[0].guards[0],&bytes);let r=read(&f);assert!(r.unavailable);assert!(r.plugins.is_empty());
}
#[test]
fn same_sized_guard_mutation_invalidates_a_warm_physical_cache(){
    let f=fixture("codebuddy-ide");let cache=Mutex::default();let mut result=PluginScan::default();observe_contract(&f.host,&f.c,&cache,&mut result).unwrap();
    let p=f.app.join("Contents/Resources/app/out/main.js");let mut b=fs::read(&p).unwrap();b[0]^=1;fs::write(&p,b).unwrap();
    let mut next=PluginScan::default();assert!(observe_contract(&f.host,&f.c,&cache,&mut next).is_err());assert!(next.plugins.is_empty());
}
#[test]
fn digest_cache_skips_rehash_only_after_matching_bound_identity(){
    let f=fixture("codebuddy-ide");let cache=Mutex::default();let p=f.c.profile(&f.host.agent_id).unwrap();let mut first=IdentityBudget::default();
    VerifiedApp::capture(&f.host,&f.c,p,&cache,&mut first).unwrap().unwrap();assert!(first.hashed_bytes>0);
    let mut second=IdentityBudget::default();VerifiedApp::capture(&f.host,&f.c,p,&cache,&mut second).unwrap().unwrap();assert_eq!(second.hashed_bytes,0);
}
#[test]
fn observer_never_consumes_another_hosts_shared_scan_allowance(){
    let f=fixture("codebuddy-ide");let mut r=PluginScan::default();r.budget.bytes=MAX_READ_BYTES;r.budget.verified_bytes=MAX_VERIFIED_BYTES;r.budget.visited=MAX_SCAN_ENTRIES;
    observe_contract(&f.host,&f.c,&Mutex::default(),&mut r).unwrap();assert_eq!(r.plugins.len(),111);assert_eq!(r.budget.bytes,MAX_READ_BYTES);assert_eq!(r.budget.verified_bytes,MAX_VERIFIED_BYTES);assert_eq!(r.budget.visited,MAX_SCAN_ENTRIES);
}
#[test]
fn registered_versions_remain_distinct_and_relative_location_has_priority(){
    let f=fixture("codebuddy-ide");ordinary(&f,"same-v1","same","1");ordinary(&f,"same-v2","same","2");
    let mut one=registration(&f,"same-v1","example.same","1");one["location"]=json!({"scheme":"https","path":"/must-not-fetch"});
    registry(&f,json!([one,registration(&f,"same-v2","example.same","2")]));let r=read(&f);
    assert!(!r.unavailable);let mut versions:Vec<_>=r.plugins.iter().filter(|p|p.plugin_id=="example.same"&&p.presence==PluginPresence::Installed).map(|p|p.version.as_str()).collect();versions.sort();assert_eq!(versions,vec!["1","2"]);
}
#[test]
fn ordinary_string_location_without_relative_path_is_provenance_only(){
    let f=fixture("codebuddy-ide");ordinary(&f,"ignored","ignored","1");registry(&f,json!([{"identifier":{"id":"example.ignored"},"version":"1","location":f.extensions.join("ignored")} ]));
    let r=read(&f);assert!(r.unavailable);let p=observed(&r,"example.ignored",PluginPresence::Declared);assert!(p.components.iter().all(|c|c.kind==PluginComponentKind::PluginReference));
}
#[test]
fn ordinary_manifest_must_match_registered_version(){
    let f=fixture("codebuddy-ide");ordinary(&f,"one","same","2");registry(&f,json!([registration(&f,"one","example.same","1")]));
    let r=read(&f);assert_eq!(observed(&r,"example.same",PluginPresence::Declared).version,"1");assert!(!r.plugins.iter().any(|p|p.plugin_id=="example.same"&&p.presence==PluginPresence::Installed));
}
#[test]
fn unregistered_ordinary_folders_are_not_installed_plugins(){
    let f=fixture("codebuddy-ide");ordinary(&f,"history","history","99");registry(&f,json!([]));assert!(!read(&f).plugins.iter().any(|p|p.plugin_id=="example.history"));
}
#[test]
fn native_false_membership_keeps_physical_plugin_but_effective_enablement_unknown(){
    let f=fixture("codebuddy-ide");let path=configured(&f,false);let r=read(&f);let p=observed(&r,"review@official",PluginPresence::Installed);
    assert_eq!(p.name,"review");assert_eq!(p.version,"2.1");assert!(p.enabled.is_none());assert!(p.revision.contains("user-configured-enabled:false"));
    assert!(p.revision.contains("stored-key"));assert!(p.source_paths.contains(&collapse_home(&path.join(".codebuddy-plugin/plugin.json"))));
}
#[test]
fn physical_catalog_without_membership_is_cache_only(){
    let f=fixture("codebuddy-ide");let root=marketplace(&f,"cache","official",json!([{"name":"review","source":"./plugins/review"}]));plugin(&root,"plugins/review",json!({"name":"review","version":"1"}));
    let r=read(&f);observed(&r,"review@official",PluginPresence::CacheOnly);assert!(!r.plugins.iter().any(|p|p.plugin_id=="review@official"&&p.presence==PluginPresence::Installed));
}
#[test]
fn forged_capabilities_cache_cannot_create_installed_or_body_owned_resources(){
    let f=fixture("codebuddy-ide");let root=marketplace(&f,"cache","official",json!([]));memberships(&f,json!({"forged@official":true}));
    json_file(&root.join(".plugins-cache.json"),json!({"version":1,"marketplaceName":"official","plugins":[{"name":"forged","installedPath":"/private/forged","skills":["/private/SECRET_SKILL.md"]}]}));
    let r=read(&f);let p=observed(&r,"forged@official",PluginPresence::Declared);assert!(p.components.iter().all(|c|c.kind!=PluginComponentKind::Skill));assert_eq!(skill_observations(&r.plugins).len(),15);
}
#[test]
fn remote_typed_source_only_uses_current_confined_destination(){
    let f=fixture("codebuddy-ide");let root=marketplace(&f,"stored","official",json!([{"name":"remote","source":{"source":"github","repo":"unused/private","url":"https://example.invalid/no-network"}}]));
    plugin(&root,"plugins/remote",json!({"name":"remote","version":"3"}));memberships(&f,json!({"remote@official":true}));let r=read(&f);
    assert_eq!(observed(&r,"remote@official",PluginPresence::Installed).version,"3");assert!(!serde_json::to_string(&r.plugins).unwrap().contains("unused/private"));
}
#[test]
fn external_local_plugin_source_is_a_reference_without_arbitrary_reads(){
    let f=fixture("codebuddy-ide");let external=f.home.home.join("private-original");put(&external.join(".codebuddy-plugin/plugin.json"),b"PRIVATE_INVALID_JSON");
    marketplace(&f,"stored","official",json!([{"name":"outside","source":{"source":"local","path":external}}]));memberships(&f,json!({"outside@official":true}));let r=read(&f);
    observed(&r,"outside@official",PluginPresence::Declared);assert!(r.unavailable);assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}
#[test]
fn unsupported_string_protocol_does_not_promote_an_unrelated_physical_cache(){
    let f=fixture("codebuddy-ide");let root=marketplace(&f,"stored","official",json!([{"name":"fake","source":"file:///private/source"}]));plugin(&root,"plugins/fake",json!({"name":"fake","version":"9"}));memberships(&f,json!({"fake@official":true}));
    let r=read(&f);observed(&r,"fake@official",PluginPresence::Declared);assert!(!r.plugins.iter().any(|p|p.plugin_id=="fake@official"&&p.presence==PluginPresence::Installed));
}
#[test]
fn external_marketplace_install_location_does_not_get_opened(){
    let f=fixture("codebuddy-ide");let external=f.home.home.join("private-market");put(&external.join(".codebuddy-plugin/marketplace.json"),b"PRIVATE_INVALID_JSON");
    json_file(&f.native.join("plugins/known_marketplaces.json"),json!({"outside":{"source":{"source":"directory","path":external},"installLocation":external}}));memberships(&f,json!({"review@outside":true}));
    let r=read(&f);observed(&r,"review@outside",PluginPresence::Declared);assert!(r.unavailable);
}
#[test]
fn native_manifest_precedence_does_not_mix_claude_and_codebuddy_metadata(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);json_file(&path.join(".claude-plugin/plugin.json"),json!({"name":"different","version":"99","mcpServers":{"never":{"command":"PRIVATE_COMMAND"}}}));
    let r=read(&f);assert_eq!(observed(&r,"review@official",PluginPresence::Installed).version,"2.1");assert!(!r.plugins.iter().any(|p|p.name=="different"));assert!(r.mcps.is_empty());
}
#[test]
fn malformed_preferred_manifest_does_not_fall_back_to_claude(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);put(&path.join(".codebuddy-plugin/plugin.json"),b"{");json_file(&path.join(".claude-plugin/plugin.json"),json!({"name":"review","version":"99"}));
    let r=read(&f);observed(&r,"review@official",PluginPresence::Declared);assert!(!r.plugins.iter().any(|p|p.version=="99"));assert!(r.unavailable);
}
#[test]
fn plugin_skill_titles_use_directories_without_frontmatter_or_body_reads(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);put(&path.join("node_modules/dependency/SKILL.md"),b"PRIVATE_DEPENDENCY");let r=read(&f);let p=observed(&r,"review@official",PluginPresence::Installed);
    let skills:Vec<_>=p.components.iter().filter(|c|c.kind==PluginComponentKind::Skill).collect();assert_eq!(skills.len(),1);assert_eq!(skills[0].name,"review");assert!(skills[0].native_id.is_none());assert_eq!(skill_observations(&r.plugins).len(),16);
}
#[test]
fn native_skill_fallback_stays_within_manifest_plugin_roots(){
    let f=fixture("codebuddy-ide");let root=marketplace(&f,"stored","official",json!([{"name":"review","source":"./plugins/review"}]));let path=plugin(&root,"plugins/review",json!({"name":"review"}));
    put(&path.join("skills/fallback/skill.md"),b"PRIVATE_FRONTMATTER_IS_NOT_PARSED");put(&f.native.join("skills/global-only/SKILL.md"),b"PRIVATE_GLOBAL_SKILL");memberships(&f,json!({"review@official":true}));
    let r=read(&f);let p=observed(&r,"review@official",PluginPresence::Installed);assert_eq!(p.version,"");assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::Skill&&c.name=="fallback"));assert!(!p.components.iter().any(|c|c.name=="global-only"));
}
#[test]
fn file_component_titles_keep_filenames_and_no_parsed_native_ids(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);json_file(&path.join(".codebuddy-plugin/plugin.json"),json!({"name":"review","commands":["commands/check.md"],"agents":"agents/reviewer.md","rules":["rules/style.mdc"]}));
    for rel in ["commands/check.md","agents/reviewer.md","rules/style.mdc"]{put(&path.join(rel),b"\0PRIVATE_COMPONENT_BODY\xff");}
    let r=read(&f);let p=observed(&r,"review@official",PluginPresence::Installed);for title in ["check.md","reviewer.md","style.mdc"]{let c=p.components.iter().find(|c|c.name==title).unwrap();assert!(c.native_id.is_none());}
}
#[test]
fn mcp_inline_and_file_declarations_are_references_without_values_or_host_servers(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);json_file(&path.join(".codebuddy-plugin/plugin.json"),json!({"name":"review","mcpServers":["servers.json"]}));
    json_file(&path.join("servers.json"),json!({"mcpServers":{"listed":{"type":"stdio","command":"PRIVATE_COMMAND","env":{"SECRET":"PRIVATE_TOKEN"}}}}));json_file(&path.join("mcp/extra.json"),json!({"second":{"url":"https://example.invalid/PRIVATE_URL","type":"sse"}}));
    let r=read(&f);let p=observed(&r,"review@official",PluginPresence::Installed);assert_eq!(p.components.iter().filter(|c|c.kind==PluginComponentKind::McpReference).count(),2);assert!(r.mcps.is_empty());assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}
#[test]
fn mcp_inline_precedence_does_not_open_fallback_config_or_env_file(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);json_file(&path.join(".codebuddy-plugin/plugin.json"),json!({"name":"review","mcpServers":{"inline":{"command":"unused","envFile":"credentials.json"}}}));
    put(&path.join(".mcp.json"),b"PRIVATE_INVALID_JSON");put(&path.join("credentials.json"),b"PRIVATE_ACCOUNT_TOKEN");let r=read(&f);
    let p=observed(&r,"review@official",PluginPresence::Installed);assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="inline"));assert!(r.mcps.is_empty());
}
#[test]
fn mcp_path_override_cannot_open_credential_store_or_skill_body(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);json_file(&path.join(".codebuddy-plugin/plugin.json"),json!({"name":"review","mcpServers":["credentials.json","SKILL.md"]}));
    put(&path.join("credentials.json"),b"PRIVATE_INVALID_CREDENTIAL_STORE");put(&path.join("SKILL.md"),b"PRIVATE_INVALID_SKILL_BODY");let r=read(&f);
    let p=observed(&r,"review@official",PluginPresence::Installed);assert_eq!(p.components.iter().filter(|c|c.kind==PluginComponentKind::McpReference&&c.native_id.is_none()).count(),2);assert!(r.mcps.is_empty());assert!(r.unavailable);
}
#[test]
fn native_false_declarations_keep_the_proved_default_skill_and_mcp_sources(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);json_file(&path.join(".codebuddy-plugin/plugin.json"),json!({"name":"review","skills":false,"mcpServers":false,"lspServers":{}}));
    json_file(&path.join(".mcp.json"),json!({"mcpServers":{"default-source":{"command":"unused"}}}));let r=read(&f);let p=observed(&r,"review@official",PluginPresence::Installed);
    assert!(!r.unavailable);assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::Skill&&c.name=="review"));assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="default-source"));
}
#[test]
fn malformed_mcp_file_does_not_hide_other_current_plugin_declarations(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);json_file(&path.join(".codebuddy-plugin/plugin.json"),json!({"name":"review","mcpServers":["broken.json","valid.json"]}));
    put(&path.join("broken.json"),b"{");json_file(&path.join("valid.json"),json!({"mcpServers":{"visible":{"command":"unused"}}}));let r=read(&f);let p=observed(&r,"review@official",PluginPresence::Installed);
    assert!(r.unavailable);assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="visible"));assert!(r.mcps.is_empty());
}
#[test]
fn oversized_native_metadata_keeps_independent_bundled_rows(){
    let f=fixture("codebuddy-ide");put(&f.native.join("plugins/known_marketplaces.json"),&vec![b' ';MAX_DOCUMENT_BYTES+1]);let r=read(&f);assert!(r.unavailable);assert_eq!(r.plugins.iter().filter(|p|p.presence==PluginPresence::Bundled).count(),102);
}
#[test]
fn oversized_component_array_is_bounded_before_path_resolution(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);json_file(&path.join(".codebuddy-plugin/plugin.json"),json!({"name":"review","skills":vec!["not-a-real-component";513]}));
    let r=read(&f);assert!(r.unavailable);assert_eq!(skill_observations(&r.plugins).len(),15);assert_eq!(r.plugins.iter().filter(|p|p.presence==PluginPresence::Bundled).count(),102);
}
#[test]
fn missing_native_files_do_not_trigger_initialization_or_migration(){
    let f=fixture("codebuddy-ide");let r=read(&f);assert!(!r.unavailable);assert!(!f.native.exists());assert!(!f.extensions.exists());
}
#[cfg(unix)]
#[test]
fn symlinked_skill_body_is_rejected_without_losing_other_application_resources(){
    let f=fixture("codebuddy-ide");let path=f.app.join("Contents/Resources/app/extensions/genie/out/extension/builtin/skill-creator/SKILL.md");fs::remove_file(&path).unwrap();symlink(f.home.home.join("private-body"),&path).unwrap();
    let r=read(&f);assert!(r.unavailable);assert_eq!(skill_observations(&r.plugins).len(),14);assert_eq!(r.plugins.iter().filter(|p|p.presence==PluginPresence::Declared).count(),9);
}
#[cfg(unix)]
#[test]
fn symlinked_plugin_root_cannot_escape_known_marketplace(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);let external=f.home.home.join("outside-plugin");fs::rename(&path,&external).unwrap();symlink(&external,&path).unwrap();let r=read(&f);
    observed(&r,"review@official",PluginPresence::Declared);assert!(r.unavailable);assert_eq!(skill_observations(&r.plugins).len(),15);
}
#[test]
fn ordinary_override_does_not_reinterpret_the_native_default_namespace(){
    let f=fixture("codebuddy-ide");configured(&f,true);std::env::set_var("VSCODE_EXTENSIONS",f.home.home.join("external-extension-override"));let r=read(&f);
    assert!(r.unavailable);observed(&r,"review@official",PluginPresence::Installed);assert_eq!(r.plugins.iter().filter(|p|p.presence==PluginPresence::Bundled).count(),102);
}
#[test]
fn watch_set_contains_fixed_registries_and_guards_but_no_component_body_read_or_remote_path(){
    let f=fixture("codebuddy-ide");let path=configured(&f,true);let paths=watch_paths_contract(&f.host,&f.c,&Mutex::default());
    assert!(paths.contains(&f.extensions.join("extensions.json")));assert!(paths.contains(&f.native.join("plugins/known_marketplaces.json")));assert!(paths.contains(&f.native.join("settings.json")));assert!(paths.contains(&path.join(".codebuddy-plugin/plugin.json")));assert!(paths.contains(&f.app.join("Contents/Resources/app/out/main.js")));
    assert!(paths.iter().all(|p|p.starts_with(&f.home.home)));
}
#[test]
fn registry_mutation_after_capture_rejects_final_snapshot(){
    let f=fixture("codebuddy-ide");configured(&f,true);let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();memberships(&f,json!({"review@official":false}));assert!(audit.revalidate(&f.host).is_err());
}
#[test]
fn selected_app_move_after_capture_rejects_final_snapshot(){
    let f=fixture("codebuddy-ide");let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();fs::rename(&f.app,f.home.home.join("Moved.app")).unwrap();assert!(audit.revalidate(&f.host).is_err());
}
#[test]
fn descriptor_rejects_unknown_fields_and_unbounded_contract_shape(){
    let mut value:Value=serde_json::from_str(include_str!("../../../../../data/native-plugin-codebuddy-desktop.json")).unwrap();value["unexpected"]=json!(true);assert!(serde_json::from_value::<Contract>(value).is_err());
    let mut f=fixture("codebuddy-ide");let duplicate=f.c.profiles[0].guards[0].clone();f.c.profiles[0].guards.push(duplicate);assert!(f.c.validate().is_err());
}

#[test]
fn hardlinked_public_guard_cannot_reuse_a_warm_digest_cache(){
    let f=fixture("codebuddy-ide");let cache=Mutex::default();let profile=f.c.profile(&f.host.agent_id).unwrap();
    VerifiedApp::capture(&f.host,&f.c,profile,&cache,&mut IdentityBudget::default()).unwrap().unwrap();
    fs::hard_link(f.app.join("Contents/Resources/app/out/main.js"),f.home.home.join("other-public-link.js")).unwrap();
    assert!(VerifiedApp::capture(&f.host,&f.c,profile,&cache,&mut IdentityBudget::default()).is_err());
}

#[test]
fn hardlinked_registry_is_not_opened_as_plugin_metadata(){
    let f=fixture("codebuddy-ide");configured(&f,true);let path=f.native.join("plugins/known_marketplaces.json");
    fs::hard_link(&path,f.home.home.join("other-registry-link.json")).unwrap();
    let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();assert!(audit.snapshot.incomplete);
    assert!(audit.snapshot.bound.iter().all(|file|file.path!=path));
    assert!(!audit.snapshot.plugins.iter().any(|p|p.plugin_id=="review@official"&&p.presence==PluginPresence::Installed));
    assert_eq!(audit.snapshot.plugins.iter().filter(|p|p.presence==PluginPresence::Bundled).count(),102);
}

#[test]
fn stat_only_skill_link_count_is_retained_and_revalidated_without_body_reads(){
    let f=fixture("codebuddy-ide");let path=f.app.join("Contents/Resources/app/extensions/genie/out/extension/builtin/skill-creator/SKILL.md");
    let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();
    fs::hard_link(&path,f.home.home.join("other-skill-link.md")).unwrap();assert!(audit.revalidate(&f.host).is_err());
    let current=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();
    let leaf=current.snapshot.leaves.iter().find(|leaf|leaf.path==path).unwrap();assert_eq!(leaf.stamp.links,2);
    assert!(current.snapshot.bound.iter().all(|file|file.path!=path));
    assert_eq!(skill_observations(&current.snapshot.plugins).len(),15);
}

#[test]
fn hook_body_overrides_remain_opaque_while_independent_mcp_metadata_survives(){
    let f=fixture("codebuddy-ide");let root=configured(&f,true);
    let cases=[
        ("hooks/hooks.json",None),
        ("custom-string.json",Some(json!("custom-string.json"))),
        ("custom-array.json",Some(json!([{"path":"custom-array.json"}]))),
        ("custom-object.json",Some(json!({"file":"custom-object.json"}))),
    ];
    for(relative,hooks)in cases{
        let mut manifest=json!({"name":"review","mcpServers":[relative,"visible.json"]});
        if let Some(hooks)=hooks{manifest["hooks"]=hooks;}
        json_file(&root.join(".codebuddy-plugin/plugin.json"),manifest);
        json_file(&root.join(relative),json!({"mcpServers":{"PRIVATE_HOOK_SERVER":{"command":"PRIVATE_HOOK_BODY"}}}));
        json_file(&root.join("visible.json"),json!({"mcpServers":{"visible":{"command":"unused"}}}));
        let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();assert!(audit.snapshot.incomplete);
        assert!(audit.snapshot.bound.iter().all(|file|file.path!=root.join(relative)));
        let p=audit.snapshot.plugins.iter().find(|p|p.plugin_id=="review@official"&&p.presence==PluginPresence::Installed).unwrap();
        assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="visible"));
        assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.source_path==collapse_home(&root.join(relative))&&c.native_id.is_none()));
        assert!(!serde_json::to_string(&audit.snapshot.plugins).unwrap().contains("PRIVATE_HOOK"));
    }
}

#[test]
fn sensitive_directory_and_auth_file_overrides_are_filename_references_only(){
    let f=fixture("codebuddy-ide");let root=configured(&f,true);
    let files=["oauth/session.json","cookies/data.json","auth.json","hooks/custom.json"];
    json_file(&root.join(".codebuddy-plugin/plugin.json"),json!({"name":"review","mcpServers":files}));
    for relative in files{json_file(&root.join(relative),json!({"mcpServers":{"PRIVATE_ACCOUNT":{"token":"PRIVATE_PAYLOAD"}}}));}
    json_file(&root.join("mcp/visible.json"),json!({"mcpServers":{"visible":{"command":"unused"}}}));
    let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();assert!(audit.snapshot.incomplete);
    for relative in files{assert!(audit.snapshot.bound.iter().all(|file|file.path!=root.join(relative)));}
    let p=audit.snapshot.plugins.iter().find(|p|p.plugin_id=="review@official"&&p.presence==PluginPresence::Installed).unwrap();
    assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="visible"));
    assert!(!serde_json::to_string(&audit.snapshot.plugins).unwrap().contains("PRIVATE_"));
}

fn set_custom_launch(f:&Fixture){
    crate::settings::mutate_settings_checked(|settings|{
        settings.agent_launch.get_or_insert_default().entry(f.host.agent_id.clone()).or_default().target=Some(
            crate::domain::agents::LaunchTarget::App{path:f.app.to_string_lossy().into_owned(),args:Vec::new(),new_instance:false,env:BTreeMap::new()});
        Ok(())
    }).unwrap();
}

#[test]
fn default_launch_selection_skips_a_wrong_bundle_before_the_allowed_user_app(){
    let mut f=fixture("codebuddy-ide");let path=crate::paths::system_probe_path("/Applications/CodeBuddy.app");
    let mut info=plist::Dictionary::new();info.insert("CFBundleIdentifier".into(),plist::Value::String("example.unrelated".into()));
    let mut bytes=Vec::new();plist::Value::Dictionary(info).to_writer_xml(&mut bytes).unwrap();put(&path.join("Contents/Info.plist"),&bytes);
    f.host.roots.insert(0,Root{path:"/Applications/CodeBuddy.app".into(),layout:"codebuddy_app".into(),presence:PluginPresence::Bundled,complete_marker:None});
    let result=read(&f);assert!(!result.unavailable);assert_eq!(result.plugins.len(),111);
    assert_eq!(select_app(&f.host).unwrap(),Some(f.app.clone()));
}

#[test]
fn a_default_launcher_outside_host_roots_cannot_fall_back_to_an_allowed_app(){
    let f=fixture("codebuddy-ide");let other=crate::paths::system_probe_path("/Applications/CodeBuddy.app");
    put(&other.join("Contents/Info.plist"),&fs::read(f.app.join("Contents/Info.plist")).unwrap());
    let result=read(&f);assert!(result.unavailable);assert!(result.plugins.is_empty());
}

#[test]
fn configured_app_preferences_do_not_inherit_default_native_observation(){
    let f=fixture("codebuddy-ide");set_custom_launch(&f);let result=read(&f);
    assert!(result.unavailable);assert!(result.plugins.is_empty());
}

#[test]
fn a_launch_preference_change_invalidates_the_final_selected_app_snapshot(){
    let f=fixture("codebuddy-ide");let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();set_custom_launch(&f);
    assert!(audit.revalidate(&f.host).is_err());
}

#[test]
fn wecom_request_gated_tool_is_an_app_owned_declaration_without_host_mcp(){
    let f=fixture("codebuddy-ide");let result=read(&f);let p=observed(&result,"@builtin/wecom-skills",PluginPresence::Bundled);
    let tool=p.components.iter().find(|c|c.kind==PluginComponentKind::Tool).unwrap();
    assert_eq!(tool.name,"wecom_mcp");assert_eq!(tool.source_path,collapse_home(&f.app.join("Contents/Resources/app/extensions/genie/out/extension/index.js")));
    assert!(tool.native_id.is_none());assert!(p.enabled.is_none());assert!(p.revision.contains("isWecomAiBot-request-unknown"));
    assert_eq!(p.components.iter().filter(|c|c.kind==PluginComponentKind::Skill).count(),12);assert!(result.mcps.is_empty());
}
#[test]
fn credential_urls_cannot_become_component_source_paths() {
    assert!(clean_relative(Path::new("/isolated-plugin"),"https://PRIVATE_USER:PRIVATE_PASSWORD@example.test/oauth.json").is_err());
}
#[test]
fn native_empty_mcp_fields_and_bad_paths_do_not_hide_independent_sources() {
    let home=TestHome::new("codebuddy-mcp-falsy-sources");let root=home.home.join("plugin");fs::create_dir_all(root.join("mcp")).unwrap();
    fs::write(root.join(".mcp.json"),br#"{"mcpServers":{"fallback":{"command":"not-run"}}}"#).unwrap();
    fs::write(root.join("mcp/independent.json"),br#"{"mcpServers":{"independent":{"command":"not-run"}}}"#).unwrap();
    for field in [json!(""),Value::Null,json!(false),json!(0),json!([]),json!({})] {
        let mut snapshot=Snapshot::default();let mut budget=MetadataBudget::default();let body=BTreeSet::new();let mut items=Vec::new();
        plugin_mcps(&root,&json!({"mcpServers":field}),&root.join("plugin.json"),Some(&body),"market","plugin",&mut items,&mut snapshot,&mut budget).unwrap();
        assert!(items.iter().any(|c|c.name=="fallback"));assert!(items.iter().any(|c|c.name=="independent"));
    }
    let mut snapshot=Snapshot::default();let mut budget=MetadataBudget::default();let body=BTreeSet::new();let mut items=Vec::new();
    plugin_mcps(&root,&json!({"mcpServers":["../outside.json",17,"mcp/independent.json"]}),&root.join("plugin.json"),Some(&body),"market","plugin",&mut items,&mut snapshot,&mut budget).unwrap();
    assert!(snapshot.incomplete);assert!(items.iter().any(|c|c.name=="independent"));
}
