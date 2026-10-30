//! Cortex Code Desktop metadata. Its plugin service is independent of cortex CLI.
//! No target, getter/migration, module, hook or component body is executed/read.
use super::*;
use std::collections::VecDeque;
use std::io::{Seek,SeekFrom};
use std::sync::Mutex;

const PACKAGE:&str="Contents/Resources/app/package.json";
const PRODUCT:&str="Contents/Resources/app/product.json";
const WORKBENCH:&str="Contents/Resources/app/out/vs/workbench/workbench.desktop.main.js";
const HASH_LIMIT:u64=64*1024*1024;
const DOC_LIMIT:usize=2*1024*1024;
const READ_LIMIT:usize=8*1024*1024;
const VISIT_LIMIT:usize=8192;
const PIN_LIMIT:usize=2048;
const ENTRY_LIMIT:usize=1024;
const SOURCE_LIMIT:usize=512;
const COMPONENT_LIMIT:usize=4096;
const PATH_DEPTH:usize=64;
const SKILL_DEPTH:usize=16;

#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Guard{path:String,bytes:u64,sha256:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Skill{name:String,path:String,bytes:u64}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence{carrier_sha256:String,contract_sha256:String,source_index_sha256:String,resource_inventory_sha256:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Contract{
    schema_version:u32,agent_id:String,app_name:String,bundle_id:String,version:String,package_name:String,package_main:String,
    engine_version:String,product_application_name:String,product_data_folder:String,product_commit:String,
    guards:Vec<Guard>,manifest_paths:Vec<String>,default_snowflake_home:String,snowflake_home_env:String,
    registry_file:String,settings_file:String,plugins_directory:String,stage_directory:String,bundled_plugins_directory:String,
    skills:Vec<Skill>,evidence:Evidence,
}
fn digest(s:&str)->bool{s.len()==64&&s.bytes().all(|v|v.is_ascii_digit()||matches!(v,b'a'..=b'f'))}
fn namespace(s:&str)->bool{!s.is_empty()&&s.len()<=64&&s.as_bytes()[0].is_ascii_alphanumeric()&&s.bytes().all(|v|v.is_ascii_alphanumeric()||matches!(v,b'_'|b'-'))}
fn relative_name(s:&str)->Result<(),()>{
    if s.is_empty()||s.len()>2048||s.contains(['\\',':','?','#'])||s.chars().any(char::is_control)||Path::new(s).is_absolute()
        ||Path::new(s).components().any(|v|!matches!(v,Component::Normal(_))){return Err(());}Ok(())
}
fn checked_path(path:&Path)->Result<(),()>{
    if !path.is_absolute()||path.components().count()>PATH_DEPTH||path.to_str().is_none_or(|v|v.len()>4096||v.contains("://")||v.chars().any(char::is_control))
        ||path.components().any(|v|matches!(v,Component::ParentDir|Component::CurDir|Component::Prefix(_))){return Err(());}checked_root(path)
}
impl Contract{
    fn validate(&self)->Result<(),()>{
        if self.schema_version!=1||self.agent_id!="cortex-code-desktop"||self.app_name!="Cortex Code.app"||self.bundle_id!="com.snowflake.coco"
            ||self.package_name!="Cortex Code"||self.package_main!="./out/bootstrap-main.js"||self.product_application_name!="coco"
            ||self.product_data_folder!=".cortexcode"||self.version.split('.').count()!=3||self.engine_version.split('.').count()!=3
            ||self.version.split('.').chain(self.engine_version.split('.')).any(|v|v.parse::<u32>().is_err())
            ||self.product_commit.len()!=40||!self.product_commit.bytes().all(|v|v.is_ascii_hexdigit())||self.skills.len()!=96
            ||self.manifest_paths.len()!=2||self.manifest_paths[0]!=".cortex-plugin/plugin.json"||self.manifest_paths[1]!=".claude-plugin/plugin.json"
            ||self.default_snowflake_home!="~/.snowflake"||self.snowflake_home_env!="SNOWFLAKE_HOME"
            ||self.registry_file!="cortex/plugins/registry.json"||self.settings_file!="cortex/settings.json"
            ||self.plugins_directory!="cortex/plugins"||self.stage_directory!="cortex/stage_cache"
            ||self.bundled_plugins_directory!="Contents/Resources/app/resources/snowflake/plugins"{return Err(());}
        let expected=BTreeSet::from(["Contents/Info.plist","Contents/MacOS/Cortex Code",PACKAGE,PRODUCT,
            "Contents/Resources/app/out/bootstrap-main.js","Contents/Resources/app/out/main.js",
            "Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.html",
            "Contents/Resources/app/out/vs/code/electron-browser/workbench/workbench.js",WORKBENCH]);
        if self.guards.len()!=expected.len(){return Err(());}let mut seen=BTreeSet::new();let mut total=0u64;
        for g in &self.guards{if !expected.contains(g.path.as_str())||!seen.insert(&g.path)||g.bytes==0||!digest(&g.sha256){return Err(());}total=total.checked_add(g.bytes).ok_or(())?;}
        if total>HASH_LIMIT{return Err(());}let mut seen=BTreeSet::new();
        for s in &self.skills{relative_name(&s.path)?;if !s.path.starts_with("Contents/Resources/app/resources/snowflake/skills/")||!s.path.ends_with("/SKILL.md")
            ||!seen.insert(&s.path)||metadata_name(&s.name).as_deref()!=Some(s.name.as_str())||s.bytes==0||s.bytes>16*1024*1024{return Err(());}}
        if [&self.evidence.carrier_sha256,&self.evidence.contract_sha256,&self.evidence.source_index_sha256,&self.evidence.resource_inventory_sha256].iter().any(|v|!digest(v)){return Err(());}Ok(())
    }
}
fn contract()->Option<&'static Contract>{
    static C:OnceLock<Option<Contract>>=OnceLock::new();
    C.get_or_init(||serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-coco-desktop.json")).ok().filter(|c|c.validate().is_ok())).as_ref()
}
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
fn roots(host:&Host)->Result<Vec<PathBuf>,()>{
    if host.agent_id!="cortex-code-desktop"||host.adapter!="coco_desktop"||host.roots.is_empty()||host.roots.len()>2{return Err(());}
    let mut seen=BTreeSet::new();host.roots.iter().map(|r|{
        if r.layout!="coco_app"||r.presence!=PluginPresence::Bundled||!["/Applications/Cortex Code.app","~/Applications/Cortex Code.app"].contains(&r.path.as_str())||!seen.insert(&r.path){return Err(());}
        let p=vscode_extensions::root_path(r);checked_path(&p)?;Ok(p)
    }).collect()
}
fn selected_app(host:&Host)->Result<Option<PathBuf>,()>{
    use crate::application::agent_launch::{info_unlocked,LaunchTarget};
    let allowed=roots(host)?;let info=info_unlocked(&host.agent_id).map_err(|_|())?;if info.configured_target.is_some(){return Err(());}
    match info.resolved_target{Some(LaunchTarget::App{path,args,new_instance,env})if args.is_empty()&&!new_instance&&env.is_empty()=>{
        let p=PathBuf::from(path);if !allowed.contains(&p){return Err(());}checked_path(&p)?;Ok(Some(p))
    },None=>Ok(None),_=>Err(())}
}
struct Proof{app:Bound,files:BTreeMap<String,Bound>,identity:String}
impl Proof{
    fn capture(host:&Host,c:&Contract,cache:&Mutex<Cache>,budget:&mut Budget)->Result<Option<Self>,()>{
        c.validate()?;let Some(path)=selected_app(host)?else{return Ok(None);};
        let mut p=Self{app:Bound::capture(path,true,budget)?,files:BTreeMap::new(),identity:String::new()};
        for g in &c.guards{let mut f=Bound::capture(p.app.path.join(&g.path),false,budget)?;if f.stamp.bytes!=g.bytes{return Err(());}hash(&mut f,&p.app,&g.sha256,cache,budget)?;p.files.insert(g.path.clone(),f);p.revalidate(host)?;}
        let b=p.files.get_mut("Contents/Info.plist").ok_or(())?.small(budget)?;let info=plist::Value::from_reader(std::io::Cursor::new(b)).map_err(|_|())?;let info=info.as_dictionary().ok_or(())?;
        for(k,v)in [("CFBundleIdentifier",c.bundle_id.as_str()),("CFBundleName","Cortex Code"),("CFBundleDisplayName","Cortex Code"),("CFBundleExecutable","Cortex Code"),("CFBundleVersion",c.version.as_str()),("CFBundleShortVersionString",c.version.as_str())]{
            if info.get(k).and_then(plist::Value::as_string)!=Some(v){return Err(());}}
        for(path,fields)in [(PACKAGE,vec![("name",c.package_name.as_str()),("version",c.version.as_str()),("main",c.package_main.as_str())]),
            (PRODUCT,vec![("appVersion",c.version.as_str()),("version",c.engine_version.as_str()),("applicationName",c.product_application_name.as_str()),("dataFolderName",c.product_data_folder.as_str()),("commit",c.product_commit.as_str()),("darwinBundleIdentifier",c.bundle_id.as_str()),("nameShort","Cortex Code")])]{
            let b=p.files.get_mut(path).ok_or(())?.small(budget)?;let value:Value=serde_json::from_slice(&b).map_err(|_|())?;
            for(k,v)in fields{if value.get(k).and_then(Value::as_str)!=Some(v){return Err(());}}
        }
        p.identity=hex::encode(Sha256::digest(serde_json::to_vec(&(&p.app.path,&p.app.stamp,p.files.iter().map(|(k,v)|(k,&v.stamp)).collect::<Vec<_>>(),c)).map_err(|_|())?));p.revalidate(host)?;Ok(Some(p))
    }
    fn revalidate(&self,host:&Host)->Result<(),()>{if selected_app(host)?.as_ref()!=Some(&self.app.path){return Err(());}self.app.revalidate()?;for f in self.files.values(){f.revalidate()?;}self.app.revalidate()}
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
            use std::os::unix::ffi::OsStrExt;use rustix::fs::{Dir,statat,AtFlags,FileType};
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
fn component(items:&mut Vec<PluginComponent>,kind:PluginComponentKind,name:&str,path:&Path,snapshot:&mut Snapshot)->Result<(),()>{
    snapshot.budget.component()?;if name.contains("://"){return Err(());}items.push(PluginComponent{kind,name:metadata_name(name).filter(|v|v==name).ok_or(())?,source_path:collapse_home(path),native_id:None});Ok(())
}
fn row(host:&Host,proof:&Proof,id:String,name:String,version:String,presence:PluginPresence,mut sources:Vec<String>,mut components:Vec<PluginComponent>,context:&str)->NativePluginObservation{
    sources.sort();sources.dedup();components.sort();components.dedup();NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id,name,version,presence,enabled:None,
        revision:format!("public-identity:{};{context};runtime=unknown;Skill/Agent names=filename-availability;private bodies/frontmatter/exports=unread",proof.identity),
        source_paths:sources,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()}
}
fn bundled(host:&Host,c:&Contract,proof:&Proof,snapshot:&mut Snapshot)->Result<NativePluginObservation,()>{
    let mut items=Vec::new();let mut sources=Vec::new();
    if snapshot.stat(&proof.app.path.join(&c.bundled_plugins_directory))?.is_some(){snapshot.incomplete=true;}
    for skill in &c.skills{let p=proof.app.path.join(&skill.path);match snapshot.stat(&p){Ok(Some(v))if v.file=>{
        if v.stamp.bytes!=skill.bytes{snapshot.incomplete=true;}component(&mut items,PluginComponentKind::Skill,&skill.name,&p,snapshot)?;sources.push(collapse_home(&p));
    },Ok(None)=>snapshot.incomplete=true,_=>snapshot.incomplete=true}}
    Ok(row(host,proof,"@builtin/skills".into(),"Cortex Code bundled Skills".into(),c.version.clone(),PluginPresence::Bundled,sources,items,"application-owned;96 first-root candidates, not 580 nested leaves;name/validity/gates/activation=unknown"))
}
struct Scope{home:PathBuf,env:Option<std::ffi::OsString>}
impl Scope{
    fn capture(c:&Contract)->Result<Self,()>{let env=std::env::var_os(&c.snowflake_home_env);let home=match &env{Some(v)if !v.is_empty()=>PathBuf::from(v),_=>expand_tilde(&c.default_snowflake_home)};checked_path(&home)?;Ok(Self{home,env})}
    fn revalidate(&self,c:&Contract)->Result<(),()>{if std::env::var_os(&c.snowflake_home_env)!=self.env{return Err(());}checked_path(&self.home)}
}
#[derive(Clone)]
struct Record{active:bool}
struct Candidate{roles:BTreeSet<&'static str>,declarations:BTreeSet<PathBuf>}
fn source_path(input:&str)->Option<PathBuf>{
    if input.is_empty()||input.len()>4096||input.contains("://")||input.contains(['?','#'])||input.chars().any(char::is_control){return None;}
    let p=if let Some(v)=input.strip_prefix("~/"){expand_tilde("~/").join(v)}else{PathBuf::from(input)};
    if checked_path(&p).is_err(){return None;}Some(p)
}
fn add_source(map:&mut BTreeMap<PathBuf,Candidate>,path:PathBuf,role:&'static str,doc:&Path,snapshot:&mut Snapshot){
    if map.len()>=SOURCE_LIMIT&&!map.contains_key(&path){snapshot.incomplete=true;return;}
    let e=map.entry(path).or_insert_with(||Candidate{roles:BTreeSet::new(),declarations:BTreeSet::new()});e.roles.insert(role);e.declarations.insert(doc.into());
}
fn inside(root:&Path,input:&str,empty:bool)->Result<PathBuf,()>{
    if input.is_empty(){return if empty{Ok(root.into())}else{Err(())};}relative_name(input)?;let p=root.join(input);checked_path(&p)?;Ok(p)
}
fn scan_skills(root:&Path,path:&Path,depth:usize,items:&mut Vec<PluginComponent>,s:&mut Snapshot)->Result<(),()>{
    if depth>SKILL_DEPTH||!path.starts_with(root){return Err(());}let leaf=path.join("SKILL.md");
    if s.stat(&leaf)?.is_some_and(|v|v.file){let name=path.file_name().and_then(|v|v.to_str()).ok_or(())?;component(items,PluginComponentKind::Skill,name,&leaf,s)?;return Ok(());}
    for(p,dir,file)in s.entries(path)?{if dir{if scan_skills(root,&p,depth+1,items,s).is_err(){s.incomplete=true;}}else if !file{s.incomplete=true;}}Ok(())
}
fn hook_body_paths(root:&Path,hooks:Option<&Value>)->Result<BTreeSet<PathBuf>,()>{
    let mut out=BTreeSet::from([root.join("hooks/hooks.json")]);let mut stack=hooks.into_iter().map(|v|(v,0usize,true)).collect::<Vec<_>>();let mut visited=0usize;
    while let Some((value,depth,collect))=stack.pop(){visited+=1;if visited>128||depth>8{return Err(());}
        match value{Value::String(v)if collect=>{
                let p=if Path::new(v).is_absolute(){let p=PathBuf::from(v);if !p.starts_with(root)||checked_path(&p).is_err(){return Err(());}p}else{inside(root,v,false)?};out.insert(p);
            },
            Value::Array(v)=>for x in v{stack.push((x,depth+1,collect));},
            Value::Object(v)=>for(k,x)in v{stack.push((x,depth+1,matches!(k.as_str(),"file"|"path")));},_=>{}}
    }Ok(out)
}
fn mcp_names(value:&Value,path:&Path,items:&mut Vec<PluginComponent>,s:&mut Snapshot)->Result<(),()>{
    let Some(object)=value.as_object()else{return Ok(());};let map=object.get("mcpServers").and_then(Value::as_object).unwrap_or(object);
    for(name,value)in map{if name=="mcpServers"||!value.is_object()&&!value.is_array(){continue;}if component(items,PluginComponentKind::McpReference,name,path,s).is_err(){s.incomplete=true;}}
    Ok(())
}
fn mcp_file(root:&Path,path:&str,manifest:&Path,opaque:&BTreeSet<PathBuf>,items:&mut Vec<PluginComponent>,s:&mut Snapshot){
    let p=match inside(root,path,false){Ok(v)=>v,Err(())=>{s.incomplete=true;let _=component(items,PluginComponentKind::PluginReference,"Opaque MCP source",manifest,s);return;}};
    if p.extension().and_then(|v|v.to_str())!=Some("json")||component_payload_is_opaque(root,&p,opaque){s.incomplete=true;let _=component(items,PluginComponentKind::PluginReference,"Opaque MCP source",manifest,s);return;}
    match s.json(&p){Ok(Some(v))=>{if mcp_names(&v,&p,items,s).is_err(){s.incomplete=true;}},Ok(None)=>{},Err(())=>{let _=manifest;s.incomplete=true;}}
}
fn components(root:&Path,manifest:&Path,value:&Value,s:&mut Snapshot)->Vec<PluginComponent>{
    let mut items=Vec::new();let paths=match value.get("skills"){
        Some(Value::String(v))=>vec![v.clone()],Some(Value::Array(v))if v.iter().all(Value::is_string)=>v.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        Some(v)if !v.is_null()=>{s.incomplete=true;vec!["skills".into()]},_=>vec!["skills".into()],
    };
    for p in paths{match inside(root,&p,true){Ok(p)=>if scan_skills(root,&p,0,&mut items,s).is_err(){s.incomplete=true;},Err(())=>s.incomplete=true}}
    match s.entries(&root.join("agents")){Ok(v)=>for(p,dir,file)in v{if !dir&&file&&p.extension().and_then(|v|v.to_str())==Some("md"){
        match s.stat(&p){Ok(Some(_))=>{if let Some(name)=p.file_name().and_then(|v|v.to_str()){if component(&mut items,PluginComponentKind::Agent,name,&p,s).is_err(){s.incomplete=true;}}},_=>s.incomplete=true}
    }else if !dir&&!file{s.incomplete=true;}},Err(())=>s.incomplete=true}
    if let Some(h)=value.get("hooks").and_then(Value::as_object){
        let h=h.get("hooks").and_then(Value::as_object).unwrap_or(h);for name in h.keys(){if component(&mut items,PluginComponentKind::Hook,name,manifest,s).is_err(){s.incomplete=true;}}
    }else{let p=root.join("hooks/hooks.json");match s.stat(&p){Ok(Some(v))if v.file=>{if component(&mut items,PluginComponentKind::Hook,"hooks.json",&p,s).is_err(){s.incomplete=true;}},Ok(None)=>{},_=>s.incomplete=true}}
    let opaque=hook_body_paths(root,value.get("hooks"));
    match value.get("mcpServers"){
        Some(v@Value::Object(_))=>{if mcp_names(v,manifest,&mut items,s).is_err(){s.incomplete=true;}},
        Some(Value::String(v))=>{if !v.is_empty(){if let Ok(opaque)=&opaque{mcp_file(root,v,manifest,opaque,&mut items,s);}else{s.incomplete=true;}}},
        Some(Value::Array(v))=>for v in v{match v.as_str(){Some(v)if !v.is_empty()=>if let Ok(opaque)=&opaque{mcp_file(root,v,manifest,opaque,&mut items,s);}else{s.incomplete=true;},Some(_)=>{},None=>s.incomplete=true}},
        _=>if let Ok(opaque)=&opaque{mcp_file(root,".mcp.json",manifest,opaque,&mut items,s);}else{s.incomplete=true;},
    }items
}
fn physical(host:&Host,c:&Contract,proof:&Proof,root:&Path,candidate:&Candidate,records:&BTreeMap<String,Record>,registry:&Path,s:&mut Snapshot)->Option<NativePluginObservation>{
    for relative in &c.manifest_paths{
        let path=root.join(relative);let value=match s.json(&path){Ok(Some(v))=>v,Ok(None)=>continue,Err(())=>{s.incomplete=true;continue;}};
        let Some(name)=value.get("name").and_then(Value::as_str).map(str::trim).filter(|v|!v.is_empty())else{continue;};
        if !namespace(name){s.incomplete=true;return None;}
        let version=value.get("version").and_then(Value::as_str).filter(|v|!v.is_empty()&&v.len()<=96&&v.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'-'|b'_'|b'+'))).unwrap_or_default().to_owned();
        if value.get("version").is_some_and(|v|!v.is_null())&&version.is_empty(){s.incomplete=true;}
        let items=components(root,&path,&value,s);let active_request=records.get(name).map(|v|v.active);
        let mut sources=candidate.declarations.iter().map(|p|collapse_home(p)).collect::<Vec<_>>();sources.push(collapse_home(&path));if active_request.is_some(){sources.push(collapse_home(registry));}
        let source_id=hex::encode(Sha256::digest(root.to_string_lossy().as_bytes()));
        return Some(row(host,proof,name.into(),name.into(),version,PluginPresence::Local,sources,items,
            &format!("physical native manifest;physical-source:{source_id};roles:{};registry-active-request={};source winner/profile/policy/trust/effective enablement/loaded ownership=unknown",candidate.roles.iter().copied().collect::<Vec<_>>().join(","),active_request.map_or("unknown",|v|if v{"true"}else{"false"}))));
    }None
}
fn reference(host:&Host,proof:&Proof,id:String,name:&str,path:&Path,active_request:Option<bool>,s:&mut Snapshot)->Result<NativePluginObservation,()>{
    let mut items=Vec::new();component(&mut items,PluginComponentKind::PluginReference,name,path,s)?;
    Ok(row(host,proof,id,name.into(),String::new(),PluginPresence::Declared,vec![collapse_home(path)],items,&format!("registry/settings reference;registry-active-request={};native manifest/materialization/ownership/effective enablement=unknown;raw source omitted",active_request.map_or("unknown",|v|if v{"true"}else{"false"}))))
}
fn global(host:&Host,c:&Contract,proof:&Proof,scope:&Scope,s:&mut Snapshot)->Result<Vec<NativePluginObservation>,()>{
    let registry=scope.home.join(&c.registry_file);let settings=scope.home.join(&c.settings_file);let directory=scope.home.join(&c.plugins_directory);
    let mut records=BTreeMap::new();let mut candidates=BTreeMap::new();let mut references=Vec::new();
    match s.json(&registry){
        Ok(Some(v))=>if let Some(map)=v.as_object(){for(name,value)in map{
            if records.len()>=SOURCE_LIMIT{s.incomplete=true;break;}
            let Some(object)=value.as_object()else{s.incomplete=true;continue;};
            let (Some(source),Some(_installed),Some(active))=(object.get("source").and_then(Value::as_str),object.get("installedAt").and_then(Value::as_str),object.get("active").and_then(Value::as_bool))else{s.incomplete=true;continue;};
            if !namespace(name){s.incomplete=true;continue;}
            records.insert(name.clone(),Record{active});
            let location=object.get("location").and_then(Value::as_str);let allowed=location.is_none_or(|v|!matches!(v,"profile"|"settings"|"bundled"|"project"));
            if source!="bundled"&&allowed{if let Some(p)=source_path(source){add_source(&mut candidates,p,"native-registry-local-source",&registry,s);}}
        }}else{s.incomplete=true;},Ok(None)=>{},Err(())=>s.incomplete=true,
    }
    match s.json(&settings){
        Ok(Some(v))=>if let Some(values)=v.get("plugins"){if let Some(values)=values.as_array(){for(i,v)in values.iter().take(SOURCE_LIMIT+1).enumerate(){
            if i>=SOURCE_LIMIT{s.incomplete=true;break;}let Some(value)=v.as_str().filter(|v|!v.trim().is_empty())else{s.incomplete=true;continue;};
            if let Some(p)=source_path(value){add_source(&mut candidates,p,"settings-local-source",&settings,s);}else{
                let id=hex::encode(Sha256::digest(format!("{}:{i}",settings.display()).as_bytes()));
                references.push(reference(host,proof,format!("@settings/{id}"),"Plugin reference",&settings,None,s)?);
            }
        }}else{s.incomplete=true;}},Ok(None)=>{},Err(())=>s.incomplete=true,
    }
    match s.entries(&directory){Ok(entries)=>for(path,dir,file)in entries{if dir{add_source(&mut candidates,path,"native-user-physical-root",&directory,s);}else if !file{s.incomplete=true;}},Err(())=>s.incomplete=true}
    let mut result=Vec::new();let mut represented=BTreeSet::new();
    for(root,candidate)in candidates{
        if let Some(item)=physical(host,c,proof,&root,&candidate,&records,&registry,s){represented.insert(item.name.clone());result.push(item);}
        else if candidate.roles.contains("settings-local-source"){references.push(reference(host,proof,format!("@source/{}",hex::encode(Sha256::digest(root.to_string_lossy().as_bytes()))),"Plugin reference",&settings,None,s)?);}
    }
    for(name,record)in records{if !represented.contains(&name){references.push(reference(host,proof,format!("@registry/{name}"),&name,&registry,Some(record.active),s)?);}}
    result.extend(references);scope.revalidate(c)?;s.revalidate()?;let fingerprint=s.fingerprint()?;
    for item in &mut result{item.revision.push_str(&format!(";metadata:{fingerprint};default-scope!=active-session"));}Ok(result)
}
fn cache()->&'static Mutex<Cache>{static C:OnceLock<Mutex<Cache>>=OnceLock::new();C.get_or_init(Mutex::default)}
fn observe_contract(host:&Host,c:&Contract,cache:&Mutex<Cache>,result:&mut PluginScan){
    let mut budget=Budget::default();let proof=match Proof::capture(host,c,cache,&mut budget){Ok(Some(p))=>p,Ok(None)=>return,Err(())=>{result.unavailable=true;return;}};
    let mut public=Snapshot::default();let mut bundle=match bundled(host,c,&proof,&mut public){Ok(v)=>v,Err(())=>{result.unavailable=true;return;}};
    let mut metadata=Snapshot::default();let scope=Scope::capture(c);let declarations=match &scope{Ok(scope)=>match global(host,c,&proof,scope,&mut metadata){Ok(v)=>v,Err(())=>{metadata.incomplete=true;Vec::new()}},Err(())=>{metadata.incomplete=true;Vec::new()}};
    let fingerprint=match public.fingerprint(){Ok(v)=>v,Err(())=>{result.unavailable=true;return;}};
    let declarations_ok=metadata.revalidate().is_ok()&&scope.as_ref().is_ok_and(|s|s.revalidate(c).is_ok());
    if proof.revalidate(host).is_err()||public.revalidate().is_err(){result.unavailable=true;return;}
    bundle.revision.push_str(&format!(";bundle-metadata:{fingerprint}"));result.plugins.push(bundle);result.unavailable|=public.incomplete;
    if declarations_ok{result.plugins.extend(declarations);}else{metadata.incomplete=true;}
    result.unavailable|=metadata.incomplete;
}
pub(super)fn observe(host:&Host,result:&mut PluginScan){let Some(c)=contract()else{result.unavailable=true;return;};observe_contract(host,c,cache(),result)}
fn watch_contract(host:&Host,c:&Contract,cache:&Mutex<Cache>)->Vec<PathBuf>{
    if c.validate().is_err(){return Vec::new();}let Ok(roots)=roots(host)else{return Vec::new();};let mut watches=BTreeSet::new();
    for root in roots{watches.insert(root.clone());for guard in &c.guards{watches.insert(root.join(&guard.path));}}
    let mut budget=Budget::default();if let Ok(Some(proof))=Proof::capture(host,c,cache,&mut budget){
        let mut public=Snapshot::default();let _=bundled(host,c,&proof,&mut public);if public.revalidate().is_ok(){watches.extend(public.watches);}
        if let Ok(scope)=Scope::capture(c){let mut s=Snapshot::default();let _=global(host,c,&proof,&scope,&mut s);if s.revalidate().is_ok()&&scope.revalidate(c).is_ok(){watches.extend(s.watches);}}
    }watches.into_iter().filter(|p|checked_path(p).is_ok()).collect()
}
pub(super)fn watch_paths(host:&Host)->Vec<PathBuf>{contract().map(|c|watch_contract(host,c,cache())).unwrap_or_default()}
/// Unknown images return None. The caller must always preserve the passive route.
pub(super)fn runtime_version(selected:&Path)->Option<String>{
    let c=contract()?;let host=Host{agent_id:c.agent_id.clone(),adapter:"coco_desktop".into(),docs_url:String::new(),manifest_paths:Vec::new(),default_mcp_files:Vec::new(),mcp_id_prefix:String::new(),resource_groups:Vec::new(),
        roots:["/Applications/Cortex Code.app","~/Applications/Cortex Code.app"].into_iter().map(|p|Root{path:p.into(),layout:"coco_app".into(),presence:PluginPresence::Bundled,complete_marker:None}).collect()};
    let p=Proof::capture(&host,c,cache(),&mut Budget::default()).ok()??;if selected!=p.app.path&&selected!=p.app.path.join("Contents/MacOS/Cortex Code"){return None;}p.revalidate(&host).ok()?;Some(c.version.clone())
}
#[cfg(test)]
mod tests;
