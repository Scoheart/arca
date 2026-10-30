//! QwenWorkCN 1.4.0 public App metadata and optional local-market installation sources.
//! No identity/registry/harness authority, component bodies or target execution.
use super::*;
use std::{collections::VecDeque,io::{Read,Seek,SeekFrom},sync::Mutex};

const HASH_LIMIT:u64=384*1024*1024;
const DOC_LIMIT:usize=2*1024*1024;
const READ_LIMIT:usize=8*1024*1024;
const VISIT_LIMIT:usize=16384;
const COMPONENT_LIMIT:usize=4096;
const ENTRY_LIMIT:usize=1024;
const PIN_LIMIT:usize=4096;
const PATH_DEPTH:usize=64;
const ASAR:&str="Contents/Resources/app.asar";
const PACKAGE:&str="package.json";
const MANIFESTS:[&str;3]=[".qwenwork-plugin/plugin.json",".qoder-plugin/plugin.json",".claude-plugin/plugin.json"];
fn digest(v:&str)->bool{v.len()==64&&v.bytes().all(|b|b.is_ascii_digit()||matches!(b,b'a'..=b'f'))}
fn local(v:&str)->Result<(),()>{if v.is_empty()||v.len()>4096||v.contains(['\\',':','?','#'])||v.chars().any(char::is_control)||Path::new(v).is_absolute()||Path::new(v).components().any(|c|!matches!(c,Component::Normal(_))){Err(())}else{Ok(())}}
fn segment(v:&str)->bool{!v.is_empty()&&v.len()<=240&&!v.contains("..")&&!v.contains(['/','\\',':'])&&metadata_name(v).as_deref()==Some(v)}
fn name(v:&str)->Option<String>{(!v.contains("://")).then(||metadata_name(v)).flatten()}
fn checked_path(p:&Path)->Result<(),()>{if !p.is_absolute()||p.components().count()>PATH_DEPTH||p.to_str().is_none_or(|s|s.len()>4096||s.chars().any(char::is_control))||p.components().any(|c|matches!(c,Component::ParentDir|Component::CurDir|Component::Prefix(_))){return Err(());}checked_root(p)}

