//! QwenWork 1.0.4 physical native metadata. No target execution or component body reads.
use super::*;
use std::{collections::VecDeque,io::{Read,Seek,SeekFrom},sync::Mutex};
const HASH_LIMIT:u64=384*1024*1024;
const DOC_LIMIT:usize=2*1024*1024;
const READ_LIMIT:usize=32*1024*1024;
const VISIT_LIMIT:usize=32768;
const COMPONENT_LIMIT:usize=4096;
const ENTRY_LIMIT:usize=4096;
const PIN_LIMIT:usize=16384;
const PATH_DEPTH:usize=64;
fn digest(v:&str)->bool{v.len()==64&&v.bytes().all(|b|b.is_ascii_digit()||matches!(b,b'a'..=b'f'))}
fn local(v:&str)->Result<(),()>{if v.is_empty()||v.len()>2048||v.contains(['\\',':','?','#'])||v.chars().any(char::is_control)||Path::new(v).is_absolute()||Path::new(v).components().any(|c|!matches!(c,Component::Normal(_))){Err(())}else{Ok(())}}
fn checked_path(p:&Path)->Result<(),()>{if !p.is_absolute()||p.components().count()>PATH_DEPTH||p.to_str().is_none_or(|s|s.len()>4096||s.chars().any(char::is_control))||p.components().any(|c|matches!(c,Component::ParentDir|Component::CurDir|Component::Prefix(_))){return Err(());}checked_root(p)}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Guard{path:String,bytes:u64,sha256:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Item{kind:PluginComponentKind,name:String,path:String,bytes:u64,native_id:Option<String>}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Group{id:String,name:String,version:String,source:String,role:String,components:Vec<Item>}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence{carrier_sha256:String,source_index_sha256:String,view_index_sha256:String,rea_parent:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Contract{schema_version:u32,agent_id:String,app_name:String,bundle_id:String,version:String,build:String,package_name:String,package_main:String,home:String,guards:Vec<Guard>,groups:Vec<Group>,evidence:Evidence}
impl Contract{
    fn validate(&self)->Result<(),()>{
        if self.schema_version!=1||self.agent_id!="qwenwork"||self.app_name!="QwenWork.app"||self.bundle_id!="com.qwenwork.desktop.mac"||self.version!="1.0.4"||self.build!="26090501"||self.package_name!="qwen-work"||self.package_main!="out/main/index.js"||self.home!=".qwenwork"||self.guards.len()!=27||self.groups.len()!=22{return Err(());}
        let mut names=BTreeSet::new();let mut bytes=0u64;
        for g in &self.guards{local(&g.path)?;if !names.insert(&g.path)||!g.path.starts_with("Contents/")||g.bytes==0||!digest(&g.sha256){return Err(());}bytes=bytes.checked_add(g.bytes).ok_or(())?;}
        if bytes>HASH_LIMIT||!names.contains(&"Contents/Info.plist".to_string())||!names.contains(&"Contents/MacOS/QwenWork".to_string())||!names.contains(&"Contents/Resources/app.asar".to_string()){return Err(());}
        let mut ids=BTreeSet::new();let mut count=0usize;
        for g in &self.groups{local(&g.source)?;if !g.source.starts_with("Contents/Resources/")||!ids.insert(&g.id)||metadata_name(&g.name).as_deref()!=Some(&g.name)||metadata_name(&g.version).is_none(){return Err(());}for i in &g.components{local(&i.path)?;if !i.path.starts_with("Contents/Resources/")||i.bytes==0||metadata_name(&i.name).as_deref()!=Some(&i.name)||!matches!(i.kind,PluginComponentKind::Skill|PluginComponentKind::Command|PluginComponentKind::Hook|PluginComponentKind::McpReference){return Err(());}if i.kind==PluginComponentKind::Skill{if !i.path.ends_with("/SKILL.md")||i.native_id.is_some(){return Err(());}count+=1;}}}
        if count!=206||[&self.evidence.carrier_sha256,&self.evidence.source_index_sha256,&self.evidence.view_index_sha256].iter().any(|v|!digest(v))||!self.evidence.rea_parent.starts_with("ev_"){return Err(());}Ok(())
    }
}
fn contract()->Option<&'static Contract>{static C:OnceLock<Option<Contract>>=OnceLock::new();C.get_or_init(||serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-qwenwork.json")).ok().filter(|c|c.validate().is_ok())).as_ref()}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
struct Stamp{dev:u64,ino:u64,bytes:u64,mode:u32,links:u64,mtime:i64,mtime_ns:i64,ctime:i64,ctime_ns:i64}
impl Stamp{
    fn capture(m:&fs::Metadata)->Result<Self,()>{
        #[cfg(unix)]{use std::os::unix::fs::MetadataExt;Ok(Self{dev:m.dev(),ino:m.ino(),bytes:m.len(),mode:m.mode(),links:m.nlink(),mtime:m.mtime(),mtime_ns:m.mtime_nsec(),ctime:m.ctime(),ctime_ns:m.ctime_nsec()})}
        #[cfg(not(unix))]{let _=m;Err(())}
    }
}
#[derive(Default)]
struct Budget{hashed:u64,read:usize,visits:usize,components:usize}
impl Budget{
    fn visit(&mut self)->Result<(),()>{self.visits=self.visits.checked_add(1).ok_or(())?;if self.visits>VISIT_LIMIT{Err(())}else{Ok(())}}
    fn component(&mut self)->Result<(),()>{self.visit()?;self.components=self.components.checked_add(1).ok_or(())?;if self.components>COMPONENT_LIMIT{Err(())}else{Ok(())}}
}
struct Bound{path:PathBuf,file:fs::File,stamp:Stamp,directory:bool}
impl Bound{
    fn capture(path:PathBuf,directory:bool,budget:&mut Budget)->Result<Self,()>{
        budget.visit()?;checked_path(&path)?;let m=fs::symlink_metadata(&path).map_err(|_|())?;
        if if directory{!m.is_dir()}else{!m.file_type().is_file()}{return Err(());}let stamp=Stamp::capture(&m)?;
        if !directory&&stamp.links!=1{return Err(());}let file=open_confined(&path,directory)?;
        let value=Self{path,file,stamp,directory};value.revalidate()?;Ok(value)
    }
    fn revalidate(&self)->Result<(),()>{
        checked_path(&self.path)?;let m=fs::symlink_metadata(&self.path).map_err(|_|())?;
        if if self.directory{!m.is_dir()}else{!m.file_type().is_file()}{return Err(());}
        if Stamp::capture(&m)?!=self.stamp||Stamp::capture(&self.file.metadata().map_err(|_|())?)?!=self.stamp{return Err(());}Ok(())
    }
    fn small(&mut self,budget:&mut Budget)->Result<Vec<u8>,()>{
        budget.visit()?;let bytes=usize::try_from(self.stamp.bytes).map_err(|_|())?;if self.directory||bytes>DOC_LIMIT{return Err(());}
        budget.read=budget.read.checked_add(bytes).ok_or(())?;if budget.read>READ_LIMIT{return Err(());}
        self.revalidate()?;self.file.seek(SeekFrom::Start(0)).map_err(|_|())?;let mut b=vec![0;bytes];self.file.read_exact(&mut b).map_err(|_|())?;self.revalidate()?;Ok(b)
    }
}
#[derive(Clone,PartialEq,Eq)]
struct Key{app:PathBuf,app_stamp:Stamp,path:PathBuf,stamp:Stamp,expected:String}
#[derive(Default)]
struct Cache{keys:VecDeque<Key>}
impl Cache{
    fn contains(&mut self,k:&Key)->bool{let Some(i)=self.keys.iter().position(|v|v==k)else{return false;};let v=self.keys.remove(i).expect("located identity key");self.keys.push_back(v);true}
    fn insert(&mut self,k:Key){if self.contains(&k){return;}if self.keys.len()>=32{self.keys.pop_front();}self.keys.push_back(k);}
}
fn hash(file:&mut Bound,app:&Bound,expected:&str,cache:&Mutex<Cache>,budget:&mut Budget)->Result<(),()>{
    budget.visit()?;file.revalidate()?;app.revalidate()?;
    let key=Key{app:app.path.clone(),app_stamp:app.stamp.clone(),path:file.path.clone(),stamp:file.stamp.clone(),expected:expected.into()};
    if !cache.lock().map_err(|_|())?.contains(&key){
        if file.stamp.bytes>HASH_LIMIT.saturating_sub(budget.hashed){return Err(());}file.file.seek(SeekFrom::Start(0)).map_err(|_|())?;
        let mut h=Sha256::new();let mut left=file.stamp.bytes;let mut b=[0u8;65536];
        while left>0{let n=file.file.read(&mut b[..left.min(65536)as usize]).map_err(|_|())?;if n==0{return Err(());}left-=n as u64;budget.hashed+=n as u64;h.update(&b[..n]);}
        if hex::encode(h.finalize())!=expected{return Err(());}file.revalidate()?;app.revalidate()?;cache.lock().map_err(|_|())?.insert(key);
    }file.revalidate()?;app.revalidate()
}
#[derive(Clone,PartialEq,Eq,serde::Serialize)]
struct Leaf{stamp:Stamp,directory:bool,file:bool}
fn signature(path:&Path)->Result<Option<Leaf>,()>{
    checked_path(path)?;let m=match fs::symlink_metadata(path){Ok(v)=>v,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(_)=>return Err(())};
    if !m.is_dir()&&!m.file_type().is_file(){return Err(());}Ok(Some(Leaf{stamp:Stamp::capture(&m)?,directory:m.is_dir(),file:m.file_type().is_file()}))
}
#[derive(Default)]
struct Snapshot{
    budget:Budget,bound:BTreeMap<PathBuf,Bound>,leaves:BTreeMap<PathBuf,Option<Leaf>>,documents:BTreeMap<PathBuf,Arc<Value>>,
    directories:BTreeMap<PathBuf,Vec<(PathBuf,bool,bool)>>,watches:BTreeSet<PathBuf>,incomplete:bool,
}
impl Snapshot{
    fn limit(&self)->Result<(),()>{if self.bound.len()+self.leaves.len()>=PIN_LIMIT{Err(())}else{Ok(())}}
    fn remember(&mut self,path:&Path)->Result<Option<Leaf>,()>{
        self.budget.visit()?;self.limit()?;let v=signature(path)?;
        if let Some(before)=self.leaves.get(path){if before!=&v{return Err(());}}else{self.leaves.insert(path.into(),v.clone());}Ok(v)
    }
    fn parents(&mut self,path:&Path)->Result<(),()>{
        checked_path(path)?;let mut parents=Vec::new();let mut next=path.parent();
        while let Some(p)=next{self.budget.visit()?;if parents.len()>=PATH_DEPTH{return Err(());}parents.push(p.to_path_buf());next=p.parent();}
        let home=expand_tilde("~/");
        for p in parents.into_iter().rev(){
            if p==Path::new("/")||p==home{if self.remember(&p)?.is_none_or(|v|!v.directory){return Err(());}continue;}
            if let Some(b)=self.bound.get(&p){b.revalidate()?;continue;}
            self.limit()?;match signature(&p)?{None=>{self.remember(&p)?;},Some(v)if v.directory=>{let b=Bound::capture(p.clone(),true,&mut self.budget)?;self.bound.insert(p,b);},_=>return Err(())}
        }Ok(())
    }
    fn pin(&mut self,path:&Path,directory:bool)->Result<bool,()>{
        self.budget.visit()?;self.parents(path)?;self.limit()?;
        if let Some(b)=self.bound.get(path){if b.directory!=directory{return Err(());}b.revalidate()?;return Ok(true);}
        match signature(path)?{None=>{self.remember(path)?;Ok(false)},Some(v)=>{
            if if directory{!v.directory}else{!v.file}{return Err(());}let b=Bound::capture(path.into(),directory,&mut self.budget)?;self.bound.insert(path.into(),b);Ok(true)
        }}
    }
    fn stat(&mut self,path:&Path)->Result<Option<Leaf>,()>{self.watches.insert(path.into());self.parents(path)?;self.remember(path)}
    fn json(&mut self,path:&Path)->Result<Option<Arc<Value>>,()>{
        if path.extension().and_then(|v|v.to_str())!=Some("json"){return Err(());}self.watches.insert(path.into());
        if !self.pin(path,false)?{return Ok(None);}if let Some(v)=self.documents.get(path){return Ok(Some(v.clone()));}
        let b=self.bound.get_mut(path).ok_or(())?.small(&mut self.budget)?;let v=Arc::new(serde_json::from_slice::<Value>(&b).map_err(|_|())?);
        self.documents.insert(path.into(),v.clone());Ok(Some(v))
    }
    fn entries(&mut self,path:&Path)->Result<Vec<(PathBuf,bool,bool)>,()>{
        self.watches.insert(path.into());if !self.pin(path,true)?{return Ok(Vec::new());}if let Some(v)=self.directories.get(path){return Ok(v.clone());}
        let mut rows=Vec::new();
        #[cfg(unix)]{
            use rustix::fs::{Dir,statat,AtFlags,FileType};
            let bound=self.bound.get(path).ok_or(())?;let mut iter=Dir::read_from(&bound.file).map_err(|_|())?;
            for item in &mut iter{let item=item.map_err(|_|())?;let name=item.file_name();if matches!(name.to_bytes(),b"."|b".."){continue;}
                self.budget.visit()?;if rows.len()>=ENTRY_LIMIT{return Err(());}let name=std::str::from_utf8(name.to_bytes()).map_err(|_|())?;
                let m=statat(&bound.file,std::ffi::OsStr::new(name),AtFlags::SYMLINK_NOFOLLOW).map_err(|_|())?;let kind=FileType::from_raw_mode(m.st_mode);
                rows.push((path.join(name),kind==FileType::Directory,kind==FileType::RegularFile));
            }bound.revalidate()?;
        }
        #[cfg(not(unix))]{return Err(());}
        rows.sort();self.directories.insert(path.into(),rows.clone());Ok(rows)
    }
    fn revalidate(&self)->Result<(),()>{for b in self.bound.values(){b.revalidate()?;}for(p,v)in &self.leaves{if &signature(p)?!=v{return Err(());}}Ok(())}
    fn fingerprint(&self)->Result<String,()>{self.revalidate()?;let files=self.bound.iter().map(|(p,b)|(p,&b.stamp)).collect::<Vec<_>>();Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(files,&self.leaves)).map_err(|_|())?)))}
}
fn roots(host:&Host)->Result<Vec<PathBuf>,()>{
    if host.agent_id!="qwenwork"||host.adapter!="qwenwork"||host.roots.len()!=2{return Err(());}
    let mut seen=BTreeSet::new();host.roots.iter().map(|r|{
        if r.layout!="qwenwork_app"||r.presence!=PluginPresence::Bundled||!["/Applications/QwenWork.app","~/Applications/QwenWork.app"].contains(&r.path.as_str())||!seen.insert(&r.path){return Err(());}
        let p=vscode_extensions::root_path(r);checked_path(&p)?;Ok(p)
    }).collect()
}
fn selected_app(host:&Host)->Result<Option<PathBuf>,()>{
    use crate::application::agent_launch::{info_unlocked,LaunchTarget};
    let allowed=roots(host)?;let info=info_unlocked(&host.agent_id).map_err(|_|())?;
    if info.configured_target.is_some(){return Err(());}
    match info.resolved_target{Some(LaunchTarget::App{path,args,new_instance,env})if args.is_empty()&&!new_instance&&env.is_empty()=>{let p=PathBuf::from(path);if !allowed.contains(&p){return Err(());}checked_path(&p)?;Ok(Some(p))},None=>Ok(None),_=>Err(())}
}
struct Proof{app:Bound,files:BTreeMap<String,Bound>,identity:String}
impl Proof{
    fn capture(host:&Host,c:&Contract)->Result<Option<Self>,()>{
        c.validate()?;let Some(path)=selected_app(host)?else{return Ok(None);};let mut budget=Budget::default();
        let mut p=Self{app:Bound::capture(path,true,&mut budget)?,files:BTreeMap::new(),identity:String::new()};
        for g in &c.guards{let mut f=Bound::capture(p.app.path.join(&g.path),false,&mut budget)?;if f.stamp.bytes!=g.bytes{return Err(());}hash(&mut f,&p.app,&g.sha256,cache(),&mut budget)?;p.files.insert(g.path.clone(),f);p.revalidate(host)?;}
        let b=p.files.get_mut("Contents/Info.plist").ok_or(())?.small(&mut budget)?;let info=plist::Value::from_reader(std::io::Cursor::new(b)).map_err(|_|())?;let info=info.as_dictionary().ok_or(())?;
        for(k,v)in[("CFBundleIdentifier",c.bundle_id.as_str()),("CFBundleName","QwenWork"),("CFBundleDisplayName","QwenWork"),("CFBundleExecutable","QwenWork"),("CFBundleShortVersionString",c.version.as_str()),("CFBundleVersion",c.build.as_str())]{if info.get(k).and_then(plist::Value::as_string)!=Some(v){return Err(());}}
        p.identity=hex::encode(Sha256::digest(serde_json::to_vec(&(&p.app.path,&p.app.stamp,p.files.iter().map(|(k,f)|(k,&f.stamp)).collect::<Vec<_>>(),c)).map_err(|_|())?));p.revalidate(host)?;Ok(Some(p))
    }
    fn revalidate(&self,host:&Host)->Result<(),()>{if selected_app(host)?.as_ref()!=Some(&self.app.path){return Err(());}self.app.revalidate()?;for f in self.files.values(){f.revalidate()?;}self.app.revalidate()}
}
fn cache()->&'static Mutex<Cache>{static C:OnceLock<Mutex<Cache>>=OnceLock::new();C.get_or_init(Mutex::default)}
fn row(host:&Host,proof:&Proof,id:String,name:String,version:String,presence:PluginPresence,sources:Vec<String>,components:Vec<PluginComponent>,role:&str)->NativePluginObservation{
    NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id,name,version,presence,enabled:None,source_paths:sources,components,
        revision:format!("bound-app:{};{role};physical-metadata-only;frontmatter/name/type/disabled/native-acceptance/effective-enablement/session-load=unknown",proof.identity),docs_url:host.docs_url.clone(),source_fingerprint:String::new()}
}
fn component(kind:PluginComponentKind,name:&str,path:&Path,native_id:Option<String>,items:&mut Vec<PluginComponent>,s:&mut Snapshot)->Result<(),()>{
    s.budget.component()?;if items.len()>=COMPONENT_LIMIT{return Err(());}let Some(meta)=s.stat(path)?else{return Ok(());};if !meta.file{return Err(());}
    items.push(PluginComponent{kind,name:metadata_name(name).ok_or(())?,source_path:collapse_home(path),native_id});Ok(())
}
fn bundled(host:&Host,c:&Contract,p:&Proof,s:&mut Snapshot)->Result<Vec<NativePluginObservation>,()>{
    let mut result=Vec::new();for g in &c.groups{
        let source=p.app.path.join(&g.source);if s.stat(&source)?.is_none(){return Err(());}let mut items=Vec::new();
        for i in &g.components{let path=p.app.path.join(&i.path);let m=s.stat(&path)?.ok_or(())?;if !m.file||m.stamp.bytes!=i.bytes{return Err(());}component(i.kind.clone(),&i.name,&path,i.native_id.clone(),&mut items,s)?;}
        result.push(row(host,p,g.id.clone(),g.name.clone(),g.version.clone(),PluginPresence::Bundled,vec![collapse_home(&source)],items,&g.role));
    }s.revalidate()?;Ok(result)
}
fn segment(v:&str)->bool{!v.is_empty()&&v.len()<=240&&!v.contains("..")&&!v.contains(['/','\\'])&&metadata_name(v).as_deref()==Some(v)}
fn direct_components(root:&Path,items:&mut Vec<PluginComponent>,s:&mut Snapshot)->Result<(),()>{
    // Native Skill and Command scanners are independent. A rejected directory
    // or leaf must not hide an unrelated component in the other source.
    for(folder,kind)in[("skills",PluginComponentKind::Skill),("commands",PluginComponentKind::Command)]{
        let entries=match s.entries(&root.join(folder)){Ok(v)=>v,Err(())=>{s.incomplete=true;continue;}};
        for(path,dir,file)in entries{
            if s.budget.visits>=VISIT_LIMIT||s.budget.components>=COMPONENT_LIMIT{return Err(());}
            let Some(name)=path.file_name().and_then(|v|v.to_str()).filter(|v|segment(v))else{s.incomplete=true;continue;};
            let candidate=if kind==PluginComponentKind::Skill&&dir{Some((path.join("SKILL.md"),name.to_string()))}
                else if kind==PluginComponentKind::Command&&file&&name.ends_with(".md"){Some((path.clone(),name.strip_suffix(".md").ok_or(())?.to_string()))}else{None};
            if let Some((path,name))=candidate{if component(kind.clone(),&name,&path,None,items,s).is_err(){s.incomplete=true;}}
        }
    }Ok(())
}
fn physical(host:&Host,p:&Proof,root:&Path,folder:&str,custom:bool,s:&mut Snapshot)->Result<Option<NativePluginObservation>,()>{
    let mut selected=None;for rel in [".qoder-plugin/plugin.json",".claude-plugin/plugin.json"]{let path=root.join(rel);if let Some(m)=s.stat(&path)?{if !m.file{return Err(());}selected=Some(path);break;}}
    let Some(path)=selected else{return Ok(None);};
    // Native discovery selects the first existing manifest. A malformed selected
    // document never grants a fallback read of the second manifest or MCP body.
    let value=match s.json(&path){Ok(Some(v))if v.is_object()=>Some(v),_=>{s.incomplete=true;None}};
    let name=value.as_ref().and_then(|v|v.get("displayName").or_else(||v.get("name"))).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||folder.into());
    let version=value.as_ref().and_then(|v|v.get("version")).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||"unknown".into());
    let mut items=Vec::new();if direct_components(root,&mut items,s).is_err(){s.incomplete=true;}
    if let Some(value)=value.as_ref(){
        if let Some(values)=value.get("connectors").and_then(Value::as_array){for v in values.iter().take(COMPONENT_LIMIT){if let Some(id)=v.get("id").and_then(Value::as_str).and_then(metadata_name){let name=v.get("name").and_then(Value::as_str).and_then(metadata_name).unwrap_or(id);component(PluginComponentKind::McpReference,&name,&path,None,&mut items,s)?;}}if values.len()>COMPONENT_LIMIT{s.incomplete=true;}}
        // The file stays opaque. A Hook declaration may alias this same file;
        // such packages keep their manifest connector references independently.
        if value.get("hooks").is_none()&&["skills","commands","agents"].iter().all(|key|value.get(key).is_none()){
            component(PluginComponentKind::McpReference,"MCP declaration file (opaque)",&root.join(".mcp.json"),None,&mut items,s)?;
        }
    }
    Ok(Some(row(host,p,folder.into(),name,version,if custom{PluginPresence::Local}else{PluginPresence::Installed},vec![collapse_home(&path)],items,
        if custom{"native-custom-physical-root;custom-folder-precedes-installed-folder;persisted-switches-unread"}else{"native-installed-physical-root;persisted-switches-unread"})))
}
fn global(host:&Host,c:&Contract,p:&Proof,s:&mut Snapshot)->Result<Vec<NativePluginObservation>,()>{
    let home=expand_tilde("~/").join(&c.home);let mut rows=BTreeMap::new();
    for(dir,custom)in[("plugins",false),("plugins-custom",true)]{
        let entries=match s.entries(&home.join(dir)){Ok(v)=>v,Err(())=>{s.incomplete=true;continue;}};
        for(root,is_dir,_)in entries{let Some(folder)=root.file_name().and_then(|v|v.to_str()).filter(|v|segment(v)&&!v.starts_with('.')&&!["员工管理","招聘管理","薪酬绩效"].contains(v))else{continue;};if !is_dir{continue;}
            match physical(host,p,&root,folder,custom,s){Ok(Some(v))=>{rows.insert(folder.to_string(),v);},Ok(None)=>{},Err(())=>s.incomplete=true}
        }
    }s.revalidate()?;let fingerprint=s.fingerprint()?;Ok(rows.into_values().map(|mut v|{v.revision.push_str(&format!(";metadata:{fingerprint}"));v}).collect())
}
pub(super)fn observe(host:&Host,result:&mut PluginScan){
    let Some(c)=contract()else{result.unavailable=true;return;};let p=match Proof::capture(host,c){Ok(Some(v))=>v,Ok(None)=>return,Err(())=>{result.unavailable=true;return;}};
    let mut public=Snapshot::default();let mut bundle=match bundled(host,c,&p,&mut public){Ok(v)=>v,Err(())=>{result.unavailable=true;return;}};
    let mut private=Snapshot::default();let declarations=global(host,c,&p,&mut private);
    let fingerprint=match public.fingerprint(){Ok(v)=>v,Err(())=>{result.unavailable=true;return;}};
    if p.revalidate(host).is_err()||public.revalidate().is_err(){result.unavailable=true;return;}
    for v in &mut bundle{v.revision.push_str(&format!(";bundle-metadata:{fingerprint}"));}result.plugins.extend(bundle);
    match declarations{Ok(v)if private.revalidate().is_ok()=>result.plugins.extend(v),_=>private.incomplete=true};result.unavailable|=private.incomplete;
}
pub(super)fn watch_paths(host:&Host)->Vec<PathBuf>{
    let Some(c)=contract()else{return Vec::new();};let Ok(roots)=roots(host)else{return Vec::new();};let mut paths=BTreeSet::new();
    for p in roots{paths.insert(p.clone());for g in &c.guards{paths.insert(p.join(&g.path));}}
    let home=expand_tilde("~/").join(&c.home);for d in ["plugins","plugins-custom"]{paths.insert(home.join(d));}
    if let Ok(Some(p))=Proof::capture(host,c){let mut s=Snapshot::default();let _=bundled(host,c,&p,&mut s);if s.revalidate().is_ok(){paths.extend(s.watches);}let mut s=Snapshot::default();let _=global(host,c,&p,&mut s);if s.revalidate().is_ok(){paths.extend(s.watches);}}
    paths.into_iter().filter(|p|checked_path(p).is_ok()).collect()
}
pub(super)fn runtime_version(selected:&Path)->Option<String>{
    let c=contract()?;let host=Host{agent_id:c.agent_id.clone(),adapter:"qwenwork".into(),docs_url:String::new(),manifest_paths:Vec::new(),default_mcp_files:Vec::new(),mcp_id_prefix:String::new(),resource_groups:Vec::new(),roots:["/Applications/QwenWork.app","~/Applications/QwenWork.app"].into_iter().map(|p|Root{path:p.into(),layout:"qwenwork_app".into(),presence:PluginPresence::Bundled,complete_marker:None}).collect()};
    let p=Proof::capture(&host,c).ok()??;if selected!=p.app.path&&selected!=p.app.path.join("Contents/MacOS/QwenWork"){return None;}p.revalidate(&host).ok()?;Some(c.version.clone())
}
#[cfg(test)]
mod tests;
