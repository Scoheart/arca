//! Cline's independent Desktop and CLI artifacts, observed without execution.
//! Agent Plugins v1 data packages and native JS modules remain distinct.
use super::*;
use super::electron_package::{app_version,Guard};
use std::ffi::OsStr;

const PLUGIN_SCHEMA:&str="https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const MCP_SCHEMA:&str="https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";
const APP_MEMBERS:[&str;4]=["Contents/Info.plist","Contents/MacOS/cline-app","Contents/MacOS/cline-cli","Contents/Resources/bin/desktop-backend/index.js"];
const WRAPPER_MEMBERS:[&str;3]=["bin/cline","bin/ca-certs.cjs","package.json"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DesktopContract {agent_id:String,version:String,bundle_id:String,product_name:String,app_paths:Vec<String>,guards:Vec<Guard>}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CliContract {agent_id:String,command:String,version:String,binary_bytes:u64,binary_sha256:String,wrapper_guards:Vec<Guard>}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {desktop:DesktopContract,cli:CliContract}
fn contract() -> &'static Contract {
    static VALUE:OnceLock<Contract>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-cline.json")).expect("audited Cline family contracts"))
}
fn unchanged(a:&fs::Metadata,b:&fs::Metadata) -> bool {
    if !b.file_type().is_file()||a.len()!=b.len()||a.modified().ok()!=b.modified().ok(){return false;}
    #[cfg(unix)] {use std::os::unix::fs::MetadataExt;
        if a.dev()!=b.dev()||a.ino()!=b.ino()||a.ctime()!=b.ctime()||a.ctime_nsec()!=b.ctime_nsec(){return false;}
    }
    true
}
fn guard_paths(root:&Path,guards:&[Guard],expected:&[&str],budget:&mut ScanBudget) -> Result<(),()> {
    if guards.len()!=expected.len()||guards.iter().map(|g|g.path.as_str()).collect::<BTreeSet<_>>()!=expected.iter().copied().collect(){return Err(());}
    let paths=guards.iter().map(|g|relative(root,&g.path)).collect::<Result<Vec<_>,_>>()?;
    let before=paths.iter().map(|p|fs::symlink_metadata(p).map_err(|_|())).collect::<Result<Vec<_>,_>>()?;
    for (g,p) in guards.iter().zip(&paths){verify_artifact(p,g.bytes,&g.sha256,budget)?;}
    for (p,a) in paths.iter().zip(&before){if !unchanged(a,&fs::symlink_metadata(p).map_err(|_|())?){return Err(());}}
    Ok(())
}
fn app_path(s:&str) -> PathBuf {if s.starts_with("~/"){expand_tilde(s)}else{crate::paths::system_probe_path(s)}}
fn verify_desktop(c:&DesktopContract) -> Result<bool,()> {
    let mut budget=ScanBudget::default();
    for app in c.app_paths.iter().map(|p|app_path(p)) {
        match fs::symlink_metadata(&app){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>continue,Ok(m)if m.file_type().is_dir()=>{},_=>return Err(())}
        let info=app.join("Contents/Info.plist");let before=fs::symlink_metadata(&info).map_err(|_|())?;
        if app_version(&app,&c.bundle_id,&c.product_name,&mut budget)?!=c.version{return Err(());}
        guard_paths(&app,&c.guards,&APP_MEMBERS,&mut budget)?;
        if !unchanged(&before,&fs::symlink_metadata(&info).map_err(|_|())?){return Err(());}
        return Ok(true);
    }
    Ok(false)
}
fn absolute(value:&OsStr) -> Result<PathBuf,()> {
    let s=value.to_str().ok_or(())?;let path=PathBuf::from(s);
    if !path.is_absolute()||s.len()>4096||s.chars().any(char::is_control)
        ||path.components().any(|p|matches!(p,Component::ParentDir|Component::Prefix(_))){return Err(());}
    Ok(path)
}
fn physical(path:&Path) -> Result<PathBuf,()> {absolute(path.as_os_str())?;fs::canonicalize(path).map_err(|_|())}
fn cli_target(wrapper:&Path,override_path:Option<&OsStr>) -> Result<PathBuf,()> {
    if let Some(p)=override_path.filter(|p|!p.is_empty()){return physical(&absolute(p)?);}
    let bin=wrapper.parent().ok_or(())?;let cache=bin.join(".cline");
    if cache.try_exists().map_err(|_|())?{return physical(&cache);}
    // This audited distribution is macOS arm64. Other platform images remain
    // unsupported; an unrecognized first candidate never falls through.
    if !cfg!(all(target_os="macos",target_arch="aarch64")){return Err(());}
    let mut directory=bin;
    loop {
        let candidate=directory.join("node_modules/@cline/cli-darwin-arm64/bin/cline");
        if candidate.try_exists().map_err(|_|())?{return physical(&candidate);}
        let Some(parent)=directory.parent()else{return Err(());};directory=parent;
    }
}
fn verify_cli(c:&CliContract,selected:&Path,override_path:Option<&OsStr>) -> Result<PathBuf,()> {
    let selected_physical=physical(selected)?;let mut budget=ScanBudget::default();
    // An exact direct native image does not run the npm wrapper and therefore
    // does not inherit its CLINE_BIN_PATH selection or certificate bootstrap.
    if fs::metadata(&selected_physical).map_err(|_|())?.len()==c.binary_bytes {
        verify_artifact(&selected_physical,c.binary_bytes,&c.binary_sha256,&mut budget)?;
        if physical(selected)?!=selected_physical{return Err(());}
        return Ok(selected_physical);
    }
    if selected_physical.file_name()!=Some(OsStr::new("cline")){return Err(());}
    let bin=selected_physical.parent().filter(|p|p.file_name()==Some(OsStr::new("bin"))).ok_or(())?;
    let root=bin.parent().ok_or(())?;
    guard_paths(root,&c.wrapper_guards,&WRAPPER_MEMBERS,&mut budget)?;
    let target=cli_target(&selected_physical,override_path)?;
    let before=fs::symlink_metadata(&target).map_err(|_|())?;
    verify_artifact(&target,c.binary_bytes,&c.binary_sha256,&mut budget)?;
    guard_paths(root,&c.wrapper_guards,&WRAPPER_MEMBERS,&mut budget)?;
    if physical(selected)?!=selected_physical||cli_target(&selected_physical,override_path)?!=target
        ||!unchanged(&before,&fs::symlink_metadata(&target).map_err(|_|())?){return Err(());}
    Ok(target)
}
pub(super) fn runtime_version(selected:&Path) -> Option<String> {
    let override_path=std::env::var_os("CLINE_BIN_PATH");
    verify_cli(&contract().cli,selected,override_path.as_deref()).ok().map(|_|contract().cli.version.clone())
}

