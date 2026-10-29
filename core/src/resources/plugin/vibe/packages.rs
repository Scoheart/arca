//! Vibe3.0.3 package declarations; file markers are not runtime admission.
use super::*;

const SCHEMA:&str="https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const MCP_SCHEMA:&str="https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";
const EXT:&str="ai.mistral.vibe";
const RESERVED:[&str;9]=["agent","agents","collaboration","file_system","process","self","skill","subagent","vibe"];
#[derive(Clone,Copy,PartialEq,Eq)]
enum Format {Native,Codex,Claude,Kimi,OpenCode}
pub(super) struct Candidate {observation:NativePluginObservation,namespace:String}

fn add(components:&mut Vec<PluginComponent>,kind:PluginComponentKind,name:&str,path:&Path,id:Option<String>) -> Result<(),()> {
    if components.len()>=MAX_COMPONENTS {return Err(());}
    components.push(PluginComponent {kind,name:metadata_name(name).ok_or(())?,source_path:collapse_home(path),native_id:id});Ok(())
}
fn identifier(value:&str) -> String {
    let mut s=value.chars().map(|c|if c.is_ascii_alphanumeric()||matches!(c,'_'|'$'){c}else{'_'}).collect::<String>();
    if s.is_empty(){s.push('_');}else if s.as_bytes()[0].is_ascii_digit(){s.insert(0,'_');}s
}
fn alias(plugin:&str,server:&str) -> String {
    let digest=<blake2::Blake2s<blake2::digest::consts::U8> as blake2::Digest>::digest(format!("{plugin}\0{server}").as_bytes());
    let mut suffix=identifier(if server.is_empty(){"server"}else{server});suffix.truncate(80);
    format!("plugin_{}_{suffix}",hex::encode(digest))
}
fn name_valid(s:&str,dots:bool) -> bool {
    !s.is_empty()&&s.len()<=64&&s.as_bytes()[0].is_ascii_alphanumeric()&&s.as_bytes()[s.len()-1].is_ascii_alphanumeric()
        &&!s.contains("--")&&!s.contains("..")&&s.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-'||(dots&&b==b'.'))
}
fn object_keys(value:&Value,allowed:&[&str]) -> bool {value.as_object().is_some_and(|m|m.keys().all(|k|allowed.contains(&k.as_str())))}
fn optional_strings(value:&Value,keys:&[&str]) -> bool {keys.iter().all(|k|value.get(k).is_none_or(|v|v.is_null()||v.is_string()))}
fn string_list(value:Option<&Value>,null:bool) -> bool {value.is_none_or(|v|(null&&v.is_null())||v.as_array().is_some_and(|a|a.len()<=MAX_COMPONENTS&&a.iter().all(Value::is_string)))}
fn string_map(value:Option<&Value>,null:bool) -> bool {value.is_none_or(|v|(null&&v.is_null())||v.as_object().is_some_and(|m|m.len()<=MAX_COMPONENTS&&m.values().all(Value::is_string)))}
fn paths(value:Option<&Value>) -> Result<Vec<String>,()> {
    match value {None|Some(Value::Null)=>Ok(Vec::new()),Some(Value::String(s))=>Ok(vec![s.clone()]),Some(Value::Array(a)) if a.len()<=64=>a.iter().map(|v|v.as_str().map(str::to_owned).ok_or(())).collect(),_=>Err(())}
}
fn declared(root:&Path,value:&str) -> Result<PathBuf,()> {
    if value.is_empty()||value.contains('\\')||(value.as_bytes().get(1)==Some(&b':')&&value.as_bytes()[0].is_ascii_alphabetic()){return Err(());}
    let tail=value.strip_prefix("./").unwrap_or(value);let p=relative(root,if tail.is_empty(){"."}else{tail})?;
    let m=fs::symlink_metadata(&p).map_err(|_|())?;if !m.is_file()&&!m.is_dir(){return Err(());}Ok(p)
}
fn portable_name(value:&str) -> Result<String,()> {
    static RE:OnceLock<regex::Regex>=OnceLock::new();let re=RE.get_or_init(||regex::Regex::new("[^a-z0-9]+").unwrap());
    let input=value.strip_prefix('@').unwrap_or(value).replace('/',"-").to_lowercase();let normalized=re.replace_all(&input,"-");let normalized=normalized.trim_matches('-');
    if normalized.is_empty(){return Err(());}if normalized.len()<=64{return Ok(normalized.to_owned());}
    let digest=<blake2::Blake2s<blake2::digest::consts::U5> as blake2::Digest>::digest(normalized.as_bytes());
    Ok(format!("{}-{}",normalized[..53].trim_end_matches('-'),hex::encode(digest)))
}
fn native_manifest(v:&Value) -> bool {
    if !object_keys(v,&["$schema","name","version","description","author","homepage","repository","license","keywords","extensions"])
        ||v.get("$schema").and_then(Value::as_str)!=Some(SCHEMA)||!v.get("name").and_then(Value::as_str).is_some_and(|s|name_valid(s,true))
        ||!optional_strings(v,&["version","description","homepage","repository","license"])||!string_list(v.get("keywords"),true) {return false;}
    if let Some(author)=v.get("author").filter(|v|!v.is_null()) {
        if !object_keys(author,&["name","email","url"])||!author.get("name").and_then(Value::as_str).is_some_and(|s|!s.is_empty())||!optional_strings(author,&["email","url"]) {return false;}
    }
    v.get("extensions").is_none_or(|v|v.is_null()||v.is_object())
}
fn extension(v:&Value) -> Option<&Value> {
    let e=v.get("extensions")?.get(EXT)?;
    if !object_keys(e,&["schemaVersion","displayName","toolNamespace","toolOverrides"])||e.get("schemaVersion").and_then(Value::as_u64)!=Some(1)
        ||!optional_strings(e,&["displayName","toolNamespace"]) {return None;}
    if e.get("displayName").and_then(Value::as_str).is_some_and(|s|s.is_empty()||s.chars().count()>100) {return None;}
    if e.get("toolNamespace").and_then(Value::as_str).is_some_and(|s|s.is_empty()||identifier(s)!=s) {return None;}
    if let Some(overrides)=e.get("toolOverrides") {
        let m=overrides.as_object()?;if m.len()>MAX_COMPONENTS{return None;}
        for item in m.values() {
            if !object_keys(item,&["name","exposure"])||!optional_strings(item,&["name","exposure"])
                ||item.get("name").and_then(Value::as_str).is_some_and(|s|s.is_empty()||identifier(s)!=s)
                ||item.get("exposure").and_then(Value::as_str).is_some_and(|s|!matches!(s,"programmatic"|"direct"|"direct_and_programmatic")) {return None;}
        }
    }
    Some(e)
}

