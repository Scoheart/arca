//! Kimi's registry and native manifest grammar, independent of writable assets.
use super::*;

fn plugin_name(name: &str) -> bool {
    let word=|s:&str|!s.is_empty()&&s.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-'));
    let mut parts=name.split('@');let base=parts.next().unwrap_or("");
    base.split('.').all(word) && parts.next().is_none_or(word) && parts.next().is_none()
}

fn record_id(entry: &Value) -> Option<String> {
    let id=entry.get("id").and_then(Value::as_str)?;
    // Native installs write manifest-derived IDs. Unexpected metadata must not
    // export an arbitrary source URL or credentials as an identifier.
    (plugin_name(id)&&id.len()<=240).then(||id.to_string())
}

fn entries(root: &Path, budget: &mut ScanBudget) -> Result<Vec<Value>, ()> {
    checked_root(root)?;
    let Some(document)=optional_json(root,"installed.json",budget)? else { return Ok(Vec::new()); };
    let list=document.as_object().and_then(|d|d.get("plugins")).and_then(Value::as_array).ok_or(())?;
    if list.len()>MAX_ENTRIES { return Err(()); }
    // The audited reader checks plugins[], not the registry's version value.
    Ok(list.clone())
}

fn mark(components: &mut Vec<PluginComponent>, kind: PluginComponentKind, name: String,
    path: &Path, native_id: Option<String>, budget: &mut ScanBudget) -> Result<(), ()> {
    if components.len()>=MAX_COMPONENTS { return Err(()); }
    budget.visit()?;
    components.push(PluginComponent {kind,name,source_path:collapse_home(path),native_id});Ok(())
}

fn reference(components: &mut Vec<PluginComponent>, kind: PluginComponentKind, name: &str,
    path: &Path, result: &mut PluginScan) -> Result<(), ()> {
    result.unavailable=true;mark(components,kind,name.to_string(),path,None,&mut result.budget)
}

pub(super) fn observe_registry(host: &Host, root: &Path, verified: bool, result: &mut PluginScan) {
    let list=match entries(root,&mut result.budget) {Ok(list)=>list,Err(())=>{result.unavailable=true;return;}};
    let mut counts=BTreeMap::<String,usize>::new();
    for entry in &list {if let Some(id)=record_id(entry){*counts.entry(id).or_default()+=1;}}
    let registry=root.join("installed.json");
    for (index,entry) in list.iter().enumerate() {
        if result.budget.visit().is_err() {result.unavailable=true;break;}
        let id=record_id(entry);
        let path=global_base(entry.get("root").and_then(Value::as_str));
        let mut item=NativePluginObservation {
            agent_id:host.agent_id.clone(),plugin_id:id.clone().unwrap_or_else(||format!("@registry/invalid-{index}")),
            name:id.clone().unwrap_or_else(||"Unrecognized Kimi registry record".into()),version:"unknown".into(),
            revision:path.as_deref().map(collapse_home).unwrap_or_else(||format!("registry:{index}")),
            presence:PluginPresence::Declared,enabled:None,source_paths:vec![collapse_home(&registry)],
            components:Vec::new(),docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
        };
        let diagnostic=if id.is_none() || path.is_none() {Some("Invalid Kimi registry record")}
            else if !verified {Some("Kimi runtime identity unverified")}
            else if counts.get(id.as_ref().unwrap()).copied().unwrap_or(0)>1 {Some("Conflicting registered plugin identity")}
            else {None};
        if let Some(label)=diagnostic {
            let source=path.as_deref().unwrap_or(&registry);
            let _=reference(&mut item.components,PluginComponentKind::PluginReference,label,source,result);
        } else {
            let path=path.as_ref().unwrap();
            if read_package(host,id.as_ref().unwrap(),path,&mut item,result).is_err() {
                item.components.clear();item.presence=PluginPresence::Declared;
                let _=reference(&mut item.components,PluginComponentKind::PluginReference,"Kimi manifest unavailable or invalid",path,result);
            }
        }
        item.source_paths.sort();item.source_paths.dedup();
        result.plugins.push(item);
        if result.budget.visited>MAX_SCAN_ENTRIES || result.budget.references>MAX_REFERENCES || result.budget.bytes>=MAX_READ_BYTES {result.unavailable=true;break;}
    }
}

