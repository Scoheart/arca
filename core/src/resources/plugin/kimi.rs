//! Selected Kimi npm distribution and user-global plugin declarations.
//! No Node entry, version probe, lifecycle script or component is executed.
use super::*;
mod packages;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    command: String,
    package_name: String,
    version: String,
    entry: String,
    builtin_skills: Vec<String>,
    members: Vec<Member>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Member { path: String, bytes: u64, sha256: String }
struct Runtime { root: PathBuf, entry: PathBuf }

fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-kimi.json"))
        .expect("audited Kimi npm contract"))
}

fn layout(c: &Contract, selected: &Path) -> Result<Runtime, ()> {
    if !selected.is_absolute() || selected.to_str().is_none_or(|s|s.chars().any(char::is_control))
        || ["NODE_OPTIONS", "NODE_PATH"].iter().any(|key|std::env::var_os(key).is_some_and(|v|!v.is_empty())) { return Err(()); }
    // The selected executable may be npm's ordinary bin symlink. Never select
    // another installation after this physical entry fails its identity gate.
    let entry=fs::canonicalize(selected).map_err(|_|())?;
    checked_root(&entry)?;
    let root=entry.parent().and_then(Path::parent).ok_or(())?.to_path_buf();
    if relative(&root,&c.entry)?!=entry { return Err(()); }
    Ok(Runtime {root,entry})
}

fn verified_runtime(c: &Contract, selected: &Path, budget: &mut ScanBudget) -> Result<Runtime, ()> {
    let runtime=layout(c,selected)?;
    for member in &c.members { verify_artifact(&relative(&runtime.root,&member.path)?,member.bytes,&member.sha256,budget)?; }
    let package=optional_json(&runtime.root,"package.json",budget)?.ok_or(())?;
    if package.get("name").and_then(Value::as_str)!=Some(c.package_name.as_str())
        || package.get("version").and_then(Value::as_str)!=Some(c.version.as_str())
        || package.get("type").and_then(Value::as_str)!=Some("module")
        || package.get("bin").and_then(|v|v.get(&c.command)).and_then(Value::as_str)!=Some(c.entry.as_str()) { return Err(()); }
    let after=layout(c,selected)?;
    if after.root!=runtime.root || after.entry!=runtime.entry { return Err(()); }
    Ok(runtime)
}

pub(super) fn runtime_version(selected: &Path) -> Option<String> {
    verified_runtime(contract(),selected,&mut ScanBudget::default()).ok().map(|_|contract().version.clone())
}

fn global_base(value: Option<&str>) -> Option<PathBuf> {
    let value=value?; let path=PathBuf::from(value);
    (path.is_absolute() && value.len()<=4096 && !value.chars().any(char::is_control)).then_some(path)
}

fn global_plugins(host: &Host) -> Option<PathBuf> {
    // Native nullish selection treats even an empty override as explicit.
    // Unsupported relative/empty values cannot redirect us into a project or
    // silently make the default home look like the client's selected home.
    if let Some(value)=std::env::var_os("KIMI_CODE_HOME") { return global_base(value.to_str()).map(|p|p.join("plugins")); }
    let [source]=host.roots.as_slice() else { return None; };
    if source.layout!="kimi_registry" || source.presence!=PluginPresence::Installed { return None; }
    let root=expand_tilde(&source.path);
    global_base(root.to_str())
}

fn bundled(host: &Host, c: &Contract, runtime: &Runtime, result: &mut PluginScan) {
    observe_compiled_skills(host,&c.version,collapse_home(&runtime.entry),
        collapse_home(&runtime.root.join("package.json")),&c.builtin_skills,result);
}

pub(super) fn observe_compiled_skills(host: &Host, version: &str, source: String, manifest: String,
    skills: &[String], result: &mut PluginScan) {
    // These are compiled-in shipped resources, not filesystem SKILL.md files.
    // Product flags, experiments, TUI scope and session membership are unknown.
    let components=skills.iter().map(|name|PluginComponent {
        kind:PluginComponentKind::Skill, name:name.clone(), source_path:source.clone(), native_id:None,
    }).collect();
    result.plugins.push(NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:"@builtin/skills".into(),name:"Kimi Code bundled Skills".into(),
        version:version.into(),revision:source.clone(),presence:PluginPresence::Bundled,enabled:None,
        source_paths:vec![source,manifest],components,
        docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    });
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let c=contract();let mut verified=false;
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command) {
        match verified_runtime(c,&selected,&mut ScanBudget::default()) {
            Ok(runtime)=>{bundled(host,c,&runtime,result);verified=true;},
            Err(())=>result.unavailable=true,
        }
    }
    observe_registered(host,verified,result);
}

pub(super) fn observe_registered(host: &Host, verified: bool, result: &mut PluginScan) {
    match global_plugins(host) {
        Some(root)=>packages::observe_registry(host,&root,verified,result),
        None=>result.unavailable=true,
    }
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let c=contract();let mut paths=crate::application::agent_launch::command_candidates(&c.command);
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command) {
        if let Ok(physical)=fs::canonicalize(&selected) { paths.push(physical); }
        if let Ok(runtime)=layout(c,&selected) {
            paths.push(runtime.root.clone());
            paths.extend(c.members.iter().filter_map(|m|relative(&runtime.root,&m.path).ok()));
        }
    }
    paths.extend(registry_watch_paths(host));
    paths.sort();paths.dedup();paths
}

pub(super) fn registry_watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut paths=Vec::new();
    if let Some(root)=global_plugins(host) {
        paths.push(root.clone());paths.push(root.join("installed.json"));
        paths.extend(packages::registered_watch_paths(&root));
    }
    paths.sort();paths.dedup();paths
}

#[cfg(test)]
mod tests;
