//! Source-bound Droid registrations and component filenames. No native startup,
//! migration, settings/account queries, plugin execution or component-body reads.
use super::*;
use std::ffi::OsStr;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract { agent_id:String, command:String, version:String, binary_bytes:u64, binary_sha256:String }
struct Registration { id:String, scope:String, path:String, version:String, portable:bool, source:PathBuf }
struct Registrations { entries:Vec<Registration>, issues:BTreeSet<PathBuf>, uncertain:bool }

fn contract() -> &'static Contract {
    static VALUE:OnceLock<Contract>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-factory-droid.json"))
        .expect("audited Factory Droid distribution contract"))
}
fn absolute(value:&OsStr) -> Result<PathBuf,()> {
    let text=value.to_str().ok_or(())?;let path=PathBuf::from(value);
    if !path.is_absolute()||text.len()>4096||text.chars().any(char::is_control)
        ||path.components().any(|c|matches!(c,Component::ParentDir|Component::Prefix(_))){return Err(());}
    Ok(path)
}
fn verified(c:&Contract,selected:&Path) -> Result<PathBuf,()> {
    absolute(selected.as_os_str())?;let physical=fs::canonicalize(selected).map_err(|_|())?;
    // A complete image binds all775 JS chunks, bytecode and native companions.
    // Its public-byte allowance is independent of other installed hosts.
    verify_artifact(&physical,c.binary_bytes,&c.binary_sha256,&mut ScanBudget::default())?;
    if fs::canonicalize(selected).map_err(|_|())?!=physical{return Err(());}
    Ok(physical)
}
fn version_for(c:&Contract,selected:&Path) -> Option<String> {verified(c,selected).ok().map(|_|c.version.clone())}
pub(super) fn runtime_version(selected:&Path) -> Option<String> {version_for(contract(),selected)}