pub(super) fn registered_watch_paths(root: &Path) -> Vec<PathBuf> {
    let mut paths=Vec::new();let mut budget=ScanBudget::default();
    if let Ok(list)=entries(root,&mut budget) {
        for entry in list {
            let Some(path)=global_base(entry.get("root").and_then(Value::as_str)).filter(|p|checked_root(p).is_ok()) else {continue;};
            paths.push(path.join("kimi.plugin.json"));paths.push(path.join(".kimi-plugin/plugin.json"));paths.push(path);
        }
    }
    paths.sort();paths.dedup();paths
}

fn plugin_path(root: &Path, value: &str) -> Result<PathBuf, ()> {
    if !value.starts_with("./") || value.len()>4096 || value.chars().any(char::is_control) {return Err(());}
    relative(root,value)
}

fn is_directory(path: &Path) -> Result<bool, ()> {
    checked_root(path)?;
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir()=>Ok(open_confined(path,true)?.metadata().map_err(|_|())?.is_dir()),
        Ok(m) if m.is_file()=>Ok(false),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(false),
        _=>Err(()),
    }
}

fn string_paths(value: &Value) -> Result<Vec<&str>, ()> {
    if let Some(s)=value.as_str() {return Ok(vec![s]);}
    let list=value.as_array().ok_or(())?;
    if list.len()>MAX_COMPONENTS {return Err(());}
    list.iter().map(|v|v.as_str().ok_or(())).collect()
}

fn dir_paths(root: &Path, value: &Value, result: &mut PluginScan) -> Result<Vec<PathBuf>, ()> {
    let mut paths=Vec::new();
    for input in string_paths(value)? {
        let path=plugin_path(root,input)?;
        if is_directory(&path)? {paths.push(path);} else {result.unavailable=true;}
    }
    paths.sort();paths.dedup();Ok(paths)
}

fn path_name(path: &Path, root: &Path, markdown: bool) -> Option<String> {
    let name=path.strip_prefix(root).ok()?.to_str()?.replace(std::path::MAIN_SEPARATOR,"/");
    let name=if markdown {name.strip_suffix(".md")?}else{name.as_str()};
    metadata_name(name)
}

fn skill_marker(path: &Path, root: &Path, conditional: bool, components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    if !regular_metadata_file(path)? {return Ok(());}
    let name=if path.file_name().is_some_and(|n|n=="SKILL.md") {
        path.parent().and_then(|p|path_name(p,root,false)).or_else(||root.file_name().and_then(|s|s.to_str()).and_then(metadata_name))
    }else{path_name(path,root,true)}.ok_or(())?;
    mark(components,if conditional {PluginComponentKind::PluginReference}else{PluginComponentKind::Skill},
        if conditional {format!("Conditional sub-Skill: {name}")}else{name},path,None,&mut result.budget)
}

fn skills(dir: &Path, root: &Path, top: bool, depth: usize, conditional: bool,
    components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    if depth>8 {result.unavailable=true;return Ok(());}
    let entries=directory_entries(dir,&mut result.budget)?;
    let mut directory_skills=BTreeSet::new();
    for entry in &entries {
        if !entry.directory {if !entry.file {result.unavailable=true;}continue;}
        let marker=entry.path.join("SKILL.md");
        if regular_metadata_file(&marker)? {
            directory_skills.insert(entry.path.clone());skill_marker(&marker,root,conditional,components,result)?;
        }
    }
    if top {
        skill_marker(&dir.join("SKILL.md"),root,conditional,components,result)?;
        for entry in &entries {
            if !entry.file || entry.path.file_name().is_some_and(|s|s=="SKILL.md") || entry.path.extension().is_none_or(|s|s!="md") {continue;}
            let shadow=entry.path.with_extension("");if directory_skills.contains(&shadow) {continue;}
            skill_marker(&entry.path,root,conditional,components,result)?;
        }
    }
    for entry in entries {
        let excluded=entry.path.file_name().and_then(|s|s.to_str()).is_none_or(|s|s.starts_with('.')||s=="node_modules");
        if !entry.directory || excluded {continue;}
        // Private frontmatter controls descent below an already found Skill.
        // Inspect names only and retain descendants as conditional references.
        skills(&entry.path,root,false,depth+1,conditional||directory_skills.contains(&entry.path),components,result)?;
    }
    Ok(())
}

