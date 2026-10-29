//! Auggie's source-bound, user-global marketplace metadata. No CLI startup,
//! settings, Git operation, plugin/hook execution or component-body reads.
use super::*;
use std::ffi::OsStr;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id:String, command:String, entrypoint:String, version:String,
    runtime_version:String, sources:Vec<SourceFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFile { path:String, bytes:u64, sha256:String }

const MARKET_MANIFESTS:[&str;2]=[".augment-plugin/marketplace.json",".claude-plugin/marketplace.json"];

fn contract() -> &'static Contract {
    static VALUE:OnceLock<Contract>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-augment.json"))
        .expect("audited Auggie source contract"))
}

fn absolute(value:&OsStr) -> Result<PathBuf,()> {
    let text=value.to_str().ok_or(())?;let path=PathBuf::from(value);
    if !path.is_absolute()||text.len()>4096||text.chars().any(char::is_control)
        ||path.components().any(|c|matches!(c,Component::ParentDir|Component::Prefix(_))){return Err(());}
    Ok(path)
}

fn verified_root(c:&Contract,selected:&Path,budget:&mut ScanBudget) -> Result<PathBuf,()> {
    absolute(selected.as_os_str())?;
    let physical=fs::canonicalize(selected).map_err(|_|())?;checked_root(&physical)?;
    if c.version.is_empty()||c.sources.len()>8
        ||physical.file_name().and_then(|v|v.to_str())!=Some(c.entrypoint.as_str())
        ||!c.sources.iter().any(|v|v.path==c.entrypoint)||!c.sources.iter().any(|v|v.path=="package.json"){return Err(());}
    let root=physical.parent().ok_or(())?;
    for source in &c.sources {
        verify_artifact(&relative(root,&source.path)?,source.bytes,&source.sha256,budget)?;
    }
    if fs::canonicalize(selected).map_err(|_|())?!=physical{return Err(());}
    Ok(root.to_path_buf())
}
fn version_for(c:&Contract,selected:&Path) -> Option<String> {
    verified_root(c,selected,&mut ScanBudget::default()).ok().map(|_|c.runtime_version.clone())
}
pub(super) fn runtime_version(selected:&Path) -> Option<String> {version_for(contract(),selected)}

// Native _u(): nonempty HOME, then nonempty USERPROFILE, then os.homedir().
// An explicitly empty unresolved HOME must not borrow Arca's real-home fallback.
fn home_from_env(home:Option<&OsStr>,profile:Option<&OsStr>,fallback:Option<&Path>) -> Result<PathBuf,()> {
    if let Some(value)=home.filter(|v|!v.is_empty()){return absolute(value);}
    if let Some(value)=profile.filter(|v|!v.is_empty()){return absolute(value);}
    if home.is_some(){return Err(());}
    absolute(fallback.ok_or(())?.as_os_str())
}
fn marketplace_root(host:&Host) -> Result<PathBuf,()> {
    let roots=host.roots.iter().filter(|r|r.layout=="marketplace_checkouts"&&r.presence==PluginPresence::Local).collect::<Vec<_>>();
    let [root]=roots.as_slice()else{return Err(());};
    let suffix=root.path.strip_prefix("~/").ok_or(())?;
    let home=std::env::var_os("HOME");let profile=std::env::var_os("USERPROFILE");let fallback=dirs::home_dir();
    let home=home_from_env(home.as_deref(),profile.as_deref(),fallback.as_deref())?;
    relative(&home,suffix)
}

// Native identifiers are raw strings, not trimmed display names.
fn native_name(value:&str) -> Option<String> {
    (!value.is_empty()&&value.len()<=240&&!value.chars().any(char::is_control)).then(||value.to_owned())
}
fn file_name(path:&Path) -> Result<&str,()> {path.file_name().and_then(|v|v.to_str()).ok_or(())}

fn reference_component(kind:PluginComponentKind,path:&Path,label:&str) -> PluginComponent {
    PluginComponent{kind,name:label.into(),source_path:collapse_home(path),native_id:None}
}
fn reference(host:&Host,id:String,name:String,path:&Path,revision:String,result:&mut PluginScan) -> Result<(),()> {
    result.budget.references+=1;
    if result.budget.references>MAX_REFERENCES{return Err(());}
    result.unavailable=true;
    result.plugins.push(NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:id,name,version:"unknown".into(),revision,
        presence:PluginPresence::Declared,enabled:None,source_paths:vec![collapse_home(path)],
        components:vec![reference_component(PluginComponentKind::PluginReference,path,"Plugin source unavailable or unverified")],
        docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    });Ok(())
}

