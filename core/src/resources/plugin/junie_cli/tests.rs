//! Isolated regression source; deliberately not executed in this task.
use super::*;
use crate::testenv::TestHome;
use serde_json::json;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};

struct ExtraEnv(Vec<(&'static str,Option<std::ffi::OsString>)>);
impl ExtraEnv {
    fn clear() -> Self {Self(ENV_GATES.iter().map(|&key|{let old=std::env::var_os(key);std::env::set_var(key,"");(key,old)}).collect())}
}
impl Drop for ExtraEnv {
    fn drop(&mut self) {for(key,value)in &self.0 {match value {Some(value)=>std::env::set_var(key,value),None=>std::env::remove_var(key)}}}
}
struct Fixture { _env:ExtraEnv, home:TestHome, c:Contract, host:Host, alias:PathBuf, data:PathBuf, version:PathBuf, extensions:PathBuf }
fn put(path:&Path,bytes:&[u8]) {fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,bytes).unwrap();}
fn seal(root:&Path,guard:&mut Guard,bytes:&[u8]) {put(&root.join(&guard.path),bytes);guard.bytes=bytes.len()as u64;guard.sha256=hex::encode(Sha256::digest(bytes));}
fn fixture()->Fixture {
    let home=TestHome::new("junie-native");let env=ExtraEnv::clear();
    let mut c:Contract=serde_json::from_str(include_str!("../../../../../data/native-plugin-junie-cli.json")).unwrap();
    let data=home.home.join(".local/share/junie");let version=data.join("versions").join(&c.version);let alias=home.home.join(".local/bin/junie");
    let shim=b"#!/bin/sh\n# INERT PUBLIC SHIM FIXTURE - NEVER EXECUTED\n";put(&alias,shim);c.shim.bytes=shim.len()as u64;c.shim.sha256=hex::encode(Sha256::digest(shim));
    #[cfg(unix)] fs::set_permissions(&alias,fs::Permissions::from_mode(0o755)).unwrap();
    let mut info=plist::Dictionary::new();
    for(k,v)in[("CFBundleIdentifier",&c.bundle_id),("CFBundleName",&c.bundle_name),("CFBundleExecutable",&c.bundle_name),("CFBundleVersion",&c.version),("CFBundleShortVersionString",&c.version)]{info.insert(k.into(),plist::Value::String(v.clone()));}
    let mut info_bytes=Vec::new();plist::Value::Dictionary(info).to_writer_xml(&mut info_bytes).unwrap();
    for guard in &mut c.guards {
        let bytes=if guard.path.ends_with("Info.plist") {info_bytes.clone()}
            else if guard.path.ends_with("junie.cfg") {format!("[Application]\napp.classpath=$APPDIR/junie-release-{}-macos-aarch64.jar\napp.mainclass={}\n[JavaOptions]\njava-options=-Djpackage.app-version={}\n",c.version,c.main_class,c.version).into_bytes()}
            else if guard.path=="channel" {b"release\n".to_vec()}
            else {format!("INERT_PUBLIC_MEMBER:{}",guard.path).into_bytes()};
        seal(&version,guard,&bytes);
    }
    // Only trusted static metadata is projected; the fixture JAR is never
    // opened as an archive or asked to contain/read a Skill resource body.
    c.zip.entries=2;c.zip.central_offset=0;c.zip.central_bytes=0;c.zip.eocd_offset=0;
    for skill in &mut c.skills {skill.header_offset=0;skill.compressed_bytes=1;}
    #[cfg(unix)] symlink(&version,data.join("current")).unwrap();
    let host=Host {agent_id:"junie-cli".into(),adapter:"junie_cli".into(),docs_url:"https://junie.jetbrains.com/docs/".into(),manifest_paths:vec![],default_mcp_files:vec![],mcp_id_prefix:String::new(),resource_groups:vec![],roots:vec![Root{path:"~/.local/bin/junie".into(),layout:"junie_shim".into(),presence:PluginPresence::Installed,complete_marker:None}]};
    let extensions=home.home.join(".junie/extensions");Fixture{_env:env,home,c,host,alias,data,version,extensions}
}
fn read(f:&Fixture)->PluginScan {let mut r=PluginScan::default();if observe_contract(&f.host,&f.c,&Mutex::default(),&mut r).is_err(){r.unavailable=true;}normalize(&mut r);r}
fn registry(f:&Fixture,value:Value){put(&f.extensions.join("extensions.json"),&serde_json::to_vec(&value).unwrap());}
fn cached(f:&Fixture,market:&str,name:&str)->PathBuf {let root=f.extensions.join(market).join(name);fs::create_dir_all(&root).unwrap();root}
fn named<'a>(r:&'a PluginScan,id:&str)->&'a NativePluginObservation {r.plugins.iter().find(|p|p.plugin_id==id).unwrap()}