fn markdown(dir: &Path, root: &Path, kind: PluginComponentKind, depth: usize,
    components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    if depth>8 {result.unavailable=true;return Ok(());}
    for entry in directory_entries(dir,&mut result.budget)? {
        let hidden=entry.path.file_name().and_then(|s|s.to_str()).is_none_or(|s|s.starts_with('.')||s=="node_modules");
        if kind==PluginComponentKind::Agent && hidden {continue;}
        if entry.directory {markdown(&entry.path,root,kind.clone(),depth+1,components,result)?;}
        else if entry.file && entry.path.extension().is_some_and(|s|s=="md") {
            mark(components,kind.clone(),path_name(&entry.path,root,true).ok_or(())?,&entry.path,None,&mut result.budget)?;
        } else if !entry.file {result.unavailable=true;}
    }
    Ok(())
}

fn string_list(value: &Value) -> bool {value.as_array().is_some_and(|a|a.iter().all(Value::is_string))}
fn string_map(value: &Value) -> bool {value.as_object().is_some_and(|m|m.values().all(Value::is_string))}
fn nonempty(value: &Value) -> bool {value.as_str().is_some_and(|s|!s.is_empty())}

fn mcp_valid(root: &Path, value: &Value) -> bool {
    let Some(m)=value.as_object() else {return false;};
    for key in ["enabled","deferred"] {if m.get(key).is_some_and(|v|!v.is_boolean()){return false;}}
    for key in ["startupTimeoutMs","toolTimeoutMs"] {if m.get(key).is_some_and(|v|!v.as_u64().is_some_and(|n|(1..=2_147_483_647).contains(&n))){return false;}}
    for key in ["enabledTools","disabledTools"] {if m.get(key).is_some_and(|v|!string_list(v)){return false;}}
    let transport=match m.get("transport") {
        Some(v)=>v.as_str(),None if m.get("command").is_some_and(Value::is_string)=>Some("stdio"),
        None if m.get("url").is_some_and(Value::is_string)=>Some("http"),_=>None,
    };
    match transport {
        Some("stdio")=>{
            let Some(command)=m.get("command").and_then(Value::as_str).filter(|s|!s.is_empty()) else {return false;};
            if m.get("args").is_some_and(|v|!string_list(v)) || m.get("env").is_some_and(|v|!string_map(v))
                || m.get("executor").is_some_and(|v|v.as_str()!=Some("local")) || m.get("runtime_id").is_some_and(|v|!nonempty(v)) {return false;}
            if command.starts_with("./") {if plugin_path(root,command).is_err(){return false;}}
            else if command.contains(['/', '\\'])||Path::new(command).is_absolute() {return false;}
            m.get("cwd").is_none_or(|v|v.as_str().is_some_and(|s|plugin_path(root,s).is_ok()))
        },
        Some("http"|"sse")=>m.get("url").and_then(Value::as_str).is_some_and(|s|url::Url::parse(s).is_ok())
            && m.get("headers").is_none_or(string_map) && m.get("auth").is_none_or(|v|v.as_str()==Some("oauth"))
            && m.get("bearerTokenEnvVar").is_none_or(nonempty),
        _=>false,
    }
}

fn mcps(id: &str, root: &Path, manifest: &Path, raw: Option<&Value>, components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    let Some(raw)=raw else {return Ok(());};
    let Some(servers)=raw.as_object().filter(|m|m.len()<=MAX_COMPONENTS) else {
        return reference(components,PluginComponentKind::McpReference,"mcpServers",manifest,result);
    };
    let mut counts=BTreeMap::<String,usize>::new();
    for name in servers.keys() {if let Some(name)=metadata_name(name){*counts.entry(name).or_default()+=1;}}
    for (name,value) in servers {
        let Some(name)=metadata_name(name).filter(|n|!n.contains("://")) else {
            reference(components,PluginComponentKind::McpReference,"Unrecognized MCP name",manifest,result)?;continue;
        };
        if counts.get(&name)!=Some(&1) || !mcp_valid(root,value) {
            reference(components,PluginComponentKind::McpReference,&name,manifest,result)?;continue;
        }
        mark(components,PluginComponentKind::Mcp,name.clone(),manifest,Some(format!("plugin-{id}:{name}")),&mut result.budget)?;
    }
    Ok(())
}

