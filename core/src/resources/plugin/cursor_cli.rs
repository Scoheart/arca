//! Cursor CLI source-bound declarations, independent of Desktop session state.
//! Never invokes the CLI, plugin code or the side-effecting helper factory.
use super::*;
use crate::domain::mcp::BuiltinMcpObservation;
use crate::resources::mcp::builtin::BuiltinMcpScan;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract { agent_id:String, command:String, profiles:Vec<Profile>, builtin_mcp:Builtin }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile { version:String, mode:Mode, sources:Vec<SourceFile> }
#[derive(Clone,Copy,PartialEq,Eq,Deserialize)]
#[serde(rename_all="snake_case")]
enum Mode { PassiveVersionOnly, NativeMetadata }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFile { path:String, bytes:u64, sha256:String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Builtin { id:String, source_file:String, source_module:String, registration_module:String }
struct Package<'a> { root:PathBuf, profile:&'a Profile }

fn contract() -> &'static Contract {
    static VALUE:OnceLock<Contract>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-cursor-cli.json"))
        .expect("audited Cursor CLI source contracts"))
}
fn verified_package<'a>(c:&'a Contract,selected:&Path,budget:&mut ScanBudget) -> Result<Package<'a>,()> {
    if !selected.is_absolute()||selected.to_str().is_none_or(|s|s.chars().any(char::is_control)){return Err(());}
    let physical=fs::canonicalize(selected).map_err(|_|())?;checked_root(&physical)?;
    if physical.file_name().and_then(|s|s.to_str())!=Some(c.command.as_str()){return Err(());}
    let root=physical.parent().ok_or(())?;
    for profile in &c.profiles {
        let Some(entry)=profile.sources.iter().find(|s|s.path==c.command)else{continue;};
        if fs::symlink_metadata(&physical).map_err(|_|())?.len()!=entry.bytes{continue;}
        let mut valid=true;
        for input in &profile.sources {
            if verify_artifact(&relative(root,&input.path)?,input.bytes,&input.sha256,budget).is_err(){valid=false;break;}
        }
        if valid {
            if fs::canonicalize(selected).map_err(|_|())?!=physical{return Err(());}
            return Ok(Package{root:root.to_path_buf(),profile});
        }
    }
    Err(())
}
fn selected_package<'a>(c:&'a Contract) -> Result<Option<Package<'a>>,()> {
    crate::application::agent_launch::find_command(&c.command)
        .map(|path|verified_package(c,&path,&mut ScanBudget::default())).transpose()
}
pub(super) fn runtime_version(selected:&Path) -> Option<String> {
    verified_package(contract(),selected,&mut ScanBudget::default()).ok().map(|p|p.profile.version.clone())
}

fn builtin_items(c:&Contract,p:&Package<'_>) -> Vec<BuiltinMcpObservation> {
    if p.profile.mode!=Mode::NativeMetadata||!p.profile.sources.iter().any(|s|s.path==c.builtin_mcp.source_file){return Vec::new();}
    let file=collapse_home(&p.root.join(&c.builtin_mcp.source_file));
    let manifest=format!("{file}!{}",c.builtin_mcp.source_module);
    let source=format!("{file}!{}",c.builtin_mcp.registration_module);
    let material=serde_json::to_vec(&(&p.profile.version,p.profile.sources.iter().map(|s|(&s.path,&s.sha256)).collect::<Vec<_>>(),&source)).expect("public source identity serializes");
    vec![BuiltinMcpObservation {
        agent_id:c.agent_id.clone(),plugin_id:format!("{}-{}",c.agent_id,c.builtin_mcp.id),plugin_version:p.profile.version.clone(),
        manifest_path:manifest,server_id:c.builtin_mcp.id.clone(),name:c.builtin_mcp.id.clone(),provider:"Cursor CLI".into(),
        source_path:source,app_version:p.profile.version.clone(),docs_url:"https://cursor.com/docs/cli/overview".into(),
        source_fingerprint:hex::encode(Sha256::digest(material)),
    }]
}
pub(crate) fn builtin_observe(agents:&BTreeMap<String,AgentDefinition>) -> BuiltinMcpScan {
    let c=contract();let mut scan=BuiltinMcpScan::default();
    if !agents.get(&c.agent_id).is_some_and(|a|a.enabled){return scan;}
    match selected_package(c) {
        Ok(Some(p)) if p.profile.mode==Mode::NativeMetadata=>{scan.items=builtin_items(c,&p);scan.unavailable=scan.items.is_empty();},
        Ok(Some(_))|Err(())=>scan.unavailable=true,Ok(None)=>{},
    }
    scan
}

