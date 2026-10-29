use super::*;
use crate::testenv::TestHome;
use serde_json::json;

const PAYLOAD: &[u8] = b"#!/bin/sh\nprintf executed > \"$HOME/amp-executed-marker\"\n";

fn host() -> Host {
    serde_json::from_value(json!({
        "agent_id":"amp","adapter":"amp","docs_url":"https://ampcode.com/docs/customize/plugins",
        "manifest_paths":[],"default_mcp_files":[],"mcp_id_prefix":"",
        "roots":[{"path":"~/.config/amp/plugins","layout":"amp_system_plugins","presence":"local"},
            {"path":"~/.cache/amp/global-plugins","layout":"amp_global_cache","presence":"cache_only"}]
    })).unwrap()
}

fn fixture_contract() -> Contract {
    Contract {command:"amp".into(),version:"0.1.0".into(),binary_bytes:PAYLOAD.len() as u64,
        binary_sha256:hex::encode(Sha256::digest(PAYLOAD)),entry_name:"/$bunfs/root/fixture".into(),
        builtin_plugins:vec!["permissions".into(),"app-store-connect".into(),"thread-visibility".into()]}
}

fn write(path: &Path, body: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,body).unwrap();
}

fn runtime(home: &TestHome) -> PathBuf {
    let selected=home.home.join("bin/amp");write(&selected,PAYLOAD);
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&selected,fs::Permissions::from_mode(0o755)).unwrap();}
    selected
}

fn scan() -> PluginScan {
    let mut result=PluginScan::default();observe_contract(&host(),&fixture_contract(),&mut result);normalize(&mut result);result
}

// A compiled factory catalog does not assert authentication or feature gates.
#[test]
fn packaged_internal_factories_are_code_markers_without_invented_mcps_or_skills() {
    let home=TestHome::new("amp-compiled");runtime(&home);let result=scan();
    assert_eq!(result.plugins.len(),1);assert!(result.mcps.is_empty());assert!(!result.unavailable);
    let item=&result.plugins[0];assert_eq!(item.version,"0.1.0");assert_eq!(item.presence,PluginPresence::Bundled);assert_eq!(item.enabled,None);
    assert_eq!(item.components.iter().map(|c|c.name.as_str()).collect::<Vec<_>>(),vec!["permissions","app-store-connect","thread-visibility"]);
    assert!(item.components.iter().all(|c|c.kind==PluginComponentKind::Code&&c.native_id.is_none()
        &&c.source_path.ends_with("amp!/$bunfs/root/fixture")));
    assert!(!home.home.join("amp-executed-marker").exists());
}

// Directory entry precedence and root-file masking do not require code reads.
#[test]
fn local_candidates_keep_native_entry_selection_and_leave_runtime_registrations_unknown() {
    let home=TestHome::new("amp-local");runtime(&home);let root=home.home.join(".config/amp/plugins");
    for name in ["plain.ts","plain.js","ignored.test.ts","ignored.test.js","included.spec.ts",
        "directory/index.ts","directory/index.js","masked.ts","masked/index.ts","not-a-plugin/SKILL.md",
        "directory/skills/example/SKILL.md","directory/.mcp.json"] {write(&root.join(name),b"PRIVATE_BODY_AND_CREDENTIAL");}
    let result=scan();let local=result.plugins.iter().filter(|p|p.presence==PluginPresence::Local).collect::<Vec<_>>();
    assert_eq!(local.len(),5);assert!(result.mcps.is_empty());
    let sources=local.iter().flat_map(|p|p.components.iter()).map(|c|c.source_path.as_str()).collect::<Vec<_>>();
    for name in ["plain.ts","plain.js","included.spec.ts","directory/index.ts","masked.ts"] {assert!(sources.iter().any(|p|p.ends_with(name)),"{name}");}
    assert!(!sources.iter().any(|p|p.ends_with("directory/index.js")||p.ends_with("masked/index.ts")||p.contains("ignored.test")));
    assert!(local.iter().all(|p|p.enabled.is_none()&&p.components.iter().all(|c|c.kind==PluginComponentKind::Code&&c.native_id.is_none())));
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_BODY_AND_CREDENTIAL"));
}

// Cache snapshots have no observable authenticated selection or entrypoint.
#[test]
fn hosted_cache_preserves_scopes_and_versions_without_guessing_active_packages() {
    let home=TestHome::new("amp-cache");runtime(&home);let root=home.home.join(".cache/amp/global-plugins");
    for name in ["ampcode.com/user/CasePlugin@v1/index.ts","ampcode.com/user/CasePlugin@v2/index.js",
        "ampcode.com/workspace/CasePlugin@v1/other.js","other.example/user/CasePlugin@v1/secret.txt",
        "ampcode.com/user/not-versioned/index.ts","ampcode.com/user/bad@name@v1/index.ts"] {write(&root.join(name),b"PRIVATE_CACHE_BODY");}
    let result=scan();let cache=result.plugins.iter().filter(|p|p.presence==PluginPresence::CacheOnly).collect::<Vec<_>>();
    assert_eq!(cache.len(),4);assert!(result.mcps.is_empty());
    assert!(cache.iter().all(|p|p.enabled.is_none()&&p.components.len()==1&&p.components[0].kind==PluginComponentKind::PluginReference));
    assert!(!serde_json::to_string(&cache).unwrap().contains("PRIVATE_CACHE_BODY"));
    assert!(!cache.iter().any(|p|p.components[0].source_path.ends_with("index.ts")||p.components[0].source_path.ends_with("index.js")));
}