fn selected_document(root:&Path,paths:&[String],budget:&mut ScanBudget) -> Result<Option<(PathBuf,Arc<Value>)>,()> {
    checked_root(root)?;
    for input in paths {
        let path=relative(root,input)?;budget.visit()?;
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>continue,
            Err(_)=>return Err(()),Ok(_)=>{},
        }
        // First existing native path masks the lower-priority manifest.
        let value=optional_document(root,input,DocumentFormat::Json5,budget)?.ok_or(())?;
        return Ok(Some((path,value)));
    }
    Ok(None)
}
fn marketplace_document(root:&Path,budget:&mut ScanBudget) -> Result<Option<(PathBuf,Arc<Value>)>,()> {
    selected_document(root,&MARKET_MANIFESTS.map(str::to_owned),budget)
}

fn normalized(path:&Path) -> Result<PathBuf,()> {
    let mut output=PathBuf::new();
    for part in path.components() {
        match part {
            Component::RootDir=>output.push(part),Component::Normal(value)=>output.push(value),Component::CurDir=>{},
            Component::ParentDir=>if !output.pop(){return Err(());},Component::Prefix(_)=>return Err(()),
        }
    }
    absolute(output.as_os_str())
}
fn local_root(market:&Path,plugin_root:Option<&str>,source:&str) -> Result<Option<PathBuf>,()> {
    if source.len()>4096||source.chars().any(char::is_control){return Err(());}
    let mut candidates=Vec::new();
    if let Some(prefix)=plugin_root.filter(|v|!v.is_empty()) {
        if prefix.len()>4096||prefix.chars().any(char::is_control){return Err(());}
        candidates.push(market.join(prefix).join(source));
    }
    candidates.push(market.join(source));
    for candidate in candidates {
        let path=normalized(&candidate)?;
        // Never inspect an escaping source just to decide whether native
        // fallback would occur. An unverified first candidate stays a reference.
        if !path.starts_with(market){return Err(());}
        checked_root(&path)?;
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir()=>return Ok(Some(path)),
            Ok(m) if m.file_type().is_symlink()=>return Err(()),
            Ok(_)=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err(()),
        }
    }
    Ok(None)
}
fn cache_key(name:&str) -> Option<String> {
    let value=name.replace("..","").replace(['/','\\'],"");
    let value=value.trim_start_matches('.').trim();
    (!value.is_empty()&&value.bytes().next().is_some_and(|b|b.is_ascii_alphanumeric())
        &&value.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'_'|b'-'))).then(||value.to_owned())
}
fn cached_root(market:&Path,name:&str) -> Result<Option<PathBuf>,()> {
    let path=market.join(".plugin-cache").join(cache_key(name).ok_or(())?);checked_root(&path)?;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),
        Ok(m) if m.is_dir()=>{},_=>return Err(()),
    }
    let marker=path.join(".git");checked_root(&marker)?;
    match fs::symlink_metadata(marker) {
        Ok(m) if m.is_dir()||m.file_type().is_file()=>Ok(Some(path)),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),_=>Err(()),
    }
}

fn component_tree(root:&Path,path:&Path,kind:PluginComponentKind,depth:usize,
    components:&mut Vec<PluginComponent>,budget:&mut ScanBudget) -> Result<(),()> {
    if depth>16{return Err(());}
    for entry in directory_entries(path,budget)? {
        let name=file_name(&entry.path)?;
        if name.starts_with('.') {continue;}
        if entry.directory {component_tree(root,&entry.path,kind.clone(),depth+1,components,budget)?;}
        else if entry.file {
            let wanted=match kind {
                PluginComponentKind::Skill=>name.ends_with(".md"),PluginComponentKind::Hook=>name.ends_with(".json"),
                _=>name.ends_with(".md")||name.ends_with(".txt"),
            };
            if wanted {
                if components.len()>=MAX_COMPONENTS{return Err(());}
                let label=entry.path.strip_prefix(root).map_err(|_|())?.to_str().and_then(native_name).ok_or(())?;
                components.push(PluginComponent{kind:kind.clone(),name:label,source_path:collapse_home(&entry.path),native_id:None});
            }
        }
        // The native recursive walker also skips child symlinks; no bodies or
        // frontmatter are needed to retain a candidate filename.
    }Ok(())
}