#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Guard{path:String,bytes:u64,sha256:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Member{path:String,offset:u64,bytes:u64,sha256:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Asar{path:String,header_json_bytes:u64,payload_offset:u64,header_sha256:String,members:Vec<Member>}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SkillSeed{source:String,folder:String,bytes:u64,mode:u32,name:String,version:Option<String>}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Index{guard:Guard,format:u32,payload_digest:String,records:Vec<SkillSeed>}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest{path:String,guard:Option<Guard>}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct LeafSeed{name:String,path:String,bytes:u64}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct MarketSource{folder:String,catalog_id:String,name:String,version:String,root:String,manifests:Vec<Manifest>,skills:Vec<LeafSeed>}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Catalog{guard:Guard,entries:Vec<MarketSource>}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence{carrier_sha256:String,frozen_contract_sha256:String,implementation_contract_sha256:String,rea_parent:String,rea_export_sha256:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Contract{schema_version:u32,agent_id:String,app_name:String,bundle_id:String,version:String,build:String,package_name:String,package_main:String,guards:Vec<Guard>,asar:Asar,index:Index,catalog:Catalog,evidence:Evidence}
impl Guard{
    fn validate(&self)->Result<(),()>{local(&self.path)?;if self.bytes==0||!digest(&self.sha256){Err(())}else{Ok(())}}
}
impl Contract{
    fn validate(&self)->Result<(),()>{
        if self.schema_version!=1||self.agent_id!="qwenwork-cn"||self.app_name!="QwenWorkCN.app"||self.bundle_id!="cn.qwenwork.desktop.mac"||self.version!="1.4.0"||self.build!="26102705"||self.package_name!="qwen-work-cn"||self.package_main!="out/main/index.js"{return Err(());}
        let mut paths=BTreeSet::new();let mut bytes=0u64;for g in &self.guards{g.validate()?;if !paths.insert(g.path.as_str()){return Err(());}bytes=bytes.checked_add(g.bytes).ok_or(())?;}
        if paths!=BTreeSet::from(["Contents/Info.plist","Contents/MacOS/QwenWorkCN",ASAR])||bytes>HASH_LIMIT||self.asar.path!=ASAR||self.asar.header_json_bytes==0||self.asar.payload_offset<self.asar.header_json_bytes||!digest(&self.asar.header_sha256){return Err(());}
        let asar=self.guards.iter().find(|g|g.path==ASAR).ok_or(())?;let mut members=BTreeSet::new();
        for m in &self.asar.members{local(&m.path)?;if !members.insert(m.path.as_str())||m.bytes==0||!digest(&m.sha256)||m.offset<self.asar.payload_offset||m.offset.checked_add(m.bytes).is_none_or(|end|end>asar.bytes){return Err(());}}
        if members!=BTreeSet::from([PACKAGE,"out/main/index.js","out/main/entry-app.js","out/main/local-identity-bootstrap.cjs","out/main/main.js","out/main/chunks/constants-BlnG02eZ.js","out/main/chunks/constants-CuIDBbnm.js"]){return Err(());}
        self.index.guard.validate()?;if self.index.guard.path!="Contents/Resources/builtin-skill-metadata-index.json"||self.index.format!=3||!digest(&self.index.payload_digest)||self.index.records.len()!=16{return Err(());}
        let mut folders=BTreeSet::new();for r in &self.index.records{if !matches!(r.source.as_str(),"skills"|"profile-skills")||!segment(&r.folder)||!folders.insert(&r.folder)||r.bytes==0||r.mode==0||name(&r.name).as_deref()!=Some(&r.name)||r.version.as_ref().is_some_and(|v|name(v).as_deref()!=Some(v)){return Err(());}}
        if self.index.records.iter().filter(|r|r.source=="skills").count()!=15||self.index.records.iter().filter(|r|r.source=="profile-skills").count()!=1{return Err(());}
        self.catalog.guard.validate()?;if self.catalog.guard.path!="Contents/Resources/plugin-market-data.json"||self.catalog.entries.len()!=15{return Err(());}
        let mut folders=BTreeSet::new();let mut leaves=BTreeSet::new();for e in &self.catalog.entries{
            if !segment(&e.folder)||!folders.insert(&e.folder)||name(&e.catalog_id).is_none()||name(&e.name).as_deref()!=Some(&e.name)||!e.version.is_empty()&&name(&e.version).as_deref()!=Some(&e.version)||e.root!=format!("Contents/Resources/plugins-example/{}",e.folder)||e.manifests.len()!=3{return Err(());}
            for(m,suffix)in e.manifests.iter().zip(MANIFESTS){if m.path!=format!("{}/{suffix}",e.root){return Err(());}if let Some(g)=&m.guard{g.validate()?;if g.path!=m.path{return Err(());}}}
            if e.manifests.iter().all(|m|m.guard.is_none()){return Err(());}
            for leaf in &e.skills{local(&leaf.path)?;if !segment(&leaf.name)||leaf.bytes==0||leaf.path!=format!("{}/skills/{}/SKILL.md",e.root,leaf.name)||!leaves.insert(&leaf.path){return Err(());}}
        }
        if leaves.len()!=140||[&self.evidence.carrier_sha256,&self.evidence.frozen_contract_sha256,&self.evidence.implementation_contract_sha256,&self.evidence.rea_export_sha256].iter().any(|v|!digest(v))||!self.evidence.rea_parent.starts_with("ev_"){return Err(());}Ok(())
    }
}
fn contract()->Option<&'static Contract>{static C:OnceLock<Option<Contract>>=OnceLock::new();C.get_or_init(||serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-qwenwork-cn.json")).ok().filter(|c|c.validate().is_ok())).as_ref()}
fn approved(g:&Guard,bytes:&[u8])->Result<(),()>{if bytes.len()as u64!=g.bytes||hex::encode(Sha256::digest(bytes))!=g.sha256{Err(())}else{Ok(())}}

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
    budget:Budget,leaves:BTreeMap<PathBuf,Option<Leaf>>,documents:BTreeMap<PathBuf,Arc<Vec<u8>>>,
    directories:BTreeMap<PathBuf,Vec<(PathBuf,bool,bool)>>,watches:BTreeSet<PathBuf>,incomplete:bool,
}
impl Snapshot{
    fn limit(&self)->Result<(),()>{if self.leaves.len()>=PIN_LIMIT{Err(())}else{Ok(())}}
    fn remember(&mut self,path:&Path)->Result<Option<Leaf>,()>{
        self.budget.visit()?;self.limit()?;let v=signature(path)?;
        if let Some(before)=self.leaves.get(path){if before!=&v{return Err(());}}else{self.leaves.insert(path.into(),v.clone());}Ok(v)
    }
    fn parents(&mut self,path:&Path)->Result<(),()>{
        checked_path(path)?;let mut parents=Vec::new();let mut next=path.parent();
        while let Some(p)=next{self.budget.visit()?;if parents.len()>=PATH_DEPTH{return Err(());}parents.push(p.to_path_buf());next=p.parent();}
        let home=expand_tilde("~/");
        for p in parents.into_iter().rev(){
            let meta=self.remember(&p)?;
            if p==Path::new("/")||p==home{if meta.is_none_or(|v|!v.directory){return Err(());}}
            else if meta.is_some_and(|v|!v.directory){return Err(());}
        }Ok(())
    }
    fn pin(&mut self,path:&Path,directory:bool)->Result<bool,()>{
        self.budget.visit()?;self.parents(path)?;self.limit()?;
        match self.remember(path)?{None=>Ok(false),Some(v)=>{
            if if directory{!v.directory}else{!v.file}{return Err(());}Ok(true)
        }}
    }
    fn stat(&mut self,path:&Path)->Result<Option<Leaf>,()>{self.watches.insert(path.into());self.parents(path)?;self.remember(path)}
    fn approved(&mut self,root:&Path,guard:&Guard)->Result<Option<Arc<Vec<u8>>>,()>{
        let path=root.join(&guard.path);if path.extension().and_then(|v|v.to_str())!=Some("json"){return Err(());}self.watches.insert(path.clone());
        if !self.pin(&path,false)?{return Ok(None);}if let Some(value)=self.documents.get(&path){approved(guard,value.as_slice())?;return Ok(Some(value.clone()));}
        let mut bound=Bound::capture(path.clone(),false,&mut self.budget)?;
        if self.leaves.get(&path).and_then(Option::as_ref).is_none_or(|m|m.stamp!=bound.stamp)||bound.stamp.bytes!=guard.bytes{return Err(());}
        let bytes=bound.small(&mut self.budget)?;bound.revalidate()?;self.remember(&path)?;approved(guard,&bytes)?;
        let value=Arc::new(bytes);self.documents.insert(path,value.clone());Ok(Some(value))
    }
    fn entries(&mut self,path:&Path)->Result<Vec<(PathBuf,bool,bool)>,()>{
        self.watches.insert(path.into());if !self.pin(path,true)?{return Ok(Vec::new());}if let Some(v)=self.directories.get(path){return Ok(v.clone());}
        let mut rows=Vec::new();
        #[cfg(unix)]{
            use rustix::fs::{Dir,statat,AtFlags,FileType};
            let bound=Bound::capture(path.into(),true,&mut self.budget)?;
            if self.leaves.get(path).and_then(Option::as_ref).is_none_or(|m|m.stamp!=bound.stamp){return Err(());}
            let mut iter=Dir::read_from(&bound.file).map_err(|_|())?;
            for item in &mut iter{let item=item.map_err(|_|())?;let name=item.file_name();if matches!(name.to_bytes(),b"."|b".."){continue;}
                self.budget.visit()?;if rows.len()>=ENTRY_LIMIT{return Err(());}let name=std::str::from_utf8(name.to_bytes()).map_err(|_|())?;
                let m=statat(&bound.file,std::ffi::OsStr::new(name),AtFlags::SYMLINK_NOFOLLOW).map_err(|_|())?;let kind=FileType::from_raw_mode(m.st_mode);
                rows.push((path.join(name),kind==FileType::Directory,kind==FileType::RegularFile));
            }bound.revalidate()?;self.remember(path)?;
        }
        #[cfg(not(unix))]{return Err(());}
        rows.sort();self.directories.insert(path.into(),rows.clone());Ok(rows)
    }
    fn revalidate(&self)->Result<(),()>{for(p,v)in &self.leaves{if &signature(p)?!=v{return Err(());}}Ok(())}
    fn fingerprint(&self)->Result<String,()>{self.revalidate()?;Ok(hex::encode(Sha256::digest(serde_json::to_vec(&self.leaves).map_err(|_|())?)))}
}

