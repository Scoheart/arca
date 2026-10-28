//! Independent Goose CLI declarations pinned to its selected native executable.
//! No command, version probe, configuration initializer or factory is executed.
use super::*;
use crate::domain::mcp::BuiltinMcpObservation;
use crate::resources::mcp::builtin::BuiltinMcpScan;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id: String, command: String, version: String, product_name: String,
    docs_url: String, binary: Binary, platform_extensions: Vec<Extension>,
    embedded_skills: Vec<EmbeddedSkill>, mcp_extensions: Vec<Extension>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binary { bytes: u64, sha256: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Extension { id: String, name: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddedSkill { name: String, member: String }

fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-goose-cli.json"))
        .expect("audited Goose CLI contract"))
}

fn verified_launcher(c: &Contract, launcher: &Path, budget: &mut ScanBudget) -> Result<PathBuf, ()> {
    if !launcher.is_absolute() || launcher.to_str().is_none_or(|s|s.chars().any(char::is_control)) { return Err(()); }
    // Only this selected command's alias is resolved. An unknown wrapper is a
    // failed observation, never permission to rank a different installation.
    let physical=fs::canonicalize(launcher).map_err(|_|())?;
    if physical.to_str().is_none_or(|s|s.chars().any(char::is_control)) { return Err(()); }
    checked_root(&physical)?;
    verify_artifact(&physical,c.binary.bytes,&c.binary.sha256,budget)?;
    if fs::canonicalize(launcher).map_err(|_|())?!=physical { return Err(()); }
    Ok(physical)
}
fn verified_executable(c: &Contract, budget: &mut ScanBudget) -> Result<Option<PathBuf>, ()> {
    crate::application::agent_launch::find_command(&c.command)
        .map(|launcher|verified_launcher(c,&launcher,budget)).transpose()
}

pub(super) fn runtime_version(path: &Path) -> Option<String> {
    let c=contract();verified_launcher(c,path,&mut ScanBudget::default()).ok().map(|_|c.version.clone())
}

fn compiled_plugins(host: &Host, c: &Contract, binary: &Path, result: &mut PluginScan) {
    for extension in &c.platform_extensions {
        let source=format!("{}#goose::agents::platform_extensions::PLATFORM_EXTENSIONS::{}",collapse_home(binary),extension.id);
        let mut components=vec![PluginComponent {kind:PluginComponentKind::Code,name:"Platform tools".into(),source_path:source.clone(),native_id:None}];
        if extension.id=="skills" {
            for skill in &c.embedded_skills {
                components.push(PluginComponent {kind:PluginComponentKind::Skill,name:skill.name.clone(),
                    source_path:format!("{}#goose::skills::builtin::BUILTIN_SKILLS_DIR/{}",collapse_home(binary),skill.member),native_id:None});
            }
        }
        result.plugins.push(NativePluginObservation {
            agent_id:host.agent_id.clone(),plugin_id:format!("@builtin/{}",extension.id),name:extension.name.clone(),
            version:c.version.clone(),revision:c.binary.sha256.clone(),presence:PluginPresence::Bundled,enabled:None,
            source_paths:components.iter().map(|item|item.source_path.clone()).collect(),components,
            docs_url:c.docs_url.clone(),source_fingerprint:String::new(),
        });
    }
}

fn builtin_items(c: &Contract, binary: &Path) -> Vec<BuiltinMcpObservation> {
    let manifest=format!("{}#goose_mcp::BUILTIN_EXTENSIONS",collapse_home(binary));
    c.mcp_extensions.iter().map(|extension| {
        let source=format!("{manifest}::{}",extension.id);
        BuiltinMcpObservation {
            agent_id:c.agent_id.clone(),plugin_id:format!("goose-{}",extension.id),plugin_version:c.version.clone(),
            manifest_path:manifest.clone(),server_id:extension.id.clone(),name:extension.name.clone(),provider:c.product_name.clone(),
            source_path:source.clone(),app_version:c.version.clone(),docs_url:c.docs_url.clone(),
            source_fingerprint:hex::encode(Sha256::digest(format!("{}:{}:{}:{source}",c.agent_id,c.version,c.binary.sha256).as_bytes())),
        }
    }).collect()
}
pub(crate) fn builtin_observe(agents: &BTreeMap<String, AgentDefinition>) -> BuiltinMcpScan {
    let c=contract();let mut scan=BuiltinMcpScan::default();
    if !agents.get(&c.agent_id).is_some_and(|a|a.enabled) { return scan; }
    // Separate finite native budget, independent of other hosts and private JSON.
    match verified_executable(c,&mut ScanBudget::default()) {
        Ok(Some(binary))=>scan.items=builtin_items(c,&binary),Ok(None)=>{},Err(())=>scan.unavailable=true,
    }
    scan
}