fn name(path:&Path) -> Result<String,()> {
    let raw=path.file_name().and_then(|s|s.to_str()).ok_or(())?;
    metadata_name(raw).filter(|s|s==raw).ok_or(())
}
fn reference(host:&Host,path:&Path,id:String,label:String,revision:String,result:&mut PluginScan) {
    result.unavailable=true;
    result.plugins.push(NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:id,name:label,version:"unknown".into(),revision,presence:PluginPresence::Declared,enabled:None,
        source_paths:vec![collapse_home(path)],components:vec![PluginComponent{kind:PluginComponentKind::PluginReference,
            name:"Plugin metadata incomplete or entry type unverified".into(),source_path:collapse_home(path),native_id:None}],
        docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    });
}
fn reserved_metadata(key:&str,value:&Value) -> bool {
    let Some(o)=value.as_object()else{return false;};
    let string=|key:&str|o.get(key).is_some_and(Value::is_string);
    match key.to_ascii_lowercase().as_str() {
        "author"|"owner"=>string("name"),"source"=>["source","repo","path","ref","sha"].iter().any(|k|string(k)),
        "metadata"=>["description","version","pluginRoot"].iter().any(|k|string(k)),_=>false,
    }
}
// Shape evidence only. Runtime expansion, connection and acceptance stay native.
fn server_shape(value:&Value) -> Result<bool,()> {
    let o=value.as_object().ok_or(())?;
    let strings=|v:&Value|v.as_object().is_some_and(|o|o.values().all(Value::is_string));
    let array=|v:&Value|v.as_array().is_some_and(|a|a.iter().all(Value::is_string));
    for key in ["command","url","cwd","envFile"] {if o.get(key).is_some_and(|v|!v.is_string()){return Err(());}}
    for key in ["env","headers"] {if o.get(key).is_some_and(|v|!strings(v)){return Err(());}}
    for key in ["args","enabledTools"] {if o.get(key).is_some_and(|v|!array(v)){return Err(());}}
    if o.get("placement").is_some_and(|v|!matches!(v.as_str(),Some("server"|"client"))){return Err(());}
    if let Some(auth)=o.get("auth") {
        let auth=auth.as_object().ok_or(())?;
        if !auth.get("CLIENT_ID").is_some_and(Value::is_string)||auth.get("CLIENT_SECRET").is_some_and(|v|!v.is_string())
            ||auth.get("scopes").is_some_and(|v|!array(v)){return Err(());}
    }
    if let Some(tls)=o.get("tls") {
        let ca=tls.get("caBundle").and_then(Value::as_str).ok_or(())?;
        if ca.trim().is_empty()||ca.trim().encode_utf16().count()>131072{return Err(());}
    }
    Ok(["command","url"].iter().any(|key|o.get(*key).and_then(Value::as_str).is_some_and(|s|!s.is_empty())))
}
fn declaration_keys(value:&Value) -> Result<Vec<String>,()> {
    let root=value.as_object().ok_or(())?;
    let (fields,wrapped)=match root.get("mcpServers").and_then(Value::as_object).filter(|v|!v.is_empty()) {
        Some(fields)=>(fields,true),None=>(root,false),
    };
    if fields.len()>MAX_COMPONENTS{return Err(());}
    let mut keys=Vec::new();
    for (key,config) in fields {
        if !wrapped&&(key=="mcpServers"||reserved_metadata(key,config)||!config.as_object().is_some_and(|o|o.contains_key("command")||o.contains_key("url"))){continue;}
        if server_shape(config)? {
            let clean=metadata_name(key).filter(|s|s==key).ok_or(())?;keys.push(clean);
        }
    }
    Ok(keys)
}
fn mcp_file(root:&Path,input:&str,servers:&mut BTreeSet<(String,PathBuf)>,budget:&mut ScanBudget) -> Result<(),()> {
    // Do not open arbitrary scripts/Markdown just because a manifest references them.
    if input.len()>4096||input.chars().any(char::is_control){return Err(());}
    if !matches!(Path::new(input).extension().and_then(|s|s.to_str()),Some("json"|"jsonc")){return Err(());}
    let path=relative(root,input)?;
    if let Some(value)=optional_document(root,input,DocumentFormat::Jsonc,budget)? {
        for key in declaration_keys(&value)?{servers.insert((key,path.clone()));}
    }
    Ok(())
}
fn manifest_mcps(root:&Path,value:&Value,source:&Path,servers:&mut BTreeSet<(String,PathBuf)>,depth:usize,budget:&mut ScanBudget) -> Result<(),()> {
    budget.visit()?;if depth>8||servers.len()>MAX_COMPONENTS{return Err(());}
    match value {
        Value::String(path)=>mcp_file(root,path,servers,budget)?,
        Value::Array(parts)=>{if parts.len()>64{return Err(());}for part in parts{manifest_mcps(root,part,source,servers,depth+1,budget)?;}},
        Value::Object(_)=>for key in declaration_keys(value)?{servers.insert((key,source.to_path_buf()));},
        _=>return Err(()),
    }
    Ok(())
}
fn package_metadata(host:&Host,source:&Root,path:&Path,id:String,alias:Option<&str>,result:&mut PluginScan) -> Result<(),()> {
    result.budget.visit()?;checked_root(path)?;
    let mut manifest=None;
    for input in &host.manifest_paths {
        if let Some(value)=optional_json(path,input,&mut result.budget)?{manifest=Some((relative(path,input)?,value));break;}
    }
    let fallback=if source.presence==PluginPresence::CacheOnly{name(path.parent().ok_or(())?)?}else{name(path)?};
    let label=manifest.as_ref().and_then(|(_,m)|m.get("name")).and_then(Value::as_str).map(|n|metadata_name(n).filter(|s|s==n).ok_or(())).transpose()?.unwrap_or(fallback);
    let version=manifest.as_ref().and_then(|(_,m)|m.get("version")).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||"unknown".into());
    let mut components=Vec::new();let mut servers=BTreeSet::new();
    for input in &host.default_mcp_files {if mcp_file(path,input,&mut servers,&mut result.budget).is_err(){result.unavailable=true;}}
    if let Some((manifest_path,m))=&manifest {
        if let Some(value)=m.get("mcpServers") {
            if manifest_mcps(path,value,manifest_path,&mut servers,0,&mut result.budget).is_err(){result.unavailable=true;}
        }
        if m.get("hooks").and_then(Value::as_object).is_some_and(|o|!o.is_empty()) {
            components.push(PluginComponent{kind:PluginComponentKind::Hook,name:"Inline hooks declaration".into(),source_path:collapse_home(manifest_path),native_id:None});
        }
    }
    if servers.len()>MAX_COMPONENTS{return Err(());}
    for (server,file) in servers {
        components.push(PluginComponent{kind:if alias.is_some(){PluginComponentKind::Mcp}else{PluginComponentKind::McpReference},
            native_id:alias.map(|alias|format!("{}{alias}-{server}",host.mcp_id_prefix)),name:server,source_path:collapse_home(&file)});
    }
    for (key,folder,kind) in [("skills","skills",PluginComponentKind::Skill),("rules","rules",PluginComponentKind::Rule),
        ("agents","agents",PluginComponentKind::Agent),("commands","commands",PluginComponentKind::Command),("hooks","hooks",PluginComponentKind::Hook)] {
        for input in component_paths(manifest.as_ref().and_then(|(_,m)|m.get(key)),folder) {
            if input.len()>4096||input.chars().any(char::is_control){result.unavailable=true;continue;}
            match relative(path,&input) {
                Ok(file)=>match regular_metadata_file(&file) {
                    Ok(true)=>components.push(PluginComponent{kind:kind.clone(),name:input,source_path:collapse_home(&file),native_id:None}),
                    Ok(false)=>if component_files(path,&file,&kind,0,&mut components,&mut result.budget).is_err(){result.unavailable=true;},
                    Err(())=>result.unavailable=true,
                },Err(())=>result.unavailable=true,
            }
        }
    }
    if components.len()>MAX_COMPONENTS{return Err(());}
    components.sort();components.dedup();
    if components.is_empty()&&manifest.is_none(){components.push(PluginComponent{kind:PluginComponentKind::PluginReference,name:"Plugin directory; no component metadata observed".into(),source_path:collapse_home(path),native_id:None});}
    result.plugins.push(NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:id,name:label,version,revision:if source.presence==PluginPresence::CacheOnly{name(path)?}else{"local".into()},
        presence:source.presence.clone(),enabled:None,source_paths:vec![collapse_home(manifest.as_ref().map(|(p,_)|p.as_path()).unwrap_or(path))],
        components,docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    });Ok(())
}
fn scan_root(host:&Host,source:&Root,result:&mut PluginScan) -> Result<(),()> {
    let root=expand_tilde(&source.path);if !root.is_absolute(){return Err(());}checked_root(&root)?;
    match source.layout.as_str() {
        "flat" if source.presence==PluginPresence::Local=>for entry in directory_entries(&root,&mut result.budget)? {
            let label=name(&entry.path)?;if label.starts_with('.'){continue;}let id=format!("@local/{label}");
            if entry.directory {
                if package_metadata(host,source,&entry.path,id.clone(),Some(&label),result).is_err(){reference(host,&entry.path,id,label,"local".into(),result);}
            }else if fs::symlink_metadata(&entry.path).is_ok_and(|m|m.file_type().is_symlink()){reference(host,&entry.path,id,label,"local".into(),result);}
        },
        "marketplace_cache" if source.presence==PluginPresence::CacheOnly=>for market in dirs(&root,&mut result.budget)? {
            for identity in dirs(&market,&mut result.budget)? {
                let id=format!("@cache/{}/{}",name(&market)?,name(&identity)?);
                for entry in directory_entries(&identity,&mut result.budget)? {
                    if entry.file{continue;}let revision=name(&entry.path)?;
                    let complete=entry.directory&&source.complete_marker.as_deref().is_some_and(|marker|relative(&entry.path,marker).is_ok_and(|p|regular_metadata_file(&p)==Ok(true)));
                    if !complete||package_metadata(host,source,&entry.path,id.clone(),None,result).is_err(){reference(host,&entry.path,id.clone(),name(&identity)?,revision,result);}
                }
            }
        },_=>return Err(()),
    }Ok(())
}
fn observe_contract(host:&Host,c:&Contract,result:&mut PluginScan) {
    if host.agent_id!=c.agent_id{result.unavailable=true;return;}
    match selected_package(c) {
        Ok(Some(p)) if p.profile.mode==Mode::NativeMetadata=>for source in &host.roots{if scan_root(host,source,result).is_err(){result.unavailable=true;}},
        Ok(Some(_))|Err(())=>result.unavailable=true,Ok(None)=>{},
    }
}
pub(super) fn observe(host:&Host,result:&mut PluginScan){observe_contract(host,contract(),result);}

