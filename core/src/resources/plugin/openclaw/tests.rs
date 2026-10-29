use super::*;
use crate::testenv::TestHome;
use serde_json::json;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};

fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn seal(root: &Path, guard: &mut Guard, bytes: &[u8]) {
    put(&root.join(&guard.path), bytes);
    guard.bytes = bytes.len() as u64;
    guard.sha256 = hex::encode(Sha256::digest(bytes));
}
fn host(profile: &Profile) -> Host {
    let roots = if profile.agent_id == "openclaw" {
        vec![Root { path: "~/.openclaw/tools/node".into(), layout: profile.layout.clone(),
            presence: PluginPresence::Bundled, complete_marker: None }]
    } else {
        vec![Root { path: "~/Applications/OpenClaw.app".into(), layout: profile.layout.clone(),
            presence: PluginPresence::Bundled, complete_marker: None }]
    };
    Host { agent_id: profile.agent_id.clone(), adapter: "openclaw".into(),
        docs_url: "https://docs.openclaw.ai/".into(), manifest_paths: vec![],
        default_mcp_files: vec![], mcp_id_prefix: String::new(), roots, resource_groups: vec![] }
}
fn prepare_profile(profile: &mut Profile, root: &Path) {
    for guard in &mut profile.guards {
        let value = match guard.path.as_str() {
            "package.json" => if profile.agent_id == "openclaw" {
                json!({"name":"openclaw","version":profile.version,"type":"module","bin":{"openclaw":profile.entry}})
            } else { json!({"name":"openclaw","version":profile.version,"type":"module"}) },
            "dist/build-info.json" => json!({"version":profile.version,"commit":profile.commit,
                "builtAt":profile.built_at,"buildId":profile.build_id}),
            _ => Value::Null,
        };
        let bytes = if value.is_null() { format!("// inert public fixture: {}\n", guard.path).into_bytes() }
            else { serde_json::to_vec(&value).unwrap() };
        seal(root, guard, &bytes);
    }
    for manifest in &mut profile.manifests {
        let mut value = json!({"id":manifest.id,"configSchema":{},"name":null,"version":null});
        if !manifest.skill_leaves.is_empty() { value["skills"] = json!(["./skills"]); }
        seal(root, &mut manifest.manifest, &serde_json::to_vec(&value).unwrap());
        if let Some(package) = &mut manifest.package {
            seal(root, package, &serde_json::to_vec(&json!({"name":format!("@openclaw/{}",manifest.id),"version":"9.8.7"})).unwrap());
        }
        for leaf in &manifest.skill_leaves { put(&root.join(leaf), b"\0PRIVATE_SKILL_BODY\xff"); }
    }
}
#[cfg(unix)]
fn fixture() -> (TestHome, Contract, Host, Host, PathBuf, PathBuf) {
    let home = TestHome::new("openclaw-bundled");
    let mut c: Contract = serde_json::from_str(include_str!("../../../../../data/native-plugin-openclaw.json")).unwrap();
    let node = home.home.join(".openclaw/tools").join(&c.managed_node);
    let cli = node.join("lib/node_modules/openclaw");
    fs::create_dir_all(&cli).unwrap();
    symlink(&node, home.home.join(".openclaw/tools/node")).unwrap();
    let app = home.home.join("Applications/OpenClaw.app");
    let worker = app.join(&c.app.worker_root);
    fs::create_dir_all(&worker).unwrap();
    prepare_profile(&mut c.cli, &cli);
    prepare_profile(&mut c.desktop, &worker);
    let mut fields = plist::Dictionary::new();
    for (key,value) in [("CFBundleIdentifier",&c.app.bundle_id),("CFBundleName",&c.app.bundle_name),
        ("CFBundleExecutable",&c.app.executable),("CFBundleShortVersionString",&c.desktop.version),
        ("CFBundleVersion",&c.app.build_number),("OpenClawGitCommit",&c.desktop.commit),
        ("OpenClawBuildTimestamp",&c.desktop.built_at),("OpenClawWorkerBuildID",&c.desktop.build_id)] {
        fields.insert(key.into(),plist::Value::String(value.clone()));
    }
    let mut info = Vec::new(); plist::Value::Dictionary(fields).to_writer_xml(&mut info).unwrap();
    seal(&app,&mut c.app.info,&info);
    seal(&app,&mut c.app.main,b"INERT_NATIVE_IDENTITY_FIXTURE");
    let cli_host = host(&c.cli); let desktop_host = host(&c.desktop);
    (home,c,cli_host,desktop_host,cli,app)
}
fn read(c:&Contract,host:&Host,cache:&Mutex<DigestCache>) -> PluginScan {
    let mut result=PluginScan::default();
    if observe_contract(host,c,cache,&mut result).is_err(){result.unavailable=true;}
    normalize(&mut result);result
}
fn skills(result:&PluginScan)->Vec<(String,String)> {
    result.plugins.iter().flat_map(|p|p.components.iter().filter(|c|c.kind==PluginComponentKind::Skill)
        .map(|c|(p.plugin_id.clone(),c.name.clone()))).collect()
}
#[cfg(unix)]
#[test]
fn cli_and_private_worker_keep_separate_identities_and_exact_skill_ownership() {
    let (_home,c,cli_host,desktop_host,_cli,_app)=fixture();let cache=Mutex::default();
    let cli=read(&c,&cli_host,&cache);let desktop=read(&c,&desktop_host,&cache);
    assert!(!cli.unavailable&&!desktop.unavailable);assert_eq!(cli.plugins.len(),64);assert_eq!(desktop.plugins.len(),7);
    assert_eq!(skills(&cli),vec![("browser".into(),"browser-automation".into()),("canvas".into(),"canvas".into()),
        ("crabbox".into(),"crabbox-apps".into()),("memory-wiki".into(),"obsidian-vault-maintainer".into()),("memory-wiki".into(),"wiki-maintainer".into())]);
    assert_eq!(skills(&desktop),vec![("browser".into(),"browser-automation".into())]);
    assert!(cli.plugins.iter().all(|p|p.agent_id=="openclaw"&&p.enabled.is_none()&&p.presence==PluginPresence::Bundled));
    assert!(desktop.plugins.iter().all(|p|p.agent_id=="openclaw-desktop"&&p.enabled.is_none()&&p.presence==PluginPresence::Bundled));
    assert!(cli.mcps.is_empty()&&desktop.mcps.is_empty());
}
#[cfg(unix)]
#[test]
fn plugin_version_comes_from_adjacent_public_package_not_host_version() {
    let (_home,c,host,_desktop,_cli,_app)=fixture();let result=read(&c,&host,&Mutex::default());
    let browser=result.plugins.iter().find(|p|p.plugin_id=="browser").unwrap();
    assert_eq!(browser.version,"9.8.7");assert!(browser.revision.contains("host:2026.9.8"));
}
#[cfg(unix)]
#[test]
fn missing_plugin_version_stays_unknown() {
    let (_home,mut c,host,_desktop,cli,_app)=fixture();
    let row=c.cli.manifests.iter_mut().find(|m|m.id=="browser").unwrap();
    if let Some(package)=&mut row.package { seal(&cli,package,br#"{"name":"@openclaw/browser"}"#); }
    let result=read(&c,&host,&Mutex::default());assert_eq!(result.plugins.iter().find(|p|p.plugin_id=="browser").unwrap().version,"unknown");
}
#[cfg(unix)]
#[test]
fn global_dependency_custodian_and_user_skills_never_become_plugin_components() {
    let (home,c,host,_desktop,cli,_app)=fixture();
    for relative in ["skills/private/SKILL.md","custodian-skills/private/SKILL.md","node_modules/dep/skills/private/SKILL.md",
        "dist/extensions/browser/skills/undeclared/SKILL.md"] {put(&cli.join(relative),b"PRIVATE_UNDECLARED_BODY");}
    put(&home.home.join(".openclaw/state/openclaw.sqlite"),b"PRIVATE_DATABASE");
    put(&home.home.join(".openclaw/openclaw.json"),b"PRIVATE_CONFIG");
    let result=read(&c,&host,&Mutex::default());assert_eq!(skills(&result).len(),5);
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}
#[cfg(unix)]
#[test]
fn unreadable_and_invalid_utf8_skill_bodies_still_supply_stat_only_availability() {
    let (_home,c,host,_desktop,cli,_app)=fixture();
    let path=cli.join("dist/extensions/browser/skills/browser-automation/SKILL.md");
    fs::set_permissions(&path,fs::Permissions::from_mode(0o000)).unwrap();
    let result=read(&c,&host,&Mutex::default());
    fs::set_permissions(path,fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!result.unavailable);assert_eq!(skills(&result).len(),5);
}
#[cfg(unix)]
#[test]
fn native_mcp_declarations_remain_references_without_commands_or_connections() {
    let (_home,mut c,host,_desktop,cli,_app)=fixture();
    let row=c.cli.manifests.iter_mut().find(|m|m.id=="browser").unwrap();
    seal(&cli,&mut row.manifest,br#"{"id":"browser","configSchema":{},"skills":["./skills"],"mcpServers":{"example":{"command":"PRIVATE_COMMAND","env":{"KEY":"PRIVATE_SECRET"}}}}"#);
    let result=read(&c,&host,&Mutex::default());let p=result.plugins.iter().find(|p|p.plugin_id=="browser").unwrap();
    assert!(result.mcps.is_empty());assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.native_id.as_deref()==Some("example")));
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}
#[cfg(unix)]
#[test]
fn unknown_build_or_public_guard_fails_closed_before_any_plugin_is_published() {
    let (_home,mut c,host,_desktop,cli,_app)=fixture();
    let build=c.cli.guards.iter_mut().find(|g|g.path=="dist/build-info.json").unwrap();
    seal(&cli,build,br#"{"version":"future","commit":"unverified","builtAt":"unknown","buildId":"unknown"}"#);
    let result=read(&c,&host,&Mutex::default());assert!(result.unavailable);assert!(result.plugins.is_empty());
}
#[cfg(unix)]
#[test]
fn entry_guard_mutation_is_not_rescued_by_same_version_package_metadata() {
    let (_home,c,host,_desktop,cli,_app)=fixture();put(&cli.join("openclaw.mjs"),b"UNVERIFIED_ENTRY");
    let result=read(&c,&host,&Mutex::default());assert!(result.unavailable);assert!(result.plugins.is_empty());
}
#[cfg(unix)]
#[test]
fn invalid_native_manifest_identity_or_schema_does_not_gain_a_bundled_row() {
    let (_home,mut c,host,_desktop,cli,_app)=fixture();
    let row=c.cli.manifests.iter_mut().find(|m|m.id=="browser").unwrap();
    seal(&cli,&mut row.manifest,br#"{"id":"another","configSchema":{},"skills":["./skills"]}"#);
    let result=read(&c,&host,&Mutex::default());assert!(result.unavailable);assert!(!result.plugins.iter().any(|p|p.plugin_id=="browser"));
}
#[cfg(unix)]
#[test]
fn declared_skill_path_escape_is_rejected_without_touching_the_external_file() {
    let (home,mut c,host,_desktop,cli,_app)=fixture();put(&home.home.join("private/SKILL.md"),b"PRIVATE_OUTSIDE_BODY");
    let row=c.cli.manifests.iter_mut().find(|m|m.id=="browser").unwrap();
    seal(&cli,&mut row.manifest,br#"{"id":"browser","configSchema":{},"skills":["../../../../../../private"]}"#);
    let result=read(&c,&host,&Mutex::default());assert!(result.unavailable);assert!(!result.plugins.iter().any(|p|p.plugin_id=="browser"));
}
#[cfg(unix)]
#[test]
fn managed_node_link_cannot_select_an_external_prefix() {
    let (home,c,host,_desktop,_cli,_app)=fixture();let link=home.home.join(".openclaw/tools/node");
    fs::remove_file(&link).unwrap();symlink(&home.home.join("private"),link).unwrap();
    let result=read(&c,&host,&Mutex::default());assert!(result.unavailable);assert!(result.plugins.is_empty());
}
#[cfg(unix)]
#[test]
fn plugin_skill_symlink_is_not_followed_or_reported_as_available() {
    let (home,c,host,_desktop,cli,_app)=fixture();let leaf=cli.join("dist/extensions/browser/skills/browser-automation/SKILL.md");
    fs::remove_file(&leaf).unwrap();let external=home.home.join("private.md");put(&external,b"PRIVATE_BODY");symlink(external,leaf).unwrap();
    let result=read(&c,&host,&Mutex::default());assert!(result.unavailable);assert!(!skills(&result).iter().any(|(id,_)|id=="browser"));
}
#[cfg(unix)]
#[test]
fn missing_manifest_keeps_the_rest_incomplete_instead_of_reading_a_ledger() {
    let (_home,c,host,_desktop,cli,_app)=fixture();fs::remove_file(cli.join("dist/extensions/browser/openclaw.plugin.json")).unwrap();
    let result=read(&c,&host,&Mutex::default());assert!(result.unavailable);assert_eq!(result.plugins.len(),63);
}
#[cfg(unix)]
#[test]
fn app_identity_guard_does_not_fall_back_to_cli_scope() {
    let (_home,c,_cli,host,_root,app)=fixture();put(&app.join("Contents/Info.plist"),b"UNVERIFIED_APP");
    let result=read(&c,&host,&Mutex::default());assert!(result.unavailable);assert!(result.plugins.is_empty());
}
#[cfg(unix)]
#[test]
fn observer_uses_its_own_budget_when_other_observers_exhaust_theirs() {
    let (_home,c,host,_desktop,_cli,_app)=fixture();let mut result=PluginScan::default();
    result.budget.bytes=MAX_READ_BYTES;result.budget.visited=MAX_SCAN_ENTRIES;
    observe_contract(&host,&c,&Mutex::default(),&mut result).unwrap();
    assert_eq!(result.plugins.len(),64);assert_eq!(result.budget.bytes,MAX_READ_BYTES);assert_eq!(result.budget.visited,MAX_SCAN_ENTRIES);
}
#[cfg(unix)]
#[test]
fn identity_budget_is_finite_and_does_not_allocate_a_claimed_large_metadata_file() {
    let (_home,mut c,host,_desktop,_cli,_app)=fixture();
    c.cli.guards[0].bytes=MAX_METADATA_BYTES+1;
    let result=read(&c,&host,&Mutex::default());assert!(result.unavailable);assert!(result.plugins.is_empty());
}
#[cfg(unix)]
#[test]
fn repeated_app_observation_reuses_only_the_bound_large_digest() {
    let (_home,c,_cli,host,_root,_app)=fixture();let cache=Mutex::default();let mut first=Budget::default();
    let proof=Verified::capture(&host,&c.desktop,&c,&cache,&mut first).unwrap().unwrap();proof.revalidate(&host,&c).unwrap();
    let mut second=Budget::default();Verified::capture(&host,&c.desktop,&c,&cache,&mut second).unwrap().unwrap();
    assert!(first.hashed_bytes>0);assert_eq!(second.hashed_bytes,0);assert!(second.read_bytes>0);
}
#[cfg(unix)]
#[test]
fn captured_entry_and_selection_are_rechecked_before_publication() {
    let (_home,c,host,_desktop,cli,_app)=fixture();let proof=Verified::capture(&host,&c.cli,&c,&Mutex::default(),&mut Budget::default()).unwrap().unwrap();
    put(&cli.join("openclaw.mjs"),b"RACED");assert!(proof.revalidate(&host,&c).is_err());
}
#[cfg(unix)]
#[test]
fn watch_set_contains_only_selected_public_package_and_app_boundaries() {
    let (home,c,cli_host,desktop_host,cli,app)=fixture();
    let cli_paths=watch_paths_contract(&cli_host,&c).unwrap();let desktop_paths=watch_paths_contract(&desktop_host,&c).unwrap();
    assert!(cli_paths.contains(&home.home.join(".openclaw/tools/node")));
    assert!(cli_paths.contains(&cli.join("openclaw.mjs")));assert!(cli_paths.contains(&cli.join("dist/extensions/browser/skills/browser-automation/SKILL.md")));
    assert!(desktop_paths.contains(&app.join("Contents/Info.plist")));assert!(desktop_paths.contains(&app.join(&c.app.worker_root).join("dist/mac-node-worker.js")));
    assert!(cli_paths.iter().chain(&desktop_paths).all(|p|!p.ends_with("openclaw.sqlite")&&!p.ends_with("openclaw.json")&&!p.ends_with("installs.json")));
}
#[cfg(unix)]
#[test]
fn passive_version_does_not_accept_the_app_worker_as_the_independent_cli() {
    let (_home,c,host,_desktop,cli,app)=fixture();
    assert_eq!(version_for(&host,&c,&cli.join("openclaw.mjs")) .as_deref(),Some("2026.9.8"));
    assert_eq!(version_for(&host,&c,&app.join(&c.app.worker_root).join("dist/mac-node-worker.js")),None);
    assert_eq!(super::super::passive_runtime_version("openclaw",Path::new("/unverified/openclaw")),Some(None));
}
#[test]
fn desktop_is_read_only_and_existing_cli_user_skills_and_launch_behavior_are_preserved() {
    let _home=TestHome::new("openclaw-definitions");let agents=crate::agents::builtin_agents();
    let desktop=&agents["openclaw-desktop"];assert_eq!(desktop.name.as_deref(),Some("OpenClaw"));assert_eq!(desktop.category.as_deref(),Some("desktop"));
    assert!(desktop.global.is_none()&&desktop.skills.is_none());
    let cli=&agents["openclaw"];assert!(cli.global.is_none());let skill=cli.skills.as_ref().unwrap();
    assert_eq!(skill.global_dir,"~/.openclaw/skills");assert_eq!(skill.probes.len(),2);
}