struct GlobalPaths {packages:PathBuf,modules:Vec<PathBuf>,data:PathBuf}
fn trimmed_override(value:Option<&OsStr>) -> Result<Option<PathBuf>,()> {
    let Some(s)=value else{return Ok(None);};let s=s.to_str().ok_or(())?.trim();
    if s.is_empty(){return Ok(None);}absolute(OsStr::new(s)).map(Some)
}
fn paths_from(home:&Path,cline:Option<&OsStr>,data:Option<&OsStr>) -> Result<GlobalPaths,()> {
    absolute(home.as_os_str())?;
    let base=trimmed_override(cline)?.unwrap_or_else(||home.join(".cline"));
    let data=trimmed_override(data)?.unwrap_or_else(||base.join("data")).join("agent-plugins");
    Ok(GlobalPaths{packages:home.join(".agents/plugins"),modules:vec![base.join("plugins"),home.join("Documents/Cline/Plugins")],data})
}
fn native_paths() -> Result<GlobalPaths,()> {
    let text=|key:&str|std::env::var(key).ok().map(|s|s.trim().to_owned()).filter(|s|!s.is_empty());
    let fallback=dirs::home_dir();
    let home=text("HOME").filter(|s|s!="~").or_else(||text("USERPROFILE"))
        .or_else(||text("HOMEDRIVE").zip(text("HOMEPATH")).map(|(d,p)|format!("{d}{p}")))
        .or_else(||fallback.as_ref().and_then(|p|p.to_str()).map(str::to_owned)).ok_or(())?;
    let cline=std::env::var_os("CLINE_DIR");let data=std::env::var_os("CLINE_DATA_DIR");
    paths_from(Path::new(&home),cline.as_deref(),data.as_deref())
}
fn exact_metadata(s:&str) -> bool {metadata_name(s).as_deref()==Some(s)}
fn manifest(v:&Value) -> Result<(&str,Option<&str>),()> {
    let m=v.as_object().ok_or(())?;
    if m.get("$schema").and_then(Value::as_str)!=Some(PLUGIN_SCHEMA){return Err(());}
    let name=m.get("name").and_then(Value::as_str).ok_or(())?;
    if name.is_empty()||name.len()>64||name.contains("--")||name.contains("..")
        ||!name.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)||!name.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
        ||!name.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||matches!(b,b'.'|b'-')){return Err(());}
    for k in ["version","description","homepage","repository","license"]{if m.get(k).is_some_and(|v|!v.is_string()){return Err(());}}
    if let Some(a)=m.get("author") {
        let a=a.as_object().ok_or(())?;
        if a.iter().any(|(k,v)|!matches!(k.as_str(),"name"|"email"|"url")||!v.is_string()){return Err(());}
    }
    if let Some(k)=m.get("keywords"){if k.as_array().is_none_or(|a|a.iter().any(|v|!v.is_string())){return Err(());}}
    // Unknown root fields and non-object extensions are warned/ignored natively.
    Ok((name,m.get("version").and_then(Value::as_str)))
}
fn strings(v:Option<&Value>,array:bool) -> bool {
    v.is_none_or(|v|if array{v.as_array().is_some_and(|a|a.iter().all(Value::is_string))}
        else{v.as_object().is_some_and(|m|m.values().all(Value::is_string))})
}
fn confined_future(base:&Path,input:&str,directory:bool) -> Result<PathBuf,()> {
    let path=if Path::new(input).is_absolute(){PathBuf::from(input)}else{base.join(input)};
    absolute(path.as_os_str())?;if !path.starts_with(base){return Err(());}
    checked_root(base)?;checked_root(&path)?;
    match fs::symlink_metadata(&path) {
        Ok(m) if if directory{m.file_type().is_dir()}else{m.file_type().is_file()}=>{},
        Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},_=>return Err(()),
    }
    Ok(path)
}
fn stdio(m:&serde_json::Map<String,Value>,root:&Path,data:&Path) -> Result<(),()> {
    if m.keys().any(|k|!matches!(k.as_str(),"type"|"command"|"args"|"env"|"cwd")){return Err(());}
    let command=m.get("command").and_then(Value::as_str).filter(|s|!s.is_empty()).ok_or(())?;
    if command.starts_with("./"){confined_future(root,command,false)?;}
    else if Path::new(command).is_absolute()||command.chars().any(|c|c.is_whitespace()||matches!(c,'/'|'\\'|'\0')){return Err(());}
    if !strings(m.get("args"),true)||!strings(m.get("env"),false){return Err(());}
    if m.get("env").and_then(Value::as_object).is_some_and(|m|m.contains_key("PLUGIN_ROOT")||m.contains_key("PLUGIN_DATA")){return Err(());}
    if let Some(cwd)=m.get("cwd") {
        let cwd=cwd.as_str().ok_or(())?;
        let base=if cwd.starts_with("./")||cwd=="${PLUGIN_ROOT}"||cwd.starts_with("${PLUGIN_ROOT}/"){root}
            else if cwd=="${PLUGIN_DATA}"||cwd.starts_with("${PLUGIN_DATA}/"){data}else{return Err(());};
        let value=cwd.replace("${PLUGIN_ROOT}",root.to_str().ok_or(())?).replace("${PLUGIN_DATA}",data.to_str().ok_or(())?);
        confined_future(base,&value,true)?;
    }
    Ok(())
}
fn remote(m:&serde_json::Map<String,Value>) -> Result<(),()> {
    if m.keys().any(|k|!matches!(k.as_str(),"type"|"url"|"headers")){return Err(());}
    let raw=m.get("url").and_then(Value::as_str).filter(|s|!s.is_empty()&&s.trim()==*s&&!s.chars().any(char::is_control)).ok_or(())?;
    let url=url::Url::parse(raw).map_err(|_|())?;
    if !matches!(url.scheme(),"http"|"https")||!url.username().is_empty()||url.password().is_some()||url.fragment().is_some(){return Err(());}
    let loopback=match url.host(){Some(url::Host::Domain("localhost"))=>true,Some(url::Host::Ipv4(ip))=>ip.octets()[0]==127,
        Some(url::Host::Ipv6(ip))=>ip==std::net::Ipv6Addr::LOCALHOST,_=>false};
    if url.scheme()!="https"&&!loopback{return Err(());}
    if !strings(m.get("headers"),false){return Err(());}
    let mut names=BTreeSet::new();
    for (k,v) in m.get("headers").and_then(Value::as_object).into_iter().flatten() {
        if k.is_empty()||!k.bytes().all(|b|b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_`|~".contains(&b))
            ||!names.insert(k.to_ascii_lowercase())||!v.as_str().ok_or(())?.chars().all(|c|c=='\t'||(' '..='\u{ff}').contains(&c)&&c!='\u{7f}') {return Err(());}
    }
    Ok(())
}
fn component(kind:PluginComponentKind,name:String,path:&Path,id:Option<String>) -> PluginComponent {
    PluginComponent{kind,name,source_path:collapse_home(path),native_id:id}
}
fn references(name:&str,path:&Path) -> PluginComponent {component(PluginComponentKind::PluginReference,name.into(),path,None)}
fn skills(root:&Path,result:&mut PluginScan,components:&mut Vec<PluginComponent>) -> Result<(),()> {
    for entry in directory_entries(&root.join("skills"),&mut result.budget)? {
        if !entry.directory {if !entry.file{result.unavailable=true;}continue;}
        let name=entry.path.file_name().and_then(|s|s.to_str()).filter(|s|exact_metadata(s)).ok_or(())?;
        let file=entry.path.join("SKILL.md");
        match regular_metadata_file(&file) {
            Ok(true)=>components.push(component(PluginComponentKind::Skill,name.into(),&file,None)),Ok(false)=>{},
            Err(())=>{result.unavailable=true;components.push(references("Unverified skill file reference",&file));},
        }
        if components.len()>=MAX_COMPONENTS{return Err(());}
    }
    Ok(())
}
fn mcps(root:&Path,name:&str,data_root:&Path,result:&mut PluginScan,components:&mut Vec<PluginComponent>) -> Result<(),()> {
    let path=root.join("mcp.json");let Some(doc)=optional_json(root,"mcp.json",&mut result.budget)?else{return Ok(());};
    let m=doc.as_object().ok_or(())?;
    if m.keys().any(|k|!matches!(k.as_str(),"$schema"|"mcpServers"))||m.get("$schema").and_then(Value::as_str)!=Some(MCP_SCHEMA){return Err(());}
    let servers=m.get("mcpServers").and_then(Value::as_object).ok_or(())?;
    if servers.len()>MAX_COMPONENTS{return Err(());}
    let canonical=fs::canonicalize(root).map_err(|_|())?;
    let digest=hex::encode(Sha256::digest(canonical.to_str().ok_or(())?.as_bytes()));
    let data=data_root.join(format!("{name}-{}",&digest[..12]));
    for (key,v) in servers {
        let id=format!("{name}.{key}");
        let valid=exact_metadata(key)&&exact_metadata(&id)&&v.as_object().is_some_and(|m|match m.get("type").and_then(Value::as_str) {
            Some("stdio")=>stdio(m,root,&data).is_ok(),Some("streamable-http"|"sse")=>remote(m).is_ok(),_=>false,
        });
        if !valid{result.unavailable=true;}
        components.push(component(if valid{PluginComponentKind::Mcp}else{PluginComponentKind::McpReference},
            if exact_metadata(key){key.clone()}else{"Unverified MCP key".into()},&path,valid.then_some(id)));
        if components.len()>=MAX_COMPONENTS{return Err(());}
    }
    Ok(())
}
fn packages(host:&Host,paths:&GlobalPaths,result:&mut PluginScan) -> Result<(),()> {
    let mut observed=Vec::new();
    for entry in directory_entries(&paths.packages,&mut result.budget)? {
        if !entry.directory {if !entry.file{result.unavailable=true;}continue;}
        let root=entry.path;let marker=root.join("plugin.json");
        match fs::symlink_metadata(&marker){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>continue,Err(_)=>{result.unavailable=true;continue;},_=>{}}
        let source=collapse_home(&root);let mut name=None;let mut version="unknown".to_owned();
        match optional_json(&root,"plugin.json",&mut result.budget) {
            Ok(Some(doc))=>match manifest(&doc) {
                Ok((n,v))=>{name=Some(n.to_owned());if let Some(v)=v.filter(|v|exact_metadata(v)){version=v.to_owned();}},
                Err(())=>result.unavailable=true,
            },_=>result.unavailable=true,
        }
        let mut components=vec![references(if name.is_some(){"Agent Plugins 1.0 package; runtime selection unknown"}else{"Unverified plugin.json candidate"},&marker)];
        if let Some(n)=&name {
            if skills(&root,result,&mut components).is_err(){result.unavailable=true;}
            if mcps(&root,n,&paths.data,result,&mut components).is_err(){result.unavailable=true;components.push(component(PluginComponentKind::McpReference,"Unverified mcp.json component".into(),&root.join("mcp.json"),None));}
        }
        let fallback=format!("@candidate/{}",hex::encode(Sha256::digest(source.as_bytes())));
        observed.push(NativePluginObservation{
            agent_id:host.agent_id.clone(),plugin_id:name.clone().unwrap_or(fallback),
            name:name.clone().unwrap_or_else(||root.file_name().and_then(|s|s.to_str()).and_then(metadata_name).unwrap_or_else(||"Unverified Cline package".into())),
            version,revision:source.clone(),presence:if name.is_some(){PluginPresence::Local}else{PluginPresence::Declared},enabled:None,
            source_paths:vec![source,collapse_home(&marker)],components,docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
        });
    }
    let mut names=BTreeMap::<String,usize>::new();let mut ids=BTreeMap::<String,usize>::new();
    for p in &observed {
        if p.presence==PluginPresence::Local{*names.entry(p.plugin_id.clone()).or_default()+=1;}
        for c in &p.components{if let Some(id)=&c.native_id{*ids.entry(id.clone()).or_default()+=1;}}
    }
    for p in &mut observed {
        let duplicate=names.get(&p.plugin_id).is_some_and(|n|*n>1);
        for c in &mut p.components {
            if c.kind==PluginComponentKind::Mcp&&(duplicate||c.native_id.as_ref().is_some_and(|id|ids.get(id).is_some_and(|n|*n>1))) {
                c.kind=PluginComponentKind::McpReference;c.native_id=None;result.unavailable=true;
            }
        }
    }
    result.plugins.extend(observed);Ok(())
}
fn modules(host:&Host,root:&Path,depth:usize,result:&mut PluginScan) -> Result<(),()> {
    if depth>16{return Err(());}
    checked_root(root)?;
    // Native discovery skips a directory whenever its root marker is a file,
    // irrespective of whether the data-only manifest itself is valid.
    let marker=root.join("plugin.json");
    match fs::symlink_metadata(&marker) {
        Ok(m)if m.file_type().is_file()=>return Ok(()),Ok(_)=>return Err(()),
        Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err(()),
    }
    for e in directory_entries(root,&mut result.budget)? {
        let name=e.path.file_name().and_then(|s|s.to_str()).ok_or(())?;
        if name.starts_with('.')||name=="node_modules"{continue;}
        if e.directory{if modules(host,&e.path,depth+1,result).is_err(){result.unavailable=true;}continue;}
        if !matches!(e.path.extension().and_then(|s|s.to_str()),Some("js"|"ts")){continue;}
        if !e.file||!regular_metadata_file(&e.path)?{result.unavailable=true;continue;}
        let source=collapse_home(&e.path);let id=format!("@module/{}",hex::encode(Sha256::digest(source.as_bytes())));
        result.plugins.push(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id,name:metadata_name(name).ok_or(())?,version:"unknown".into(),
            revision:source.clone(),presence:PluginPresence::Local,enabled:None,source_paths:vec![source],
            components:vec![references("JS/TS file candidate; module body and runtime contributions unobserved",&e.path)],docs_url:host.docs_url.clone(),source_fingerprint:String::new()});
    }
    Ok(())
}
fn observe_sources(host:&Host,paths:&GlobalPaths,result:&mut PluginScan) {
    if packages(host,paths,result).is_err(){result.unavailable=true;}
    for root in &paths.modules{if modules(host,root,0,result).is_err(){result.unavailable=true;}}
}
fn bound(host:&Host) -> Result<bool,()> {
    let c=contract();
    if host.agent_id==c.desktop.agent_id{return verify_desktop(&c.desktop);}
    if host.agent_id==c.cli.agent_id {
        let Some(selected)=crate::application::agent_launch::find_command(&c.cli.command)else{return Ok(false);};
        let override_path=std::env::var_os("CLINE_BIN_PATH");verify_cli(&c.cli,&selected,override_path.as_deref())?;return Ok(true);
    }
    Err(())
}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {
    match bound(host) {
        Ok(true)=>match native_paths(){Ok(paths)=>observe_sources(host,&paths,result),Err(())=>result.unavailable=true},
        Ok(false)=>{},Err(())=>result.unavailable=true,
    }
}
fn watch_module_directories(root:&Path,depth:usize,budget:&mut ScanBudget,watches:&mut BTreeSet<PathBuf>) -> Result<(),()> {
    if depth>16{return Err(());}checked_root(root)?;
    watches.insert(root.to_path_buf());watches.insert(root.join("plugin.json"));
    if root.join("plugin.json").try_exists().map_err(|_|())?{return Ok(());}
    for e in directory_entries(root,budget)? {
        let name=e.path.file_name().and_then(|s|s.to_str()).ok_or(())?;
        if e.directory&&!name.starts_with('.')&&name!="node_modules"{watch_module_directories(&e.path,depth+1,budget,watches)?;}
    }
    Ok(())
}
pub(super) fn watch_paths(host:&Host) -> Vec<PathBuf> {
    let c=contract();let mut watches=BTreeSet::new();
    if host.agent_id==c.desktop.agent_id {
        for app in c.desktop.app_paths.iter().map(|p|app_path(p)) {
            watches.insert(app.join("Contents/Info.plist"));for g in &c.desktop.guards{watches.insert(app.join(&g.path));}watches.insert(app);
        }
    }else if host.agent_id==c.cli.agent_id {
        watches.extend(crate::application::agent_launch::command_candidates(&c.cli.command));
        if let Some(selected)=crate::application::agent_launch::find_command(&c.cli.command) {
            if let Ok(p)=physical(&selected) {
                watches.insert(p.clone());if let Some(bin)=p.parent(){watches.insert(bin.join(".cline"));
                    if let Some(root)=bin.parent(){for g in &c.cli.wrapper_guards{watches.insert(root.join(&g.path));}}
                }
                let override_path=std::env::var_os("CLINE_BIN_PATH");if let Ok(target)=cli_target(&p,override_path.as_deref()){watches.insert(target);}
            }
        }
    }
    if let Ok(paths)=native_paths() {
        watches.insert(paths.packages.clone());watches.insert(paths.data.clone());watches.extend(paths.modules.clone());
        if bound(host).is_ok_and(|v|v) {
            let mut result=PluginScan::default();observe_sources(host,&paths,&mut result);
            for p in result.plugins {
                for s in p.source_paths.into_iter().chain(p.components.into_iter().map(|c|c.source_path)){watches.insert(expand_tilde(&s));}
            }
            // Parent directory changes must reveal newly added fixed components.
            let mut budget=ScanBudget::default();
            if let Ok(entries)=directory_entries(&paths.packages,&mut budget) {
                for e in entries.into_iter().filter(|e|e.directory){
                    let skills=e.path.join("skills");
                    if let Ok(entries)=directory_entries(&skills,&mut budget){for s in entries.into_iter().filter(|e|e.directory){watches.extend([s.path.join("SKILL.md"),s.path]);}}
                    watches.extend([e.path.join("plugin.json"),e.path.join("mcp.json"),skills,e.path]);
                }
            }
            for root in &paths.modules{let _=watch_module_directories(root,0,&mut budget,&mut watches);}
        }
    }
    watches.into_iter().collect()
}

#[cfg(test)]
mod tests;
