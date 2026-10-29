//! Pi's compiled extension declarations and global resource metadata.
//! No bootstrap, module, package manager, private component body or MCP runs.
use super::*;
use super::electron_package::Guard;
use std::ffi::OsStr;

const ENTRY:&str="dist/bundle/cli.js";
const KERNEL:&str="dist/bundle/chunks/chunk-TJXZSVTD.js";
const GUARD_PATHS:[&str;5]=["package.json",ENTRY,"dist/bundle/cli-runtime.js",KERNEL,"dist/bundle/chunks/chunk-WJUNYZC5.js"];
const FIELDS:[&str;4]=["extensions","skills","prompts","themes"];
const IGNORE_FILES:[&str;3]=[".gitignore",".ignore",".fdignore"];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Builtin {name:String,replaceable:bool}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {agent_id:String,command:String,version:String,package_name:String,entry_path:String,config_dir:String,guards:Vec<Guard>,builtin_extensions:Vec<Builtin>}
fn contract() -> &'static Contract {
    static VALUE:OnceLock<Contract>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-pi.json")).expect("audited Pi distribution contract"))
}
fn absolute(path:&Path) -> Result<PathBuf,()> {
    let text=path.to_str().ok_or(())?;
    if !path.is_absolute()||text.len()>4096||text.chars().any(char::is_control){return Err(());}
    let mut clean=PathBuf::from("/");
    for c in path.components(){match c{Component::Normal(p)=>clean.push(p),Component::ParentDir=>{if !clean.pop(){return Err(());}},Component::RootDir|Component::CurDir=>{},_=>return Err(())}}
    Ok(clean)
}
fn resolve_input(value:&str,base:&Path,home:&Path,trim:bool,require_absolute:bool) -> Result<PathBuf,()> {
    let value=if trim{value.trim()}else{value};if value.is_empty()||value.chars().any(char::is_control){return Err(());}
    let path=if value=="~"{home.to_path_buf()}else if let Some(tail)=value.strip_prefix("~/"){home.join(tail)}
        else if value.starts_with("file://"){
            let url=url::Url::parse(value).map_err(|_|())?;
            if !url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some(){return Err(());}
            url.to_file_path().map_err(|_|())?
        }else{PathBuf::from(value)};
    if require_absolute&&!path.is_absolute(){return Err(());}
    absolute(&if path.is_absolute(){path}else{base.join(path)})
}
fn unchanged(a:&fs::Metadata,b:&fs::Metadata) -> bool {
    if !b.file_type().is_file()||a.len()!=b.len()||a.modified().ok()!=b.modified().ok(){return false;}
    #[cfg(unix)] {use std::os::unix::fs::MetadataExt;
        if a.dev()!=b.dev()||a.ino()!=b.ino()||a.ctime()!=b.ctime()||a.ctime_nsec()!=b.ctime_nsec(){return false;}
    }
    true
}
fn verify_runtime(c:&Contract,selected:&Path,package_override:Option<&OsStr>) -> Result<PathBuf,()> {
    absolute(selected)?;let physical=fs::canonicalize(selected).map_err(|_|())?;
    let root=physical.ancestors().nth(3).ok_or(())?.to_path_buf();
    if c.entry_path!=ENTRY||root.join(ENTRY)!=physical||c.package_name!="@earendil-works/pi-coding-agent"||c.config_dir!=".pi"
        ||c.guards.len()!=GUARD_PATHS.len()||c.guards.iter().map(|g|g.path.as_str()).collect::<BTreeSet<_>>()!=GUARD_PATHS.into_iter().collect(){return Err(());}
    if let Some(value)=package_override.filter(|v|!v.is_empty()) {
        let home=native_home()?;let other=resolve_input(value.to_str().ok_or(())?,&home,&home,false,true)?;
        if fs::canonicalize(other).map_err(|_|())?!=root{return Err(());}
    }
    let paths=c.guards.iter().map(|g|relative(&root,&g.path)).collect::<Result<Vec<_>,_>>()?;
    let before=paths.iter().map(|p|fs::symlink_metadata(p).map_err(|_|())).collect::<Result<Vec<_>,_>>()?;
    let mut budget=ScanBudget::default();for (g,p) in c.guards.iter().zip(&paths){verify_artifact(p,g.bytes,&g.sha256,&mut budget)?;}
    for (p,before) in paths.iter().zip(&before){if !unchanged(before,&fs::symlink_metadata(p).map_err(|_|())?){return Err(());}}
    if fs::canonicalize(selected).map_err(|_|())?!=physical{return Err(());}
    Ok(root)
}
pub(super) fn runtime_version(selected:&Path) -> Option<String> {
    let value=std::env::var_os("PI_PACKAGE_DIR");verify_runtime(contract(),selected,value.as_deref()).ok().map(|_|contract().version.clone())
}
struct HomePaths {home:PathBuf,agent:PathBuf}
fn native_home() -> Result<PathBuf,()> {
    if let Some(home)=std::env::var_os("HOME").filter(|p|!p.is_empty()){return absolute(Path::new(&home));}
    absolute(&dirs::home_dir().ok_or(())?)
}
fn paths_from(home:&Path,value:Option<&OsStr>) -> Result<HomePaths,()> {
    let home=absolute(home)?;
    let agent=match value.filter(|p|!p.is_empty()) {
        Some(p)=>resolve_input(p.to_str().ok_or(())?,&home,&home,false,true)?,None=>home.join(".pi/agent"),
    };
    Ok(HomePaths{home,agent})
}
fn native_paths() -> Result<HomePaths,()> {let value=std::env::var_os("PI_CODING_AGENT_DIR");paths_from(&native_home()?,value.as_deref())}
fn exact(s:&str) -> bool {metadata_name(s).as_deref()==Some(s)}
fn component(kind:PluginComponentKind,name:&str,path:&Path) -> PluginComponent {
    PluginComponent{kind,name:metadata_name(name).unwrap_or_else(||"Unverified resource reference".into()),source_path:collapse_home(path),native_id:None}
}
fn reference(name:&str,path:&Path) -> PluginComponent {component(PluginComponentKind::PluginReference,name,path)}
fn add_reference(name:&str,path:&Path,result:&mut PluginScan,components:&mut Vec<PluginComponent>) {
    if components.len()<MAX_COMPONENTS{components.push(reference(name,path));}else{result.unavailable=true;}
}
fn watch_resource(path:&Path,watches:&mut BTreeSet<PathBuf>) {
    if watches.len()<MAX_SCAN_ENTRIES&&checked_root(path).is_ok(){watches.insert(path.to_path_buf());}
}
fn bundled(host:&Host,c:&Contract,root:&Path,result:&mut PluginScan) {
    for builtin in &c.builtin_extensions {
        let source=collapse_home(&root.join(KERNEL));let id=format!("builtin:{}",builtin.name);
        result.plugins.push(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id.clone(),name:builtin.name.clone(),version:c.version.clone(),revision:c.guards.iter().find(|g|g.path==KERNEL).map(|g|g.sha256.clone()).unwrap_or_default(),presence:PluginPresence::Bundled,enabled:None,
            source_paths:vec![source.clone(),collapse_home(&root.join("package.json"))],components:vec![PluginComponent{kind:PluginComponentKind::Code,
                name:if builtin.replaceable{"Bundled extension; runtime replacement unobserved"}else{"Bundled extension declaration"}.into(),source_path:format!("{source}!{id}"),native_id:None}],docs_url:host.docs_url.clone(),source_fingerprint:String::new()});
    }
}

