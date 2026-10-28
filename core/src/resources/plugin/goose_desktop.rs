//! Goose's default desktop catalog and confined global Open Plugins metadata.
//! Native discovery/auto-update mutate private state and are never invoked.
use super::*;
use super::electron_package::{app_version, guard_asar, Guard};
use crate::domain::mcp::BuiltinMcpObservation;
use crate::resources::mcp::builtin::BuiltinMcpScan;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id: String, version: String, bundle_id: String, product_name: String,
    app_paths: Vec<String>, docs_url: String, guards: Vec<Guard>, backend: Guard,
    catalog_member: String, platform_extensions: Vec<Extension>, mcp_extensions: Vec<Extension>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Extension { id: String, name: String }
fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-goose-desktop.json"))
        .expect("audited Goose Desktop contract"))
}
fn app_path(input: &str) -> PathBuf {
    if input.starts_with("~/") { expand_tilde(input) } else { crate::paths::system_probe_path(input) }
}
fn verified_app(c: &Contract, backend: bool, budget: &mut ScanBudget) -> Result<Option<PathBuf>, ()> {
    for app in c.app_paths.iter().map(|p| app_path(p)) {
        match fs::symlink_metadata(&app) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound => continue,
            Ok(m) if m.file_type().is_dir() => {}, _ => return Err(()),
        }
        if app_version(&app,&c.bundle_id,&c.product_name,budget)?!=c.version { return Err(()); }
        guard_asar(&relative(&app,"Contents/Resources/app.asar")?,&c.guards,budget)?;
        if backend { verify_artifact(&relative(&app,&c.backend.path)?,c.backend.bytes,&c.backend.sha256,budget)?; }
        return Ok(Some(app));
    }
    Ok(None)
}
pub(crate) fn builtin_observe(agents: &BTreeMap<String, AgentDefinition>) -> BuiltinMcpScan {
    let c=contract(); let mut scan=BuiltinMcpScan::default();
    if !agents.get(&c.agent_id).is_some_and(|a|a.enabled) { return scan; }
    // Full native hashing has a separate finite budget, so this native package
    // cannot consume another host's private metadata or artifact allowance.
    let app=match verified_app(c,true,&mut ScanBudget::default()) {
        Ok(Some(app))=>app, Ok(None)=>return scan, Err(())=>{scan.unavailable=true;return scan;},
    };
    let asar=app.join("Contents/Resources/app.asar"); let binary=app.join(&c.backend.path);
    for extension in &c.mcp_extensions {
        let source=format!("{}#goose_mcp::BUILTIN_EXTENSIONS::{}",collapse_home(&binary),extension.id);
        scan.items.push(BuiltinMcpObservation {
            agent_id:c.agent_id.clone(),plugin_id:format!("goose-{}",extension.id),plugin_version:c.version.clone(),
            manifest_path:format!("{}!{}#PU",collapse_home(&asar),c.catalog_member),
            server_id:extension.id.clone(),name:extension.name.clone(),provider:c.product_name.clone(),
            source_path:source.clone(),app_version:c.version.clone(),docs_url:c.docs_url.clone(),
            source_fingerprint:hex::encode(Sha256::digest(format!("{}:{}:{source}",c.version,c.backend.sha256).as_bytes())),
        });
    }
    scan
}
pub(crate) fn builtin_watch_paths(agents: &BTreeMap<String, AgentDefinition>) -> Vec<PathBuf> {
    let c=contract(); if !agents.get(&c.agent_id).is_some_and(|a|a.enabled) { return Vec::new(); }
    c.app_paths.iter().map(|p|app_path(p)).flat_map(|app|[
        app.clone(),app.join("Contents/Info.plist"),app.join("Contents/Resources/app.asar"),app.join(&c.backend.path),
    ]).collect()
}
fn plugin_root(source: &Root) -> Result<PathBuf, ()> {
    // The backend ignores relative overrides; Desktop clears them before spawn.
    let override_root=std::env::var_os("GOOSE_PATH_ROOT").map(PathBuf::from).filter(|p|p.is_absolute());
    let path=override_root.map(|p|p.join(".agents/plugins")).unwrap_or_else(||expand_tilde(&source.path));
    if path.to_str().is_none_or(|s|s.chars().any(char::is_control)) { return Err(()); }
    checked_root(&path)?; Ok(path)
}
#[derive(Default)]
struct ComponentPaths { paths: Vec<String>, exclusive: bool }
fn strings(value: &Value) -> Result<Vec<String>, ()> {
    match value {
        Value::String(s)=>Ok(vec![s.clone()]),
        Value::Array(a) if a.len()<=64=>a.iter().map(|v|v.as_str().map(str::to_owned).ok_or(())).collect(),
        _=>Err(()),
    }
}
fn component_paths(value: Option<&Value>) -> Result<ComponentPaths, ()> {
    let Some(value)=value.filter(|v|!v.is_null()) else { return Ok(ComponentPaths::default()); };
    let result=if let Some(v)=value.as_object() {
        ComponentPaths { paths:v.get("paths").map(strings).transpose()?.unwrap_or_default(),
            exclusive:v.get("exclusive").map(|v|v.as_bool().ok_or(())).transpose()?.unwrap_or(false) }
    } else { ComponentPaths { paths:strings(value)?,exclusive:false } };
    if result.paths.iter().any(|p|!p.starts_with("./")) { return Err(()); }
    Ok(result)
}
fn is_dir(path: &Path) -> Result<bool, ()> {
    checked_root(path)?;
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir()=>Ok(true),Ok(m) if m.is_file()=>Ok(false),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(false),_=>Err(()),
    }
}
fn skill(root: &Path, directory: &Path, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    let path=directory.join("SKILL.md"); if !regular_metadata_file(&path)? { return Ok(()); }
    if components.len()>=MAX_COMPONENTS { return Err(()); }
    let label=path.strip_prefix(root).map_err(|_|())?.to_str().and_then(metadata_name).ok_or(())?;
    components.push(PluginComponent { kind:PluginComponentKind::Skill,name:label,source_path:collapse_home(&path),native_id:None }); Ok(())
}
fn skills(root: &Path, value: Option<&Value>, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let selected=component_paths(value)?; let mut paths=Vec::new();
    if !selected.exclusive { paths.push(root.join("skills")); }
    for p in selected.paths { paths.push(relative(root,&p)?); }
    for path in paths.into_iter().collect::<BTreeSet<_>>() {
        if !is_dir(&path)? { continue; }
        skill(root,&path,components)?;
        for child in dirs(&path,budget)? { skill(root,&child,components)?; }
    }
    if value.is_none_or(Value::is_null) && !is_dir(&root.join("skills"))? { skill(root,root,components)?; }
    Ok(())
}
fn stdio(value: &Value) -> bool {
    value.is_object() && value.get("command").is_some_and(Value::is_string)
        && value.get("args").is_none_or(|v|v.as_array().is_some_and(|a|a.len()<=MAX_COMPONENTS&&a.iter().all(Value::is_string)))
        && value.get("env").is_none_or(|v|v.as_object().is_some_and(|o|o.len()<=MAX_COMPONENTS&&o.values().all(Value::is_string)))
        && value.get("cwd").is_none_or(|v|v.is_null()||v.is_string())
}
fn extension_key(name: &str) -> String {
    name.chars().filter(|c|!c.is_whitespace()).map(|c| {
        if c.is_ascii_alphanumeric() || matches!(c,'_'|'-') { c.to_ascii_lowercase() } else { '_' }
    }).collect()
}
fn servers(value: &Value, folder: &str, source: &Path) -> Result<Vec<PluginComponent>, ()> {
    let object=value.as_object().filter(|o|o.len()<=MAX_COMPONENTS).ok_or(())?;
    object.iter().map(|(name,value)| {
        if !stdio(value) { return Err(()); }
        let name=metadata_name(name).ok_or(())?;
        let key=extension_key(&format!("{folder}:{name}"));
        Ok(PluginComponent { kind:PluginComponentKind::Mcp,native_id:Some(key),name,source_path:collapse_home(source) })
    }).collect()
}
fn mcps(root: &Path, folder: &str, manifest: &str, value: Option<&Value>, budget: &mut ScanBudget) -> Result<Vec<PluginComponent>, ()> {
    let value=value.filter(|v|!v.is_null());
    let inline=value.filter(|v|v.as_object().is_some_and(|o|!o.keys().all(|k|matches!(k.as_str(),"paths"|"exclusive"))));
    let selected=if inline.is_some() { ComponentPaths::default() } else { component_paths(value)? };
    let mut paths=Vec::new(); if !selected.exclusive { paths.push(".mcp.json".to_string()); }
    paths.extend(selected.paths); let mut seen=BTreeSet::new(); let mut components=Vec::new();
    for path in paths {
        let file=relative(root,&path)?; if !seen.insert(file.clone()) { continue; }
        let Some(document)=optional_json(root,&path,budget)? else { continue; };
        if !document.is_object() { return Err(()); }
        if let Some(value)=document.get("mcpServers") { components.extend(servers(value,folder,&file)?); }
        if components.len()>MAX_COMPONENTS { return Err(()); }
    }
    if let Some(value)=inline { components.extend(servers(value,folder,&relative(root,manifest)?)?); }
    if components.len()>MAX_COMPONENTS { return Err(()); } Ok(components)
}
#[derive(Deserialize, Default)]
struct Manifest { name: Option<String>, version: Option<String>, skills: Option<Value>, #[serde(rename="mcpServers")] mcp_servers: Option<Value> }
fn package(host: &Host, root: &Path, result: &mut PluginScan) -> Result<(), ()> {
    checked_root(root)?; result.budget.visit()?;
    let folder=root.file_name().and_then(|s|s.to_str()).and_then(metadata_name).ok_or(())?;
    let mut selected=None;
    for path in &host.manifest_paths {
        if let Some(value)=optional_json(root,path,&mut result.budget)? { selected=Some((path.clone(),value));break; }
    }
    let (manifest,path)=match selected {
        Some((path,value))=>(serde_json::from_value::<Manifest>(value.as_ref().clone()).map_err(|_|())?,Some(path)),
        None=>(Manifest::default(),None),
    };
    let mut source_paths=vec![match &path {Some(p)=>collapse_home(&relative(root,p)?),None=>collapse_home(root)}];
    let mut presence=PluginPresence::Local;
    match optional_json(root,".goose-plugin-install.json",&mut result.budget) {
        Ok(Some(value))=>{
            if value.get("source_type").and_then(Value::as_str)==Some("git")
                && value.get("source").is_some_and(Value::is_string)
                && value.get("format").and_then(Value::as_str).is_some_and(|s|matches!(s,"open-plugins"|"gemini")) {
                presence=PluginPresence::Installed; source_paths.push(collapse_home(&root.join(".goose-plugin-install.json")));
            }
        },Ok(None)=>{},Err(())=>result.unavailable=true,
    }
    let mut components=Vec::new();
    if skills(root,manifest.skills.as_ref(),&mut components,&mut result.budget).is_err() { result.unavailable=true; }
    // The native loader drops a plugin's entire MCP set on any parse failure.
    // Keep Skill observations independent, and never invent a winning duplicate.
    match mcps(root,&folder,path.as_deref().unwrap_or("plugin.json"),manifest.mcp_servers.as_ref(),&mut result.budget) {
        Ok(values) if values.len()<=MAX_COMPONENTS.saturating_sub(components.len())=>components.extend(values),
        _=>result.unavailable=true,
    }
    components.sort();components.dedup();
    result.plugins.push(NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:folder.clone(),
        name:manifest.name.as_deref().and_then(metadata_name).unwrap_or(folder),
        version:manifest.version.as_deref().and_then(metadata_name).unwrap_or_else(||"unknown".into()),
        revision:"global".into(),presence,enabled:None,source_paths,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    }); Ok(())
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let c=contract(); if host.agent_id!=c.agent_id { result.unavailable=true;return; }
    match verified_app(c,false,&mut result.budget) {
        Ok(Some(app))=>for extension in &c.platform_extensions {
            let source=format!("{}!{}#PU::{}",collapse_home(&app.join("Contents/Resources/app.asar")),c.catalog_member,extension.id);
            result.plugins.push(NativePluginObservation {
                agent_id:host.agent_id.clone(),plugin_id:format!("@builtin/{}",extension.id),name:extension.name.clone(),
                version:c.version.clone(),revision:c.guards[1].sha256.clone(),presence:PluginPresence::Bundled,enabled:None,
                source_paths:vec![source.clone()],components:vec![PluginComponent{kind:PluginComponentKind::Code,name:"Platform tools".into(),source_path:source,native_id:None}],
                docs_url:c.docs_url.clone(),source_fingerprint:String::new(),
            });
        },Ok(None)=>{},Err(())=>result.unavailable=true,
    }
    for source in &host.roots {
        let observed=(|| {
            if source.layout!="goose_global_plugins" {return Err(());}
            for root in dirs(&plugin_root(source)?,&mut result.budget)? {
                if package(host,&root,result).is_err() { result.unavailable=true; }
            } Ok::<(),()>(())
        })(); if observed.is_err() { result.unavailable=true; }
    }
}
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    host.roots.iter().filter_map(|source|plugin_root(source).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    #[test]
    fn global_paths_use_absolute_override_and_ignore_relative_native_override() {
        let home=TestHome::new("goose-global-path");let host=hosts().iter().find(|h|h.agent_id=="goose-desktop").unwrap();
        assert_eq!(plugin_root(&host.roots[0]).unwrap(),home.home.join("goose-root/.agents/plugins"));
        std::env::set_var("GOOSE_PATH_ROOT","relative");assert_eq!(plugin_root(&host.roots[0]).unwrap(),home.home.join(".agents/plugins"));
        assert!(component_paths(Some(&serde_json::json!({"paths":"../escape"}))).is_err());
        assert!(component_paths(Some(&serde_json::json!({"paths":null}))).is_err());
    }

    #[test]
    fn native_mcp_grammar_keeps_default_custom_and_inline_sources_without_exporting_values() {
        let home=TestHome::new("goose-components");let root=home.home.join("folder-id");fs::create_dir_all(root.join("extra")).unwrap();
        fs::write(root.join(".mcp.json"),r#"{"mcpServers":{"same":{"command":"PRIVATE_SENTINEL","env":{"TOKEN":"PRIVATE_SENTINEL"}}}}"#).unwrap();
        fs::write(root.join("extra/mcp.json"),r#"{"mcpServers":{"same":{"command":"other"}}}"#).unwrap();
        let selection=serde_json::json!({"paths":["./extra/mcp.json"]});
        let rows=mcps(&root,"folder-id","plugin.json",Some(&selection),&mut ScanBudget::default()).unwrap();
        assert_eq!(rows.len(),2);assert!(rows.iter().all(|r|r.native_id.as_deref()==Some("folder-id_same")));
        assert_eq!(extension_key("Folder Name:Server.ONE 🚀"),"foldername_server_one_");
        assert_eq!(extension_key("__cli__ent__"),"__cli__ent__");
        assert!(!serde_json::to_string(&rows).unwrap().contains("PRIVATE_SENTINEL"));
        let selection=serde_json::json!({"paths":"./extra/mcp.json","exclusive":true});
        assert_eq!(mcps(&root,"folder-id","plugin.json",Some(&selection),&mut ScanBudget::default()).unwrap().len(),1);
        let inline=serde_json::json!({"inline":{"command":"${PLUGIN_ROOT}/server"}});
        assert_eq!(mcps(&root,"folder-id","plugin.json",Some(&inline),&mut ScanBudget::default()).unwrap().len(),2);
        fs::write(root.join("extra/mcp.json"),r#"{"mcpServers":{"bad":{"url":"https://example.invalid"}}}"#).unwrap();
        assert!(mcps(&root,"folder-id","plugin.json",Some(&selection),&mut ScanBudget::default()).is_err());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(root.join(".mcp.json"),root.join("linked.json")).unwrap();
            assert!(mcps(&root,"folder-id","plugin.json",Some(&serde_json::json!({"paths":"./linked.json"})),&mut ScanBudget::default()).is_err());
        }
    }

    #[test]
    fn skills_are_root_and_immediate_children_and_installed_metadata_has_no_activation_effect() {
        let home=TestHome::new("goose-skill-paths");let root=home.home.join("folder-id");
        fs::create_dir_all(root.join("skills/child/deep")).unwrap();
        for p in ["skills/SKILL.md","skills/child/SKILL.md","skills/child/deep/SKILL.md"] {fs::write(root.join(p),"PRIVATE_SENTINEL").unwrap();}
        fs::write(root.join("plugin.json"),r#"{"name":"Different Manifest Name","version":"1.0","hooks":{"fake":{}},"agents":["ignored"]}"#).unwrap();
        fs::write(root.join(".goose-plugin-install.json"),r#"{"source_type":"git","source":"https://user:PRIVATE_SENTINEL@example.invalid/repo","format":"open-plugins"}"#).unwrap();
        let host=hosts().iter().find(|h|h.agent_id=="goose-desktop").unwrap();let mut scan=PluginScan::default();package(host,&root,&mut scan).unwrap();normalize(&mut scan);
        assert!(!scan.unavailable);assert_eq!(scan.plugins[0].plugin_id,"folder-id");assert_eq!(scan.plugins[0].presence,PluginPresence::Installed);assert_eq!(scan.plugins[0].enabled,None);
        assert_eq!(scan.plugins[0].components.len(),2);assert!(scan.plugins[0].components.iter().all(|c|c.kind==PluginComponentKind::Skill));
        assert!(!serde_json::to_string(&scan.plugins).unwrap().contains("PRIVATE_SENTINEL"));
        assert!(builtin_observe(&BTreeMap::new()).items.is_empty());assert!(builtin_watch_paths(&BTreeMap::new()).is_empty());
    }
}