fn launcher_watch_paths(c: &Contract) -> Vec<PathBuf> {
    let mut paths=crate::application::agent_launch::command_candidates(&c.command);
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command) {
        if let Ok(physical)=fs::canonicalize(selected) { paths.push(physical); }
    }
    paths.into_iter().filter(|path|path.to_str().is_some_and(|s|!s.chars().any(char::is_control)))
        .collect::<BTreeSet<_>>().into_iter().collect()
}
pub(crate) fn builtin_watch_paths(agents: &BTreeMap<String, AgentDefinition>) -> Vec<PathBuf> {
    let c=contract();if !agents.get(&c.agent_id).is_some_and(|a|a.enabled) { return Vec::new(); }launcher_watch_paths(c)
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let c=contract();if host.agent_id!=c.agent_id {result.unavailable=true;return;}
    match verified_executable(c,&mut ScanBudget::default()) {
        Ok(Some(binary))=>compiled_plugins(host,c,&binary,result),Ok(None)=>{},Err(())=>result.unavailable=true,
    }
    super::goose_plugins::observe(host,result);
}
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut paths=launcher_watch_paths(contract());paths.extend(super::goose_plugins::watch_paths(host));paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    fn fixture(home: &TestHome) -> (Contract, PathBuf) {
        let path=home.home.join("bin/goose");
        fs::write(&path,b"#!/bin/sh\nprintf triggered > \"$HOME/executed-marker\"\n").unwrap();
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&path,fs::Permissions::from_mode(0o755)).unwrap(); }
        let mut c: Contract=serde_json::from_str(include_str!("../../../../data/native-plugin-goose-cli.json")).unwrap();
        c.binary.bytes=53;
        c.binary.sha256="5f836c6c9227f0f57ed000e134a5fbfb5282cafd5d42abf74489ec70e1b87442".into();
        (c,path)
    }

    #[test]
    fn selected_launcher_is_hashed_without_execution_and_changed_content_is_rejected() {
        let home=TestHome::new("goose-cli-guard");let (c,path)=fixture(&home);
        assert_eq!(verified_executable(&c,&mut ScanBudget::default()).unwrap(),Some(path.clone()));
        assert_eq!(crate::application::agent_launch::runtime_version("goose").unwrap(),None);
        assert!(!home.home.join("executed-marker").exists());
        fs::write(&path,b"#!/bin/sh\nexit 0\n").unwrap();
        let fallback=home.home.join(".local/bin/goose");fs::create_dir_all(fallback.parent().unwrap()).unwrap();
        fs::write(&fallback,b"#!/bin/sh\nprintf triggered > \"$HOME/executed-marker\"\n").unwrap();
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&fallback,fs::Permissions::from_mode(0o755)).unwrap(); }
        assert!(verified_executable(&c,&mut ScanBudget::default()).is_err());
        assert!(!home.home.join("executed-marker").exists());
    }

    #[cfg(unix)]
    #[test]
    fn only_the_selected_launcher_alias_is_resolved_and_watch_inputs_keep_both_paths() {
        let home=TestHome::new("goose-cli-alias");let (c,launcher)=fixture(&home);
        let physical=home.home.join("runtime/goose");fs::create_dir_all(physical.parent().unwrap()).unwrap();
        fs::rename(&launcher,&physical).unwrap();std::os::unix::fs::symlink(&physical,&launcher).unwrap();
        assert_eq!(verified_executable(&c,&mut ScanBudget::default()).unwrap(),Some(physical.clone()));
        let paths=launcher_watch_paths(&c);assert!(paths.contains(&launcher));assert!(paths.contains(&physical));
        assert!(!home.home.join("executed-marker").exists());
    }

    #[test]
    fn platform_tools_and_embedded_skills_do_not_create_false_host_mcps() {
        let home=TestHome::new("goose-cli-types");let (c,path)=fixture(&home);
        let host=hosts().iter().find(|h|h.agent_id=="goose").unwrap();let mut scan=PluginScan::default();
        compiled_plugins(host,&c,&path,&mut scan);normalize(&mut scan);
        assert_eq!(scan.plugins.len(),12);assert!(scan.mcps.is_empty());
        assert_eq!(scan.plugins.iter().flat_map(|p|&p.components).filter(|c|c.kind==PluginComponentKind::Code).count(),12);
        let skills=scan.plugins.iter().find(|p|p.plugin_id=="@builtin/skills").unwrap();
        assert_eq!(skills.components.iter().filter(|c|c.kind==PluginComponentKind::Skill).map(|c|c.name.as_str()).collect::<Vec<_>>(),vec!["goose-doc-guide","web-search"]);
        assert!(scan.plugins.iter().all(|p|p.enabled.is_none()&&p.presence==PluginPresence::Bundled));
        let mcps=builtin_items(&c,&path);assert_eq!(mcps.len(),4);
        assert_eq!(mcps.iter().map(|m|m.server_id.as_str()).collect::<Vec<_>>(),vec!["autovisualiser","computercontroller","memory","tutorial"]);
        assert!(mcps.iter().all(|m|m.agent_id=="goose"&&m.manifest_path.contains("goose_mcp::BUILTIN_EXTENSIONS")));
    }

    #[test]
    fn missing_or_disabled_host_has_no_compiled_resources_or_active_version_fallback() {
        let home=TestHome::new("goose-cli-disabled");
        assert_eq!(verified_executable(contract(),&mut ScanBudget::default()).unwrap(),None);
        let result=builtin_observe(&BTreeMap::new());assert!(result.items.is_empty()&&!result.unavailable);
        assert!(builtin_watch_paths(&BTreeMap::new()).is_empty());
        assert_eq!(passive_runtime_version("goose",&home.home.join("missing")),Some(None));
    }
}