fn hooks(raw: Option<&Value>, manifest: &Path, components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    let Some(raw)=raw else {return Ok(());};
    let Some(list)=raw.as_array().filter(|a|a.len()<=MAX_COMPONENTS) else {return reference(components,PluginComponentKind::PluginReference,"Invalid Hook declarations",manifest,result);};
    for value in list {
        let valid=value.as_object().is_some_and(|m|m.keys().all(|k|["event","matcher","command","timeout"].contains(&k.as_str()))
            && m.get("event").and_then(Value::as_str).is_some_and(|s|matches!(s,
                "PreToolUse"|"PostToolUse"|"PostToolUseFailure"|"PermissionRequest"|"PermissionResult"|"UserPromptSubmit"|"UserPromptQueued"|"TurnStarted"|"Stop"|"StopFailure"|"Interrupt"|"SessionStart"|"SessionEnd"|"SessionHeartbeat"|"SubagentStart"|"SubagentStop"|"TaskStarted"|"PreCompact"|"PostCompact"|"Notification"))
            && m.get("command").is_some_and(nonempty) && m.get("matcher").is_none_or(Value::is_string)
            && m.get("timeout").is_none_or(|v|v.as_u64().is_some_and(|n|(1..=600).contains(&n))));
        if valid {mark(components,PluginComponentKind::Hook,value["event"].as_str().unwrap().into(),manifest,None,&mut result.budget)?;}
        else {reference(components,PluginComponentKind::PluginReference,"Invalid Hook declaration",manifest,result)?;}
    }
    Ok(())
}

fn commands(root: &Path, manifest: &Path, raw: Option<&Value>, components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    let Some(raw)=raw else {return Ok(());};
    let paths=match string_paths(raw) {Ok(paths)=>paths,Err(())=>return reference(components,PluginComponentKind::PluginReference,"Invalid Command declarations",manifest,result)};
    for input in paths {
        let path=match plugin_path(root,input) {Ok(path)=>path,Err(())=>{reference(components,PluginComponentKind::PluginReference,"Invalid Command path",manifest,result)?;continue;}};
        if is_directory(&path)? {markdown(&path,&path,PluginComponentKind::Command,0,components,result)?;}
        else if regular_metadata_file(&path)? && path.extension().is_some_and(|s|s=="md") {
            mark(components,PluginComponentKind::Command,path_name(&path,path.parent().ok_or(())?,true).ok_or(())?,&path,None,&mut result.budget)?;
        } else {reference(components,PluginComponentKind::PluginReference,"Unavailable Command path",manifest,result)?;}
    }
    Ok(())
}

fn context(root: &Path, manifest: &Path, raw: &Value, components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    for key in ["systemPrompt","skillInstructions"] {
        if let Some(value)=raw.get(key) {
            if value.as_str().is_some_and(|s|!s.trim().is_empty()&&(key!="systemPrompt"||s.len()<=32768)) {
                mark(components,PluginComponentKind::Context,key.into(),manifest,None,&mut result.budget)?;
            } else if !value.is_string() || value.as_str().is_some_and(|s|key=="systemPrompt"&&s.len()>32768) {
                reference(components,PluginComponentKind::PluginReference,"Invalid context declaration",manifest,result)?;
            }
        }
    }
    if let Some(value)=raw.get("systemPromptPath") {
        let path=value.as_str().and_then(|s|plugin_path(root,s.trim()).ok());
        if let Some(path)=path.filter(|p|regular_metadata_file(p).unwrap_or(false)&&fs::symlink_metadata(p).is_ok_and(|m|m.len()<=32768)) {
            mark(components,PluginComponentKind::Context,"systemPromptPath".into(),&path,None,&mut result.budget)?;
        }else{reference(components,PluginComponentKind::PluginReference,"Unavailable context path",manifest,result)?;}
    }
    if let Some(value)=raw.get("sessionStart") {
        if value.as_object().and_then(|m|m.get("skill")).and_then(Value::as_str).is_some_and(|s|!s.trim().is_empty()) {
            mark(components,PluginComponentKind::PluginReference,"sessionStart.skill".into(),manifest,None,&mut result.budget)?;
        }else{reference(components,PluginComponentKind::PluginReference,"Invalid sessionStart declaration",manifest,result)?;}
    }
    for key in ["tools","apps","inject","configFile","config_file","bootstrap"] {
        if raw.get(key).is_some() {mark(components,PluginComponentKind::PluginReference,format!("Unsupported Kimi field: {key}"),manifest,None,&mut result.budget)?;}
    }
    Ok(())
}

