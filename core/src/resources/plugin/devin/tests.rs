//! Regression source only: the task intentionally does not run this module.
use super::*;
use crate::testenv::TestHome;
use serde_json::json;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};

struct EnvReset(&'static str, Option<std::ffi::OsString>);
impl EnvReset {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var_os(key); std::env::set_var(key, value); Self(key, old)
    }
}
impl Drop for EnvReset {
    fn drop(&mut self) {
        match &self.1 { Some(value) => std::env::set_var(self.0, value), None => std::env::remove_var(self.0) }
    }
}
struct Fixture {
    // Restore this variable before TestHome releases the process environment lock.
    _env: EnvReset, home: TestHome, c: Contract, host: Host,
    app: PathBuf, extensions: PathBuf, plugins: PathBuf,
}
fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, bytes).unwrap();
}
fn seal(app: &Path, guard: &mut Guard, bytes: &[u8]) {
    put(&app.join(&guard.path), bytes); guard.bytes = bytes.len() as u64;
    guard.sha256 = hex::encode(Sha256::digest(bytes));
}
fn fixture() -> Fixture {
    let home = TestHome::new("devin-metadata"); let env = EnvReset::set("VSCODE_APPDATA", "");
    let mut c: Contract = serde_json::from_str(include_str!("../../../../../data/native-plugin-devin.json")).unwrap();
    let app = home.home.join("Applications/Devin.app");
    let mut info = plist::Dictionary::new();
    for (key, value) in [("CFBundleIdentifier", &c.bundle_id), ("CFBundleName", &c.bundle_name),
        ("CFBundleDisplayName", &c.bundle_name), ("CFBundleExecutable", &c.executable),
        ("CFBundleShortVersionString", &c.version), ("CFBundleVersion", &c.version)] {
        info.insert(key.into(), plist::Value::String(value.clone()));
    }
    let mut plist_bytes = Vec::new(); plist::Value::Dictionary(info).to_writer_xml(&mut plist_bytes).unwrap();
    for guard in &mut c.guards {
        let bytes = match guard.path.as_str() {
            "Contents/Info.plist" => plist_bytes.clone(),
            "Contents/Resources/app/package.json" => serde_json::to_vec(&json!({"name":"Devin", "version":c.base_version,"main":c.package_main})).unwrap(),
            "Contents/Resources/app/product.json" => serde_json::to_vec(&json!({"nameShort":"Devin","nameLong":"Devin","applicationName":"devin-desktop","dataFolderName":".devin","windsurfVersion":c.version,"commit":c.commit})).unwrap(),
            _ => format!("INERT_PUBLIC_GUARD:{}", guard.path).into_bytes(),
        };
        seal(&app, guard, &bytes);
    }
    for package in &mut c.builtin_extensions {
        let (publisher, name) = package.id.split_once('.').unwrap();
        let mut value = json!({"publisher":publisher,"name":name,"version":package.version,
            "displayName":name,"contributes":{"commands":[{"command":"ordinary.ide.command","title":"PRIVATE_COMMAND_TITLE"}]}});
        if package.id == "vscode.mermaid-markdown-features" {
            value["contributes"]["languageModelTools"] = json!([{"name":"renderMermaidDiagram","modelDescription":"PRIVATE_TOOL_DESCRIPTION","when":"chatSessionType == local"}]);
        }
        seal(&app, &mut package.manifest, &serde_json::to_vec(&value).unwrap());
    }
    // Two identity members are also bundled extension manifests.
    for guard in &mut c.guards {
        let bytes = fs::read(app.join(&guard.path)).unwrap();
        guard.bytes = bytes.len() as u64; guard.sha256 = hex::encode(Sha256::digest(bytes));
    }
    for name in &c.session_skills {
        put(&app.join(&c.session_skill_root).join(name).join("SKILL.md"), b"\0PRIVATE_SKILL_BODY\xff");
    }
    let host = Host { agent_id:"windsurf".into(), adapter:"devin".into(), docs_url:"https://docs.devin.ai/desktop/".into(),
        manifest_paths:vec![], default_mcp_files:vec![], mcp_id_prefix:String::new(), resource_groups:vec![],
        roots:vec![Root {path:"~/Applications/Devin.app".into(),layout:"devin_app".into(),presence:PluginPresence::Bundled,complete_marker:None}] };
    let extensions = home.home.join(".devin/extensions");
    let plugins = home.home.join("Library/Application Support/Devin/User/agent-plugins");
    Fixture { _env:env, home, c, host, app, extensions, plugins }
}
fn read(f: &Fixture) -> PluginScan {
    let mut result = PluginScan::default();
    if observe_contract(&f.host, &f.c, &Mutex::default(), &mut result).is_err() { result.unavailable = true; }
    normalize(&mut result); result
}
fn extension(f: &Fixture, folder: &str, name: &str, version: &str, contributes: Value) -> PathBuf {
    let root = f.extensions.join(folder);
    put(&root.join("package.json"), &serde_json::to_vec(&json!({"publisher":"example","name":name,"version":version,"contributes":contributes})).unwrap()); root
}
fn row(root: &Path, folder: &str, id: &str, version: &str) -> Value {
    json!({"identifier":{"id":id},"version":version,"relativeLocation":folder,
        "location":{"scheme":"file","path":root.join(folder)},"metadata":{"unused":"PRIVATE_REGISTRY_METADATA"}})
}
fn extension_ledger(f: &Fixture, rows: Value) { put(&f.extensions.join("extensions.json"), &serde_json::to_vec(&rows).unwrap()); }
fn plugin(f: &Fixture, folder: &str, manifest: &str, value: Value) -> PathBuf {
    let root = f.plugins.join(folder); put(&root.join(manifest), &serde_json::to_vec(&value).unwrap()); root
}
fn install_plugins(f: &Fixture, roots: &[PathBuf]) {
    let rows: Vec<_> = roots.iter().map(|root| json!({"pluginUri":url::Url::from_file_path(root).unwrap().as_str(),"marketplace":"public/reviewed"})).collect();
    put(&f.plugins.join("installed.json"), &serde_json::to_vec(&json!({"version":1,"installed":rows})).unwrap());
}
fn named<'a>(result: &'a PluginScan, name: &str) -> &'a NativePluginObservation {
    result.plugins.iter().find(|plugin| plugin.name == name).unwrap()
}
fn skill_paths(plugin: &NativePluginObservation) -> Vec<String> {
    plugin.components.iter().filter(|component| component.kind == PluginComponentKind::Skill).map(|component| component.source_path.clone()).collect()
}