#[derive(Debug,PartialEq,Eq)]
struct PublishedSkill{source:String,folder:String,path:String,name:String,version:Option<String>,bytes:u64,mode:u32}
fn parse_index(c:&Contract,bytes:&[u8])->Result<Vec<PublishedSkill>,()>{
    approved(&c.index.guard,bytes)?;let document:Value=serde_json::from_slice(bytes).map_err(|_|())?;let payload=document.get("payload").and_then(Value::as_object).ok_or(())?;
    // The complete approved byte hash binds native JSON insertion order. The
    // published digest was independently verified with JSON.stringify; sorting
    // a serde map would implement a different native digest protocol.
    if document.get("digest").and_then(Value::as_str)!=Some(c.index.payload_digest.as_str())||payload.get("format").and_then(Value::as_u64)!=Some(3)||payload.get("appVersion").and_then(Value::as_str)!=Some(c.version.as_str())||payload.get("sources")!=Some(&serde_json::json!(["skills","profile-skills"])){return Err(());}
    let records=payload.get("records").and_then(Value::as_array).ok_or(())?;if records.len()!=c.index.records.len(){return Err(());}let mut seen=BTreeSet::new();let mut result=Vec::new();
    for value in records{let source=value.get("source").and_then(Value::as_str).ok_or(())?;let folder=value.get("folderName").and_then(Value::as_str).ok_or(())?;if !segment(folder)||!seen.insert(folder){return Err(());}
        let seed=c.index.records.iter().find(|r|r.folder==folder&&r.source==source).ok_or(())?;let stamp=value.get("stamp").and_then(Value::as_array).filter(|v|v.len()==2).ok_or(())?;
        let mut numbers=[0u64;2];for(i,v)in stamp.iter().enumerate(){let v=v.as_str().filter(|v|!v.is_empty()&&v.bytes().all(|b|b.is_ascii_digit())).ok_or(())?;numbers[i]=v.parse().map_err(|_|())?;}
        if numbers!=[seed.bytes,seed.mode as u64]||!value.get("contentDigest").and_then(Value::as_str).is_some_and(digest){return Err(());}
        let metadata=value.get("metadata").and_then(Value::as_object).ok_or(())?;if metadata.get("description").and_then(Value::as_str).is_none()||metadata.get("descriptionZh").is_some_and(|v|!v.is_string()){return Err(());}
        let title=metadata.get("name").and_then(Value::as_str).and_then(name).ok_or(())?;let version=match metadata.get("version"){None=>None,Some(Value::String(v))=>Some(name(v).ok_or(())?),_=>return Err(())};
        if title!=seed.name||version!=seed.version{return Err(());}result.push(PublishedSkill{source:source.into(),folder:folder.into(),path:format!("Contents/Resources/{source}/{folder}/SKILL.md"),name:title,version,bytes:seed.bytes,mode:seed.mode});
    }Ok(result)
}
fn select_manifest(source:&MarketSource,present:&[bool])->Result<Option<usize>,()>{
    if present.len()!=source.manifests.len(){return Err(());}for(i,exists)in present.iter().enumerate(){if *exists{return source.manifests[i].guard.as_ref().map(|_|Some(i)).ok_or(());}}Ok(None)
}
struct ManifestMetadata{name:String,version:String}
fn manifest_metadata(bytes:&[u8],folder:&str)->Result<ManifestMetadata,()>{
    let value:Value=serde_json::from_slice(bytes).map_err(|_|())?;let object=value.as_object().ok_or(())?;
    let title=["displayName","name"].into_iter().find_map(|k|object.get(k).and_then(Value::as_str).and_then(name)).unwrap_or_else(||folder.into());
    let version=object.get("version").and_then(Value::as_str).and_then(name).unwrap_or_default();Ok(ManifestMetadata{name:title,version})
}