#[derive(Clone,Copy,PartialEq,Eq)]
enum Kind {Extension,Skill,Prompt,Theme}
fn kind(field:&str) -> Kind {match field{"extensions"=>Kind::Extension,"skills"=>Kind::Skill,"prompts"=>Kind::Prompt,_=>Kind::Theme}}
fn output_kind(kind:Kind) -> PluginComponentKind {match kind{Kind::Extension=>PluginComponentKind::Code,Kind::Skill=>PluginComponentKind::Skill,Kind::Prompt=>PluginComponentKind::Command,Kind::Theme=>PluginComponentKind::Theme}}
fn filename(path:&Path) -> Result<&str,()> {path.file_name().and_then(|p|p.to_str()).filter(|s|exact(s)).ok_or(())}
fn push_file(path:&Path,kind:Kind,components:&mut Vec<PluginComponent>) -> Result<(),()> {
    let label=if kind==Kind::Skill&&path.file_name()==Some(OsStr::new("SKILL.md")){filename(path.parent().ok_or(())?)?}else{filename(path)?};
    if components.len()>=MAX_COMPONENTS{return Err(());}
    components.push(component(output_kind(kind),label,path));Ok(())
}
fn note_failure(path:&Path,result:&mut PluginScan,components:&mut Vec<PluginComponent>) {
    result.unavailable=true;if components.len()<MAX_COMPONENTS{components.push(reference("Unverified component path or metadata",path));}
}
fn pi_manifest(doc:Option<&Value>) -> Option<BTreeMap<String,Vec<String>>> {
    let pi=doc?.as_object()?.get("pi")?.as_object()?;let mut output=BTreeMap::new();
    for field in FIELDS {
        if let Some(values)=pi.get(field).and_then(Value::as_array).filter(|a|a.len()<=MAX_ENTRIES&&a.iter().all(Value::is_string)) {
            output.insert(field.into(),values.iter().filter_map(Value::as_str).map(str::to_owned).collect());
        }
    }
    Some(output)
}
fn ignore_references(root:&Path,result:&mut PluginScan,components:&mut Vec<PluginComponent>) {
    for file in IGNORE_FILES {
        let path=root.join(file);
        match fs::symlink_metadata(&path) {
            Ok(_)=>if components.len()<MAX_COMPONENTS{components.push(reference("Native ignore rules; filename visibility unobserved",&path));},
            Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>note_failure(&path,result,components),
        }
    }
}
fn exists_metadata(path:&Path) -> Result<Option<fs::Metadata>,()> {
    checked_root(path)?;
    match fs::symlink_metadata(path){Ok(m)=>Ok(Some(m)),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),Err(_)=>Err(())}
}
fn package_path(root:&Path,input:&str) -> Result<PathBuf,()> {
    if input.len()>4096||input.chars().any(char::is_control){return Err(());}checked_root(root)?;relative(root,input)
}
// The automatic extension resolver uses literal manifest paths, then index.ts
// before index.js. It does not expand manifest globs in this particular reader.
fn extension_entry(root:&Path,result:&mut PluginScan,components:&mut Vec<PluginComponent>,watches:&mut BTreeSet<PathBuf>) -> Result<bool,()> {
    watch_resource(&root.join("package.json"),watches);
    let doc=match optional_document(root,"package.json",DocumentFormat::JsonBom,&mut result.budget){Ok(v)=>v,Err(())=>{note_failure(&root.join("package.json"),result,components);None}};
    if let Some(doc)=doc {
        if let Some(entries)=pi_manifest(Some(&doc)).and_then(|m|m.get("extensions").cloned()).filter(|a|!a.is_empty()) {
            let mut found=false;
            for input in entries {
                let path=match package_path(root,&input){Ok(p)=>p,Err(())=>{note_failure(&root.join("package.json"),result,components);found=true;continue;}};
                match exists_metadata(&path) {
                    Ok(Some(_))=>{watch_resource(&path,watches);push_file(&path,Kind::Extension,components)?;found=true;},Ok(None)=>{},
                    Err(())=>{note_failure(&path,result,components);found=true;},
                }
            }
            if found{return Ok(true);}
        }
    }
    for input in ["index.ts","index.js"] {
        let path=root.join(input);match exists_metadata(&path) {
            Ok(Some(_))=>{watch_resource(&path,watches);push_file(&path,Kind::Extension,components)?;return Ok(true);},Ok(None)=>{},
            Err(())=>{note_failure(&path,result,components);return Ok(true);},
        }
    }
    Ok(false)
}
fn collect_dir(root:&Path,resource:Kind,recursive:bool,agents_skills:bool,depth:usize,result:&mut PluginScan,components:&mut Vec<PluginComponent>,watches:&mut BTreeSet<PathBuf>) -> Result<(),()> {
    if depth>16||components.len()>=MAX_COMPONENTS{return Err(());}result.budget.visit()?;checked_root(root)?;watch_resource(root,watches);
    if resource==Kind::Extension {
        if extension_entry(root,result,components,watches)?{return Ok(());}
        ignore_references(root,result,components);
        for e in directory_entries(root,&mut result.budget)? {
            let name=filename(&e.path)?;if name.starts_with('.')||name=="node_modules"{continue;}
            if e.file&&matches!(e.path.extension().and_then(|s|s.to_str()),Some("ts"|"js")){push_file(&e.path,resource,components)?;}
            else if e.directory{if extension_entry(&e.path,result,components,watches).is_err(){note_failure(&e.path,result,components);}}
            else if !e.file{note_failure(&e.path,result,components);}
        }
        return Ok(());
    }
    ignore_references(root,result,components);
    if resource==Kind::Skill {
        let marker=root.join("SKILL.md");
        match exists_metadata(&marker) {
            Ok(Some(m))if m.file_type().is_file()=>{push_file(&marker,resource,components)?;return Ok(());},
            Ok(_)=>{},Err(())=>{note_failure(&marker,result,components);return Ok(());},
        }
    }
    for e in directory_entries(root,&mut result.budget)? {
        let name=filename(&e.path)?;if name.starts_with('.')||name=="node_modules"{continue;}
        if e.directory {
            if recursive&&collect_dir(&e.path,resource,true,agents_skills,depth+1,result,components,watches).is_err(){note_failure(&e.path,result,components);}
        }else if e.file {
            let extension=e.path.extension().and_then(|s|s.to_str());
            let accepted=match resource{Kind::Skill=>extension==Some("md")&&if agents_skills{depth>0}else{depth==0},Kind::Prompt=>extension==Some("md"),Kind::Theme=>extension==Some("json"),_=>false};
            if accepted{push_file(&e.path,resource,components)?;}
        }else{note_failure(&e.path,result,components);}
    }
    Ok(())
}
fn collect_path(path:&Path,resource:Kind,result:&mut PluginScan,components:&mut Vec<PluginComponent>,watches:&mut BTreeSet<PathBuf>) -> Result<(),()> {
    result.budget.visit()?;checked_root(path)?;watch_resource(path,watches);
    match exists_metadata(path)? {
        Some(m)if m.file_type().is_file()=>push_file(path,resource,components),
        Some(m)if m.file_type().is_dir()=>collect_dir(path,resource,true,false,0,result,components,watches),
        Some(_)=>Err(()),None=>Ok(()),
    }
}
fn pattern(input:&str) -> bool {input.starts_with(['!','+','-'])||input.chars().any(|c|matches!(c,'*'|'?'|'{'|'}'|'['|']'|'('|')'))}
fn manifest_entries(root:&Path,entries:&[String],field:&str,result:&mut PluginScan,components:&mut Vec<PluginComponent>,watches:&mut BTreeSet<PathBuf>) {
    for entry in entries {
        if components.len()>=MAX_COMPONENTS{result.unavailable=true;break;}
        if pattern(entry){components.push(reference("Native manifest pattern; matching unobserved",&root.join("package.json")));continue;}
        match package_path(root,entry) {
            Ok(path)=>if collect_path(&path,kind(field),result,components,watches).is_err(){note_failure(&path,result,components);},
            Err(())=>note_failure(&root.join("package.json"),result,components),
        }
    }
}