pub(super) fn base_from_env(home:Option<&OsStr>,fallback:Option<&Path>,home_override:Option<&OsStr>,environment:Option<&OsStr>) -> Result<PathBuf,()> {
    let directory=match environment.map(OsStr::to_str) {
        None|Some(Some("production"))=>".factory",Some(Some("development"))=>".factory-dev",_=>return Err(()),
    };
    let root=if let Some(value)=home_override.filter(|v|!v.is_empty()) {absolute(value)?}
        else if let Some(value)=home {absolute(value)?}else{absolute(fallback.ok_or(())?.as_os_str())?};
    Ok(root.join(directory))
}
fn native_base() -> Result<PathBuf,()> {
    let home=std::env::var_os("HOME");let fallback=dirs::home_dir();
    let home_override=std::env::var_os("FACTORY_HOME_OVERRIDE");let environment=std::env::var_os("FACTORY_ENV");
    base_from_env(home.as_deref(),fallback.as_deref(),home_override.as_deref(),environment.as_deref())
}
fn registry_root(host:&Host,base:&Path) -> Result<PathBuf,()> {
    absolute(base.as_os_str())?;checked_root(base)?;
    let roots=host.roots.iter().filter(|r|r.layout=="factory_installed_registry"&&r.presence==PluginPresence::Installed).collect::<Vec<_>>();
    let [root]=roots.as_slice()else{return Err(());};
    relative(base,root.path.strip_prefix("~/.factory/").ok_or(())?)
}
fn raw_name(value:&str) -> Option<String> {
    (!value.is_empty()&&value.len()<=240&&!value.chars().any(char::is_control)).then(||value.into())
}
fn name(id:&str) -> String {
    id.char_indices().find(|(i,c)|*i>0&&*c=='@'&&i+1<id.len())
        .and_then(|(i,_)|raw_name(&id[..i])).unwrap_or_else(||id.into())
}
fn reference_component(kind:PluginComponentKind,path:&Path,label:&str) -> PluginComponent {
    PluginComponent{kind,name:label.into(),source_path:collapse_home(path),native_id:None}
}
fn registry_reference(host:&Host,path:&Path,result:&mut PluginScan) {
    result.unavailable=true;let source=collapse_home(path);
    let id=hex::encode(Sha256::digest(source.as_bytes()));
    result.plugins.push(NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:format!("@registry/{}",&id[..16]),name:"Plugin registration".into(),
        version:"unknown".into(),revision:source.clone(),presence:PluginPresence::Declared,enabled:None,source_paths:vec![source],
        components:vec![reference_component(PluginComponentKind::PluginReference,path,"Registration metadata unavailable or invalid")],
        docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    });
}
fn entry(id:&str,value:&Value,source:&Path) -> Option<Registration> {
    let id=raw_name(id)?;let o=value.as_object()?;
    let scope=o.get("scope")?.as_str()?;if !matches!(scope,"org"|"project"|"user"){return None;}
    for key in ["installPath","version","installedAt","lastUpdated","source"]{if !o.get(key).is_some_and(Value::is_string){return None;}}
    if o.get("portableOnly").is_some_and(|v|!v.is_boolean()){return None;}
    if o.get("marketplaceSourceHash").is_some_and(|v|!v.as_str().is_some_and(|v|v.len()==64&&v.bytes().all(|c|c.is_ascii_digit()||matches!(c,b'a'..=b'f')))){return None;}
    if let Some(npm)=o.get("npm") {
        let npm=npm.as_object()?;
        if !["spec","version"].iter().all(|k|npm.get(*k).is_some_and(Value::is_string))
            ||["resolved","integrity"].iter().any(|k|npm.get(*k).is_some_and(|v|!v.is_string())){return None;}
    }
    Some(Registration{id,scope:scope.into(),path:o["installPath"].as_str()?.into(),
        version:o["version"].as_str()?.into(),portable:o.get("portableOnly")==Some(&Value::Bool(true)),source:source.to_path_buf()})
}
fn registrations(root:&Path,budget:&mut ScanBudget) -> Registrations {
    let mut groups=BTreeMap::<String,Vec<Registration>>::new();let mut issues=BTreeSet::new();let mut uncertain=false;
    let legacy=root.join("installed_plugins.json");
    match optional_document(root,"installed_plugins.json",DocumentFormat::JsonBom,budget) {
        Ok(Some(value))=>if let Some(plugins)=value.get("plugins").and_then(Value::as_object) {
            for (id,values) in plugins {
                let Some(values)=values.as_array()else{issues.insert(legacy.clone());continue;};
                for value in values {
                    if value.get("scope").and_then(Value::as_str)==Some("project"){continue;}
                    if let Some(record)=entry(id,value,&legacy){groups.entry(id.clone()).or_default().push(record);}
                    else{issues.insert(legacy.clone());}
                }
            }
        }else{issues.insert(legacy.clone());},Ok(None)=>{},Err(())=>{issues.insert(legacy.clone());uncertain=true;},
    }
    let directory=root.join("installed_plugins");
    match directory_entries(&directory,budget) {
        Ok(mut files)=>{
            // Native readdir().sort() uses UTF16 lexical order, not natural order
            // or platform byte order. Filenames do not supply plugin identities.
            files.sort_by_cached_key(|v|v.path.file_name().unwrap_or_default().to_string_lossy().encode_utf16().collect::<Vec<_>>());
            for file in files {
                if file.path.extension().and_then(|s|s.to_str())!=Some("json"){continue;}
                if !file.file{issues.insert(file.path);uncertain=true;continue;}
                let Some(input)=file.path.file_name().and_then(|s|s.to_str())else{issues.insert(file.path);uncertain=true;continue;};
                match optional_document(&directory,input,DocumentFormat::JsonBom,budget) {
                    Ok(Some(value))=>{
                        let id=value.get("pluginId").and_then(Value::as_str);let v=value.get("entry");
                        if v.and_then(|v|v.get("scope")).and_then(Value::as_str)==Some("project"){continue;}
                        if let Some(record)=id.zip(v).and_then(|(id,v)|entry(id,v,&file.path)) {
                            let records=groups.entry(record.id.clone()).or_default();records.retain(|r|r.scope!=record.scope);records.push(record);
                        }else{issues.insert(file.path);}
                    },Ok(None)=>{},Err(())=>{issues.insert(file.path);uncertain=true;},
                }
            }
        },Err(())=>{issues.insert(directory);uncertain=true;},
    }
    Registrations{entries:groups.into_values().flatten().collect(),issues,uncertain}
}

