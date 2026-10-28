//! Current Wake package and global registry declarations. All reads are inert;
//! no daemon, CLI, hook, setup action, module, migration or writer is invoked.
use super::*;
use super::electron_package::{app_version, guarded_json, Guard};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    agent_id: String, product_name: String, bundle_id: String, shell_version: String,
    version: String, home_directory: String, backend: String, resources: String,
    installer: Guard, bun: bun_package::Contract, native_plugins: Vec<Native>,
    shipped_plugins: Vec<Guard>, skills_manifest: Guard, cli_path: String, cli: Cli,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Native { id: String, name: String, version: String, components: Vec<Contribution> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contribution { kind: PluginComponentKind, name: String, native_id: Option<String> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cli {
    product_name: String, version: String, bun: bun_package::Contract,
    skill_source: String, code_skills: Vec<String>, builtin_plugins: Vec<super::qoder_cn_cli::Builtin>,
}
fn profiles() -> &'static [Profile] {
    static VALUE: OnceLock<Vec<Profile>> = OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-qoder-wake.json")).expect("audited Wake package profiles"))
}
fn app_path(source: &Root) -> PathBuf {
    if source.path.starts_with("/Applications/") { crate::paths::system_probe_path(&source.path) } else { expand_tilde(&source.path) }
}
fn absolute(path: PathBuf) -> Result<PathBuf, ()> {
    // Native path.resolve does not expand '~'; a relative path depends on the
    // unobserved native CWD. Never substitute Arca's own working directory.
    if !path.is_absolute() || path.to_str().is_none_or(|s|s.chars().any(char::is_control)) { return Err(()); }
    let mut clean=PathBuf::new();
    for part in path.components() { match part { Component::ParentDir=>{clean.pop();},Component::CurDir=>{},other=>clean.push(other.as_os_str()) } }
    if expand_tilde("~/").starts_with(&clean) { return Err(()); }
    checked_root(&clean)?;Ok(clean)
}
fn native_home(profile: &Profile) -> Result<PathBuf, ()> {
    let value=std::env::var_os("QODERWAKE_HOME").map(|v|v.into_string().map_err(|_|())).transpose()?;
    let root=value.filter(|s|!s.trim().is_empty()).map(PathBuf::from).unwrap_or_else(||expand_tilde("~/").join(&profile.home_directory));
    let root=absolute(root)?;
    let Some(isolation)=std::env::var_os("QODERWAKE_TEST_ISOLATION_ROOT").filter(|v|!v.is_empty()) else {return Ok(root);};
    let isolation=PathBuf::from(isolation.into_string().map_err(|_|())?.trim());
    let isolation=absolute(isolation)?;
    let temporary=std::env::temp_dir();
    if !isolation.starts_with(&temporary) {return Err(());}
    if root.starts_with(&temporary) {Ok(root)} else {absolute(isolation.join("runtime"))}
}
fn valid_name(name: &str) -> bool {
    let b=name.as_bytes();(2..=64).contains(&b.len()) && b[0].is_ascii_lowercase()
        && b.iter().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||*c==b'-')
}
fn valid_version(version: &str) -> bool {
    static VALUE: OnceLock<regex::Regex> = OnceLock::new();
    VALUE.get_or_init(||regex::Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$").expect("native Wake SemVer grammar")).is_match(version)
        && metadata_name(version).is_some()
}
fn snapshot(host: &Host, id: &str, name: &str, version: &str, presence: PluginPresence, source: &str) -> Result<NativePluginObservation, ()> {
    Ok(NativePluginObservation {agent_id:host.agent_id.clone(),plugin_id:metadata_name(id).ok_or(())?,name:metadata_name(name).ok_or(())?,
        version:metadata_name(version).ok_or(())?,revision:"native-declaration".into(),presence,enabled:None,source_paths:vec![source.into()],
        components:Vec::new(),docs_url:host.docs_url.clone(),source_fingerprint:String::new()})
}
fn add(item: &mut NativePluginObservation, kind: PluginComponentKind, name: &str, source: String, native_id: Option<String>) -> Result<(), ()> {
    if item.components.len()>=MAX_COMPONENTS || native_id.as_ref().is_some_and(|id|metadata_name(id).is_none()) {return Err(());}
    item.components.push(PluginComponent {kind,name:metadata_name(name).ok_or(())?,source_path:source,native_id});Ok(())
}
fn file(root: &Path, path: &str) -> Result<String, ()> {
    let path=relative(root,path)?;if !regular_metadata_file(&path)? {return Err(());}Ok(collapse_home(&path))
}
fn package(host: &Host, root: &Path, manifest: &Value, presence: PluginPresence, registry: Option<&Path>) -> Result<NativePluginObservation, ()> {
    if manifest.get("schemaVersion").and_then(Value::as_str)!=Some("qoderwake.plugin.v1") {return Err(());}
    let name=manifest.get("name").and_then(Value::as_str).filter(|s|valid_name(s)).ok_or(())?;
    let version=manifest.get("version").and_then(Value::as_str).filter(|s|valid_version(s)).ok_or(())?;
    let display=manifest.get("displayName").and_then(Value::as_str).ok_or(())?;
    let source=file(root,"qoderwake-plugin.json")?;
    let mut item=snapshot(host,name,display,version,presence,&source)?;
    item.revision=format!("manifest:{}:{}",collapse_home(root),hex::encode(Sha256::digest(serde_json::to_vec(manifest).map_err(|_|())?)));
    if let Some(registry)=registry {item.source_paths.push(collapse_home(registry));}
    let capabilities=manifest.get("capabilities").and_then(Value::as_object).ok_or(())?;
    if let Some(trigger)=capabilities.get("trigger") {
        let trigger=trigger.as_object().ok_or(())?;
        for (field,webhook) in [("eventSources",false),("webhookEventSources",true)] {
            let Some(entries)=trigger.get(field) else {continue;};
            for entry in entries.as_array().filter(|v|v.len()<=MAX_COMPONENTS).ok_or(())? {
                let kind=entry.get("kind").and_then(Value::as_str).ok_or(())?;
                if !webhook && !valid_name(kind) {return Err(());}
                add(&mut item,PluginComponentKind::Channel,kind,source.clone(),None)?;
                let runtime=entry.get("runtime").and_then(Value::as_object).ok_or(())?;
                let module=if webhook {runtime.get("module")} else {
                    if runtime.get("language").and_then(Value::as_str)!=Some("python") {return Err(());}
                    runtime.get("entry")
                };
                if let Some(module)=module {
                    let module=module.as_str().ok_or(())?;
                    if !webhook && !module.ends_with(".py") {return Err(());}
                    add(&mut item,if webhook {PluginComponentKind::Code} else {PluginComponentKind::Executable},module,file(root,module)?,None)?;
                }
            }
        }
    }
    if let Some(events)=capabilities.get("events") {
        let module=events.get("module").and_then(Value::as_str).ok_or(())?;
        let path=file(root,module)?;
        for name in events.get("subscribe").and_then(Value::as_array).filter(|v|v.len()<=MAX_COMPONENTS).ok_or(())? {
            add(&mut item,PluginComponentKind::Hook,name.as_str().ok_or(())?,path.clone(),None)?;
        }
    }
    if let Some(agent_loop)=capabilities.get("agentLoop") {
        let module=agent_loop.get("module").and_then(Value::as_str).ok_or(())?;
        add(&mut item,PluginComponentKind::Code,"Agent Loop",file(root,module)?,None)?;
        if let Some(runtimes)=agent_loop.get("runtimes") {
            for runtime in runtimes.as_array().filter(|v|v.len()<=MAX_COMPONENTS).ok_or(())? {
                add(&mut item,PluginComponentKind::Workflow,runtime.get("id").and_then(Value::as_str).ok_or(())?,source.clone(),None)?;
            }
        }
    }
    if capabilities.contains_key("settings") {add(&mut item,PluginComponentKind::Policy,"Native settings schema",source,None)?;}
    item.components.sort();item.components.dedup();Ok(item)
}
fn installed_path(root: &Path, name: &str, version: &str, entry: &Value) -> Result<PathBuf, ()> {
    if !valid_name(name) || !valid_version(version) {return Err(());}
    match entry.get("path") {
        Some(Value::String(path))=>absolute(PathBuf::from(path)),
        _=>absolute(root.join("plugins").join(name).join(version)),
    }
}
fn installed(host: &Host, profile: &Profile, result: &mut PluginScan) -> Result<(), ()> {
    let home=native_home(profile)?;
    let Some(registry)=optional_json(&home,"plugins/registry.json",&mut result.budget)? else {return Ok(());};
    if registry.get("schemaVersion").and_then(Value::as_str)!=Some("qoderwake.plugin.registry.v1") {return Err(());}
    let entries=registry.get("plugins").and_then(Value::as_object).filter(|v|v.len()<=MAX_ENTRIES).ok_or(())?;
    for (name,entry) in entries {
        let read=(||{
            if !valid_name(name) {return Err(());}
            let versions=entry.get("versions").and_then(Value::as_object).filter(|v|v.len()<=MAX_ENTRIES).ok_or(())?;
            for (version,record) in versions {
                result.budget.visit()?;if !record.is_object() {return Err(());}
                let path=installed_path(&home,name,version,record)?;
                let registry=home.join("plugins/registry.json");
                let item=if !path.try_exists().map_err(|_|())? {
                    snapshot(host,name,name,version,PluginPresence::Declared,&collapse_home(&registry))?
                } else {
                    let manifest=optional_json(&path,"qoderwake-plugin.json",&mut result.budget)?.ok_or(())?;
                    if manifest.get("name").and_then(Value::as_str)!=Some(name) || manifest.get("version").and_then(Value::as_str)!=Some(version) {return Err(());}
                    package(host,&path,&manifest,PluginPresence::Installed,Some(&registry))?
                };
                result.plugins.push(item);
            }Ok::<(),()>(())
        })();if read.is_err(){result.unavailable=true;}
    }Ok(())
}
fn bundled(host: &Host, app: &Path, profile: &Profile, result: &mut PluginScan) -> Result<(), ()> {
    if app_version(app,&profile.bundle_id,&profile.product_name,&mut result.budget)?!=profile.shell_version {return Err(());}
    let contents=relative(app,"Contents/Resources")?;
    let installer=guarded_json(&relative(&contents,&profile.installer.path)?,&profile.installer,&mut result.budget)?;
    if installer.get("schemaVersion").and_then(Value::as_str)!=Some("qoderwake.nativeInstaller.v1") {return Err(());}
    let graph=bun_package::Graph::open(&relative(&contents,&profile.backend)?,&profile.bun,&mut result.budget)?;
    let source=graph.source(&profile.bun.entry_name)?;
    for fact in &profile.native_plugins {
        let mut item=snapshot(host,&fact.id,&fact.name,&fact.version,PluginPresence::Bundled,&source)?;
        item.revision=format!("audited-wake:{}",profile.version);
        for part in &fact.components {add(&mut item,part.kind.clone(),&part.name,source.clone(),part.native_id.clone())?;}
        result.plugins.push(item);
    }
    let resources=relative(&contents,&profile.resources)?;
    for guard in &profile.shipped_plugins {
        let manifest=guarded_json(&relative(&resources,&guard.path)?,guard,&mut result.budget)?;
        let root=relative(&resources,&guard.path)?.parent().ok_or(())?.to_path_buf();
        match package(host,&root,&manifest,PluginPresence::Bundled,None) {Ok(item)=>result.plugins.push(item),Err(())=>result.unavailable=true,}
    }
    let manifest=guarded_json(&relative(&resources,&profile.skills_manifest.path)?,&profile.skills_manifest,&mut result.budget)?;
    if manifest.get("version").and_then(Value::as_str)!=Some("1") {return Err(());}
    let skills=manifest.get("skills").and_then(Value::as_array).filter(|v|v.len()<=MAX_COMPONENTS).ok_or(())?;
    let mut seen=BTreeSet::new();
    for skill in skills {
        let id=skill.get("skillId").and_then(Value::as_str).filter(|s|valid_name(s)).ok_or(())?;
        if !seen.insert(id) {return Err(());}
        let dir=skill.get("dirName").and_then(Value::as_str).ok_or(())?;
        let version=skill.get("version").and_then(Value::as_str).filter(|s|valid_version(s)).ok_or(())?;
        let name=skill.get("name").and_then(Value::as_str).ok_or(())?;
        let source=collapse_home(&relative(&resources,&profile.skills_manifest.path)?);
        let mut item=snapshot(host,&format!("@builtin/skill/{id}"),name,version,PluginPresence::Bundled,&source)?;
        add(&mut item,PluginComponentKind::Skill,id,file(&resources,&format!("builtin-skills/{dir}/SKILL.md"))?,None)?;
        result.plugins.push(item);
    }
    graph.unchanged()?;
    let cli=bun_package::Graph::open(&relative(&contents,&profile.cli_path)?,&profile.cli.bun,&mut result.budget)?;
    let mut items=super::qoder_cn_cli::bundled(host,&cli,&profile.cli.product_name,&profile.cli.version,&profile.cli.skill_source,&profile.cli.code_skills,&profile.cli.builtin_plugins,result)?;
    for item in &mut items {
        item.plugin_id=format!("@cli/{}",item.plugin_id);
        item.name=format!("{}: {}",profile.cli.product_name,item.name);
        // Wake explicitly passes --disable-builtin-skills. These are physical
        // bundled CLI declarations, not evidence of a Wake session's Skills.
    }
    result.plugins.extend(items);cli.unchanged()?;Ok(())
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let Some(profile)=profiles().iter().find(|p|p.agent_id==host.agent_id) else {result.unavailable=true;return;};
    for root in host.roots.iter().filter(|r|r.layout=="qoder_wake_app") {
        let path=app_path(root);
        match fs::symlink_metadata(&path) {Err(e) if e.kind()==std::io::ErrorKind::NotFound=>continue,Ok(m) if m.is_dir()=>{},_=>{result.unavailable=true;break;}}
        if bundled(host,&path,profile,result).is_err(){result.unavailable=true;}break;
    }
    if installed(host,profile,result).is_err(){result.unavailable=true;}
}
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut paths=host.roots.iter().map(app_path).collect::<Vec<_>>();
    let Some(profile)=profiles().iter().find(|p|p.agent_id==host.agent_id) else {return paths;};
    if let Ok(home)=native_home(profile) {
        paths.push(home.join("plugins"));let mut budget=ScanBudget::default();
        if let Ok(Some(registry))=optional_json(&home,"plugins/registry.json",&mut budget) {
            if registry.get("schemaVersion").and_then(Value::as_str)==Some("qoderwake.plugin.registry.v1") {
                if let Some(plugins)=registry.get("plugins").and_then(Value::as_object).filter(|v|v.len()<=MAX_ENTRIES) {
                    for (name,plugin) in plugins {if let Some(versions)=plugin.get("versions").and_then(Value::as_object).filter(|v|v.len()<=MAX_ENTRIES) {
                        for (version,entry) in versions {if budget.visit().is_err(){return paths;}if let Ok(path)=installed_path(&home,name,version,entry){paths.push(path);}}
                    }}
                }
            }
        }
    }paths.sort();paths.dedup();paths
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_registry_identifiers_and_paths_remain_metadata_only() {
        assert!(valid_name("qa-support-loop"));assert!(!valid_name("../outside"));assert!(!valid_name("a"));
        assert!(valid_version("1.2.4-cn"));assert!(!valid_version("1.2"));assert!(!valid_version("1/2/3"));
        assert!(absolute(PathBuf::from("~/plugins")).is_err());assert!(absolute(PathBuf::from("relative/plugins")).is_err());
        assert!(absolute(expand_tilde("~/")).is_err());assert!(absolute(PathBuf::from("/")).is_err());
        let host=hosts().iter().find(|h|h.adapter=="qoder_wake").unwrap();
        let item=snapshot(host,"fixture","Fixture","1.0.0",PluginPresence::Declared,"registry.json").unwrap();
        let value=serde_json::to_value(item).unwrap();assert!(value.get("asset").is_none());assert!(value.get("available_actions").is_none());assert!(value["enabled"].is_null());
    }
}