fn marker(root:&Path,path:&Path,kind:PluginComponentKind,components:&mut Vec<PluginComponent>) -> Result<(),()> {
    if !regular_metadata_file(path)? {return Ok(());}
    let label=path.strip_prefix(root).map_err(|_|())?.to_str().ok_or(())?;
    add(components,kind,label,path,None)
}
fn immediate_skills(root:&Path,base:&Path,components:&mut Vec<PluginComponent>,budget:&mut ScanBudget) -> Result<(),()> {
    for entry in directory_entries(base,budget)? {
        if entry.directory {marker(root,&entry.path.join("SKILL.md"),PluginComponentKind::Skill,components)?;}
        else if !entry.file {return Err(());}
    }
    Ok(())
}
fn walk(root:&Path,base:&Path,kind:PluginComponentKind,depth:usize,components:&mut Vec<PluginComponent>,budget:&mut ScanBudget) -> Result<(),()> {
    if depth>8 {return Err(());}
    if regular_metadata_file(base)? {
        if !base.extension().and_then(|s|s.to_str()).is_some_and(|s|s.eq_ignore_ascii_case("md")){return Err(());}
        return marker(root,base,kind,components);
    }
    for entry in directory_entries(base,budget)? {
        if entry.directory {walk(root,&entry.path,kind.clone(),depth+1,components,budget)?;}
        else if entry.file {
            let wanted=match kind {PluginComponentKind::Skill=>entry.path.file_name().is_some_and(|s|s=="SKILL.md"),_=>entry.path.extension().is_some_and(|s|s=="md")};
            if wanted {marker(root,&entry.path,kind.clone(),components)?;}
        }else{return Err(());}
    }
    Ok(())
}
fn foreign_skills(root:&Path,base:&Path,codex:bool,components:&mut Vec<PluginComponent>,budget:&mut ScanBudget) -> Result<(),()> {
    if codex {if !base.is_dir(){return Err(());}return immediate_skills(root,base,components,budget);}
    if regular_metadata_file(base)? {if !base.file_name().is_some_and(|s|s=="SKILL.md"){return Err(());}return marker(root,base,PluginComponentKind::Skill,components);}
    let direct=base.join("SKILL.md");if regular_metadata_file(&direct)?{return marker(root,&direct,PluginComponentKind::Skill,components);}
    immediate_skills(root,base,components,budget)
}
fn code_files(base:&Path,depth:usize,budget:&mut ScanBudget,output:&mut Vec<PathBuf>) -> Result<(),()> {
    if depth>8||output.len()>MAX_COMPONENTS{return Err(());}
    for entry in directory_entries(base,budget)? {
        if entry.directory {code_files(&entry.path,depth+1,budget,output)?;}
        else if entry.file&&entry.path.extension().and_then(|s|s.to_str()).is_some_and(|s|matches!(s.to_ascii_lowercase().as_str(),"js"|"ts"|"mjs"|"cjs"|"mts"|"cts")) {output.push(entry.path);}
        else if !entry.file{return Err(());}
    }
    Ok(())
}
fn export_paths(v:&Value,depth:usize,out:&mut Vec<String>) -> Result<(),()> {
    if depth>8||out.len()>64{return Err(());}
    match v {Value::String(s)=>out.push(s.clone()),Value::Object(m)=>for v in m.values(){export_paths(v,depth+1,out)?;},Value::Array(a)=>for v in a{export_paths(v,depth+1,out)?;},_=>{}}Ok(())
}
fn opencode_markers(root:&Path,result:&mut PluginScan) -> Result<Vec<PathBuf>,()> {
    let mut markers=Vec::new();
    for dir in [".opencode/plugins",".opencode/tools"] {code_files(&relative(root,dir)?,0,&mut result.budget,&mut markers)?;}
    for file in ["opencode.json","opencode.jsonc",".opencode/opencode.json",".opencode/opencode.jsonc"] {let p=relative(root,file)?;if regular_metadata_file(&p)?{markers.push(p);}}
    for p in dirs(&relative(root,".opencode/skills")?,&mut result.budget)? {let p=p.join("SKILL.md");if regular_metadata_file(&p)?{markers.push(p);}}
    let package=match optional_json(root,"package.json",&mut result.budget){Ok(v)=>v,Err(())=>{result.unavailable=true;None}};
    if let Some(pkg)=package {
        if pkg.get("keywords").and_then(Value::as_array).is_some_and(|a|a.iter().any(|v|matches!(v.as_str(),Some("opencode"|"opencode-plugin")))) {
            let mut entries=Vec::new();for key in ["main","module","exports"] {if let Some(v)=pkg.get(key){export_paths(v,0,&mut entries)?;}}
            if entries.iter().any(|s|relative(root,s).ok().is_some_and(|p|regular_metadata_file(&p).unwrap_or(false))) {markers.push(root.join("package.json"));}
        }
    }
    Ok(markers)
}