fn folders(path:&Path,budget:&mut ScanBudget) -> Result<Vec<ScanEntry>,()> {
    checked_root(path)?;
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir()=>directory_entries(path,budget),Ok(m) if m.is_file()=>Ok(Vec::new()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(Vec::new()),_=>Err(()),
    }
}
fn marker(root:&Path,path:&Path,kind:PluginComponentKind,components:&mut Vec<PluginComponent>) -> Result<(),()> {
    if components.len()>=MAX_COMPONENTS{return Err(());}
    let label=path.strip_prefix(root).map_err(|_|())?.to_str().ok_or(())?;
    if label.len()>4096||label.chars().any(char::is_control){return Err(());}
    components.push(PluginComponent{kind,name:label.into(),source_path:collapse_home(path),native_id:None});Ok(())
}
fn commands(root:&Path,path:&Path,depth:usize,components:&mut Vec<PluginComponent>,budget:&mut ScanBudget) -> Result<(),()> {
    if depth>16{return Err(());}let mut complete=true;
    for file in folders(path,budget)? {
        if file.directory{if commands(root,&file.path,depth+1,components,budget).is_err(){complete=false;}}
        else if file.file{marker(root,&file.path,PluginComponentKind::Command,components)?;}
        else{marker(root,&file.path,PluginComponentKind::PluginReference,components)?;complete=false;}
    }if complete{Ok(())}else{Err(())}
}
fn skills(root:&Path,path:&Path,depth:usize,components:&mut Vec<PluginComponent>,budget:&mut ScanBudget) -> Result<(),()> {
    if depth>16{return Err(());}let mut complete=true;
    for file in folders(path,budget)? {
        if file.directory {
            let skill=file.path.join("SKILL.md");
            match regular_metadata_file(&skill) {
                Ok(true)=>marker(root,&skill,PluginComponentKind::Skill,components)?,
                Ok(false)=>if skills(root,&file.path,depth+1,components,budget).is_err(){complete=false;},
                Err(())=>{marker(root,&file.path,PluginComponentKind::PluginReference,components)?;complete=false;},
            }
        }else if !file.file{marker(root,&file.path,PluginComponentKind::PluginReference,components)?;complete=false;}
    }if complete{Ok(())}else{Err(())}
}
fn markdown(root:&Path,folder:&str,kind:PluginComponentKind,components:&mut Vec<PluginComponent>,budget:&mut ScanBudget) -> Result<(),()> {
    let mut complete=true;
    for file in folders(&root.join(folder),budget)? {
        if file.path.extension().and_then(|v|v.to_str())!=Some("md"){continue;}
        if file.file{marker(root,&file.path,kind.clone(),components)?;}
        else{marker(root,&file.path,PluginComponentKind::PluginReference,components)?;complete=false;}
    }if complete{Ok(())}else{Err(())}
}
fn hooks(root:&Path,components:&mut Vec<PluginComponent>) -> Result<(),()> {
    for input in ["hooks/hooks.json","hooks.json"] {
        let path=root.join(input);
        if regular_metadata_file(&path)?{marker(root,&path,PluginComponentKind::Hook,components)?;break;}
    }Ok(())
}