enum Source {Npm{name:String,version:Option<String>},Git{host:String,path:String},Local(PathBuf),Opaque}
fn safe_npm_name(name:&str) -> bool {
    let tail=name.strip_prefix('@').unwrap_or(name);let parts=tail.split('/').collect::<Vec<_>>();
    let count=if name.starts_with('@'){2}else{1};
    parts.len()==count&&parts.iter().all(|p|!p.is_empty()&&*p!="."&&*p!=".."&&p.len()<=214&&p.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'_'|b'-')))
}
fn git_source(value:&str) -> Option<(String,String)> {
    let value=value.trim();let explicit=value.starts_with("git:");let text=if explicit{value[4..].trim()}else{value};
    let mut text=text.to_owned();
    if explicit&&!text.contains("://")&&!text.starts_with("git@") {
        for (prefix,host) in [("github:","github.com"),("gitlab:","gitlab.com"),("bitbucket:","bitbucket.org")] {
            if let Some(tail)=text.strip_prefix(prefix){text=format!("{host}/{tail}");break;}
        }
        text=format!("https://{text}");
    }
    let (host,path)=if explicit&&text.starts_with("git@") {
        let (host,path)=text[4..].split_once(':')?;(host.to_owned(),path.to_owned())
    }else{
        let url=url::Url::parse(&text).ok()?;
        if !matches!(url.scheme(),"http"|"https"|"ssh"|"git")||!url.username().is_empty()||url.password().is_some()||url.query().is_some(){return None;}
        (url.host_str()?.to_owned(),url.path().trim_start_matches('/').to_owned())
    };
    let path=path.split('@').next()?;let path=path.strip_suffix(".git").unwrap_or(path).to_owned();
    if host.is_empty()||!host.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'-'))||path.split('/').count()<2
        ||path.split('/').any(|p|p.is_empty()||p=="."||p==".."||!p.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'_'|b'-'))){return None;}
    Some((host,path))
}
fn parse_source(value:&str,paths:&HomePaths) -> Source {
    if value.len()>4096||value.chars().any(char::is_control){return Source::Opaque;}
    if let Some(spec)=value.strip_prefix("npm:") {
        let spec=spec.trim();let split=spec.rfind('@').filter(|n|*n>0);
        let (name,version)=split.map(|n|(&spec[..n],Some(spec[n+1..].to_owned()))).unwrap_or((spec,None));
        if safe_npm_name(name){return Source::Npm{name:name.into(),version};}return Source::Opaque;
    }
    let trimmed=value.trim();
    if trimmed.starts_with("git:")||["http:","https:","ssh:"].iter().any(|p|trimmed.starts_with(p)) {
        return git_source(value).map(|(host,path)|Source::Git{host,path}).unwrap_or(Source::Opaque);
    }
    if trimmed.starts_with("builtin:")||trimmed.starts_with("github:"){return Source::Opaque;}
    resolve_input(value,&paths.agent,&paths.home,true,false).map(Source::Local).unwrap_or(Source::Opaque)
}
fn npm_matches(requested:Option<&str>,installed:&str) -> Option<bool> {
    let Some(requested)=requested else{return Some(!installed.is_empty());};
    let version=semver::Version::parse(installed).ok()?;let normalized=requested.trim().trim_start_matches('v');
    if let Ok(exact)=semver::Version::parse(normalized) {return Some(version.major==exact.major&&version.minor==exact.minor&&version.patch==exact.patch&&version.pre==exact.pre);}
    if normalized.is_empty()||normalized=="*"||normalized.eq_ignore_ascii_case("x"){return Some(version.pre.is_empty());}
    // Rust bare partials are caret ranges; npm bare partials are wildcards.
    // Only equivalent simple forms are admitted; compound npm ranges remain
    // unresolved instead of borrowing Rust's different grammar.
    if !normalized.chars().any(|c|matches!(c,'|'|' '|','|'='|'>'|'<')) {
        let req=if normalized.starts_with(['^','~']){normalized.to_owned()}else{
            let parts=normalized.split('.').collect::<Vec<_>>();
            if parts.len()>3{return None;}
            let mut result=Vec::new();let mut wildcard=false;
            for part in parts {
                if matches!(part,"*"|"x"|"X"){wildcard=true;result.push("*");}
                else if !wildcard&&!part.is_empty()&&part.bytes().all(|b|b.is_ascii_digit()){result.push(part);}
                else{result.clear();break;}
            }
            if result.is_empty(){String::new()}else{if result.len()<3&&!wildcard{result.push("*");}result.join(".")}
        };
        if !req.is_empty(){if let Ok(req)=semver::VersionReq::parse(&req){return Some(req.matches(&version));}}
        if normalized.chars().next().is_some_and(|c|c.is_ascii_alphabetic())&&normalized.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-')){return Some(true);}
    }
    None
}
fn package_components(root:&Path,doc:Option<&Value>,filter:Option<&serde_json::Map<String,Value>>,result:&mut PluginScan,components:&mut Vec<PluginComponent>,watches:&mut BTreeSet<PathBuf>) {
    if doc.and_then(|v|v.get("pi")).and_then(Value::as_object).is_some_and(|m|FIELDS.iter().any(|f|m.get(*f).and_then(Value::as_array).is_some_and(|a|a.len()>MAX_ENTRIES))) {
        note_failure(&root.join("package.json"),result,components);return;
    }
    let manifest=pi_manifest(doc);
    for field in FIELDS {
        let filtered=filter.and_then(|m|m.get(field));
        let autoload_off=filter.and_then(|m|m.get("autoload"))==Some(&Value::Bool(false));
        if autoload_off&&filtered.and_then(Value::as_array).is_none_or(|a|a.is_empty()){continue;}
        if filtered.is_some_and(|v|v.as_array().is_none_or(|a|a.iter().any(|v|!v.is_string()))){note_failure(&root.join("package.json"),result,components);continue;}
        if filtered.is_some()||autoload_off{add_reference("Native package filter; effective visibility unobserved",root,result,components);}
        let explicit=manifest.as_ref().and_then(|m|m.get(field));
        let use_convention=if filter.is_none(){manifest.is_none()}else if filtered.is_some(){explicit.is_none_or(|a|a.is_empty())}else{explicit.is_none()};
        if use_convention {
            let path=root.join(field);if collect_path(&path,kind(field),result,components,watches).is_err(){note_failure(&path,result,components);}
        }else if let Some(entries)=explicit{manifest_entries(root,entries,field,result,components,watches);}
    }
}
fn registrations(host:&Host,paths:&HomePaths,doc:Option<&Value>,result:&mut PluginScan,watches:&mut BTreeSet<PathBuf>) {
    let Some(value)=doc.and_then(|v|v.get("packages"))else{return;};
    let Some(entries)=value.as_array().filter(|a|a.len()<=MAX_ENTRIES)else{result.unavailable=true;return;};let mut seen=BTreeSet::new();
    for (index,entry) in entries.iter().enumerate() {
        if result.budget.visit().is_err(){result.unavailable=true;break;}
        let raw=entry.as_str().or_else(||entry.as_object().and_then(|m|m.get("source")).and_then(Value::as_str));
        let source=raw.map(|s|parse_source(s,paths)).unwrap_or(Source::Opaque);
        let filter=entry.as_object();let (identity,root,requested,managed_npm)=match source {
            Source::Npm{name,version}=>(format!("npm:{name}"),Some(paths.agent.join("npm/node_modules").join(&name)),version,true),
            Source::Git{host,path}=>(format!("git:{host}/{path}"),Some(paths.agent.join("git").join(host).join(path)),None,false),
            Source::Local(path)=>(format!("local:{}",collapse_home(&path)),Some(path),None,false),
            Source::Opaque=>(format!("@unresolved/{index}"),None,None,false),
        };
        if !seen.insert(identity.clone()){continue;}
        let settings=paths.agent.join("settings.json");let mut components=vec![reference("Global package registration; session selection unknown",&settings)];
        let mut version="unknown".to_owned();let mut name=if identity.starts_with("@unresolved/"){"Unresolved Pi package registration".into()}else{identity.clone()};
        let mut presence=PluginPresence::Declared;let mut source_paths=vec![collapse_home(&settings)];
        if let Some(root)=root {
            source_paths.push(collapse_home(&root));
            // A package root is not a recursive watch target: unrelated files
            // and nested dependency trees do not belong to this observer.
            watch_resource(&root.join("package.json"),watches);
            match exists_metadata(&root) {
                Ok(Some(m))if m.file_type().is_dir()=>{
                    let metadata=match optional_document(&root,"package.json",DocumentFormat::JsonBom,&mut result.budget){Ok(v)=>v,Err(())=>{note_failure(&root.join("package.json"),result,&mut components);None}};
                    if let Some(n)=metadata.as_ref().and_then(|v|v.get("name")).and_then(Value::as_str).filter(|s|exact(s)){name=n.into();}
                    let observed=metadata.as_ref().and_then(|v|v.get("version")).and_then(Value::as_str).filter(|s|exact(s));if let Some(v)=observed{version=v.into();}
                    let accepted=!managed_npm||observed.is_some_and(|v|npm_matches(requested.as_deref(),v)==Some(true));
                    if accepted {
                        presence=if identity.starts_with("local:"){PluginPresence::Local}else{PluginPresence::Installed};
                        package_components(&root,metadata.as_deref(),filter,result,&mut components,watches);
                        // A bare local extension directory has no package fields.
                        if identity.starts_with("local:")&&components.len()==1&&pi_manifest(metadata.as_deref()).is_none(){components.push(reference("Local extension directory candidate; import unobserved",&root));}
                    }else{components.push(reference("Managed npm cache version or fallback unresolved",&root));}
                },
                Ok(Some(m))if m.file_type().is_file()&&identity.starts_with("local:")=>{
                    watch_resource(&root,watches);presence=PluginPresence::Local;if push_file(&root,Kind::Extension,&mut components).is_err(){note_failure(&root,result,&mut components);}
                },
                Ok(None)=>components.push(reference("Package not materialized at its managed location",&root)),
                _=>note_failure(&root,result,&mut components),
            }
        }else{note_failure(&settings,result,&mut components);}
        result.plugins.push(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:identity,name,version,revision:source_paths.last().cloned().unwrap_or_default(),presence,enabled:None,source_paths,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()});
    }
}
fn global_resources(host:&Host,paths:&HomePaths,doc:Option<&Value>,result:&mut PluginScan,mut components:Vec<PluginComponent>,watches:&mut BTreeSet<PathBuf>) {
    for field in FIELDS {
        let entries=doc.and_then(|v|v.get(field));
        // This readonly projection follows the current foreign reader's skill
        // directory input without invoking its settings migration or writes.
        let entries=if field=="skills"{entries.and_then(|v|if v.is_object(){v.get("customDirectories")}else{Some(v)})}else{entries};
        if let Some(value)=entries {
            if let Some(entries)=value.as_array().filter(|a|a.len()<=MAX_ENTRIES) {
                for entry in entries {
                    let Some(input)=entry.as_str()else{note_failure(&paths.agent.join("settings.json"),result,&mut components);continue;};
                    if input.starts_with("builtin:"){continue;}
                    if pattern(input){add_reference("Native resource pattern; effective selection unobserved",&paths.agent.join("settings.json"),result,&mut components);continue;}
                    match resolve_input(input,&paths.agent,&paths.home,true,false) {
                        Ok(path)=>if collect_path(&path,kind(field),result,&mut components,watches).is_err(){note_failure(&path,result,&mut components);},
                        Err(())=>note_failure(&paths.agent.join("settings.json"),result,&mut components),
                    }
                }
            }else{note_failure(&paths.agent.join("settings.json"),result,&mut components);}
        }
        let path=paths.agent.join(field);let recursive=matches!(kind(field),Kind::Extension|Kind::Skill);
        if collect_dir(&path,kind(field),recursive,false,0,result,&mut components,watches).is_err(){note_failure(&path,result,&mut components);}
    }
    let shared=paths.home.join(".agents/skills");
    if collect_dir(&shared,Kind::Skill,true,true,0,result,&mut components,watches).is_err(){note_failure(&shared,result,&mut components);}
    if components.is_empty(){return;}
    result.plugins.push(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:"@global/resources".into(),name:"Pi global resource candidates".into(),version:"unknown".into(),revision:collapse_home(&paths.agent),presence:PluginPresence::Local,enabled:None,
        source_paths:vec![collapse_home(&paths.agent),collapse_home(&shared)],components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()});
}
fn scan_sources(host:&Host,paths:&HomePaths,result:&mut PluginScan) -> BTreeSet<PathBuf> {
    let mut references=Vec::new();let mut watches=BTreeSet::new();watch_resource(&paths.agent.join("settings.json"),&mut watches);
    let doc=match optional_document(&paths.agent,"settings.json",DocumentFormat::JsonBom,&mut result.budget) {
        Ok(doc)=>doc,Err(())=>{note_failure(&paths.agent.join("settings.json"),result,&mut references);None},
    };
    if doc.as_ref().is_some_and(|v|!v.is_object()){note_failure(&paths.agent.join("settings.json"),result,&mut references);}
    let doc=doc.as_deref().filter(|v|v.is_object());
    registrations(host,paths,doc,result,&mut watches);global_resources(host,paths,doc,result,references,&mut watches);watches
}
fn append_bound(host:&Host,c:&Contract,root:&Path,paths:Result<HomePaths,()>,result:&mut PluginScan) {
    // Full inventory includes other Agents before Pi; the per-Agent desktop
    // query does not. Give this independently bounded adapter the same fresh
    // metadata allowance in both scopes, without changing prior observers.
    let mut scan=PluginScan::default();bundled(host,c,root,&mut scan);
    match paths{Ok(paths)=>{scan_sources(host,&paths,&mut scan);},Err(())=>scan.unavailable=true}
    result.plugins.extend(scan.plugins);result.unavailable|=scan.unavailable;
}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {
    let c=contract();if host.agent_id!=c.agent_id{result.unavailable=true;return;}
    let Some(selected)=crate::application::agent_launch::find_command(&c.command)else{return;};let override_root=std::env::var_os("PI_PACKAGE_DIR");
    match verify_runtime(c,&selected,override_root.as_deref()) {
        Ok(root)=>append_bound(host,c,&root,native_paths(),result),
        Err(())=>result.unavailable=true,
    }
}
pub(super) fn watch_paths(host:&Host) -> Vec<PathBuf> {
    let c=contract();let mut paths=crate::application::agent_launch::command_candidates(&c.command).into_iter().collect::<BTreeSet<_>>();let mut bound=false;
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command) {
        if let Ok(physical)=fs::canonicalize(&selected) {
            paths.insert(physical.clone());if let Some(root)=physical.ancestors().nth(3){for g in &c.guards{paths.insert(root.join(&g.path));}}
        }
        let override_root=std::env::var_os("PI_PACKAGE_DIR");bound=verify_runtime(c,&selected,override_root.as_deref()).is_ok();
    }
    if let Ok(home)=native_paths() {
        watch_resource(&home.agent.join("settings.json"),&mut paths);
        if bound {
            let mut result=PluginScan::default();paths.extend(scan_sources(host,&home,&mut result));
        }
    }
    paths.into_iter().collect()
}

#[cfg(test)]
mod tests;
