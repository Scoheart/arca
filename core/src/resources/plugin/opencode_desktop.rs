//! Inert OpenCode 2 desktop declarations. Server and GUI availability are
//! independent; neither package metadata nor config operations prove activation.
use super::*;
use super::electron_package::{app_version, guard_asar, Guard};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id: String, version: String, bundle_id: String, product_name: String, app_path: String,
    asar_guards: Vec<Guard>, runtime: bun_package::Contract, server_member: String, gui_member: String,
    server_plugins: Vec<ServerPlugin>, gui_plugins: Vec<GuiPlugin>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerPlugin { id: String, phase: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GuiPlugin { id: String, platforms: Vec<String> }
fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-opencode-desktop.json"))
        .expect("audited OpenCode desktop contract"))
}
fn observation(host: &Host, id: String, name: String, presence: PluginPresence, path: String) -> NativePluginObservation {
    NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:id, name, version:"unknown".into(),
        revision:String::new(), presence, enabled:None, source_paths:vec![path], components:Vec::new(),
        docs_url:host.docs_url.clone(), source_fingerprint:String::new() }
}
fn code(plugin: &mut NativePluginObservation, name: &str, source: String) -> Result<(), ()> {
    if plugin.components.len()>=MAX_COMPONENTS { return Err(()); }
    plugin.components.push(PluginComponent { kind:PluginComponentKind::Code, name:metadata_name(name).ok_or(())?, source_path:source, native_id:None }); Ok(())
}
fn bundled(host: &Host, result: &mut PluginScan) -> Result<(), ()> {
    let c=contract(); if c.agent_id!=host.agent_id { return Err(()); }
    let app=crate::paths::system_probe_path(&c.app_path);
    if !app.try_exists().map_err(|_| ())? { return Ok(()); }
    if app_version(&app,&c.bundle_id,&c.product_name,&mut result.budget)?!=c.version { return Err(()); }
    let asar=relative(&app,"Contents/Resources/app.asar")?;
    guard_asar(&asar,&c.asar_guards,&mut result.budget)?;
    let binary=relative(&app,"Contents/Resources/opencode-cli")?;
    let graph=bun_package::Graph::open(&binary,&c.runtime,&mut result.budget)?;
    let server_source=graph.source(&c.server_member)?;
    let gui_source=format!("{}!{}",collapse_home(&asar),c.gui_member);
    let mut plugins=Vec::new();
    for declaration in &c.server_plugins {
        let source=format!("{server_source}#{}",declaration.id);
        let mut plugin=observation(host,format!("@builtin/server/{}",declaration.id),declaration.id.clone(),PluginPresence::Bundled,source.clone());
        plugin.version=c.version.clone(); plugin.revision=c.runtime.table_sha256.clone();
        code(&mut plugin,&format!("server · {}",declaration.phase),source.clone())?;
        if declaration.id=="opencode.browser" {
            plugin.components.push(PluginComponent { kind:PluginComponentKind::Skill, name:"browser".into(), source_path:format!("{server_source}#LT"), native_id:None });
        }
        plugins.push(plugin);
    }
    for declaration in &c.gui_plugins {
        // This contract is the macOS package. Reserved definitions for another
        // OS are not offered as locally available GUI plugins.
        if !declaration.platforms.is_empty() && !declaration.platforms.iter().any(|p|p=="macos") { continue; }
        let source=format!("{gui_source}#{}",declaration.id);
        let mut plugin=observation(host,format!("@builtin/gui/{}",declaration.id),format!("GUI · {}",declaration.id),PluginPresence::Bundled,source.clone());
        plugin.version=c.version.clone(); plugin.revision=c.asar_guards.iter().find(|g|g.path==c.gui_member).ok_or(())?.sha256.clone();
        code(&mut plugin,"GUI definition",source)?; plugins.push(plugin);
    }
    graph.unchanged()?; result.plugins.extend(plugins); Ok(())
}
fn absolute(path: PathBuf) -> Result<PathBuf, ()> {
    if !path.is_absolute() { return Err(()); }
    let mut normalized=PathBuf::new();
    for part in path.components() {
        // Check a traversed prefix before lexical '..' can remove it. A native
        // file URL through a symlink must not become a different guessed path.
        if !normalized.as_os_str().is_empty() && fs::symlink_metadata(&normalized).is_ok_and(|m|m.file_type().is_symlink()) { return Err(()); }
        match part { Component::CurDir=>{},Component::ParentDir=>{if !normalized.pop(){return Err(());}},part=>normalized.push(part.as_os_str()) }
    }
    if expand_tilde("~/").starts_with(&normalized) { return Err(()); }
    checked_root(&normalized)?; Ok(normalized)
}
fn selected_path(variable: &str, fallback: PathBuf) -> Result<PathBuf, ()> {
    absolute(std::env::var_os(variable).filter(|v|!v.is_empty()).map(PathBuf::from).unwrap_or(fallback))
}
fn config_root() -> Result<PathBuf, ()> {
    let xdg=std::env::var_os("XDG_CONFIG_HOME").filter(|v|!v.is_empty()).map(PathBuf::from).unwrap_or_else(||expand_tilde("~/.config"));
    selected_path("OPENCODE_CONFIG_DIR",xdg.join("opencode"))
}
fn cache_root() -> Result<PathBuf, ()> {
    let xdg=std::env::var_os("XDG_CACHE_HOME").filter(|v|!v.is_empty()).map(PathBuf::from).unwrap_or_else(||expand_tilde("~/.cache"));
    absolute(xdg.join("opencode/npm"))
}
enum Source { Local(PathBuf), Npm { name:String, spec:String }, Unresolved }
struct Declaration { identity:String, name:String, source:Source, config:PathBuf }
fn declaration(input: &str, config: &Path, discovered: bool, ordinal: usize) -> Result<Option<Declaration>, ()> {
    if input.is_empty() || input.len()>4096 || input.chars().any(char::is_control) { return Err(()); }
    // These are native selection operations against plugin IDs, never packages.
    if input.starts_with('-') || input=="*" || input.ends_with(".*") || input.starts_with("opencode.") {
        if input=="-" { return Err(()); } return Ok(None);
    }
    let local=if input.starts_with("file://") {
        let url=url::Url::parse(input).map_err(|_|())?;
        if !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() { return Err(()); }
        Some(absolute(url.to_file_path().map_err(|_|())?)?)
    } else if Path::new(input).is_absolute() { Some(absolute(PathBuf::from(input))?) }
    else if input.starts_with("./") || input.starts_with("../") { Some(absolute(config.parent().ok_or(())?.join(input))?) }
    else { None };
    if let Some(path)=local {
        if !discovered && regular_metadata_file(&path)? { return Err(()); }
        let name=path.file_name().and_then(|v|v.to_str()).and_then(metadata_name).ok_or(())?;
        return Ok(Some(Declaration { identity:format!("local:{}",hex::encode(Sha256::digest(path.as_os_str().as_encoded_bytes()))),name,source:Source::Local(path),config:config.into() }));
    }
    static NPM:OnceLock<regex::Regex>=OnceLock::new();
    let npm=NPM.get_or_init(||regex::Regex::new(r"^(?P<name>(?:@[A-Za-z0-9_-][A-Za-z0-9_.-]*/)?[A-Za-z0-9_-][A-Za-z0-9_.-]*)(?:@(?P<spec>[A-Za-z0-9_.~^*+<>=| -]+))?$").unwrap());
    if let Some(capture)=npm.captures(input) {
        let name=metadata_name(&capture["name"]).ok_or(())?;let spec=capture.name("spec").map_or("latest",|s|s.as_str()).to_owned();
        return Ok(Some(Declaration { identity:format!("npm:{}",hex::encode(Sha256::digest(input.as_bytes()))),name:name.clone(),source:Source::Npm{name,spec},config:config.into() }));
    }
    // Non-registry URLs and substitutions may carry credentials. Neither their
    // specifier nor options are copied into observation fields.
    Ok(Some(Declaration { identity:format!("unresolved:{}",hex::encode(Sha256::digest(format!("{}:{ordinal}",config.display()).as_bytes()))),name:"Unresolved plugin declaration".into(),source:Source::Unresolved,config:config.into() }))
}
fn document(path: &Path, result: &mut PluginScan) -> Result<Vec<Declaration>, ()> {
    checked_root(path)?;
    let Some(value)=optional_document(path.parent().ok_or(())?,path.file_name().and_then(|v|v.to_str()).ok_or(())?,DocumentFormat::Jsonc,&mut result.budget)? else{return Ok(Vec::new());};
    if !value.is_object() { return Err(()); }
    let mut declarations=Vec::new();
    // V2's inert normalizer concatenates legacy and native lists within each
    // document. Successive global documents accumulate operations as well.
    for key in ["plugin","plugins"] {
        let Some(values)=value.get(key) else{continue;};
        for (ordinal,value) in values.as_array().filter(|v|v.len()<=MAX_ENTRIES).ok_or(())?.iter().enumerate() {
            let input=match value {
                Value::String(s)=>s.as_str(),
                Value::Array(v) if key=="plugin"&&v.len()==2&&v[1].is_object()=>v[0].as_str().ok_or(())?,
                Value::Object(v) if key=="plugins"&&v.get("options").is_none_or(Value::is_object)=>v.get("package").and_then(Value::as_str).ok_or(())?,
                _=>return Err(()),
            };
            if let Some(row)=declaration(input,path,false,declarations.len()+ordinal)? { declarations.push(row); }
        }
    }
    Ok(declarations)
}
fn entry(root: &Path, input: &str) -> Result<Option<PathBuf>, ()> {
    if !input.starts_with("./") { return Err(()); }
    let path=relative(root,input)?;
    if regular_metadata_file(&path)? { Ok(Some(path)) } else { Ok(None) }
}
fn exported(value: &Value) -> Result<&str, ()> {
    // Conditional/custom exports need native resolution. Do not choose one
    // condition or evaluate package code to make an observation look complete.
    value.as_str().ok_or(())
}
fn package(root: &Path, expected: Option<&str>, plugin: &mut NativePluginObservation, result: &mut PluginScan) -> Result<bool, ()> {
    checked_root(root)?;
    if !fs::symlink_metadata(root).is_ok_and(|m|m.is_dir()) { return Ok(false); }
    let manifest=optional_json(root,"package.json",&mut result.budget)?;
    if let Some(value)=&manifest {
        if !value.is_object() { return Err(()); }
        if let Some(expected)=expected { if value.get("name").and_then(Value::as_str)!=Some(expected) { return Err(()); } }
        if let Some(name)=value.get("name").and_then(Value::as_str).and_then(metadata_name) { plugin.name=name; }
        if let Some(version)=value.get("version").and_then(Value::as_str).and_then(metadata_name) { plugin.version=version; }
        plugin.source_paths.push(collapse_home(&root.join("package.json")));
    } else if expected.is_some() { return Err(()); }
    let named=expected.is_some() || manifest.as_ref().is_some_and(|v|v.get("name").is_some_and(Value::is_string)&&v.get("exports").is_some());
    let mut entries=BTreeMap::new();
    if named {
        let value=manifest.as_ref().ok_or(())?;
        if let Some(exports)=value.get("exports") {
            if exports.as_object().is_some_and(|map|map.keys().any(|key|!key.starts_with('.'))) { return Err(()); }
            for (kind,keys) in [("server",vec!["./server","."]),("gui",vec!["./gui-experimental"]),("tui",vec!["./tui"]),("rpc",vec!["./rpc"])] {
                let target=if let Some(map)=exports.as_object() { keys.iter().find_map(|key|map.get(*key)) }
                    else if kind=="server" { Some(exports) } else { None };
                if let Some(target)=target { if let Some(path)=entry(root,exported(target)?)? { entries.insert(kind,path); } }
            }
        } else {
            // Installed npm packages can resolve their server through main.
            if let Some(main)=value.get("main").and_then(Value::as_str) {
                let main=if main.starts_with("./"){main.to_owned()}else{format!("./{main}")};
                if let Some(path)=entry(root,&main)? { entries.insert("server",path); }
            }
        }
    } else {
        for (kind,names) in [("server",vec!["server.ts","server.tsx","server.js","index.ts","index.tsx","index.js"]),("gui",vec!["gui-experimental.cjs","gui-experimental.js"]),("tui",vec!["tui.ts","tui.tsx","tui.js"]),("rpc",vec!["rpc.ts","rpc.tsx","rpc.js"])] {
            for name in names { if let Some(path)=entry(root,&format!("./{name}"))? { entries.insert(kind,path); break; } }
        }
    }
    if entries.get("gui").is_some_and(|p|!matches!(p.extension().and_then(|v|v.to_str()),Some("js"|"cjs"))) { entries.remove("gui"); }
    if !entries.contains_key("server") && !entries.contains_key("gui") { return Ok(false); }
    for (kind,path) in &entries {
        if *kind=="rpc"&&!entries.contains_key("server") { continue; }
        code(plugin,&format!("{kind} entry declaration"),collapse_home(path))?;
    }
    Ok(true)
}
fn cached_package(root: &Path, name: &str, spec: &str, result: &mut PluginScan) -> Result<PathBuf, ()> {
    let directory=relative(root,&format!("{name}@{spec}"))?;
    let mut generations=BTreeMap::new();
    for row in directory_entries(&directory,&mut result.budget)? {
        let name=row.path.file_name().and_then(|v|v.to_str()).ok_or(())?;
        if !name.is_empty()&&name.bytes().all(|c|c.is_ascii_digit()) {
            if !row.directory { return Err(()); }
            let number=name.parse::<u64>().map_err(|_|())?;
            if number>9_007_199_254_740_991 || generations.insert(number,row.path).is_some() { return Err(()); }
        }
    }
    let selected=generations.last_key_value().map(|(_,p)|p).unwrap_or(&directory);
    relative(selected,&format!("node_modules/{name}"))
}
fn observe_declaration(host: &Host, row: Declaration, cache: Option<&Path>, result: &mut PluginScan) -> Result<(), ()> {
    let mut plugin=observation(host,row.identity,row.name,PluginPresence::Declared,collapse_home(&row.config));
    match row.source {
        Source::Unresolved=>result.unavailable=true,
        Source::Local(path)=>{
            checked_root(&path)?;
            if regular_metadata_file(&path)? {
                plugin.presence=PluginPresence::Local;code(&mut plugin,"server entry declaration",collapse_home(&path))?;
            } else if fs::symlink_metadata(&path).is_ok_and(|m|m.is_dir()) {
                if !package(&path,None,&mut plugin,result)? { return Ok(()); }
                plugin.presence=PluginPresence::Local;
            }
            plugin.source_paths.push(collapse_home(&path));
        },
        Source::Npm{name,spec}=>if let Some(cache)=cache {
            let path=cached_package(cache,&name,&spec,result)?;
            if fs::symlink_metadata(&path).is_ok_and(|m|m.is_dir()) {
                if package(&path,Some(&name),&mut plugin,result)? { plugin.presence=PluginPresence::CacheOnly; }
            }
        },
    }
    // IDs exported by JS and dynamic MCP/tool/Skill registrations stay unknown.
    result.plugins.push(plugin); Ok(())
}
fn global(host: &Host, result: &mut PluginScan) -> Result<(), ()> {
    let root=config_root()?;let cache=cache_root();
    if cache.is_err() { result.unavailable=true; }
    let mut rows=Vec::new();
    for folder in ["plugin","plugins"] {
        for entry in directory_entries(&root.join(folder),&mut result.budget)? {
            if entry.directory || entry.file&&matches!(entry.path.extension().and_then(|v|v.to_str()),Some("js"|"ts")) {
                if let Some(row)=declaration(entry.path.to_str().ok_or(())?,&entry.path,true,0)? { rows.push(row); }
            } else if !entry.file { result.unavailable=true; }
        }
    }
    for file in [root.join("opencode.json"),root.join("opencode.jsonc")] {
        match document(&file,result) { Ok(declarations)=>rows.extend(declarations),Err(())=>result.unavailable=true }
    }
    if let Some(file)=std::env::var_os("OPENCODE_CONFIG").filter(|v|!v.is_empty()) {
        match absolute(PathBuf::from(file)).and_then(|file|document(&file,result)) { Ok(declarations)=>rows.extend(declarations),Err(())=>result.unavailable=true }
    }
    let mut seen=BTreeSet::new();rows.reverse();rows.retain(|r|seen.insert(r.identity.clone()));rows.reverse();
    for row in rows {
        if observe_declaration(host,row,cache.as_deref().ok(),result).is_err() { result.unavailable=true; }
    }
    Ok(())
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    if bundled(host,result).is_err() { result.unavailable=true; }
    if global(host,result).is_err() { result.unavailable=true; }
}
pub(super) fn watch_paths(_host: &Host) -> Vec<PathBuf> {
    let mut paths=vec![crate::paths::system_probe_path(&contract().app_path)];
    if let Ok(root)=config_root() { paths.extend([root.clone(),root.join("plugin"),root.join("plugins"),root.join("opencode.json"),root.join("opencode.jsonc")]); }
    if let Ok(root)=cache_root() { paths.push(root); }
    if let Some(path)=std::env::var_os("OPENCODE_CONFIG").filter(|v|!v.is_empty()).and_then(|p|absolute(PathBuf::from(p)).ok()) { paths.push(path); }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    fn host() -> &'static Host { hosts().iter().find(|h|h.agent_id=="opencode-desktop").unwrap() }
    #[test]
    fn native_lists_accumulate_legacy_entries_and_keep_operations_out_of_packages() {
        let home=TestHome::new("opencode-desktop-config");let root=home.home.join(".config/opencode");fs::create_dir_all(&root).unwrap();
        fs::write(root.join("opencode.json"),r#"{"plugin":[["old-plugin",{"token":"secret-option"}]],"plugins":["-*","opencode.provider.chatgpt","opencode.provider.*",{"package":"new-plugin","options":{"token":"secret-option"}}]}"#).unwrap();
        fs::write(root.join("opencode.jsonc"),"{/* preserve this */\"plugins\":[\"last-plugin\",],}").unwrap();
        let mut scan=PluginScan::default();global(host(),&mut scan).unwrap();
        assert_eq!(scan.plugins.iter().map(|p|p.name.as_str()).collect::<Vec<_>>(),vec!["old-plugin","new-plugin","last-plugin"]);
        assert!(scan.plugins.iter().all(|p|p.enabled.is_none()&&p.components.is_empty()&&p.presence==PluginPresence::Declared));
        assert!(!serde_json::to_string(&scan.plugins).unwrap().contains("secret-option"));assert!(!scan.unavailable);
    }
    #[test]
    fn physical_local_and_latest_registry_cache_parts_do_not_invent_dynamic_resources() {
        let home=TestHome::new("opencode-desktop-cache");let root=home.home.join(".config/opencode");fs::create_dir_all(root.join("plugins/local")).unwrap();
        fs::write(root.join("plugins/local/server.js"),"throw Error('must never execute')").unwrap();
        fs::write(root.join("plugins/standalone.js"),"throw Error('must never execute')").unwrap();
        fs::write(root.join("opencode.json"),r#"{"plugins":["example-plugin"]}"#).unwrap();
        for (generation,version) in [("9","1.0.0"),("10","2.0.0")] {
            let package=home.home.join(".cache/opencode/npm/example-plugin@latest").join(generation).join("node_modules/example-plugin");fs::create_dir_all(&package).unwrap();
            fs::write(package.join("package.json"),serde_json::json!({"name":"example-plugin","version":version,"exports":{"./server":"./server.js","./gui-experimental":"./gui.cjs","./rpc":"./rpc.js"},"scripts":{"postinstall":"secret-script"}}).to_string()).unwrap();
            for name in ["server.js","gui.cjs","rpc.js"] { fs::write(package.join(name),"throw Error('must never execute')").unwrap(); }
        }
        let mut scan=PluginScan::default();global(host(),&mut scan).unwrap();assert!(!scan.unavailable);assert_eq!(scan.plugins.len(),3);
        let cached=scan.plugins.iter().find(|p|p.name=="example-plugin").unwrap();assert_eq!(cached.version,"2.0.0");assert_eq!(cached.presence,PluginPresence::CacheOnly);assert_eq!(cached.components.len(),3);
        assert!(scan.plugins.iter().flat_map(|p|&p.components).all(|c|c.kind==PluginComponentKind::Code&&c.native_id.is_none()));
        assert!(!serde_json::to_string(&scan.plugins).unwrap().contains("secret-script"));
    }
    #[test]
    fn unsafe_roots_files_exports_and_credentials_fail_closed_without_reading_bodies() {
        let home=TestHome::new("opencode-desktop-boundaries");let root=home.home.join(".config/opencode");fs::create_dir_all(root.join("conditional")).unwrap();
        fs::write(root.join("server.js"),"never execute").unwrap();
        assert!(declaration("./server.js",&root.join("opencode.json"),false,0).is_err());
        let mut scan=PluginScan::default();let row=declaration("https://user:secret-token@example.invalid/plugin",&root.join("opencode.json"),false,0).unwrap().unwrap();
        observe_declaration(host(),row,None,&mut scan).unwrap();assert!(scan.unavailable);
        assert!(!serde_json::to_string(&scan.plugins).unwrap().contains("secret-token"));
        fs::write(root.join("conditional/package.json"),r#"{"name":"conditional","exports":{".":{"import":"./server.js"}}}"#).unwrap();
        let mut plugin=observation(host(),"test".into(),"test".into(),PluginPresence::Declared,String::new());
        assert!(package(&root.join("conditional"),None,&mut plugin,&mut PluginScan::default()).is_err());
        fs::write(root.join("conditional/package.json"),r#"{"name":"conditional","exports":{"import":"./server.js","default":"./server.js"}}"#).unwrap();
        assert!(package(&root.join("conditional"),None,&mut plugin,&mut PluginScan::default()).is_err());
        std::env::set_var("OPENCODE_CONFIG_DIR","relative");assert!(config_root().is_err());
        #[cfg(unix)] { std::os::unix::fs::symlink(root.join("server.js"),root.join("link.js")).unwrap();assert!(declaration(root.join("link.js").to_str().unwrap(),&root.join("opencode.json"),true,0).is_err()); }
    }
}