fn roots(host:&Host)->Result<Vec<PathBuf>,()>{
    if host.agent_id!="qwenwork-cn"||host.adapter!="qwenwork_cn"||host.roots.len()!=2{return Err(());}
    let mut seen=BTreeSet::new();host.roots.iter().map(|r|{
        if r.layout!="qwenwork_cn_app"||r.presence!=PluginPresence::Bundled||!["/Applications/QwenWorkCN.app","~/Applications/QwenWorkCN.app"].contains(&r.path.as_str())||!seen.insert(&r.path){return Err(());}
        let p=vscode_extensions::root_path(r);checked_path(&p)?;Ok(p)
    }).collect()
}
fn selected_app(host:&Host)->Result<Option<PathBuf>,()>{
    use crate::application::agent_launch::{info_unlocked,LaunchTarget};
    let allowed=roots(host)?;let info=info_unlocked(&host.agent_id).map_err(|_|())?;
    if info.configured_target.is_some(){return Err(());}
    match info.resolved_target{Some(LaunchTarget::App{path,args,new_instance,env})if args.is_empty()&&!new_instance&&env.is_empty()=>{
        let p=PathBuf::from(path);if !allowed.contains(&p){return Err(());}checked_path(&p)?;Ok(Some(p))
    },None=>Ok(None),_=>Err(())}
}
struct Proof{app:Bound,files:BTreeMap<String,Bound>,identity:String}
impl Proof{
    fn capture(host:&Host,c:&Contract)->Result<Option<Self>,()>{
        c.validate()?;let Some(path)=selected_app(host)?else{return Ok(None);};let mut budget=Budget::default();
        let mut p=Self{app:Bound::capture(path,true,&mut budget)?,files:BTreeMap::new(),identity:String::new()};
        for g in &c.guards{let mut file=Bound::capture(p.app.path.join(&g.path),false,&mut budget)?;if file.stamp.bytes!=g.bytes{return Err(());}hash(&mut file,&p.app,&g.sha256,cache(),&mut budget)?;p.files.insert(g.path.clone(),file);p.selection(host)?;}
        let bytes=p.files.get_mut("Contents/Info.plist").ok_or(())?.small(&mut budget)?;let info=plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(|_|())?;let info=info.as_dictionary().ok_or(())?;
        for(k,v)in[("CFBundleIdentifier",c.bundle_id.as_str()),("CFBundleName","QwenWorkCN"),("CFBundleDisplayName","QwenWorkCN"),("CFBundleExecutable","QwenWorkCN"),("CFBundleShortVersionString",c.version.as_str()),("CFBundleVersion",c.build.as_str())]{if info.get(k).and_then(plist::Value::as_string)!=Some(v){return Err(());}}
        let integrity=info.get("ElectronAsarIntegrity").and_then(plist::Value::as_dictionary).and_then(|v|v.get("Resources/app.asar")).and_then(plist::Value::as_dictionary).ok_or(())?;
        if integrity.get("algorithm").and_then(plist::Value::as_string)!=Some("SHA256")||integrity.get("hash").and_then(plist::Value::as_string)!=Some(c.asar.header_sha256.as_str()){return Err(());}
        // The complete ASAR digest authenticates the published header, entry,
        // bootstrap and member locations. Read only the approved package JSON
        // at its fixed range; never allocate/parse the 8,854,756-byte header or
        // apply the older 8-MiB helper's implicit rejection limit.
        let member=c.asar.members.iter().find(|m|m.path==PACKAGE).ok_or(())?;let file=p.files.get_mut(ASAR).ok_or(())?;
        if member.bytes>DOC_LIMIT as u64{return Err(());}file.revalidate()?;file.file.seek(SeekFrom::Start(member.offset)).map_err(|_|())?;
        let mut bytes=vec![0;usize::try_from(member.bytes).map_err(|_|())?];file.file.read_exact(&mut bytes).map_err(|_|())?;file.revalidate()?;
        if hex::encode(Sha256::digest(&bytes))!=member.sha256{return Err(());}let package:Value=serde_json::from_slice(&bytes).map_err(|_|())?;
        if package.get("name").and_then(Value::as_str)!=Some(c.package_name.as_str())||package.get("version").and_then(Value::as_str)!=Some(c.version.as_str())||package.get("main").and_then(Value::as_str)!=Some(c.package_main.as_str()){return Err(());}
        p.identity=hex::encode(Sha256::digest(serde_json::to_vec(&(&p.app.path,&p.app.stamp,p.files.iter().map(|(k,v)|(k,&v.stamp)).collect::<Vec<_>>(),c)).map_err(|_|())?));p.selection(host)?;Ok(Some(p))
    }
    fn revalidate(&self)->Result<(),()>{self.app.revalidate()?;for f in self.files.values(){f.revalidate()?;}self.app.revalidate()}
    fn selection(&self,host:&Host)->Result<(),()>{if selected_app(host)?.as_ref()!=Some(&self.app.path){return Err(());}self.revalidate()}
    fn selected(&self,path:&Path)->Result<(),()>{if path!=self.app.path.as_path()&&path!=self.app.path.join("Contents/MacOS/QwenWorkCN"){return Err(());}self.revalidate()}
}
fn cache()->&'static Mutex<Cache>{static C:OnceLock<Mutex<Cache>>=OnceLock::new();C.get_or_init(Mutex::default)}
fn row(host:&Host,p:&Proof,id:String,title:String,version:String,sources:Vec<String>,components:Vec<PluginComponent>,role:String)->NativePluginObservation{
    NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id,name:title,version,presence:PluginPresence::Bundled,enabled:None,source_paths:sources,components,
        revision:format!("bound-app:{};{role};bundled-availability-only;installed/identity/harness/native-id/frontmatter-acceptance/effective-enablement/session-load=unknown",p.identity),docs_url:host.docs_url.clone(),source_fingerprint:String::new()}
}
fn skill_component(title:&str,path:&Path,s:&mut Snapshot)->Result<PluginComponent,()>{
    s.budget.component()?;Ok(PluginComponent{kind:PluginComponentKind::Skill,name:name(title).ok_or(())?,source_path:collapse_home(path),native_id:None})
}
fn app_sources(host:&Host,c:&Contract,p:&Proof,s:&mut Snapshot)->Result<Vec<NativePluginObservation>,()>{
    let bytes=s.approved(&p.app.path,&c.index.guard)?.ok_or(())?;let records=parse_index(c,bytes.as_slice())?;
    let mut inventory=BTreeMap::new();
    // Native orderedBuiltinSkillSourceDirs is main first; profile duplicates
    // are ignored before metadata selection. Only filenames/stat are read.
    for source in ["skills","profile-skills"]{
        let root=p.app.path.join("Contents/Resources").join(source);if !s.stat(&root)?.is_some_and(|v|v.directory){return Err(());}
        for(path,directory,file)in s.entries(&root)?{let folder=path.file_name().and_then(|v|v.to_str()).ok_or(())?;
            if inventory.contains_key(folder){continue;}if !directory{if !file{return Err(());}continue;}if !segment(folder){return Err(());}inventory.insert(folder.to_owned(),source);
        }
    }
    if inventory.len()!=records.len(){return Err(());}for record in &records{
        if inventory.get(&record.folder)!=Some(&record.source.as_str()){return Err(());}let leaf=s.stat(&p.app.path.join(&record.path))?.ok_or(())?;
        if !leaf.file||leaf.stamp.bytes!=record.bytes||leaf.stamp.mode!=record.mode{return Err(());}
    }
    let mut rows=Vec::new();for(source,id,label)in[("skills","@builtin/skills","QwenWorkCN App Skills"),("profile-skills","@builtin/profile-skills","QwenWorkCN profile Skills")]{
        let mut items=Vec::new();let mut versions=BTreeMap::new();for record in records.iter().filter(|r|r.source==source){
            items.push(skill_component(&record.name,&p.app.path.join(&record.path),s)?);versions.insert(&record.folder,&record.version);
        }
        rows.push(row(host,p,id.into(),label.into(),c.version.clone(),vec![collapse_home(&p.app.path.join(&c.index.guard.path)),collapse_home(&p.app.path.join("Contents/Resources").join(source))],items,
            format!("application-owned-published-metadata;source:{source};declared-Skill-versions:{};group-version=App-context;body-digests/cache/policies-unobserved",serde_json::to_string(&versions).map_err(|_|())?)));
    }Ok(rows)
}
fn catalog_ready(c:&Contract,p:&Proof,s:&mut Snapshot)->Result<(),()>{
    let bytes=s.approved(&p.app.path,&c.catalog.guard)?.ok_or(())?;let value:Value=serde_json::from_slice(bytes.as_slice()).map_err(|_|())?;let values=value.get("plugins").and_then(Value::as_array).ok_or(())?;
    if values.len()!=c.catalog.entries.len(){return Err(());}let mut seen=BTreeSet::new();for value in values{
        let id=value.get("plugin_id").and_then(Value::as_str).ok_or(())?;if !seen.insert(id){return Err(());}let entry=c.catalog.entries.iter().find(|e|e.catalog_id==id).ok_or(())?;
        let folder=["folder_name","plugin_name","plugin_id"].into_iter().find_map(|k|value.get(k).and_then(Value::as_str).filter(|s|!s.is_empty())).ok_or(())?;
        if folder!=entry.folder{return Err(());}
    }Ok(())
}
fn market_source(host:&Host,p:&Proof,e:&MarketSource,s:&mut Snapshot)->Result<NativePluginObservation,()>{
    let mut present=vec![false;e.manifests.len()];for(i,candidate)in e.manifests.iter().enumerate(){let path=p.app.path.join(&candidate.path);match s.stat(&path)?{None=>{},Some(leaf)=>{if !leaf.file{return Err(());}present[i]=true;break;}}}
    let chosen=select_manifest(e,&present)?.ok_or(())?;let guard=e.manifests[chosen].guard.as_ref().ok_or(())?;let bytes=s.approved(&p.app.path,guard)?.ok_or(())?;let manifest=manifest_metadata(bytes.as_slice(),&e.folder)?;
    if manifest.version!=e.version{return Err(());}let mut items=Vec::new();
    for leaf in &e.skills{let path=p.app.path.join(&leaf.path);match s.stat(&path){Ok(Some(v))if v.file&&v.stamp.bytes==leaf.bytes=>match skill_component(&leaf.name,&path,s){Ok(v)=>items.push(v),Err(())=>s.incomplete=true},_=>s.incomplete=true}}
    Ok(row(host,p,format!("@market-source/{}",e.folder),e.name.clone(),manifest.version,vec![collapse_home(&p.app.path.join(&guard.path))],items,
        format!("optional-local-market-installation-source;catalog-entry:{};declared-manifest-name:{};selected-manifest:{};not-installed-state;Skill-titles=directory-availability",e.catalog_id,manifest.name,MANIFESTS[chosen])))
}
fn source_inventory(host:&Host,c:&Contract,p:&Proof,s:&mut Snapshot)->Vec<NativePluginObservation>{
    let mut rows=Vec::new();match app_sources(host,c,p,s){Ok(v)=>rows.extend(v),Err(())=>s.incomplete=true}
    match catalog_ready(c,p,s){Ok(())=>for entry in &c.catalog.entries{match market_source(host,p,entry,s){Ok(mut row)=>{row.source_paths.push(collapse_home(&p.app.path.join(&c.catalog.guard.path)));rows.push(row);},Err(())=>s.incomplete=true}},Err(())=>s.incomplete=true}
    rows
}
fn observe_contract(host:&Host,c:&Contract,result:&mut PluginScan){
    let p=match Proof::capture(host,c){Ok(Some(v))=>v,Ok(None)=>return,Err(())=>{result.unavailable=true;return;}};
    let mut snapshot=Snapshot::default();let mut rows=source_inventory(host,c,&p,&mut snapshot);let fingerprint=match snapshot.fingerprint(){Ok(v)=>v,Err(())=>{result.unavailable=true;return;}};
    if p.selection(host).is_err()||snapshot.revalidate().is_err(){result.unavailable=true;return;}for row in &mut rows{row.revision.push_str(&format!(";public-metadata:{fingerprint}"));}
    result.plugins.extend(rows);result.unavailable|=snapshot.incomplete;
}
pub(super)fn observe(host:&Host,result:&mut PluginScan){let Some(c)=contract()else{result.unavailable=true;return;};observe_contract(host,c,result)}
fn watch_contract(host:&Host,c:&Contract)->Vec<PathBuf>{
    let Ok(roots)=roots(host)else{return Vec::new();};let mut paths=BTreeSet::new();for root in roots{
        paths.insert(root.clone());for guard in &c.guards{paths.insert(root.join(&guard.path));}paths.insert(root.join(&c.index.guard.path));paths.insert(root.join(&c.catalog.guard.path));
        for source in ["skills","profile-skills"]{paths.insert(root.join("Contents/Resources").join(source));}
        for record in &c.index.records{paths.insert(root.join(format!("Contents/Resources/{}/{}/SKILL.md",record.source,record.folder)));}
        for entry in &c.catalog.entries{paths.insert(root.join(&entry.root));for m in &entry.manifests{paths.insert(root.join(&m.path));}for skill in &entry.skills{paths.insert(root.join(&skill.path));}}
    }paths.into_iter().filter(|p|checked_path(p).is_ok()).collect()
}
pub(super)fn watch_paths(host:&Host)->Vec<PathBuf>{contract().map(|c|watch_contract(host,c)).unwrap_or_default()}
pub(super)fn runtime_version(selected:&Path)->Option<String>{
    let c=contract()?;let host=Host{agent_id:c.agent_id.clone(),adapter:"qwenwork_cn".into(),docs_url:String::new(),manifest_paths:Vec::new(),default_mcp_files:Vec::new(),mcp_id_prefix:String::new(),resource_groups:Vec::new(),roots:["/Applications/QwenWorkCN.app","~/Applications/QwenWorkCN.app"].into_iter().map(|p|Root{path:p.into(),layout:"qwenwork_cn_app".into(),presence:PluginPresence::Bundled,complete_marker:None}).collect()};
    let allowed=roots(&host).ok()?;if !allowed.iter().any(|p|selected==p.as_path()||selected==p.join("Contents/MacOS/QwenWorkCN")){return None;}let proof=Proof::capture(&host,c).ok()??;proof.selected(selected).ok()?;proof.selection(&host).ok()?;Some(c.version.clone())
}
#[cfg(test)]
mod tests;