fn mcp_valid(root:&Path,v:&Value,format:Format) -> bool {
    let Some(m)=v.as_object() else{return false;};
    if format==Format::Native {
        if !standard_mcp_declaration(v)||v.get("type").and_then(Value::as_str)==Some("sse"){return false;}
    }
    let nullable=matches!(format,Format::Claude|Format::Kimi|Format::OpenCode);
    if let Some(command)=v.get("command").and_then(Value::as_str).filter(|s|!s.is_empty()) {
        if format==Format::Codex&&v.get("type").is_some_and(|v|!v.is_null()&&v.as_str()!=Some("stdio")){return false;}
        if !string_list(v.get("args"),nullable)||!string_map(v.get("env"),nullable)||v.get("env").is_some_and(|e|e.get("PLUGIN_ROOT").is_some()||e.get("PLUGIN_DATA").is_some()){return false;}
        let relative_command=command.strip_prefix("./").or_else(||["${CLAUDE_PLUGIN_ROOT}/","${KIMI_PLUGIN_ROOT}/"].into_iter().find_map(|p|command.strip_prefix(p)));
        if let Some(path)=relative_command {if relative(root,path).is_err(){return false;}}
        else if command.contains(['/', '\\'])||matches!(command,"."|".."){return false;}
        if let Some(cwd)=v.get("cwd").filter(|v|!v.is_null()) {
            let Some(cwd)=cwd.as_str()else{return false;};
            let tail=["${PLUGIN_ROOT}","${PLUGIN_DATA}","${CLAUDE_PLUGIN_ROOT}","${CLAUDE_PLUGIN_DATA}","${KIMI_PLUGIN_ROOT}"].into_iter().find_map(|prefix|cwd.strip_prefix(prefix).filter(|s|s.is_empty()||s.starts_with('/')).map(|s|s.trim_start_matches('/'))).unwrap_or(cwd.strip_prefix("./").unwrap_or(cwd));
            if !tail.is_empty()&&relative(root,tail).is_err(){return false;}
        }
        return true;
    }
    if m.contains_key("command")&&format==Format::Codex{return false;}
    if format==Format::Codex&&(v.get("type").and_then(Value::as_str)!=Some("http")||!optional_strings(v,&["bearer_token_env_var","oauth_resource"])){return false;}
    let Some(input)=v.get("url").and_then(Value::as_str)else{return false;};
    let Ok(url)=url::Url::parse(input)else{return false;};
    if format==Format::Native {
        if let Some(headers)=v.get("headers").and_then(Value::as_object) {
            let mut seen=BTreeSet::new();
            if headers.iter().any(|(key,value)|key.is_empty()||!key.bytes().all(|b|b.is_ascii_alphanumeric()||b"!#$%&'*+.^_`|~-".contains(&b))||!seen.insert(key.to_ascii_lowercase())||value.as_str().is_none_or(|s|s.chars().any(|c|(c as u32)<0x20||(c as u32)>0xff||c=='\x7f'))) {return false;}
        }
    }
    matches!(url.scheme(),"http"|"https")&&url.host_str().is_some()&&string_map(v.get("headers"),nullable)
}
type McpDecl=(String,PathBuf,bool);
fn mapping(root:&Path,v:&Value,source:&Path,format:Format,decls:&mut Vec<McpDecl>) -> Result<(),()> {
    let raw=if format==Format::Codex {v}else{v.get("mcpServers").filter(|v|v.is_object()).unwrap_or(v)};
    let raw=raw.as_object().filter(|m|m.len()<=MAX_COMPONENTS).ok_or(())?;
    for (key,v) in raw {if metadata_name(key).is_none(){return Err(());}decls.push((key.clone(),source.to_path_buf(),mcp_valid(root,v,format)));}
    if decls.len()>MAX_COMPONENTS{return Err(());}Ok(())
}
fn mcp_file(root:&Path,file:&str,format:Format,decls:&mut Vec<McpDecl>,budget:&mut ScanBudget) -> Result<(),()> {
    let Some(value)=optional_json(root,file,budget)?else{return Ok(());};
    if format==Format::Native&&(!object_keys(&value,&["$schema","mcpServers"])||value.get("$schema").and_then(Value::as_str)!=Some(MCP_SCHEMA)){return Err(());}
    let map=if format==Format::Native||format==Format::Codex {value.get("mcpServers").ok_or(())?}else{&value};
    mapping(root,map,&relative(root,file)?,format,decls)
}
fn foreign_mcp(root:&Path,manifest:&Value,manifest_path:&Path,format:Format,decls:&mut Vec<McpDecl>,result:&mut PluginScan) {
    if format!=Format::Kimi&&mcp_file(root,".mcp.json",format,decls,&mut result.budget).is_err(){result.unavailable=true;}
    let Some(v)=manifest.get("mcpServers").filter(|v|!v.is_null())else{return;};
    if v.is_object(){if mapping(root,v,manifest_path,format,decls).is_err(){result.unavailable=true;}return;}
    if format==Format::Kimi{result.unavailable=true;return;}
    let paths=if format==Format::Codex {v.as_str().map(|s|vec![s.to_string()]).ok_or(())}else{paths(Some(v))};
    let Ok(paths)=paths else{result.unavailable=true;return;};
    let mut seen=BTreeSet::from([root.join(".mcp.json")]);
    for path in paths {
        if format==Format::Claude&&!path.starts_with("./"){result.unavailable=true;continue;}
        let Ok(target)=declared(root,&path)else{result.unavailable=true;continue;};if !seen.insert(target){continue;}
        if mcp_file(root,&path,format,decls,&mut result.budget).is_err(){result.unavailable=true;}
    }
}
fn opencode_mcp(root:&Path,decls:&mut Vec<McpDecl>,components:&mut Vec<PluginComponent>,result:&mut PluginScan) -> Result<(),()> {
    for file in ["opencode.json","opencode.jsonc",".opencode/opencode.json",".opencode/opencode.jsonc"] {
        let v=match optional_document(root,file,DocumentFormat::Jsonc,&mut result.budget){Ok(Some(v))=>v,Ok(None)=>continue,Err(())=>{result.unavailable=true;continue;}};
        let Some(map)=v.get("mcp").filter(|v|!v.is_null())else{continue;};let Some(map)=map.as_object().filter(|m|m.len()<=MAX_COMPONENTS)else{result.unavailable=true;continue;};
        for (key,raw) in map {
            if metadata_name(key).is_none(){result.unavailable=true;continue;}
            let path=relative(root,file)?;
            if raw.get("enabled")==Some(&Value::Bool(false)){add(components,PluginComponentKind::McpReference,key,&path,None)?;continue;}
            let mut translated=serde_json::Map::new();
            let valid=match raw.get("type").and_then(Value::as_str) {
                Some("local")=>{
                    let args=raw.get("command").and_then(Value::as_array).filter(|a|!a.is_empty()&&a.iter().all(Value::is_string));
                    if let Some(args)=args {translated.insert("command".into(),args[0].clone());translated.insert("args".into(),Value::Array(args[1..].to_vec()));if let Some(env)=raw.get("environment"){translated.insert("env".into(),env.clone());}if let Some(cwd)=raw.get("cwd").filter(|v|v.is_string()){translated.insert("cwd".into(),cwd.clone());}true}else{false}
                },
                Some("remote")=>{
                    if raw.get("oauth").is_some_and(|v|!v.is_null()&&*v!=Value::Bool(false)){false}
                    else {if let Some(url)=raw.get("url"){translated.insert("url".into(),url.clone());}if let Some(headers)=raw.get("headers"){translated.insert("headers".into(),headers.clone());}true}
                },_=>false,
            };
            let translated=Value::Object(translated);
            // OpenCode's {env:NAME} template expansion is intentionally not
            // performed by Vibe's declarative importer.
            let has_env=serde_json::to_string(&translated).map_err(|_|())?.contains("{env:");
            decls.push((metadata_name(key).ok_or(())?,path,valid&&!has_env&&mcp_valid(root,&translated,Format::OpenCode)));
        }
    }
    Ok(())
}
fn vendor_components(root:&Path,v:&Value,format:Format,components:&mut Vec<PluginComponent>,result:&mut PluginScan) {
    for (key,kind) in [("skills",PluginComponentKind::Skill),("commands",PluginComponentKind::Command)] {
        if key=="commands"&&format==Format::Codex{continue;}
        let mut selected=match paths(v.get(key)){Ok(v)=>v,Err(())=>{result.unavailable=true;if kind==PluginComponentKind::Command||format==Format::Kimi{continue;}Vec::new()}};
        if matches!(format,Format::Codex|Format::Claude)&&root.join(key).is_dir(){selected.push(format!("./{key}/"));}
        if key=="skills"&&selected.is_empty()&&matches!(format,Format::Claude|Format::Kimi)&&(format==Format::Claude||v.get(key).is_none_or(Value::is_null))&&regular_metadata_file(&root.join("SKILL.md")).unwrap_or(false){selected.push("./SKILL.md".into());}
        for path in selected {
            let read=(||{if matches!(format,Format::Claude|Format::Kimi)&&!path.starts_with("./"){return Err(());}let p=declared(root,&path)?;if kind==PluginComponentKind::Skill {foreign_skills(root,&p,format==Format::Codex,components,&mut result.budget)}else{walk(root,&p,kind.clone(),0,components,&mut result.budget)}})();
            if read.is_err(){result.unavailable=true;}
        }
    }
    // These authored declarations are not claims that Vibe admitted their
    // bodies, event variants, interface fields or executable implementations.
    if matches!(format,Format::Claude|Format::Kimi) {
        if let Some(hooks)=v.get("hooks").filter(|v|!v.is_null()) {
            match hooks {
                Value::String(s)=>match declared(root,s){Ok(p)=>if marker(root,&p,PluginComponentKind::Hook,components).is_err(){result.unavailable=true;},Err(())=>result.unavailable=true},
                Value::Object(_)=>{let manifest=if format==Format::Claude{".claude-plugin/plugin.json"}else if root.join("kimi.plugin.json").is_file(){"kimi.plugin.json"}else{".kimi-plugin/plugin.json"};if add(components,PluginComponentKind::Hook,"Declared hooks",&root.join(manifest),None).is_err(){result.unavailable=true;}},_=>result.unavailable=true,
            }
        }
        if format==Format::Claude&&marker(root,&root.join("hooks/hooks.json"),PluginComponentKind::Hook,components).is_err(){result.unavailable=true;}
    }
    for key in ["agents","lspServers","outputStyles","apps","interface"] {
        if v.get(key).is_some_and(|v|!v.is_null())&&add(components,PluginComponentKind::PluginReference,&format!("Not imported: {key}"),root,None).is_err(){result.unavailable=true;}
    }
    if format==Format::Codex&&v.get("hooks").is_some_and(|v|!v.is_null()) {if add(components,PluginComponentKind::PluginReference,"Not imported: hooks",root,None).is_err(){result.unavailable=true;}}
}
fn native_extensions(root:&Path,components:&mut Vec<PluginComponent>,result:&mut PluginScan) {
    let hooks=root.join(EXT).join("hooks.toml");
    if marker(root,&hooks,PluginComponentKind::Hook,components).is_err(){result.unavailable=true;}
    for (folder,kind,entrypoint) in [("knowledge",PluginComponentKind::Context,Some("KNOWLEDGE.md")),("agents",PluginComponentKind::Agent,None)] {
        let read=(||{let base=relative(root,&format!("{EXT}/{folder}"))?;for entry in directory_entries(&base,&mut result.budget)? {
            if let Some(file)=entrypoint {if entry.directory {marker(root,&entry.path.join(file),kind.clone(),components)?;}else if !entry.file{return Err(());}}
            else if entry.file&&entry.path.extension().is_some_and(|s|s=="toml"){marker(root,&entry.path,kind.clone(),components)?;}else if !entry.file&&!entry.directory{return Err(());}
        }Ok::<(),()>(())})();if read.is_err(){result.unavailable=true;}
    }
    let libraries=(||{let Some(v)=optional_json(root,"libraries.json",&mut result.budget)?else{return Ok(());};if !object_keys(&v,&["schemaVersion","node","python"])||v.get("schemaVersion").and_then(Value::as_u64)!=Some(1){return Err(());}
        for language in ["node","python"] {if let Some(map)=v.get(language){let map=map.as_object().filter(|m|m.len()<=128).ok_or(())?;for (name,path) in map {let p=declared(root,path.as_str().ok_or(())?)?;checked_root(&p)?;if !p.is_dir(){return Err(());}add(components,PluginComponentKind::Code,&format!("{language}: {name}"),&p,None)?;}}}Ok::<(),()>(())})();if libraries.is_err(){result.unavailable=true;}
    let connectors=(||{let Some(v)=optional_json(root,"connectors.json",&mut result.budget)?else{return Ok(());};if !object_keys(&v,&["schemaVersion","connectors"])||v.get("schemaVersion").and_then(Value::as_u64)!=Some(1){return Err(());}let entries=v.get("connectors").and_then(Value::as_array).filter(|a|a.len()<=128).ok_or(())?;let mut seen=BTreeSet::new();
        for item in entries {if !object_keys(item,&["id","tools"])||!string_list(item.get("tools"),false){return Err(());}let id=item.get("id").and_then(Value::as_str).ok_or(())?;if !seen.insert(id){return Err(());}add(components,PluginComponentKind::Extension,&format!("Connector: {id}"),&root.join("connectors.json"),None)?;}Ok::<(),()>(())})();if connectors.is_err(){result.unavailable=true;}
}