#[cfg(unix)]
#[test]
fn linked_and_special_entries_stay_references_and_still_mask_same_stem_directories() {
    use std::os::unix::fs::symlink;
    let home=TestHome::new("amp-paths");runtime(&home);let root=home.home.join(".config/amp/plugins");
    write(&root.join("masked/index.ts"),b"PRIVATE_DIRECTORY_BODY");let outside=home.home.join("outside.ts");write(&outside,b"PRIVATE_OUTSIDE_BODY");
    symlink(&outside,root.join("masked.ts")).unwrap();
    rustix::fs::mkfifoat(rustix::fs::CWD,&root.join("pipe.ts"),rustix::fs::Mode::RUSR|rustix::fs::Mode::WUSR).unwrap();
    write(&root.join("fallback/index.js"),b"PRIVATE_BODY");symlink(&outside,root.join("fallback/index.ts")).unwrap();
    let result=scan();assert!(result.unavailable);assert!(result.mcps.is_empty());
    let refs=result.plugins.iter().filter(|p|p.presence==PluginPresence::Declared).collect::<Vec<_>>();assert_eq!(refs.len(),2);
    assert!(refs.iter().all(|p|p.components[0].kind==PluginComponentKind::PluginReference));
    assert!(!result.plugins.iter().flat_map(|p|&p.components).any(|c|c.source_path.ends_with("masked/index.ts")));
    assert!(result.plugins.iter().flat_map(|p|&p.components).any(|c|c.kind==PluginComponentKind::Code&&c.source_path.ends_with("fallback/index.js")));
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_"));
}

// Native configuration uses truthy selection; cache uses nullish selection.
#[test]
fn xdg_roots_keep_their_different_empty_override_rules_without_project_fallback() {
    let home=TestHome::new("amp-overrides");
    std::env::set_var("XDG_CONFIG_HOME","");std::env::set_var("XDG_CACHE_HOME","");
    assert_eq!(system_root(&host()).unwrap(),home.home.join(".config/amp/plugins"));assert!(cache_root(&host()).is_err());
    for value in ["relative","~/.config","/absolute/../project"] {
        std::env::set_var("XDG_CONFIG_HOME",value);assert!(system_root(&host()).is_err());
        std::env::set_var("XDG_CACHE_HOME",value);assert!(cache_root(&host()).is_err());
    }
    std::env::set_var("XDG_CONFIG_HOME",home.home.join("custom-config"));
    std::env::set_var("XDG_CACHE_HOME",home.home.join("custom-cache"));
    assert_eq!(system_root(&host()).unwrap(),home.home.join("custom-config/amp/plugins"));
    assert_eq!(cache_root(&host()).unwrap(),home.home.join("custom-cache/amp/global-plugins"));
}

// A selected wrapper or changed binary cannot run to supply a version label.
#[test]
fn unsupported_runtime_keeps_local_entries_declared_and_blocks_active_version_probe() {
    let home=TestHome::new("amp-passive");let selected=runtime(&home);write(&home.home.join(".config/amp/plugins/example.ts"),b"PRIVATE_BODY");
    assert!(verified_runtime(&fixture_contract(),&selected,&mut ScanBudget::default()).is_ok());
    fs::write(&selected,b"#!/bin/sh\nprintf executed > \"$HOME/amp-executed-marker\"\n# changed").unwrap();
    assert!(verified_runtime(&fixture_contract(),&selected,&mut ScanBudget::default()).is_err());
    let result=scan();assert!(result.unavailable);assert_eq!(result.plugins.len(),1);assert_eq!(result.plugins[0].presence,PluginPresence::Declared);
    assert_eq!(passive_runtime_version("amp",&selected),Some(None));
    assert_eq!(crate::application::agent_launch::runtime_version("amp").unwrap(),None);
    assert!(!home.home.join("amp-executed-marker").exists());
}

#[test]
fn watchers_include_native_binary_plugin_roots_and_both_directory_entry_candidates() {
    let home=TestHome::new("amp-watch");let selected=runtime(&home);let root=home.home.join(".config/amp/plugins");
    write(&root.join("example/index.js"),b"PRIVATE_BODY");let paths=watch_paths(&host());
    for want in [selected,root.clone(),root.join("example"),root.join("example/index.ts"),root.join("example/index.js"),home.home.join(".cache/amp/global-plugins")] {assert!(paths.contains(&want),"{}",want.display());}
}