fn launcher_watch_paths(c:&Contract) -> Vec<PathBuf> {
    let mut paths=crate::application::agent_launch::command_candidates(&c.command);
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command) {
        if let Some(parent)=selected.parent(){paths.push(parent.to_path_buf());}
        if let Ok(physical)=fs::canonicalize(selected) {
            paths.push(physical.clone());
            if let Some(root)=physical.parent(){paths.push(root.to_path_buf());for profile in &c.profiles{for source in &profile.sources{if let Ok(path)=relative(root,&source.path){paths.push(path);}}}}
        }
    }paths
}
fn watch_contract(host:&Host,c:&Contract) -> Vec<PathBuf> {
    let mut paths=launcher_watch_paths(c);let mut budget=ScanBudget::default();
    for source in &host.roots {
        let root=expand_tilde(&source.path);paths.push(root.clone());
        let packages=if source.layout=="flat"{dirs(&root,&mut budget).unwrap_or_default()}else {
            let mut packages=Vec::new();for market in dirs(&root,&mut budget).unwrap_or_default(){paths.push(market.clone());
                for identity in dirs(&market,&mut budget).unwrap_or_default(){paths.push(identity.clone());packages.extend(dirs(&identity,&mut budget).unwrap_or_default());}}
            packages
        };
        for package in packages {
            paths.push(package.clone());
            paths.extend(host.manifest_paths.iter().chain(&host.default_mcp_files).filter_map(|p|relative(&package,p).ok()));
            if let Some(marker)=&source.complete_marker{if let Ok(path)=relative(&package,marker){paths.push(path);}}
        }
    }paths.sort();paths.dedup();paths
}
pub(super) fn watch_paths(host:&Host) -> Vec<PathBuf>{watch_contract(host,contract())}
pub(crate) fn builtin_watch_paths(agents:&BTreeMap<String,AgentDefinition>) -> Vec<PathBuf> {
    let c=contract();if !agents.get(&c.agent_id).is_some_and(|a|a.enabled){return Vec::new();}
    let mut paths=launcher_watch_paths(c);paths.sort();paths.dedup();paths
}

#[cfg(test)]
mod tests;