fn server_shape(value:&Value) -> bool {
    let Some(o)=value.as_object()else{return false;};
    let strings=|v:&Value|v.as_object().is_some_and(|o|o.values().all(Value::is_string));
    let nonempty=|key:&str|o.get(key).and_then(Value::as_str).is_some_and(|s|!s.is_empty());
    if o.get("type").is_some_and(|v|!v.is_string())||o.get("cwd").is_some_and(|v|!v.is_string()){return false;}
    match o.get("type").and_then(Value::as_str).unwrap_or("stdio") {
        "stdio"=>nonempty("command")
            &&o.get("args").is_none_or(|v|v.as_array().is_some_and(|a|a.iter().all(Value::is_string)))
            &&o.get("env").is_none_or(strings),
        "http"|"sse"=>nonempty("url")&&o.get("headers").is_none_or(strings)
            &&o.get("clientId").is_none_or(Value::is_string),_=>false,
    }
}
fn truthy(value:&Value) -> bool {
    match value {
        Value::Null=>false,Value::Bool(v)=>*v,Value::String(v)=>!v.is_empty(),
        Value::Number(v)=>v.as_f64().is_some_and(|v|v!=0.0&&!v.is_nan()),_=>true,
    }
}
fn mcp_components(root:&Path,id:&str,components:&mut Vec<PluginComponent>,budget:&mut ScanBudget) -> Result<(),()> {
    let path=root.join(".mcp.json");
    let Some(value)=optional_document(root,".mcp.json",DocumentFormat::Json5,budget)?else{return Ok(());};
    let Some(servers)=value.get("mcpServers")else{return Ok(());};
    let entries:Vec<(String,&Value)>=match servers {
        Value::Object(o)=>o.iter().map(|(k,v)|(k.clone(),v)).collect(),
        Value::Array(a)=>a.iter().enumerate().map(|(i,v)|(i.to_string(),v)).collect(),_=>return Err(()),
    };
    if entries.len()>MAX_COMPONENTS.saturating_sub(components.len()){return Err(());}
    // A bad stdio args value throws during Wpn and discards that file's return.
    let throwing_args=entries.iter().any(|(_,v)|v.as_object().is_some_and(|o|
        o.get("type").and_then(Value::as_str).unwrap_or("stdio")=="stdio"
        &&o.get("command").is_some_and(truthy)
        &&o.get("args").is_some_and(|v|!v.is_null()&&!v.is_array())));
    let mut complete=true;
    for (key,config) in entries {
        let Some(name)=native_name(&key)else{complete=false;continue;};
        let known=!throwing_args&&server_shape(config);
        components.push(PluginComponent{kind:if known{PluginComponentKind::Mcp}else{PluginComponentKind::McpReference},
            name,source_path:collapse_home(&path),native_id:known.then(||format!("{id}/{key}"))});
        complete&=known;
    }
    if complete{Ok(())}else{Err(())}
}

fn package(host:&Host,id:String,name:String,market:&Path,manifest_path:&Path,path:&Path,presence:PluginPresence,
    result:&mut PluginScan) -> Result<(),()> {
    result.budget.visit()?;checked_root(path)?;
    let revision=format!("{}::{}",collapse_home(market),path.strip_prefix(market).map_err(|_|())?.to_str().ok_or(())?);
    let mut plugin=NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:id.clone(),name,version:"unknown".into(),revision,presence,enabled:None,
        source_paths:vec![collapse_home(manifest_path),collapse_home(path)],components:Vec::new(),
        docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    };
    match selected_document(path,&host.manifest_paths,&mut result.budget) {
        Ok(Some((source,value)))=>{
            plugin.source_paths.push(collapse_home(&source));
            plugin.version=value.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||"unknown".into());
        },Ok(None)=>{},Err(())=>{
            result.unavailable=true;
            plugin.components.push(reference_component(PluginComponentKind::PluginReference,path,"Optional plugin manifest unavailable or invalid"));
        },
    }
    for (folder,kind) in [("commands",PluginComponentKind::Command),("agents",PluginComponentKind::Agent),
        ("rules",PluginComponentKind::Rule),("skills",PluginComponentKind::Skill),("hooks",PluginComponentKind::Hook)] {
        let source=path.join(folder);
        if component_tree(path,&source,kind,0,&mut plugin.components,&mut result.budget).is_err() {
            result.unavailable=true;
            plugin.components.push(reference_component(PluginComponentKind::PluginReference,&source,"Component filenames incompletely observed"));
        }
    }
    if mcp_components(path,&id,&mut plugin.components,&mut result.budget).is_err() {
        result.unavailable=true;
        plugin.components.push(reference_component(PluginComponentKind::McpReference,&path.join(".mcp.json"),"MCP metadata incomplete or nonstandard"));
    }
    result.plugins.push(plugin);Ok(())
}