fn js_trim(value:&str) -> &str {
    value.trim_matches(|c|matches!(c,'\t'|'\n'|'\u{b}'|'\u{c}'|'\r'|' '|'\u{a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}'))
}
fn oauth(value:&Value) -> bool {
    if value==&Value::Bool(false){return true;}
    let Some(o)=value.as_object()else{return false;};
    let nonblank=|v:&Value|v.as_str().is_some_and(|v|!js_trim(v).is_empty());
    let url=|v:&Value|v.as_str().is_some_and(|s|url::Url::parse(s).is_ok());
    if o.get("scopes").is_some_and(|v|!v.as_array().is_some_and(|v|v.iter().all(nonblank)))
        ||o.get("resource").is_some_and(|v|v!=&Value::Bool(false)&&!url(v))
        ||o.get("authorizationServerIssuer").is_some_and(|v|!url(v))
        ||["clientId","clientSecret"].iter().any(|k|o.get(*k).is_some_and(|v|!nonblank(v)))
        ||o.get("callbackPort").is_some_and(|v|!v.as_f64().is_some_and(|v|v.fract()==0.0&&(1.0..=65535.0).contains(&v)))
        ||o.get("tokenEndpointAuthMethod").is_some_and(|v|!matches!(v.as_str(),Some("none"|"client_secret_basic"|"client_secret_post"))){return false;}
    if let Some(metadata)=o.get("clientMetadataUrl") {
        static DOTS:OnceLock<regex::Regex>=OnceLock::new();
        let pattern=DOTS.get_or_init(||regex::Regex::new(r"(?i)(?:^|/)(?:\.\.|\.(?:%2e)?|%2e(?:\.|%2e)?)(?:/|$|[?#])").expect("native OAuth dot-segment expression"));
        let Some(text)=metadata.as_str()else{return false;};let Ok(parsed)=url::Url::parse(text)else{return false;};
        if pattern.is_match(text)||parsed.scheme()!="https"||parsed.path()=="/"||!parsed.username().is_empty()
            ||parsed.password().is_some_and(|v|!v.is_empty())||parsed.query().is_some_and(|v|!v.is_empty())||parsed.fragment().is_some_and(|v|!v.is_empty())
            ||o.contains_key("clientId")||o.contains_key("clientSecret")
            ||o.get("tokenEndpointAuthMethod").is_some_and(|v|v!="none"){return false;}
    }
    if (o.contains_key("clientId")||o.contains_key("clientSecret"))&&!o.contains_key("authorizationServerIssuer"){return false;}
    true
}
fn server(value:&Value) -> bool {
    let Some(o)=value.as_object()else{return false;};
    let strings=|v:&Value|v.as_object().is_some_and(|v|v.values().all(Value::is_string));
    let array=|v:&Value|v.as_array().is_some_and(|v|v.iter().all(Value::is_string));
    if o.get("type").is_some_and(|v|!v.is_string())||o.get("disabled").is_some_and(|v|!v.is_boolean())
        ||["enabledTools","disabledTools"].iter().any(|k|o.get(*k).is_some_and(|v|!array(v)))
        ||["timeout","connectTimeout"].iter().any(|k|o.get(*k).is_some_and(|v|!v.as_f64().is_some_and(|v|v.fract()==0.0&&v>0.0&&v<=2147483647.0))){return false;}
    match o.get("type").and_then(Value::as_str).unwrap_or("stdio") {
        "stdio"=>o.get("command").is_some_and(Value::is_string)&&o.get("args").is_none_or(array)&&o.get("env").is_none_or(strings),
        "http"|"sse"=>o.get("url").is_some_and(Value::is_string)&&o.get("headers").is_none_or(strings)&&o.get("oauth").is_none_or(oauth),_=>false,
    }
}
fn permissions(value:&Value) -> bool {
    let Some(o)=value.as_object()else{return false;};
    let record=|v:&Value|v.as_object().is_some_and(|o|o.get("approvedAt").is_some_and(Value::is_string)
        &&matches!(o.get("impactLevel").and_then(Value::as_str),Some("none"|"low"|"medium"|"high"))
        &&o.get("serverIdentity").is_none_or(Value::is_string));
    o.get("servers").is_none_or(|v|v.as_object().is_some_and(|v|v.values().all(record)))
        &&o.get("tools").is_none_or(|v|v.as_object().is_some_and(|v|v.values().all(|v|v.as_object().is_some_and(|v|v.values().all(record)))))
}
fn mcp(root:&Path,components:&mut Vec<PluginComponent>,budget:&mut ScanBudget) -> Result<(),()> {
    let base=root.join("mcp.json");if !regular_metadata_file(&base)?{return Ok(());}
    let Some(value)=optional_document(root,"mcp.json",DocumentFormat::Json,budget)?else{return Err(());};
    let mut source=base;let mut fallback=None;
    if value.get("$schema").and_then(Value::as_str).is_some_and(|s|s.starts_with("https://agent-plugins.org/schemas/")) {
        if let Some(config)=optional_document(root,".mcp.json",DocumentFormat::Json,budget)?{source=root.join(".mcp.json");fallback=Some(config);}
    }
    let value=fallback.as_ref().unwrap_or(&value);
    let direct=source.file_name()==Some(OsStr::new(".mcp.json"))&&!value.get("mcpServers").is_some_and(Value::is_object);
    let servers=if direct{value.as_object()}
        else{value.get("mcpServers").and_then(Value::as_object)}.ok_or(())?;
    if servers.len()>MAX_COMPONENTS.saturating_sub(components.len()){return Err(());}
    let complete=servers.iter().all(|(k,v)|raw_name(k).is_some()&&server(v))
        &&(direct||value.get("persistentPermissions").is_none_or(permissions));
    for key in servers.keys() {
        let Some(name)=raw_name(key)else{continue;};
        components.push(PluginComponent{kind:if complete{PluginComponentKind::Mcp}else{PluginComponentKind::McpReference},
            name,source_path:collapse_home(&source),native_id:complete.then(||key.clone())});
    }
    if complete{Ok(())}else{Err(())}
}