#[test]
fn bundled_extensions_and_application_owned_skills_keep_separate_identity_and_no_fake_mcps() {
    let f = fixture(); let result = read(&f);
    assert!(!result.unavailable); assert_eq!(result.plugins.len(), 103); assert!(result.mcps.is_empty());
    let group = named(&result, "Devin bundled session Skills");
    assert_eq!(group.plugin_id, "@builtin/sessions-skills"); assert_eq!(skill_paths(group).len(), 12);
    assert!(result.plugins.iter().all(|plugin| plugin.agent_id == "windsurf" && plugin.enabled.is_none()));
    assert!(result.plugins.iter().flat_map(|plugin| &plugin.components).all(|component| component.kind != PluginComponentKind::Command));
    assert_eq!(result.plugins.iter().flat_map(|plugin| &plugin.components).filter(|component| component.kind == PluginComponentKind::Tool).count(), 1);
    assert_eq!(skill_observations(&result.plugins).len(), 12);
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn dependency_skills_and_unregistered_extension_history_are_not_native_installations() {
    let f = fixture(); extension_ledger(&f, json!([]));
    extension(&f, "history-only", "history", "99", json!({}));
    put(&f.app.join("Contents/Resources/app/node_modules/playwright-core/lib/tools/trace/SKILL.md"), b"PRIVATE_DEPENDENCY_SKILL");
    let result = read(&f); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 103);
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("playwright"));
}

#[test]
fn ordinary_registry_preserves_registered_versions_and_relative_location_precedence() {
    let f = fixture(); extension(&f,"same-v1","same","1",json!({})); extension(&f,"same-v2","same","2",json!({}));
    let mut first = row(&f.extensions,"same-v1","example.same","1"); first["location"] = json!({"scheme":"https","path":"/ignored"});
    extension_ledger(&f, json!([first, row(&f.extensions,"same-v2","example.same","2")]));
    let result = read(&f); assert!(!result.unavailable);
    let rows: Vec<_> = result.plugins.iter().filter(|plugin| plugin.plugin_id == "example.same").collect();
    assert_eq!(rows.len(), 2); assert!(rows.iter().all(|plugin| plugin.presence == PluginPresence::Installed));
    assert_eq!(rows.iter().map(|plugin| plugin.version.as_str()).collect::<BTreeSet<_>>(), BTreeSet::from(["1","2"]));
}

