//! Current Qoder CLI CN declarations. Environment paths are lexical native
//! defaults, never evidence of a selected session or an initialized plugin.
use super::*;
use super::qoder::Package;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id: String, product_name: String, version: String, command: String,
    environment_prefix: String, config_directory: String, bun: bun_package::Contract,
    skill_source: String, code_skills: Vec<String>, builtin_plugins: Vec<Builtin>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Builtin { id: String, source: String, components: Vec<Contribution> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contribution { kind: PluginComponentKind, name: String, source: String }
fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-qoder-cn-cli.json")).expect("audited Qoder CLI CN contract"))
}
fn absolute(path: PathBuf) -> Result<PathBuf, ()> {
    let path = PathBuf::from(icu_normalizer::ComposingNormalizer::new_nfc().normalize(path.to_str().ok_or(())?).into_owned());
    if !path.is_absolute() || path.to_string_lossy().chars().any(char::is_control) { return Err(()); }
    let mut clean = PathBuf::new();
    for part in path.components() { match part { Component::ParentDir => { clean.pop(); }, Component::CurDir => {}, other => clean.push(other.as_os_str()) } }
    if expand_tilde("~/").starts_with(&clean) { return Err(()); }
    checked_root(&clean)?; Ok(clean)
}
fn environment(suffix: &str) -> Result<Option<String>, ()> {
    std::env::var_os(format!("{}{suffix}",contract().environment_prefix)).filter(|v|!v.is_empty())
        .map(|v|v.into_string().map_err(|_|())).transpose()
}
fn folder(input: &str) -> Result<String, ()> {
    let name = icu_normalizer::ComposingNormalizer::new_nfc().normalize(input).into_owned();
    static ALLOWED: OnceLock<regex::Regex> = OnceLock::new();
    let allowed = ALLOWED.get_or_init(||regex::Regex::new(r"^[\p{L}\p{N}\p{M}._ -]+$").expect("native folder grammar"));
    let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    if name.is_empty() || name.chars().count()>64 || name.starts_with(' ') || name.ends_with(' ') || name.ends_with('.')
        || !allowed.is_match(&name) || ["con","prn","aux","nul","com1","com2","com3","com4","com5","com6","com7","com8","com9","lpt1","lpt2","lpt3","lpt4","lpt5","lpt6","lpt7","lpt8","lpt9"].contains(&stem.as_str())
        || [".git",".svn",".hg",".vscode",".idea",".husky",".github","node_modules","bower_components"].contains(&name.to_lowercase().as_str()) { return Err(()); }
    Ok(name)
}
fn native_home() -> Result<PathBuf, ()> {
    let home = environment("_CLI_HOME")?.map(PathBuf::from).unwrap_or_else(||expand_tilde("~/"));
    // Native path.resolve/join depends on its own CWD for relative values.
    // '~' in CONFIG_DIR is not shell-expanded by the native reader.
    if !home.is_absolute() { return Err(()); } Ok(home)
}
fn plugins_root() -> Result<PathBuf, ()> {
    // The path module validates the folder before selecting any override.
    let name = folder(environment("_CONFIG_DIR_NAME")?.as_deref().unwrap_or(&contract().config_directory))?;
    if let Some(input) = environment("_PLUGIN_CACHE_DIR")? {
        let path = if let Some(tail) = input.strip_prefix('~') { native_home()?.join(tail.trim_start_matches('/')) } else { PathBuf::from(input) };
        return absolute(path);
    }
    // The native module validates CONFIG_DIR_NAME at import even when CONFIG_DIR
    // selects a different root; preserve that acceptance boundary.
    let root = if let Some(path) = environment("_CONFIG_DIR")? { absolute(PathBuf::from(path))? } else { absolute(native_home()?.join(name))? };
    absolute(root.join("plugins"))
}
fn executable() -> Option<PathBuf> {
    crate::application::agent_launch::find_command(&contract().command)
        .or_else(||{let path=expand_tilde(&format!("~/.local/bin/{}",contract().command));path.is_file().then_some(path)})
}
fn snapshot(host: &Host, id: &str, name: &str, version: &str, source: &str) -> NativePluginObservation {
    NativePluginObservation { agent_id:host.agent_id.clone(),plugin_id:id.into(),name:name.into(),version:version.into(),
        revision:format!("audited-bun:{version}"),presence:PluginPresence::Bundled,enabled:None,
        source_paths:vec![source.into()],components:Vec::new(),docs_url:host.docs_url.clone(),source_fingerprint:String::new() }
}
fn authored(p: &Package<'_>, budget: &mut ScanBudget) -> Result<Option<(String,Arc<Value>)>, ()> {
    for input in [".qoder-plugin/plugin.json",".claude-plugin/plugin.json"] {
        let Some(value) = p.json(input,budget)? else { continue; };
        let name = value.get("name").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
        if name.contains(' ') || value.get("version").is_some_and(|v|!v.is_string()) { return Err(()); }
        // Native current grammar reads workflowsPath + workflowsPaths. Unknown
        // 'workflows' is ignored. This is an inert descriptor for the shared
        // component reader; no source file or native configuration is rewritten.
        let mut projected = value.as_object().ok_or(())?.clone(); projected.remove("workflows");
        let mut workflows = Vec::new();
        if let Some(path) = value.get("workflowsPath") { workflows.push(Value::String(path.as_str().ok_or(())?.into())); }
        if let Some(paths) = value.get("workflowsPaths") {
            let paths = paths.as_array().filter(|p|p.len()<=64).ok_or(())?;
            for path in paths { workflows.push(Value::String(path.as_str().ok_or(())?.into())); }
        }
        if !workflows.is_empty() { projected.insert("workflows".into(),Value::Array(workflows)); }
        return Ok(Some((input.into(),Arc::new(Value::Object(projected)))));
    }
    Ok(None)
}
fn bundle(host: &Host, graph: &bun_package::Graph, result: &mut PluginScan) -> Result<Vec<NativePluginObservation>, ()> {
    let c = contract(); let source = graph.source(&c.skill_source)?;
    let p = Package::Bun { graph,prefix:"packages/core/src/skills/builtin/" };
    let mut skills = snapshot(host,"@builtin/skills",&format!("{} packaged Skills",c.product_name),&c.version,&source);
    super::qoder_cli::files(&p,"",PluginComponentKind::Skill,0,&mut skills.components,&mut result.budget)?;
    for name in &c.code_skills {
        skills.components.push(PluginComponent {kind:PluginComponentKind::Skill,name:metadata_name(name).ok_or(())?,source_path:source.clone(),native_id:None});
    }
    let mut out = vec![skills];
    for builtin in &c.builtin_plugins {
        let source = graph.source(&builtin.source)?;
        let mut item = snapshot(host,&builtin.id,&builtin.id,&c.version,&source);
        if builtin.id == "qoder-security" {
            let p = Package::Bun {graph,prefix:"vendor/qoder-security/"};
            let manifest = authored(&p,&mut result.budget)?.ok_or(())?;
            item = super::qoder_cli::sdk_package(host,&p,&builtin.id,PluginPresence::Bundled,&[source.clone()],Some(manifest),result)?;
        } else if builtin.id == "qoder-sites" {
            let p = Package::Bun {graph,prefix:"vendor/qoder-sites/"};
            let meta = p.json("artifact.json",&mut result.budget)?.ok_or(())?;
            if meta.get("schemaVersion").and_then(Value::as_u64)!=Some(1) { return Err(()); }
            item.version = meta.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
            item.source_paths.push(p.path("artifact.json")?);
            super::qoder_cli::files(&p,"skills",PluginComponentKind::Skill,0,&mut item.components,&mut result.budget)?;
        }
        for declaration in &builtin.components {
            if item.components.len()>=MAX_COMPONENTS { return Err(()); }
            item.components.push(PluginComponent {kind:declaration.kind.clone(),name:metadata_name(&declaration.name).ok_or(())?,source_path:graph.source(&declaration.source)?,native_id:None});
        }
        item.components.sort(); item.components.dedup(); out.push(item);
    }
    graph.unchanged()?; Ok(out)
}
fn foreign(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    ["qoder-computer-use@qoder-bundler","computer-use@qoder-bundler","security-scan@qoder-bundler"].contains(&id.as_str())
        || (id=="qoder-context@qoderapp-bundler" && ["QODER_CLIENT_TYPE","QODERCN_CLIENT_TYPE"].iter().all(|key|std::env::var(key).map_or(true,|v|v.trim().is_empty())))
}
fn installed(host: &Host, root: &Path, result: &mut PluginScan) -> Result<(), ()> {
    let Some(state) = optional_json(root,"installed_plugins_v2.json",&mut result.budget)? else { return Ok(()); };
    if state.get("version").and_then(Value::as_u64)!=Some(2) { return Err(()); }
    let entries = state.get("plugins").and_then(Value::as_object).filter(|v|v.len()<=MAX_ENTRIES).ok_or(())?;
    for (id,records) in entries {
        if foreign(id) { continue; }
        let records = records.as_array().filter(|v|v.len()<=MAX_ENTRIES).ok_or(())?;
        if metadata_name(id).is_none() { result.unavailable=true; continue; }
        for (ordinal,record) in records.iter().enumerate() {
            result.budget.visit()?;
            if !matches!(record.get("scope").and_then(Value::as_str),Some("user"|"managed")) { continue; }
            let path = absolute(PathBuf::from(record.get("installPath").and_then(Value::as_str).ok_or(())?))?;
            let provenance = vec![collapse_home(&root.join("installed_plugins_v2.json"))];
            if !path.try_exists().map_err(|_| ())? {
                let mut item = snapshot(host,id,id,"unknown",&provenance[0]);item.presence=PluginPresence::Declared;
                item.revision=format!("registered:{ordinal}");result.plugins.push(item);continue;
            }
            let p=Package::Disk(&path);
            let manifest = match authored(&p,&mut result.budget) {Ok(value)=>value,Err(())=>{result.unavailable=true;continue;}};
            match super::qoder_cli::sdk_package(host,&p,id,PluginPresence::Installed,&provenance,manifest,result) {
                Ok(mut item)=>{item.revision=format!("registered:{ordinal}:{}",collapse_home(&path));result.plugins.push(item);},
                Err(())=>result.unavailable=true,
            }
        }
    }
    Ok(())
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let c=contract();
    if host.agent_id!=c.agent_id {result.unavailable=true;return;}
    let Some(command)=executable() else {return;};
    let Ok(path)=fs::canonicalize(command) else {result.unavailable=true;return;};
    let graph=match bun_package::Graph::open(&path,&c.bun,&mut result.budget) {Ok(value)=>value,Err(())=>{result.unavailable=true;return;}};
    match bundle(host,&graph,result) {Ok(items)=>result.plugins.extend(items),Err(())=>result.unavailable=true,}
    match plugins_root() {Ok(root)=>{if installed(host,&root,result).is_err(){result.unavailable=true;}},Err(())=>result.unavailable=true,}
}
pub(super) fn runtime_version(path: &Path) -> Option<String> {
    let path=fs::canonicalize(path).ok()?;
    let graph=bun_package::Graph::open(&path,&contract().bun,&mut ScanBudget::default()).ok()?;
    graph.unchanged().ok()?;Some(contract().version.clone())
}
pub(super) fn watch_paths(_: &Host) -> Vec<PathBuf> {
    let mut paths=Vec::new();
    if let Some(command)=executable(){paths.push(command.clone());if let Ok(path)=fs::canonicalize(command){paths.push(path);}}
    let Ok(root)=plugins_root() else{return paths;};paths.push(root.clone());let mut budget=ScanBudget::default();
    if let Ok(Some(state))=optional_json(&root,"installed_plugins_v2.json",&mut budget) {
        if let Some(entries)=state.get("plugins").and_then(Value::as_object).filter(|v|v.len()<=MAX_ENTRIES) {
            for (id,records) in entries {
                if foreign(id){continue;}
                for record in records.as_array().into_iter().flatten().take(MAX_ENTRIES) {
                    if budget.visit().is_err(){return paths;}
                    if !matches!(record.get("scope").and_then(Value::as_str),Some("user"|"managed")){continue;}
                    if let Some(path)=record.get("installPath").and_then(Value::as_str).and_then(|v|absolute(PathBuf::from(v)).ok()){paths.push(path);}
                }
            }
        }
    }
    paths.sort();paths.dedup();paths
}