fn marketplace(host:&Host,market:&Path,source:&Path,value:&Value,result:&mut PluginScan) -> Result<(),()> {
    let name=value.get("name").and_then(Value::as_str).and_then(native_name).ok_or(())?;
    let entries=value.get("plugins").and_then(Value::as_array).ok_or(())?;
    if entries.len()>MAX_ENTRIES{return Err(());}
    let plugin_root=value.get("metadata").and_then(|v|v.get("pluginRoot")).and_then(Value::as_str);
    for (index,entry) in entries.iter().enumerate() {
        result.budget.visit()?;
        let Some(entry_name)=entry.get("name").and_then(Value::as_str).and_then(native_name)else{result.unavailable=true;continue;};
        let id=format!("{entry_name}@{name}");
        let candidate=match entry.get("source") {
            Some(Value::String(local))=>local_root(market,plugin_root,local).map(|p|p.map(|p|(p,PluginPresence::Local))),
            Some(Value::Object(remote)) if remote.get("source").and_then(Value::as_str)==Some("url")
                &&remote.get("url").is_some_and(Value::is_string)=>cached_root(market,&entry_name).map(|p|p.map(|p|(p,PluginPresence::CacheOnly))),
            _=>Err(()),
        };
        match candidate {
            Ok(Some((path,presence)))=>package(host,id,entry_name,market,source,&path,presence,result)?,
            _=>reference(host,id,entry_name,source,format!("{}::declaration:{index}",collapse_home(market)),result)?,
        }
    }Ok(())
}

fn market_reference(host:&Host,path:&Path,result:&mut PluginScan) -> Result<(),()> {
    let name=file_name(path).ok().and_then(native_name).unwrap_or_else(||"Marketplace".into());
    let source=collapse_home(path);
    reference(host,format!("@marketplace/{source}"),name,path,source,result)
}
fn global_markets(host:&Host,root:&Path,result:&mut PluginScan) -> Result<(),()> {
    checked_root(root)?;
    match marketplace_document(root,&mut result.budget) {
        Ok(Some((source,value)))=>match marketplace(host,root,&source,&value,result) {
            Ok(())=>return Ok(()),Err(())=>market_reference(host,root,result)?,
        },Ok(None)=>{},Err(())=>market_reference(host,root,result)?,
    }
    for entry in directory_entries(root,&mut result.budget)? {
        if file_name(&entry.path)?.starts_with('.') {continue;}
        if !entry.directory&&!fs::symlink_metadata(&entry.path).is_ok_and(|m|m.file_type().is_symlink()){continue;}
        let document=marketplace_document(&entry.path,&mut result.budget);
        match document {
            Ok(Some((source,value)))=>if marketplace(host,&entry.path,&source,&value,result).is_err(){market_reference(host,&entry.path,result)?;},
            _=>market_reference(host,&entry.path,result)?,
        }
    }Ok(())
}

fn disambiguate(start:usize,result:&mut PluginScan) {
    let mut locations=BTreeMap::<String,BTreeSet<String>>::new();
    let mut namespaces=BTreeMap::<String,BTreeSet<(String,String)>>::new();
    for plugin in &result.plugins[start..] {
        locations.entry(plugin.plugin_id.clone()).or_default().insert(plugin.revision.clone());
        for component in &plugin.components {
            if let Some(id)=&component.native_id {
                namespaces.entry(id.clone()).or_default().insert((plugin.plugin_id.clone(),plugin.revision.clone()));
            }
        }
    }
    for plugin in &mut result.plugins[start..] {
        let ambiguous=locations.get(&plugin.plugin_id).is_some_and(|paths|paths.len()>1);
        if ambiguous{result.unavailable=true;}
        for component in &mut plugin.components {
            let collision=component.native_id.as_ref().is_some_and(|id|namespaces.get(id).is_some_and(|sources|sources.len()>1));
            if component.kind==PluginComponentKind::Mcp&&(ambiguous||collision) {
                result.unavailable=true;component.kind=PluginComponentKind::McpReference;component.native_id=None;
            }
        }
    }
}
fn observe_contract(host:&Host,c:&Contract,result:&mut PluginScan) {
    if host.agent_id!=c.agent_id {result.unavailable=true;return;}
    let Some(selected)=crate::application::agent_launch::find_command(&c.command)else{return;};
    if verified_root(c,&selected,&mut result.budget).is_err(){result.unavailable=true;return;}
    let start=result.plugins.len();
    match marketplace_root(host) {
        Ok(root)=>if global_markets(host,&root,result).is_err(){result.unavailable=true;},Err(())=>result.unavailable=true,
    }
    disambiguate(start,result);
}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {observe_contract(host,contract(),result);}

fn watch_contract(host:&Host,c:&Contract) -> Vec<PathBuf> {
    let mut paths=crate::application::agent_launch::command_candidates(&c.command);
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command) {
        paths.push(selected.clone());
        if let Ok(physical)=fs::canonicalize(selected) {
            if let Some(root)=physical.parent(){for source in &c.sources{if let Ok(path)=relative(root,&source.path){paths.push(path);}}}
            paths.push(physical);
        }
    }
    if let Ok(root)=marketplace_root(host){paths.push(root);}
    paths.sort();paths.dedup();paths
}
pub(super) fn watch_paths(host:&Host) -> Vec<PathBuf> {watch_contract(host,contract())}

#[cfg(test)]
mod tests;