#[test]
fn known_jars_project_two_application_owned_skills_without_archive_or_body_reads() {
    let f=fixture();let r=read(&f);assert!(!r.unavailable);assert_eq!(r.plugins.len(),1);assert!(r.mcps.is_empty());
    let p=named(&r,"@builtin/skills");assert_eq!(p.presence,PluginPresence::Bundled);assert!(p.enabled.is_none());
    assert_eq!(p.components.iter().map(|v|v.name.as_str()).collect::<BTreeSet<_>>(),BTreeSet::from(["demo-setup","session-history"]));
    assert!(p.components.iter().all(|v|v.kind==PluginComponentKind::Skill&&v.native_id.is_none()&&v.source_path.contains(".jar!/agent-skills/")));
    assert_eq!(skill_observations(&r.plugins).len(),2);assert!(!f.home.home.join(".junie").exists());
}
#[test]
fn passive_version_reads_only_verified_public_selection_and_no_user_configuration() {
    let f=fixture();put(&f.extensions.join("extensions.json"),b"MALFORMED_PRIVATE_REGISTRY");
    assert_eq!(version_contract(&f.alias,&f.c,&Mutex::default()),Some("3893.6".into()));
    assert!(read(&f).unavailable);
}
#[test]
fn current_selects_one_version_without_searching_highest_cached_build() {
    let f=fixture();put(&f.data.join("versions/99999.1/junie"),b"UNSELECTED_PRIVATE_BODY");
    assert_eq!(version_contract(&f.alias,&f.c,&Mutex::default()),Some("3893.6".into()));
    fs::remove_file(f.data.join("current")).unwrap();assert!(version_contract(&f.alias,&f.c,&Mutex::default()).is_none());
}
#[test]
fn unknown_runtime_or_environment_cannot_trigger_a_version_fallback() {
    let f=fixture();put(&f.version.join(&f.c.guards.iter().find(|g|g.path.ends_with(".jar")).unwrap().path),b"CHANGED_JAR");
    assert!(version_contract(&f.alias,&f.c,&Mutex::default()).is_none());assert!(read(&f).plugins.is_empty());
    std::env::set_var("JUNIE_VERSION","9999.1");assert!(version_contract(&f.alias,&f.c,&Mutex::default()).is_none());
}
#[test]
fn earlier_unknown_path_entry_is_not_replaced_with_the_known_alias() {
    let f=fixture();let earlier=f.home.home.join("bin/junie");put(&earlier,b"INERT_OTHER_BINARY");
    #[cfg(unix)] fs::set_permissions(&earlier,fs::Permissions::from_mode(0o755)).unwrap();
    assert!(version_contract(&f.alias,&f.c,&Mutex::default()).is_none());assert!(read(&f).plugins.is_empty());
}
#[test]
fn flat_marketplace_map_and_qualified_case_sensitive_ids_follow_the_native_loader() {
    let f=fixture();registry(&f,json!({"Public":{"source":{"type":"git","url":"https://example.invalid/repo"},"extensions":["One","one"]}}));
    cached(&f,"Public","One");cached(&f,"Public","one");let r=read(&f);assert!(!r.unavailable);assert_eq!(r.plugins.len(),3);
    assert_eq!(named(&r,"One@Public").presence,PluginPresence::Installed);assert_eq!(named(&r,"one@Public").presence,PluginPresence::Installed);
    assert!(named(&r,"One@Public").version.is_empty());assert!(r.plugins.iter().all(|p|p.enabled.is_none()));
}
#[test]
fn optional_source_does_not_remove_listed_physical_candidates() {
    let f=fixture();registry(&f,json!({"A":{"extensions":["native"]},"B":{"source":null,"extensions":["native"]}}));
    cached(&f,"A","native");cached(&f,"B","native");let r=read(&f);assert!(!r.unavailable);
    assert_eq!(named(&r,"native@A").presence,PluginPresence::Installed);assert_eq!(named(&r,"native@B").presence,PluginPresence::Installed);
}
#[test]
fn cache_without_membership_is_not_installed_and_missing_cache_stays_declared() {
    let f=fixture();registry(&f,json!({"market":{"extensions":["missing"]}}));cached(&f,"market","history");
    let r=read(&f);assert!(r.unavailable);assert_eq!(named(&r,"missing@market").presence,PluginPresence::Declared);
    assert!(!r.plugins.iter().any(|p|p.plugin_id=="history@market"));
}
#[test]
fn wrapper_and_bad_typed_fields_do_not_become_partial_valid_installations() {
    let f=fixture();cached(&f,"good","native");registry(&f,json!({"good":{"extensions":["native"]},"bad":{"extensions":[true]}}));
    let r=read(&f);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);
    registry(&f,json!({"marketplaces":{"good":{"extensions":["native"]}}}));let r=read(&f);assert_eq!(r.plugins.len(),1);
    assert!(!r.plugins.iter().any(|p|p.plugin_id=="native@good"));
}
#[test]
fn typed_source_paths_are_provenance_only_and_never_followed() {
    let f=fixture();registry(&f,json!({"M":{"source":{"type":"local","path":"/PRIVATE_OUTSIDE"},"extensions":["native"]}}));
    cached(&f,"M","native");let r=read(&f);assert!(!r.unavailable);assert_eq!(named(&r,"native@M").presence,PluginPresence::Installed);
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_OUTSIDE"));
}
#[test]
fn project_registry_and_unrelated_global_skill_roots_are_never_scanned_as_plugins() {
    let f=fixture();put(&f.home.home.join("project/.junie/extensions.json"),b"PRIVATE_PROJECT");
    put(&f.home.home.join(".agents/skills/private/SKILL.md"),b"PRIVATE_BODY");let r=read(&f);assert!(!r.unavailable);assert_eq!(r.plugins.len(),1);
}
#[test]
fn owned_skill_markers_and_markdown_files_expose_filename_availability_only() {
    let f=fixture();registry(&f,json!({"M":{"extensions":["native"]}}));let root=cached(&f,"M","native");
    for file in ["skills/path-label/SKILL.md","commands/review.md","agents/helper.md","guidelines/style.md"]{put(&root.join(file),b"\xffPRIVATE_FRONTMATTER_AND_BODY");}
    let r=read(&f);assert!(!r.unavailable);let p=named(&r,"native@M");assert_eq!(p.components.iter().filter(|c|c.kind==PluginComponentKind::Skill).count(),1);
    assert!(p.components.iter().all(|c|c.native_id.is_none()));assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}