pub(super) fn read_package(host:&Host,root:&Path,presence:PluginPresence,result:&mut PluginScan) -> Result<Option<Candidate>,()> {
    checked_root(root)?;result.budget.visit()?;
    let native=match optional_json(root,"plugin.json",&mut result.budget){Ok(v)=>v,Err(())=>{result.unavailable=true;None}};
    let schema=native.as_ref().and_then(|v|v.get("$schema")).and_then(Value::as_str);
    let (format,manifest_path,value,code)=if schema==Some(SCHEMA) {(Format::Native,root.join("plugin.json"),native.clone().ok_or(())?,Vec::new())}
    else if schema.is_some_and(|s|s.starts_with("https://agent-plugins.org/schemas/")){return Err(());}
    else {
        let mut matches=Vec::new();for (fmt,files) in [(Format::Codex,vec![".codex-plugin/plugin.json"]),(Format::Claude,vec![".claude-plugin/plugin.json"]),(Format::Kimi,vec!["kimi.plugin.json",".kimi-plugin/plugin.json"])] {
            for file in files {if regular_metadata_file(&relative(root,file)?)?{matches.push((fmt,file));break;}}
        }
        let markers=opencode_markers(root,result)?;if !markers.is_empty(){matches.push((Format::OpenCode,"package.json"));}
        if matches.len()>1{return Err(());}let Some((fmt,file))=matches.first().copied()else{return Ok(None);};
        if presence==PluginPresence::Bundled{return Err(());}
        let value=if fmt==Format::OpenCode {match optional_json(root,"package.json",&mut result.budget){Ok(Some(v)) if v.is_object()=>v,Ok(None)=>Arc::new(serde_json::json!({})),_=>{result.unavailable=true;Arc::new(serde_json::json!({}))}}}else{optional_json(root,file,&mut result.budget)?.ok_or(())?};
        (fmt,root.join(file),value,markers)
    };
    if !value.is_object()||(format==Format::Native&&!native_manifest(&value))||(!matches!(format,Format::Native|Format::OpenCode)&&!optional_strings(&value,&["name","version","description"])) {return Err(());}
    let provided=value.get("name").and_then(Value::as_str).filter(|s|!s.trim().is_empty());
    if format!=Format::OpenCode&&provided.is_some_and(|s|s.len()>240||s.chars().any(char::is_control)){return Err(());}
    let raw_name=provided.map(str::to_owned).or_else(||matches!(format,Format::Codex|Format::OpenCode).then(||root.file_name().and_then(|s|s.to_str()).and_then(metadata_name)).flatten()).ok_or(())?;
    let name=if format==Format::OpenCode{portable_name(&raw_name)?}else{raw_name};
    if format==Format::Claude&&(!name.as_bytes()[0].is_ascii_lowercase()||!name_valid(&name,false)){return Err(());}
    if format==Format::Kimi&&(name.len()>64||!name.as_bytes()[0].is_ascii_alphanumeric()||!name.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||matches!(b,b'_'|b'-'))){return Err(());}
    let e=if format==Format::Native {let e=extension(&value);if value.get("extensions").and_then(|v|v.get(EXT)).is_some()&&e.is_none(){result.unavailable=true;}e}else{None};
    let namespace=e.and_then(|v|v.get("toolNamespace")).and_then(Value::as_str).map(str::to_owned).unwrap_or_else(||identifier(&name));
    if RESERVED.contains(&namespace.as_str())&&!(presence==PluginPresence::Bundled&&namespace=="vibe"){return Err(());}
    let version=value.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||"unknown".into());
    if format==Format::Claude&&version!="unknown"&&semver::Version::parse(&version).is_err(){return Err(());}
    let display=e.and_then(|v|v.get("displayName")).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||name.clone());
    let mut components=Vec::new();let mut decls=Vec::new();
    match format {
        Format::Native=>{
            if immediate_skills(root,&root.join("skills"),&mut components,&mut result.budget).is_err(){result.unavailable=true;}
            if mcp_file(root,"mcp.json",format,&mut decls,&mut result.budget).is_err(){result.unavailable=true;}
            if e.is_some(){native_extensions(root,&mut components,result);}
        },
        Format::OpenCode=>{
            for dir in ["skills",".opencode/skills",".agents/skills",".claude/skills"] {if immediate_skills(root,&root.join(dir),&mut components,&mut result.budget).is_err(){result.unavailable=true;}}
            if opencode_mcp(root,&mut decls,&mut components,result).is_err(){result.unavailable=true;}
            if components.iter().all(|c|c.kind!=PluginComponentKind::Skill)&&!decls.iter().any(|(_,_,valid)|*valid){result.unavailable=true;return Ok(None);}
            for path in code {if path.extension().and_then(|s|s.to_str()).is_some_and(|s|matches!(s.to_ascii_lowercase().as_str(),"js"|"ts"|"mjs"|"cjs"|"mts"|"cts")){let label=path.strip_prefix(root).map_err(|_|())?.to_str().ok_or(())?;add(&mut components,PluginComponentKind::PluginReference,&format!("Not imported: {label}"),&path,None)?;}}
        },
        _=>{vendor_components(root,&value,format,&mut components,result);foreign_mcp(root,&value,&manifest_path,format,&mut decls,result);},
    }
    let mut counts=BTreeMap::new();for (id,_,_) in &decls{*counts.entry(id.clone()).or_insert(0usize)+=1;}
    for (id,path,valid) in decls {
        let valid=valid&&(format!=Format::Codex||counts.get(&id)==Some(&1));
        if valid {add(&mut components,PluginComponentKind::Mcp,&id,&path,Some(alias(&name,&id)))?;}
        else {result.unavailable=true;add(&mut components,PluginComponentKind::McpReference,&id,&path,None)?;}
    }
    components.sort();components.dedup();
    let manifest_path=if format==Format::OpenCode&&!regular_metadata_file(&manifest_path)? {root.to_path_buf()}else{manifest_path};
    let observation=NativePluginObservation {agent_id:host.agent_id.clone(),plugin_id:if presence==PluginPresence::Bundled{format!("@builtin/{name}")}else{name},name:display,version,revision:collapse_home(root),presence,enabled:None,source_paths:vec![collapse_home(&manifest_path)],components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()};
    Ok(Some(Candidate {observation,namespace}))
}