#[test]
fn string_location_without_relative_location_stays_ignored_provenance() {
    let f = fixture(); let root = extension(&f,"ignored","ignored","1",json!({}));
    extension_ledger(&f,json!([{"identifier":{"id":"example.ignored"},"version":"1","location":root.to_string_lossy()}]));
    let result = read(&f); assert!(result.unavailable);
    let item = result.plugins.iter().find(|plugin| plugin.plugin_id == "example.ignored").unwrap();
    assert_eq!(item.presence, PluginPresence::Declared); assert!(item.components.is_empty());
}

#[test]
fn uri_object_locations_only_materialize_inside_the_selected_extensions_root() {
    let f = fixture(); let root = extension(&f,"selected","selected","1",json!({}));
    extension_ledger(&f,json!([{"identifier":{"id":"example.selected"},"version":"1","location":{"scheme":"file","path":root}}]));
    assert!(!read(&f).unavailable);
    extension_ledger(&f,json!([{"identifier":{"id":"example.escape"},"version":"1","location":{"scheme":"file","path":f.home.home.join("outside")}}]));
    let result = read(&f); assert!(result.unavailable);
    assert!(result.plugins.iter().any(|plugin| plugin.plugin_id == "example.escape" && plugin.presence == PluginPresence::Declared));
}

#[test]
fn ordinary_and_agent_plugin_registries_are_not_interchangeable() {
    let f = fixture(); extension_ledger(&f,json!({"version":1,"installed":[]}));
    put(&f.plugins.join("installed.json"), b"[]");
    let result = read(&f); assert!(result.unavailable); assert_eq!(result.plugins.len(),103);
}

