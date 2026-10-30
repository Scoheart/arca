//! Metadata sources are availability/declarations, never effective session state.
use super::*;
use super::metadata::Snapshot;

fn safe_text(s:&str)->bool{!s.is_empty()&&s.len()<=240&&!s.chars().any(char::is_control)&&!s.contains("://")&&!s.contains(['?','#','@'])}
fn plugin_name(s:&str)->bool{let b=s.as_bytes();!b.is_empty()&&b.len()<=128&&b[0].is_ascii_alphanumeric()&&b[b.len()-1].is_ascii_alphanumeric()&&b.iter().all(|v|v.is_ascii_lowercase()||v.is_ascii_digit()||*v==b'-')}
fn absolute_value(input:&str)->Result<PathBuf,()>{
 if input.len()>4096||input.chars().any(char::is_control)||input.contains(['\0','$']){return Err(());}
 let p=if input=="~"{expand_tilde("~/")}else if input.starts_with("~/"){expand_tilde(input)}else{PathBuf::from(input)};
 if !p.is_absolute()||p.components().count()>64||p.components().any(|c|matches!(c,Component::ParentDir|Component::Prefix(_))){return Err(());}checked_root(&p)?;Ok(p)
}
pub(super) fn global_root()->Result<PathBuf,()>{
 if let Some(value)=std::env::var_os("CORTEX_HOME"){let value=value.to_str().ok_or(())?.trim();if !value.is_empty(){return absolute_value(value);}}
 let home=match std::env::var_os("SNOWFLAKE_HOME"){Some(v)if !v.is_empty()=>absolute_value(v.to_str().ok_or(())?)?,_=>expand_tilde("~/.snowflake")};
 let root=home.join("cortex");checked_root(&root)?;Ok(root)
}
fn relative_source(root:&Path,input:&str)->Result<PathBuf,()>{
 if input.len()>4096||input.chars().any(char::is_control)||input.contains(['\\','$'])||input.contains("://"){return Err(());}
 let tail=Path::new(input);if tail.is_absolute()||tail.components().any(|c|matches!(c,Component::ParentDir|Component::Prefix(_))){return Err(());}
 let mut path=root.to_path_buf();for part in tail.components(){if let Component::Normal(v)=part{path.push(v);}}
 if path.components().count()>64{return Err(());}checked_root(&path)?;Ok(path)
}
fn component(kind:PluginComponentKind,name:&str,path:&Path,native_id:Option<String>)->PluginComponent{
 PluginComponent{kind,name:name.into(),source_path:collapse_home(path),native_id}
}
fn add_component(s:&mut Snapshot,out:&mut Vec<PluginComponent>,value:PluginComponent){if s.component(){out.push(value);}}
fn row(host:&Host,id:&str,version:String,presence:PluginPresence,paths:Vec<String>,components:Vec<PluginComponent>,revision:String)->NativePluginObservation{
 NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id.into(),name:id.into(),version,presence,enabled:None,source_paths:paths,components,revision,docs_url:host.docs_url.clone(),source_fingerprint:String::new()}
}
#[derive(Clone)]
struct RegistryEntry{active:bool}
fn registry(root:&Path,s:&mut Snapshot)->Result<BTreeMap<String,RegistryEntry>,()>{
 let Some(v)=s.registry(&root.join("plugins/registry.json"))?else{return Ok(BTreeMap::new());};
 let object=v.as_object().ok_or(())?;if object.len()>1024{return Err(());}
 let mut entries=BTreeMap::new();
 for (key,value)in object{
  let o=value.as_object().ok_or(())?;
  if !o.get("source").and_then(Value::as_str).is_some_and(|v|!v.trim().is_empty())||!o.get("installedAt").and_then(Value::as_str).is_some_and(|v|!v.trim().is_empty()){return Err(());}
  let active=o.get("active").and_then(Value::as_bool).ok_or(())?;
  if !plugin_name(key){s.incomplete=true;continue;}entries.insert(key.clone(),RegistryEntry{active});
 }
 Ok(entries)
}
fn manifest(root:&Path,s:&mut Snapshot)->Result<Option<(PathBuf,Arc<Value>)>,()>{
 for folder in [".cortex-plugin",".claude-plugin"]{
  let path=root.join(folder).join("plugin.json");
  if s.stat(&path)?.is_some(){let v=s.json(&path,DocumentFormat::Json)?.ok_or(())?;return Ok(Some((path,v)));}
 }
 Ok(None)
}
fn paths(manifest:&Value,key:&str)->Result<Vec<String>,()>{
 match manifest.get(key){None=>Ok(vec![key.into()]),Some(Value::String(v))=>Ok(vec![v.clone()]),Some(Value::Array(v))if v.iter().all(Value::is_string)=>Ok(v.iter().filter_map(Value::as_str).map(str::to_owned).collect()),_=>Err(())}
}
fn skill_root(root:&Path,s:&mut Snapshot,out:&mut Vec<PluginComponent>)->Result<(),()>{
 let Some(kind)=s.stat(root)?else{return Ok(());};
 let root=if kind.file&&root.file_name().is_some_and(|n|n=="SKILL.md"){root.parent().ok_or(())?.to_path_buf()}else if kind.directory{root.into()}else{return Ok(());};
 let own=root.join("SKILL.md");
 if s.stat(&own)?.is_some_and(|v|v.file){
  let name=root.file_name().and_then(|v|v.to_str()).filter(|v|safe_text(v)).ok_or(())?;
  add_component(s,out,component(PluginComponentKind::Skill,name,&own,None));return Ok(());
 }
 // The selected CLI inspects only direct child directories; it does not recurse.
 for (child,directory,_)in s.entries(&root)?{if !directory{continue;}
  let leaf=child.join("SKILL.md");if !s.stat(&leaf)?.is_some_and(|v|v.file){continue;}
  if let Some(name)=child.file_name().and_then(|v|v.to_str()).filter(|v|safe_text(v)){add_component(s,out,component(PluginComponentKind::Skill,name,&leaf,None));}else{s.incomplete=true;}
 }
 Ok(())
}
fn component_sources(root:&Path,m:&Value,s:&mut Snapshot,out:&mut Vec<PluginComponent>){
 for (key,kind)in [("skills",PluginComponentKind::Skill),("commands",PluginComponentKind::Command),("agents",PluginComponentKind::Agent)]{
  if m.get(key).and_then(Value::as_array).is_some_and(|v|v.len()>512){s.incomplete=true;continue;}
  let declared=match paths(m,key){Ok(v)=>v,Err(())=>{s.incomplete=true;vec![key.into()]}};
  for input in declared{
   let path=match relative_source(root,&input){Ok(p)=>p,Err(())=>{s.incomplete=true;continue;}};
   if kind==PluginComponentKind::Skill{if skill_root(&path,s,out).is_err(){s.incomplete=true;}continue;}
   match s.stat(&path){Ok(Some(v))if v.directory||v.file=>add_component(s,out,component(kind.clone(),&format!("{key} source"),&path,None)),Ok(_)=>{},Err(())=>s.incomplete=true}
  }
 }
}
fn mcp_map(value:&Value,source:&Path,name:&str,out:&mut Vec<PluginComponent>,s:&mut Snapshot){
 let Some(map)=value.as_object()else{s.incomplete=true;return;};
 if map.len()>512{s.incomplete=true;return;}
 for key in map.keys(){if safe_text(key){add_component(s,out,component(PluginComponentKind::McpReference,key,source,Some(format!("{name}-{key}"))));}else{s.incomplete=true;}}
}
fn body_references(root:&Path,m:&Value)->Result<Vec<PathBuf>,()>{
 let mut bodies=Vec::new();
 for key in ["skills","commands","agents"]{if m.get(key).and_then(Value::as_array).is_some_and(|v|v.len()>512){return Err(());}for input in paths(m,key).unwrap_or_else(|_|vec![key.into()]){if bodies.len()>=512{return Err(());}bodies.push(relative_source(root,&input)?);}}
 match m.get("hooks"){
  Some(Value::String(v))if !v.is_empty()=>bodies.push(relative_source(root,v)?),
  Some(Value::Array(values))=>{if values.len()>512{return Err(());}for v in values{if bodies.len()>=512{return Err(());}bodies.push(relative_source(root,v.as_str().ok_or(())?)?);}},
  Some(Value::Object(_))=>{
   let mut stack=vec![(&m["hooks"],0usize)];let mut visited=0usize;
   while let Some((value,depth))=stack.pop(){visited+=1;if visited>1024||depth>32{return Err(());}
    match value{
     Value::Object(object)=>{if object.len()>1024{return Err(());}for(key,value)in object{
      let key=key.to_ascii_lowercase();
      if matches!(key.as_str(),"command"|"args"){return Err(());}
      if matches!(key.as_str(),"file"|"path"|"filepath"|"script"|"promptfile"|"source"|"src"){
       if let Some(path)=value.as_str(){bodies.push(relative_source(root,path)?);}else if !value.is_null(){return Err(());}
      }else{if stack.len()>=1024{return Err(());}stack.push((value,depth+1));}
     }},Value::Array(values)=>{if stack.len()+values.len()>1024{return Err(());}for value in values{stack.push((value,depth+1));}},_=>{}
    }
   }
  },None|Some(Value::Null)|Some(Value::Bool(false))|Some(Value::String(_))=>bodies.push(root.join("hooks/hooks.json")),
  _=>return Err(()),
 }
 if bodies.len()>512{return Err(());}Ok(bodies)
}
fn mcp_sources(root:&Path,m:&Value,manifest:&Path,name:&str,s:&mut Snapshot,out:&mut Vec<PluginComponent>){
 match m.get("mcpServers"){
  Some(Value::Object(_))=>mcp_map(&m["mcpServers"],manifest,name,out,s),
  value=>{
   let references=match value{
    Some(Value::String(v))if !v.is_empty()=>vec![v.clone()],Some(Value::Array(v))=>{if v.len()>512{s.incomplete=true;return;}if v.iter().any(|x|!x.is_string()){s.incomplete=true;}v.iter().filter_map(Value::as_str).map(str::to_owned).collect()},
    None|Some(Value::Null)|Some(Value::Bool(false))|Some(Value::String(_))=>vec![".mcp.json".into()],_=>{s.incomplete=true;return;}
   };
   let body_roots=match body_references(root,m){Ok(v)=>v,Err(())=>{s.incomplete=true;return;}};
   let bodies=body_roots.iter().cloned().collect::<BTreeSet<_>>();
   // References sharing keys are declarations; effective last-wins and all
   // native placeholder/environment/policy effects remain unknown.
   for input in references{
    let path=match relative_source(root,&input){Ok(v)=>v,Err(())=>{s.incomplete=true;continue;}};
    if path.extension().is_none_or(|v|v!="json")||component_payload_is_opaque(root,&path,&bodies)||body_roots.iter().any(|p|path.starts_with(p)){s.incomplete=true;continue;}
    match s.json(&path,DocumentFormat::Json){Ok(Some(v))=>mcp_map(v.get("mcpServers").unwrap_or(&v),&path,name,out,s),Ok(None)=>{},Err(())=>s.incomplete=true}
   }
  }
 }
}
fn hook_sources(root:&Path,m:&Value,manifest:&Path,s:&mut Snapshot,out:&mut Vec<PluginComponent>){
 let value=m.get("hooks");
 if let Some(Value::Object(map))=value{
  if map.len()>512{s.incomplete=true;return;}for key in map.keys(){if safe_text(key){add_component(s,out,component(PluginComponentKind::Hook,key,manifest,None));}else{s.incomplete=true;}}return;
 }
 let paths=match value{Some(Value::String(v))if !v.is_empty()=>vec![v.clone()],Some(Value::Array(v))=>{if v.len()>512{s.incomplete=true;return;}if v.iter().any(|v|!v.is_string()){s.incomplete=true;}v.iter().filter_map(Value::as_str).map(str::to_owned).collect()},None|Some(Value::Null)|Some(Value::Bool(false))|Some(Value::String(_))=>vec!["hooks/hooks.json".into()],_=>{s.incomplete=true;return;}};
 for input in paths{match relative_source(root,&input).and_then(|p|s.stat(&p).map(|v|(p,v))){Ok((path,Some(v)))if v.file=>add_component(s,out,component(PluginComponentKind::Hook,"hook source (body unread)",&path,None)),Ok(_)=>{},Err(())=>s.incomplete=true}}
}
fn package(host:&Host,root:&Path,source:&str,presence:PluginPresence,registry:&BTreeMap<String,RegistryEntry>,s:&mut Snapshot)->Result<Option<NativePluginObservation>,()>{
 let Some((path,m))=manifest(root,s)?else{return Ok(None);};
 let name=m.get("name").and_then(Value::as_str).filter(|v|plugin_name(v)).ok_or(())?;
 let version=m.get("version").and_then(Value::as_str).filter(|v|safe_text(v)).unwrap_or("").to_owned();
 let directory=root.file_name().and_then(|v|v.to_str()).unwrap_or("");let record=registry.get(directory).or_else(||registry.get(name));
 let tracked=source=="installed"&&record.is_some();let presence=if tracked{PluginPresence::Installed}else{presence};
 let mut components=Vec::new();component_sources(root,&m,s,&mut components);mcp_sources(root,&m,&path,name,s,&mut components);hook_sources(root,&m,&path,s,&mut components);
 let flag=record.map(|r|r.active.to_string()).unwrap_or_else(||"unknown".into());
 let default_request=if source=="bundled-external"{"false"}else{"true"};
 Ok(Some(row(host,name,version,presence,vec![collapse_home(root),collapse_home(&path)],components,format!("observed-source:{source}; registry-tracked:{tracked}; registry-active-request:{flag}; source-default-active-request:{default_request}; component source/candidate metadata; effective runtime and body IDs unknown"))))
}
fn scan_packages(host:&Host,root:&Path,source:&str,presence:PluginPresence,registry:&BTreeMap<String,RegistryEntry>,s:&mut Snapshot,out:&mut Vec<NativePluginObservation>){
 let entries=match s.entries(root){Ok(v)=>v,Err(())=>{s.incomplete=true;return;}};
 for (path,directory,_)in entries{if !directory||path.file_name().and_then(|v|v.to_str()).is_none_or(|v|v.starts_with('.')){continue;}
  match package(host,&path,source,presence.clone(),registry,s){Ok(Some(v))=>out.push(v),Ok(None)=>{},Err(())=>s.incomplete=true}
 }
}
fn bundled_root(base:&Path,name:&str,s:&mut Snapshot)->Result<Option<PathBuf>,()>{
 let mut candidates=Vec::new();
 if let Some(parent)=base.parent().and_then(Path::parent){candidates.push(parent.join(name));}
 candidates.push(base.join(name));
 if let Some(parent)=base.parent(){candidates.push(parent.join(name));}
 candidates.push(base.join("_internal").join(name));
 if let Some(parent)=base.parent(){candidates.push(parent.join("_internal").join(name));}
 let share=expand_tilde("~/.local/share/cortex");
 candidates.extend([Path::new("/usr/local/share/cortex/_internal").join(name),Path::new("/usr/local/share/cortex").join(name),share.join("_internal").join(name),share.join(name)]);
 for candidate in candidates{if s.stat(&candidate)?.is_some_and(|v|v.directory){return Ok(Some(candidate));}}
 // Joe appends every immediate installed-version entry. When their enumeration
 // order could change the selected resource, retain ambiguity instead of
 // substituting Arca's sorted directory order for Node's runtime observation.
 let mut possible=Vec::new();
 for (version,directory,file)in s.entries(&share)?{
  if !directory{if !file{return Err(());}continue;}
  for candidate in [version.join("_internal").join(name),version.join(name)]{
   if s.stat(&candidate)?.is_some_and(|v|v.directory){possible.push(candidate);break;}
  }
 }
 match possible.len(){0=>Ok(None),1=>Ok(possible.pop()),_=>Err(())}
}
pub(super) fn collect(host:&Host,proof:&runtime::Proof,s:&mut Snapshot)->Vec<NativePluginObservation>{
 let mut rows=Vec::new();let mut records=BTreeMap::new();
 if let Ok(global)=global_root(){
  records=match registry(&global,s){Ok(v)=>v,Err(())=>{s.incomplete=true;BTreeMap::new()}};
  let settings=global.join("settings.json");
  match s.json(&settings,DocumentFormat::Json){
   Ok(Some(value))=>if let Some(values)=value.get("plugins"){
    if let Some(values)=values.as_array(){if values.len()>256{s.incomplete=true;}else{for (index,value)in values.iter().enumerate(){
     let Some(input)=value.as_str()else{s.incomplete=true;continue;};
     match absolute_value(input){Ok(path)=>match package(host,&path,"settings",PluginPresence::Local,&BTreeMap::new(),s){Ok(Some(v))=>rows.push(v),Ok(None)=>{},Err(())=>s.incomplete=true},Err(())=>{
      // Do not expose or resolve remote URLs, relative/project references,
      // options, credentials or unsupported source dialects.
      let id=format!("@settings/reference-{index}");rows.push(row(host,&id,String::new(),PluginPresence::Declared,vec![collapse_home(&settings)],Vec::new(),"native settings plugin reference; source value and resolution withheld".into()));
     }}
    }}}else{s.incomplete=true;}
   },Ok(None)=>{},Err(())=>s.incomplete=true,
  }
  scan_packages(host,&global.join("plugins"),"installed",PluginPresence::Local,&records,s,&mut rows);
  for (name,entry)in &records{if !rows.iter().any(|r|r.plugin_id==*name&&r.revision.starts_with("observed-source:installed")){
   rows.push(row(host,name,String::new(),PluginPresence::Declared,vec![collapse_home(&global.join("plugins/registry.json"))],Vec::new(),format!("native-registry reference; registry-active-request:{}; physical membership/loading=unknown",entry.active)));
  }}
 }else{s.incomplete=true;}
 for (folder,source)in [("bundled_plugins","bundled"),("bundled_external_plugins","bundled-external")]{
  match bundled_root(proof.base(),folder,s){Ok(Some(path))=>{let empty=BTreeMap::new();let registry=if folder=="bundled_plugins"{&empty}else{&records};scan_packages(host,&path,source,PluginPresence::Bundled,registry,s,&mut rows)},Ok(None)=>{},Err(())=>s.incomplete=true}
 }
 match bundled_root(proof.base(),"bundled_skills",s){
  Ok(Some(root))=>{
   let mut components=Vec::new();match s.entries(&root){Ok(entries)=>for(path,directory,_)in entries{
    if !directory||path.file_name().and_then(|v|v.to_str()).is_none_or(|v|v.starts_with('.')||v.starts_with("__")){continue;}
    if skill_root(&path,s,&mut components).is_err(){s.incomplete=true;}
   },Err(())=>s.incomplete=true}
   if !components.is_empty(){let mut r=row(host,"@builtin/skills",proof.version().into(),PluginPresence::Bundled,vec![collapse_home(&root)],components,"application-owned candidate roots; client owner version; Skill frontmatter, validity, shadowing, experimental/account gates and invocation unknown".into());r.name="CoCo bundled Skills".into();rows.push(r);}
  },Ok(None)=>{},Err(())=>s.incomplete=true,
 }
 rows
}