#[test]
fn plugin_json_does_not_supply_native_ownership_version_or_component_overrides() {
    let f=fixture();registry(&f,json!({"M":{"extensions":["native"]}}));let root=cached(&f,"M","native");
    put(&root.join(".plugin/plugin.json"),br#"{"version":"999","skills":"/PRIVATE_OUTSIDE"}"#);let r=read(&f);assert!(!r.unavailable);
    let p=named(&r,"native@M");assert!(p.version.is_empty());assert!(p.components.is_empty());
}
#[test]
fn mcp_fields_remain_names_only_references_without_connection_or_secret_output() {
    let f=fixture();registry(&f,json!({"M":{"extensions":["native"]}}));let root=cached(&f,"M","native");
    put(&root.join("mcp/a.json"),br#"{/*native*/"mcpServers":{"same":{"command":"PRIVATE_COMMAND","env":{"TOKEN":"PRIVATE_SECRET"}},"remote":{"url":"https://example.invalid/PRIVATE_URL"}},}"#);
    put(&root.join(".mcp.json"),br#"{"same":{"url":"https://example.invalid"},"disabled":{"command":"unused","enabled":false}}"#);
    let r=read(&f);assert!(!r.unavailable);assert!(r.mcps.is_empty());let p=named(&r,"native@M");
    assert_eq!(p.components.iter().filter(|v|v.kind==PluginComponentKind::McpReference).count(),4);
    assert_eq!(p.components.iter().filter(|v|v.kind==PluginComponentKind::McpReference&&v.name=="same").count(),2);
    assert!(p.components.iter().all(|v|v.kind!=PluginComponentKind::Mcp));assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}
#[test]
fn hooks_are_only_file_references_without_json_payload_or_helper_calls() {
    let f=fixture();registry(&f,json!({"M":{"extensions":["native"]}}));let root=cached(&f,"M","native");
    put(&root.join("hooks/hooks.json"),b"NOT_JSON_PRIVATE_HOOK_BODY");let r=read(&f);assert!(!r.unavailable);
    assert!(named(&r,"native@M").components.iter().any(|c|c.kind==PluginComponentKind::TypeDeclaration));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}
#[cfg(unix)]
#[test]
fn unsafe_cache_or_registry_symlinks_do_not_read_outside_or_drop_bundled_availability() {
    let f=fixture();registry(&f,json!({"M":{"extensions":["native"]}}));let outside=f.home.home.join("outside");fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(f.extensions.join("M")).unwrap();symlink(&outside,f.extensions.join("M/native")).unwrap();
    let r=read(&f);assert!(r.unavailable);assert_eq!(named(&r,"native@M").presence,PluginPresence::Declared);assert_eq!(named(&r,"@builtin/skills").components.len(),2);
}
#[cfg(unix)]
#[test]
fn untrusted_current_link_never_follows_an_outside_installation() {
    let f=fixture();fs::remove_file(f.data.join("current")).unwrap();symlink("/PRIVATE_OUTSIDE/3893.6",f.data.join("current")).unwrap();
    assert!(version_contract(&f.alias,&f.c,&Mutex::default()).is_none());assert!(read(&f).plugins.is_empty());
}
#[cfg(unix)]
#[test]
fn unreadable_skill_bodies_are_still_stat_only_candidates() {
    let f=fixture();registry(&f,json!({"M":{"extensions":["native"]}}));let root=cached(&f,"M","native");let leaf=root.join("skills/unreadable/SKILL.md");put(&leaf,b"PRIVATE_BODY");
    fs::set_permissions(&leaf,fs::Permissions::from_mode(0)).unwrap();let r=read(&f);assert!(!r.unavailable);assert!(named(&r,"native@M").components.iter().any(|v|v.kind==PluginComponentKind::Skill));
}
#[test]
fn identity_hashing_has_an_independent_budget_and_cached_fd_stamps() {
    let f=fixture();let cache=Mutex::default();let mut b=Budget::default();Verified::capture(&f.alias,&f.c,&cache,&mut b).unwrap();assert!(b.hashed_bytes>0);
    let mut second=Budget::default();Verified::capture(&f.alias,&f.c,&cache,&mut second).unwrap();assert_eq!(second.hashed_bytes,0);
    let mut r=PluginScan::default();r.budget.bytes=MAX_READ_BYTES;r.budget.verified_bytes=MAX_VERIFIED_BYTES;r.budget.visited=MAX_SCAN_ENTRIES;
    observe_contract(&f.host,&f.c,&cache,&mut r).unwrap();assert_eq!(r.plugins.len(),1);assert_eq!(r.budget.bytes,MAX_READ_BYTES);assert_eq!(r.budget.verified_bytes,MAX_VERIFIED_BYTES);assert_eq!(r.budget.visited,MAX_SCAN_ENTRIES);
}
#[test]
fn changing_selected_current_or_metadata_fd_invalidates_final_publication() {
    let f=fixture();registry(&f,json!({"M":{"extensions":["native"]}}));cached(&f,"M","native");let proof=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();
    let file=f.extensions.join("extensions.json");let bytes=fs::read(&file).unwrap();fs::rename(&file,f.extensions.join("old.json")).unwrap();put(&file,&bytes);assert!(proof.revalidate().is_err());
    let proof=Verified::capture(&f.alias,&f.c,&Mutex::default(),&mut Budget::default()).unwrap();fs::remove_file(f.data.join("current")).unwrap();assert!(proof.revalidate().is_err());
}
#[test]
fn finite_metadata_limits_and_unsafe_ids_fail_closed_without_body_reads() {
    let f=fixture();registry(&f,json!({"M":{"extensions":["../escape"]}}));let r=read(&f);assert!(r.unavailable);assert!(r.plugins.iter().all(|p|p.components.iter().all(|c|!c.source_path.contains("escape"))));
    put(&f.extensions.join("extensions.json"),&vec![b'x';MAX_DOCUMENT_BYTES+1]);let r=read(&f);assert!(r.unavailable);assert_eq!(r.plugins.len(),1);
}
#[test]
fn watches_include_fixed_selected_and_metadata_paths_but_no_project_or_source_targets() {
    let f=fixture();registry(&f,json!({"M":{"source":{"type":"local","path":"/PRIVATE_SOURCE"},"extensions":["native"]}}));let root=cached(&f,"M","native");put(&root.join("skills/s/SKILL.md"),b"PRIVATE_BODY");
    let paths=watch_contract(&f.host,&f.c,&Mutex::default());assert!(paths.contains(&f.alias));assert!(paths.contains(&f.data.join("current")));assert!(paths.contains(&f.extensions.join("extensions.json")));assert!(paths.contains(&root.join("skills/s/SKILL.md")));
    assert!(paths.iter().all(|p|!p.to_string_lossy().contains("PRIVATE_SOURCE")&&!p.to_string_lossy().contains("project")));
}

#[test]
fn availability_fingerprint_tracks_physical_skill_metadata_without_reading_body() {
    let f=fixture();registry(&f,json!({"M":{"extensions":["native"]}}));let root=cached(&f,"M","native");let marker=root.join("skills/s/SKILL.md");put(&marker,b"ONE");
    let first=read(&f);let before=named(&first,"native@M").source_fingerprint.clone();put(&marker,b"LONGER_PRIVATE_SKILL_BODY");let second=read(&f);
    assert_ne!(before,named(&second,"native@M").source_fingerprint);assert!(!serde_json::to_string(&second.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn cached_digest_does_not_accept_a_changed_expected_guard_or_visit_exhaustion() {
    let mut f=fixture();let cache=Mutex::default();assert!(version_contract(&f.alias,&f.c,&cache).is_some());
    f.c.guards.iter_mut().find(|g|g.path.ends_with(".jar")).unwrap().sha256="0".repeat(64);assert!(version_contract(&f.alias,&f.c,&cache).is_none());
    let mut s=Snapshot::default();let mut b=Budget{visited:MAX_VISITS,..Default::default()};assert!(s.list(&f.extensions,&mut b).is_err());
}