fn read_package(_host: &Host, id: &str, root: &Path, item: &mut NativePluginObservation, result: &mut PluginScan) -> Result<(), ()> {
    if !is_directory(root)? {return Err(());}
    let primary=root.join("kimi.plugin.json");let alternate=root.join(".kimi-plugin/plugin.json");
    let path=if regular_metadata_file(&primary)? {primary} else if regular_metadata_file(&alternate)? {alternate.clone()}else{return Err(());};
    item.source_paths.push(collapse_home(&path));
    let raw=optional_json(root,path.strip_prefix(root).map_err(|_|())?.to_str().ok_or(())?,&mut result.budget)?.ok_or(())?;
    if !raw.is_object() || !raw.get("name").and_then(Value::as_str).is_some_and(|s|plugin_name(s.trim())) {return Err(());}
    let skill_roots=match raw.get("skills") {Some(value)=>dir_paths(root,value,result)?,None=>Vec::new()};
    let agent_roots=match raw.get("agents") {Some(value)=>dir_paths(root,value,result)?,None=>{
        let path=root.join("agents");if is_directory(&path)? {vec![path]}else{Vec::new()}
    }};
    let mut components=Vec::new();
    if raw.get("skills").is_none() && skill_marker(&root.join("SKILL.md"),root,false,&mut components,result).is_err() {
        reference(&mut components,PluginComponentKind::PluginReference,"Root Skill metadata incomplete",&root.join("SKILL.md"),result)?;
    }
    for dir in skill_roots {
        if skills(&dir,&dir,true,0,false,&mut components,result).is_err() {
            reference(&mut components,PluginComponentKind::PluginReference,"Skill metadata incomplete",&dir,result)?;
        }
    }
    for dir in agent_roots {
        if markdown(&dir,&dir,PluginComponentKind::Agent,0,&mut components,result).is_err() {
            reference(&mut components,PluginComponentKind::PluginReference,"Agent metadata incomplete",&dir,result)?;
        }
    }
    mcps(id,root,&path,raw.get("mcpServers"),&mut components,result)?;
    hooks(raw.get("hooks"),&path,&mut components,result)?;
    // Command/context warnings are independent of otherwise valid MCPs.
    if commands(root,&path,raw.get("commands"),&mut components,result).is_err() {
        reference(&mut components,PluginComponentKind::PluginReference,"Command metadata incomplete",&path,result)?;
    }
    context(root,&path,&raw,&mut components,result)?;
    if path!=alternate {
        match regular_metadata_file(&alternate) {
            Ok(true)=>{
                item.source_paths.push(collapse_home(&alternate));
                mark(&mut components,PluginComponentKind::PluginReference,"Shadowed Kimi manifest".into(),&alternate,None,&mut result.budget)?;
            },
            Err(())=>{reference(&mut components,PluginComponentKind::PluginReference,"Shadowed manifest metadata incomplete",&alternate,result)?;},
            Ok(false)=>{},
        }
    }
    components.sort();components.dedup();
    item.name=raw.get("interface").and_then(|v|v.get("displayName")).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||id.to_string());
    item.version=raw.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||"unknown".into());
    item.presence=PluginPresence::Installed;item.components=components;Ok(())
}