pub(super) fn resolve_candidates(mut candidates:Vec<Candidate>,result:&mut PluginScan) {
    let mut names=BTreeMap::new();for c in &candidates{if c.observation.presence==PluginPresence::Local{*names.entry(c.observation.plugin_id.clone()).or_insert(0usize)+=1;}}
    let mut namespaces=BTreeMap::new();for c in &candidates{if c.observation.presence==PluginPresence::Local&&names.get(&c.observation.plugin_id)==Some(&1){*namespaces.entry(c.namespace.clone()).or_insert(0usize)+=1;}}
    for c in &mut candidates {
        if c.observation.presence==PluginPresence::Local&&(names.get(&c.observation.plugin_id)!=Some(&1)||namespaces.get(&c.namespace)!=Some(&1)) {c.observation.components.clear();result.unavailable=true;}
    }
    result.plugins.extend(candidates.into_iter().map(|c|c.observation));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    fn host() -> &'static Host { hosts().iter().find(|h|h.agent_id=="mistral-vibe").unwrap() }
    fn write(root:&Path,name:&str,value:&str) {let p=root.join(name);fs::create_dir_all(p.parent().unwrap()).unwrap();fs::write(p,value).unwrap();}
    fn native(root:&Path,name:&str) {write(root,"plugin.json",&serde_json::json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":name,"version":"1.0.0"}).to_string());}
    fn scan(root:&Path) -> PluginScan {let mut r=PluginScan::default();if let Ok(Some(c))=read_package(host(),root,PluginPresence::Local,&mut r){resolve_candidates(vec![c],&mut r);}normalize(&mut r);r}

    // Catches accidental SSE support, wrong digest-size aliases, recursive
    // native Skills, private body export, and implicit vendor extension loading.
    #[test]
    fn native_metadata_keeps_component_types_and_exact_private_mcp_alias() {
        let home=TestHome::new("vibe-native");let root=home.home.join("package");native(&root,"demo");
        write(&root,"skills/first/SKILL.md","PRIVATE_BODY");write(&root,"skills/deep/nested/SKILL.md","PRIVATE_BODY");
        write(&root,"ai.mistral.vibe/hooks.toml","PRIVATE_BODY");
        write(&root,"mcp.json",r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"service":{"type":"stdio","command":"node","env":{"TOKEN":"PRIVATE_VALUE"}},"legacy":{"type":"sse","url":"https://example.com/mcp"}}}"#);
        let r=scan(&root);assert_eq!(r.plugins.len(),1);assert_eq!(r.mcps.len(),1);
        assert_eq!(r.mcps[0].server_id,"plugin_dddc3328ff8a7945_service");
        assert_eq!(r.plugins[0].components.iter().filter(|c|c.kind==PluginComponentKind::Skill).count(),1);
        assert!(!r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Hook));
        assert_eq!(r.plugins[0].enabled,None);assert!(r.unavailable);
        assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
    }

    #[test]
    fn native_format_wins_and_foreign_ambiguity_or_unsupported_schema_never_falls_back() {
        let home=TestHome::new("vibe-formats");let root=home.home.join("package");native(&root,"demo");
        write(&root,".claude-plugin/plugin.json",r#"{"name":"foreign","version":"1.0.0"}"#);
        assert_eq!(scan(&root).plugins[0].name,"demo");
        fs::remove_file(root.join("plugin.json")).unwrap();write(&root,"kimi.plugin.json",r#"{"name":"kimi"}"#);
        let mut r=PluginScan::default();assert!(read_package(host(),&root,PluginPresence::Local,&mut r).is_err());
        write(&root,"plugin.json",r#"{"$schema":"https://agent-plugins.org/schemas/2.0.0/plugin.schema.json","name":"demo"}"#);
        assert!(read_package(host(),&root,PluginPresence::Local,&mut r).is_err());
        write(&root,"plugin.json",r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"demo","unexpected":true}"#);
        assert!(read_package(host(),&root,PluginPresence::Local,&mut r).is_err());
    }

    #[test]
    fn global_name_and_namespace_collisions_do_not_manufacture_host_mcps() {
        let home=TestHome::new("vibe-collisions");let root=home.home.join("packages");let mut r=PluginScan::default();let mut candidates=Vec::new();
        for folder in ["one","two"] {let p=root.join(folder);native(&p,"demo");write(&p,"mcp.json",r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"service":{"type":"stdio","command":"node"}}}"#);candidates.push(read_package(host(),&p,PluginPresence::Local,&mut r).unwrap().unwrap());}
        resolve_candidates(candidates,&mut r);normalize(&mut r);assert_eq!(r.plugins.len(),2);assert!(r.mcps.is_empty()&&r.unavailable);
    }

    #[test]
    fn executable_only_opencode_imports_do_not_become_mcp_or_loaded_plugins() {
        let home=TestHome::new("vibe-opencode");let root=home.home.join("package");
        write(&root,".opencode/plugins/example.ts","PRIVATE_CODE");let r=scan(&root);assert!(r.mcps.is_empty()&&r.plugins.is_empty());
        write(&root,"opencode.jsonc",r#"{"mcp":{"disabled":{"type":"local","command":["node"],"enabled":false},"oauth":{"type":"remote","url":"https://example.com/mcp","oauth":true},"usable":{"type":"local","command":["node","PRIVATE_ARG"]}}}"#);
        let r=scan(&root);assert_eq!(r.mcps.len(),1);assert_eq!(r.mcps[0].name,"usable");
        assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
    }

    #[cfg(unix)]
    #[test]
    fn component_symlinks_and_declared_parent_paths_never_leave_the_package() {
        let home=TestHome::new("vibe-confined");let root=home.home.join("package");native(&root,"demo");let outside=home.home.join("outside");fs::create_dir_all(&outside).unwrap();write(&outside,"SKILL.md","PRIVATE_BODY");
        fs::create_dir_all(root.join("skills")).unwrap();std::os::unix::fs::symlink(&outside,root.join("skills/escape")).unwrap();
        let r=scan(&root);assert!(r.plugins[0].components.is_empty());assert!(r.unavailable);
    }

    #[test]
    fn foreign_skill_roots_keep_the_native_adapters_immediate_selection_rules() {
        let home=TestHome::new("vibe-foreign-skills");let codex=home.home.join("codex");
        write(&codex,".codex-plugin/plugin.json",r#"{"name":"example","skills":"custom"}"#);
        write(&codex,"custom/first/SKILL.md","PRIVATE_BODY");write(&codex,"custom/deep/nested/SKILL.md","PRIVATE_BODY");
        let r=scan(&codex);assert_eq!(r.plugins[0].components.iter().filter(|c|c.kind==PluginComponentKind::Skill).count(),1);
        assert!(r.plugins[0].components[0].source_path.ends_with("custom/first/SKILL.md"));
        let claude=home.home.join("claude");write(&claude,".claude-plugin/plugin.json",r#"{"name":"example","skills":"./single"}"#);
        write(&claude,"single/SKILL.md","PRIVATE_BODY");write(&claude,"single/nested/SKILL.md","PRIVATE_BODY");
        let r=scan(&claude);assert_eq!(r.plugins[0].components.len(),1);assert!(r.plugins[0].components[0].source_path.ends_with("single/SKILL.md"));
    }

    #[test]
    fn opencode_metadata_errors_do_not_hide_independent_portable_capabilities() {
        let home=TestHome::new("vibe-partial-opencode");let root=home.home.join("package");
        write(&root,"package.json","malformed");write(&root,"opencode.json","malformed");
        write(&root,".opencode/opencode.jsonc",r#"{"mcp":{"usable":{"type":"local","command":["node"]}}}"#);
        write(&root,".agents/skills/example/SKILL.md","PRIVATE_BODY");
        let r=scan(&root);assert_eq!(r.mcps.len(),1);assert_eq!(r.mcps[0].name,"usable");
        assert!(r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill&&c.source_path.ends_with(".agents/skills/example/SKILL.md")));
        assert!(r.unavailable);assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
    }

    #[test]
    fn claude_root_skill_fallback_and_invalid_command_selection_remain_independent() {
        let home=TestHome::new("vibe-claude-defaults");let root=home.home.join("package");
        write(&root,".claude-plugin/plugin.json",r#"{"name":"example","skills":[],"commands":17}"#);
        write(&root,"SKILL.md","PRIVATE_BODY");write(&root,"commands/ignored.md","PRIVATE_BODY");
        let r=scan(&root);assert_eq!(r.plugins[0].components.len(),1);assert_eq!(r.plugins[0].components[0].kind,PluginComponentKind::Skill);
        assert!(r.plugins[0].components[0].source_path.ends_with("package/SKILL.md"));assert!(r.unavailable);
    }
}