#[test]
fn agent_plugin_current_written_version_is_required_without_storage_or_cache_fallback() {
    let f = fixture(); plugin(&f,"cache-only",".plugin/plugin.json",json!({"name":"Not installed"}));
    put(&f.plugins.join("installed.json"), br#"{"version":2,"installed":[]}"#);
    put(&f.home.home.join("Library/Application Support/Devin/User/globalStorage/state.vscdb"), b"PRIVATE_STORAGE_SENTINEL");
    let before = fs::read(f.plugins.join("installed.json")).unwrap(); let result = read(&f);
    assert!(result.unavailable); assert_eq!(result.plugins.len(),103);
    assert_eq!(fs::read(f.plugins.join("installed.json")).unwrap(),before);
}

#[test]
fn missing_registries_do_not_create_initialization_files_or_call_migration_helpers() {
    let f = fixture(); let result = read(&f); assert!(!result.unavailable);
    assert!(!f.extensions.exists()); assert!(!f.plugins.exists());
    assert!(!f.home.home.join(".windsurf").exists());
}

#[test]
fn open_plugin_manifest_has_priority_and_malformed_selected_format_never_falls_back() {
    let f = fixture(); let root = plugin(&f,"native",".plugin/plugin.json",json!({"name":"Open format","version":"1"}));
    put(&root.join(".claude-plugin/plugin.json"), br#"{"name":"Wrong Claude"}"#);
    put(&root.join("plugin.json"), br#"{"name":"Wrong root"}"#); install_plugins(&f,&[root.clone()]);
    let result = read(&f); assert!(!result.unavailable); assert_eq!(named(&result,"Open format").presence,PluginPresence::Installed);
    put(&root.join(".plugin/plugin.json"), b"malformed"); let result = read(&f); assert!(result.unavailable);
    assert!(!result.plugins.iter().any(|plugin| plugin.name == "Wrong Claude" || plugin.name == "Wrong root"));
}

#[test]
fn claude_path_segment_selects_claude_format_even_with_a_root_manifest() {
    let f = fixture(); let root = plugin(&f,".claude/native","plugin.json",json!({"name":"Wrong root"}));
    install_plugins(&f,&[root]); let result = read(&f); assert!(result.unavailable);
    assert!(!result.plugins.iter().any(|plugin| plugin.name == "Wrong root"));
}

#[test]
fn installed_plugin_paths_are_percent_decoded_once_without_remote_or_parent_escape_reads() {
    let f = fixture(); let root = plugin(&f,"space plugin",".plugin/plugin.json",json!({"name":"Space plugin"}));
    install_plugins(&f,&[root]); assert!(!read(&f).unavailable);
    put(&f.plugins.join("installed.json"), br#"{"version":1,"installed":[{"pluginUri":"https://example.invalid/PRIVATE_QUERY?token=PRIVATE_SECRET","marketplace":"public"},{"pluginUri":"file:///outside/plugin","marketplace":"public"}]}"#);
    let result = read(&f); assert!(result.unavailable);
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
    assert_eq!(result.plugins.iter().filter(|plugin|plugin.presence == PluginPresence::Declared).count(),2);
}

#[test]
fn default_and_override_skill_paths_merge_while_exclusive_omits_the_default() {
    let f = fixture(); let root = plugin(&f,"native",".plugin/plugin.json",json!({"name":"Owned Skills","skills":["extra",12," "]}));
    put(&root.join("skills/default/SKILL.md"),b"\xffPRIVATE_DEFAULT"); put(&root.join("extra/custom/SKILL.md"),b"\xffPRIVATE_CUSTOM");
    install_plugins(&f,&[root.clone()]); let result = read(&f); assert!(!result.unavailable);
    assert_eq!(skill_paths(named(&result,"Owned Skills")).len(),2);
    put(&root.join(".plugin/plugin.json"),br#"{"name":"Owned Skills","skills":{"paths":["extra"],"exclusive":true}}"#);
    let result = read(&f); assert!(!result.unavailable); let paths = skill_paths(named(&result,"Owned Skills"));
    assert_eq!(paths.len(),1); assert!(paths[0].ends_with("extra/custom/SKILL.md"));
}

#[test]
fn empty_skill_search_uses_root_marker_without_reading_frontmatter() {
    let f = fixture(); let root = plugin(&f,"native","plugin.json",json!({"name":"Root Skill","skills":{"paths":[],"exclusive":true}}));
    put(&root.join("SKILL.md"),b"\0PRIVATE_FRONTMATTER\xff"); install_plugins(&f,&[root]);
    let result = read(&f); assert!(!result.unavailable); assert_eq!(skill_paths(named(&result,"Root Skill")).len(),1);
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn plugin_commands_agents_rules_and_hooks_remain_stat_only_metadata() {
    let f = fixture(); let root = plugin(&f,"native","plugin.json",json!({"name":"Files","commands":"extra.md","hooks":{"SessionStart":[{"command":"PRIVATE_HOOK_COMMAND"}]}}));
    for name in ["commands/default.md","extra.md","agents/helper.md","rules/style.instructions.md"] { put(&root.join(name),b"\xffPRIVATE_BODY"); }
    install_plugins(&f,&[root]); let result = read(&f); assert!(!result.unavailable);
    let kinds: BTreeSet<_> = named(&result,"Files").components.iter().map(|component|component.kind.clone()).collect();
    assert!(kinds.contains(&PluginComponentKind::Command)); assert!(kinds.contains(&PluginComponentKind::Agent));
    assert!(kinds.contains(&PluginComponentKind::Rule)); assert!(kinds.contains(&PluginComponentKind::TypeDeclaration));
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn inline_mcp_and_file_paths_follow_distinct_native_shapes_without_secret_output() {
    let f = fixture(); let root = plugin(&f,"native",".plugin/plugin.json",json!({"name":"MCP references","mcpServers":{
        "local":{"command":"PRIVATE_EXECUTABLE","envFile":"PRIVATE_ENV_FILE","env":{"TOKEN":"PRIVATE_SECRET"}},
        "http":{"type":"sse","url":"https://example.invalid/PRIVATE_TOKEN"},
        "wrong-serverUrl":{"serverUrl":"https://example.invalid"},"wrong-ws":{"type":"ws","url":"ws://example.invalid"}}}));
    put(&root.join(".mcp.json"),br#"{"mcpServers":{"wrong-default":{"command":"unused"}}}"#);
    install_plugins(&f,&[root]); let result = read(&f); assert!(!result.unavailable); assert!(result.mcps.is_empty());
    let names = named(&result,"MCP references").components.iter().filter(|component|component.kind==PluginComponentKind::McpReference).map(|component|component.name.as_str()).collect::<BTreeSet<_>>();
    assert_eq!(names,BTreeSet::from(["http","local"])); assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn external_mcp_paths_keep_first_duplicate_name_and_do_not_open_env_files() {
    let f = fixture(); let root = plugin(&f,"native","plugin.json",json!({"name":"File MCP","mcpServers":{"paths":["extra.json"],"exclusive":false}}));
    put(&root.join(".mcp.json"),br#"{"mcpServers":{"same":{"command":"not-run","envFile":"secret.env"}}}"#);
    put(&root.join("extra.json"),br#"{"same":{"url":"https://example.invalid"},"extra":{"type":"http","url":"https://example.invalid"}}"#);
    put(&root.join("secret.env"),b"PRIVATE_ENV_BODY"); install_plugins(&f,&[root.clone()]); let result = read(&f);
    assert!(!result.unavailable); let references:Vec<_> = named(&result,"File MCP").components.iter().filter(|component|component.kind==PluginComponentKind::McpReference).collect();
    assert_eq!(references.len(),2); assert!(references.iter().find(|component|component.name=="same").unwrap().source_path.ends_with(".mcp.json"));
    assert!(!watch_paths_contract(&f.host,&f.c,&Mutex::default()).contains(&root.join("secret.env")));
}

#[test]
fn malformed_sibling_registration_keeps_independent_safe_packages_visible() {
    let f = fixture(); extension(&f,"safe","safe","1",json!({}));
    extension_ledger(&f,json!([row(&f.extensions,"safe","example.safe","1"),{"identifier":false}]));
    let result = read(&f); assert!(result.unavailable);
    assert!(result.plugins.iter().any(|plugin|plugin.plugin_id=="example.safe" && plugin.presence==PluginPresence::Installed));
    assert_eq!(result.plugins.iter().filter(|plugin|plugin.presence==PluginPresence::Bundled).count(),103);
}

#[test]
fn unknown_host_version_or_public_guard_cannot_fall_back_to_windsurf_or_cloud_devin() {
    let mut f = fixture(); put(&f.app.join("Contents/Resources/app/out/main.js"),b"CHANGED_SOURCE");
    let result = read(&f); assert!(result.unavailable); assert!(result.plugins.is_empty());
    f.host.agent_id="devin".into(); assert!(read(&f).plugins.is_empty());
}

#[test]
fn unknown_environment_skips_default_global_metadata_without_following_overrides() {
    let f = fixture(); let _env = EnvReset::set("VSCODE_PORTABLE","/outside/unknown");
    extension(&f,"safe","safe","1",json!({})); extension_ledger(&f,json!([row(&f.extensions,"safe","example.safe","1")]));
    let result = read(&f); assert!(result.unavailable); assert_eq!(result.plugins.len(),103);
}

#[test]
fn independent_metadata_budget_does_not_consume_existing_hosts_allowances() {
    let f = fixture(); let mut result = PluginScan::default();
    result.budget.visited=MAX_SCAN_ENTRIES; result.budget.bytes=MAX_READ_BYTES; result.budget.verified_bytes=MAX_VERIFIED_BYTES;
    observe_contract(&f.host,&f.c,&Mutex::default(),&mut result).unwrap(); assert_eq!(result.plugins.len(),103);
    assert_eq!(result.budget.visited,MAX_SCAN_ENTRIES); assert_eq!(result.budget.bytes,MAX_READ_BYTES); assert_eq!(result.budget.verified_bytes,MAX_VERIFIED_BYTES);
}

#[test]
fn streaming_digest_cache_reuses_only_identical_physical_paths_and_fds() {
    let f = fixture(); let cache = Mutex::default(); let mut first = IdentityBudget::default();
    VerifiedApp::capture(&f.host,&f.c,&cache,&mut first).unwrap().unwrap(); assert!(first.hashed_bytes>0);
    let mut second = IdentityBudget::default(); VerifiedApp::capture(&f.host,&f.c,&cache,&mut second).unwrap().unwrap();
    assert_eq!(second.hashed_bytes,0);
    let main = f.app.join("Contents/Resources/app/out/main.js"); let old=fs::read(&main).unwrap(); fs::remove_file(&main).unwrap(); put(&main,&old);
    let mut replacement = IdentityBudget::default(); VerifiedApp::capture(&f.host,&f.c,&cache,&mut replacement).unwrap().unwrap();
    assert!(replacement.hashed_bytes>0);
}

#[test]
fn fd_and_selected_path_cas_reject_metadata_replacement_before_publication() {
    let f = fixture(); let root=extension(&f,"safe","safe","1",json!({})); extension_ledger(&f,json!([row(&f.extensions,"safe","example.safe","1")]));
    let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();
    fs::rename(root.join("package.json"),root.join("old.json")).unwrap(); put(&root.join("package.json"),br#"{"publisher":"example","name":"safe","version":"1"}"#);
    assert!(audit.revalidate(&f.host).is_err());
}

#[test]
fn snapshot_rejects_skill_inode_changes_without_reading_its_body() {
    let f = fixture(); let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();
    let leaf=f.app.join(&f.c.session_skill_root).join("commit/SKILL.md");
    fs::remove_file(&leaf).unwrap(); put(&leaf,b"PRIVATE_NEW_BODY"); assert!(audit.revalidate(&f.host).is_err());
}

#[test]
fn malformed_or_oversized_metadata_charges_the_finite_read_allowance() {
    let f=fixture(); let mut snapshot=Snapshot::default(); let mut budget=MetadataBudget::default();
    put(&f.extensions.join("extensions.json"),&vec![b'x';MAX_DOCUMENT_BYTES+1]);
    assert!(snapshot.read_document(&f.extensions.join("extensions.json"),false,&mut budget).is_err());
    budget.bytes=MAX_METADATA_BYTES; assert!(snapshot.read_document(&f.app.join("Contents/Resources/app/package.json"),false,&mut budget).is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_app_manifest_registry_and_skill_leaf_never_escape_confined_roots() {
    let f=fixture(); let outside=f.home.home.join("outside.json"); put(&outside,b"PRIVATE_OUTSIDE");
    fs::create_dir_all(&f.extensions).unwrap(); symlink(&outside,f.extensions.join("extensions.json")).unwrap();
    let leaf=f.app.join(&f.c.session_skill_root).join("commit/SKILL.md"); fs::remove_file(&leaf).unwrap(); symlink(&outside,&leaf).unwrap();
    let result=read(&f); assert!(result.unavailable); assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}

#[cfg(unix)]
#[test]
fn unreadable_skill_bodies_still_allow_stat_only_availability() {
    let f=fixture(); for name in &f.c.session_skills {fs::set_permissions(f.app.join(&f.c.session_skill_root).join(name).join("SKILL.md"),fs::Permissions::from_mode(0)).unwrap();}
    let result=read(&f); assert!(!result.unavailable); assert_eq!(skill_paths(named(&result,"Devin bundled session Skills")).len(),12);
}

#[test]
fn confined_component_overrides_reject_parent_and_absolute_paths() {
    let f=fixture(); let root=plugin(&f,"native","plugin.json",json!({"name":"Unsafe paths","skills":{"paths":["../escape","/outside"],"exclusive":true}}));
    install_plugins(&f,&[root]); let result=read(&f); assert!(result.unavailable);
    assert!(skill_paths(named(&result,"Unsafe paths")).is_empty());
}

#[test]
fn watches_include_both_registries_and_owned_metadata_without_legacy_or_secret_paths() {
    let f=fixture(); let root=plugin(&f,"native",".plugin/plugin.json",json!({"name":"Watched","mcpServers":{"x":{"command":"unused","envFile":"secret.env"}}}));
    install_plugins(&f,&[root.clone()]); let paths=watch_paths_contract(&f.host,&f.c,&Mutex::default());
    assert!(paths.contains(&f.extensions.join("extensions.json"))); assert!(paths.contains(&f.plugins.join("installed.json")));
    assert!(paths.contains(&root.join(".plugin/plugin.json"))); assert!(paths.contains(&f.app.join(&f.c.session_skill_root).join("commit/SKILL.md")));
    assert!(!paths.contains(&root.join("secret.env"))); assert!(paths.iter().all(|path|!path.to_string_lossy().contains(".windsurf")));
}

#[cfg(unix)]
#[test]
fn unsafe_agent_registry_root_does_not_hide_safe_ordinary_extensions_or_enter_watches() {
    let f=fixture(); extension(&f,"safe","safe","1",json!({})); extension_ledger(&f,json!([row(&f.extensions,"safe","example.safe","1")]));
    fs::create_dir_all(f.plugins.parent().unwrap()).unwrap(); let outside=f.home.home.join("private-outside"); fs::create_dir_all(&outside).unwrap();
    symlink(&outside,&f.plugins).unwrap(); let result=read(&f); assert!(result.unavailable);
    assert!(result.plugins.iter().any(|plugin|plugin.plugin_id=="example.safe" && plugin.presence==PluginPresence::Installed));
    assert_eq!(result.plugins.iter().filter(|plugin|plugin.presence==PluginPresence::Bundled).count(),103);
    let paths=watch_paths_contract(&f.host,&f.c,&Mutex::default()); assert!(!paths.contains(&f.plugins)); assert!(!paths.contains(&f.plugins.join("installed.json")));
}

#[test]
fn selected_app_priority_and_environment_are_rechecked_before_publication() {
    let mut f=fixture(); f.host.roots.insert(0,Root {path:"/Applications/Devin.app".into(),layout:"devin_app".into(),presence:PluginPresence::Bundled,complete_marker:None});
    let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap();
    fs::create_dir_all(crate::paths::system_probe_path("/Applications/Devin.app")).unwrap();
    assert!(audit.revalidate(&f.host).is_err());
    fs::remove_dir(crate::paths::system_probe_path("/Applications/Devin.app")).unwrap();
    let audit=capture(&f.host,&f.c,&Mutex::default()).unwrap().unwrap(); let _env=EnvReset::set("VSCODE_APPDATA","/unknown");
    assert!(audit.revalidate(&f.host).is_err());
}

#[test]
fn visit_and_component_exhaustion_cannot_emit_unbounded_metadata() {
    let f=fixture(); let mut snapshot=Snapshot::default(); let mut budget=MetadataBudget {visited:MAX_VISITS,..Default::default()};
    assert!(snapshot.directories(&f.app.join("Contents/Resources/app/extensions"),&mut budget).is_err());
    let mut components=Vec::new(); let mut budget=MetadataBudget {components:MAX_OWNED_COMPONENTS,..Default::default()};
    assert!(add_component(&mut components,PluginComponentKind::Skill,"unbounded",&f.app,None,&mut budget).is_err()); assert!(components.is_empty());
}

#[test]
fn normalized_component_paths_stay_confined_without_requiring_removed_intermediate_dirs() {
    let f=fixture(); let root=plugin(&f,"native","plugin.json",json!({"name":"Normalized","skills":{"paths":["unused/../extra"],"exclusive":true}}));
    put(&root.join("extra/SKILL.md"),b"PRIVATE_BODY"); install_plugins(&f,&[root]);
    let result=read(&f); assert!(!result.unavailable); assert_eq!(skill_paths(named(&result,"Normalized")).len(),1);
}

#[test]
fn an_unsafe_extension_entry_keeps_basic_installation_metadata_without_opening_code() {
    let f=fixture(); let root=extension(&f,"safe","safe","1",json!({}));
    put(&root.join("package.json"),br#"{"publisher":"example","name":"safe","version":"1","main":"../../outside.js"}"#);
    extension_ledger(&f,json!([row(&f.extensions,"safe","example.safe","1")]));
    let result=read(&f); assert!(result.unavailable);
    let item=result.plugins.iter().find(|plugin|plugin.plugin_id=="example.safe").unwrap(); assert_eq!(item.presence,PluginPresence::Installed);
    assert!(item.components.iter().any(|component|component.kind==PluginComponentKind::Extension));
    assert!(item.components.iter().all(|component|!component.source_path.ends_with("outside.js")));
}

#[test]
fn a_skill_body_misdeclared_as_an_mcp_file_never_enters_the_json_reader() {
    let f=fixture(); let root=plugin(&f,"native","plugin.json",json!({"name":"No body reads","mcpServers":{"paths":["skills/a/SKILL.md"],"exclusive":true}}));
    let body=root.join("skills/a/SKILL.md"); put(&body,b"PRIVATE_SKILL_BODY"); install_plugins(&f,&[root]);
    let mut snapshot=Snapshot::default(); let mut budget=MetadataBudget::default();
    assert!(snapshot.read_document(&body,true,&mut budget).is_err()); assert_eq!(budget.bytes,0); assert!(snapshot.bound.is_empty());
    let result=read(&f); assert!(result.unavailable); assert_eq!(skill_paths(named(&result,"No body reads")).len(),1);
}