fn observe_record(host:&Host,record:&Registration,uncertain:bool,result:&mut PluginScan) -> Result<(),()> {
    result.budget.visit()?;
    let root=absolute(OsStr::new(&record.path)).ok();
    let physical=root.as_ref().is_some_and(|p|checked_root(p).is_ok()&&fs::symlink_metadata(p).is_ok_and(|m|m.is_dir()));
    let source=collapse_home(&record.source);
    let mut plugin=NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:record.id.clone(),name:name(&record.id),
        version:metadata_name(&record.version).unwrap_or_else(||"unknown".into()),
        revision:format!("{}::{}",record.scope,root.as_ref().map(|p|collapse_home(p)).unwrap_or_else(||source.clone())),
        presence:if physical&&!uncertain{PluginPresence::Installed}else{PluginPresence::Declared},enabled:None,source_paths:vec![source],
        components:Vec::new(),docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    };
    if !physical {
        result.unavailable=true;
        plugin.components.push(reference_component(PluginComponentKind::PluginReference,&record.source,"Registered package unavailable or selection incomplete"));
        result.plugins.push(plugin);return Ok(());
    }
    let root=root.ok_or(())?;plugin.source_paths.push(collapse_home(&root));
    if uncertain {
        result.unavailable=true;
        plugin.components.push(reference_component(PluginComponentKind::PluginReference,&record.source,"Registration selection incomplete; physical metadata retained"));
    }
    for (folder,kind) in [("droids",PluginComponentKind::Agent),("output-styles",PluginComponentKind::OutputStyle)] {
        if record.portable&&folder=="output-styles"{continue;}
        if markdown(&root,folder,kind,&mut plugin.components,&mut result.budget).is_err() {
            result.unavailable=true;plugin.components.push(reference_component(PluginComponentKind::PluginReference,&root.join(folder),"Component filenames incomplete"));
        }
    }
    for (folder,skill) in [("skills",true),("commands",false)] {
        let path=root.join(folder);
        let observed=if skill{skills(&root,&path,0,&mut plugin.components,&mut result.budget)}else{commands(&root,&path,0,&mut plugin.components,&mut result.budget)};
        if observed.is_err(){result.unavailable=true;plugin.components.push(reference_component(PluginComponentKind::PluginReference,&path,"Component filenames incomplete"));}
    }
    if hooks(&root,&mut plugin.components).is_err(){result.unavailable=true;plugin.components.push(reference_component(PluginComponentKind::PluginReference,&root.join("hooks"),"Hook file selection unverified"));}
    if record.portable {
        let path=root.join("mcp.json");
        if fs::symlink_metadata(&path).is_ok(){plugin.components.push(reference_component(PluginComponentKind::McpReference,&path,"MCP excluded by portable installation"));}
    }else if mcp(&root,&mut plugin.components,&mut result.budget).is_err() {
        result.unavailable=true;plugin.components.push(reference_component(PluginComponentKind::McpReference,&root.join("mcp.json"),"MCP metadata unavailable or invalid"));
    }
    if uncertain {
        for component in &mut plugin.components {
            if component.kind==PluginComponentKind::Mcp{component.kind=PluginComponentKind::McpReference;component.native_id=None;}
        }
    }
    result.plugins.push(plugin);Ok(())
}
fn disambiguate(start:usize,result:&mut PluginScan) {
    let mut locations=BTreeMap::<String,BTreeSet<String>>::new();let mut namespaces=BTreeMap::<String,BTreeSet<(String,String)>>::new();
    for plugin in &result.plugins[start..] {
        locations.entry(plugin.plugin_id.clone()).or_default().insert(plugin.revision.clone());
        for component in &plugin.components{if let Some(id)=&component.native_id{namespaces.entry(id.clone()).or_default().insert((plugin.plugin_id.clone(),plugin.revision.clone()));}}
    }
    for plugin in &mut result.plugins[start..] {
        let ambiguous=locations.get(&plugin.plugin_id).is_some_and(|v|v.len()>1);
        if ambiguous{result.unavailable=true;}
        for component in &mut plugin.components {
            let collision=component.native_id.as_ref().is_some_and(|id|namespaces.get(id).is_some_and(|v|v.len()>1));
            if component.kind==PluginComponentKind::Mcp&&(ambiguous||collision){component.kind=PluginComponentKind::McpReference;component.native_id=None;result.unavailable=true;}
        }
    }
}
fn observe_at(host:&Host,c:&Contract,selected:&Path,base:&Path,result:&mut PluginScan) {
    if host.agent_id!=c.agent_id||verified(c,selected).is_err(){result.unavailable=true;return;}
    observe_registered(host,base,result);
}
// Callers bind their own independently audited runtime before sharing only
// the byte-identical installed-registry and component grammar.
pub(super) fn observe_registered(host:&Host,base:&Path,result:&mut PluginScan) {
    let Ok(root)=registry_root(host,base)else{result.unavailable=true;return;};
    let registered=registrations(&root,&mut result.budget);let start=result.plugins.len();
    for path in registered.issues{registry_reference(host,&path,result);}
    for record in registered.entries{if observe_record(host,&record,registered.uncertain,result).is_err(){result.unavailable=true;break;}}
    disambiguate(start,result);
}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {
    let c=contract();let Some(selected)=crate::application::agent_launch::find_command(&c.command)else{return;};
    match native_base(){Ok(base)=>observe_at(host,c,&selected,&base,result),Err(())=>result.unavailable=true,}
}
fn watch_at(host:&Host,c:&Contract,base:&Path) -> Vec<PathBuf> {
    let mut paths=crate::application::agent_launch::command_candidates(&c.command);
    let mut bound=false;
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command){paths.push(selected.clone());if let Ok(p)=verified(c,&selected){paths.push(p);bound=true;}}
    paths.extend(registry_watch_paths(host,base,bound));
    paths.sort();paths.dedup();paths
}
pub(super) fn registry_watch_paths(host:&Host,base:&Path,bound:bool) -> Vec<PathBuf> {
    let mut paths=Vec::new();
    if let Ok(root)=registry_root(host,base) {
        paths.extend([root.join("installed_plugins.json"),root.join("installed_plugins")]);
        if bound {
            let registered=registrations(&root,&mut ScanBudget::default());
            for record in registered.entries{if let Ok(path)=absolute(OsStr::new(&record.path)){if checked_root(&path).is_ok(){paths.push(path);}}}
        }
    }
    paths.sort();paths.dedup();paths
}
pub(super) fn watch_paths(host:&Host) -> Vec<PathBuf> {
    match native_base(){Ok(base)=>watch_at(host,contract(),&base),Err(())=>crate::application::agent_launch::command_candidates(&contract().command),}
}

#[cfg(test)]
mod tests;
