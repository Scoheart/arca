//! Pinned Kilo Desktop bundle metadata. No native service, module or helper runs.
//! Desktop manifests and its child CLI remain separate from standalone Kilo CLI.
use super::*;
use std::collections::VecDeque;
use std::io::{Seek,SeekFrom};
use std::sync::Mutex;

const ASAR:&str="Contents/Resources/app.asar";
const CHILD:&str="Contents/Resources/kilo/darwin-arm64/bin/kilo";
const MAX_HASH:u64=768*1024*1024;
const MAX_HEADER:u64=32*1024*1024;
const MAX_READ:usize=1024*1024;
const MAX_DOCUMENT:usize=128*1024;
const MAX_VISITS:usize=2048;
const MAX_COMPONENTS:usize=1024;
const MAX_CACHE:usize=32;
const MAX_MANAGED_ENTRIES:usize=512;

#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard{path:String,bytes:u64,sha256:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Header{pickle_bytes:u64,utf8_bytes:u64,sha256:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Member{path:String,offset:u64,bytes:u64,sha256:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeManifest{id:String,name:String,version:String,member:Member}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Skill{name:String,resource_path:String,offset:u64,bytes:u64}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct McpReference{plugin_id:String,name:String,gate:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence{archive_sha256:String,contract_sha256:String,source_index_sha256:String,global_authority_sha256:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract{
    schema_version:u32,agent_id:String,app_name:String,bundle_id:String,version:String,package_name:String,
    child_version:String,child_version_offset:u64,managed_sdk_directory:String,guards:Vec<Guard>,header:Header,package:Member,main:Member,
    plugins:Vec<NativeManifest>,skill:Skill,mcp_references:Vec<McpReference>,evidence:Evidence,
}
fn digest(value:&str)->bool{value.len()==64&&value.bytes().all(|v|v.is_ascii_digit()||matches!(v,b'a'..=b'f'))}
fn version(value:&str)->bool{value.split('.').count()==3&&value.split('.').all(|v|!v.is_empty()&&v.parse::<u32>().is_ok())}
fn native_id(value:&str)->bool{!value.is_empty()&&value.len()<=96&&value.bytes().all(|v|v.is_ascii_lowercase()||v.is_ascii_digit()||v==b'-')}
impl Contract{
    fn guard(&self,path:&str)->Result<&Guard,()>{self.guards.iter().find(|v|v.path==path).ok_or(())}
    fn validate(&self)->Result<(),()>{
        if self.schema_version!=1||self.agent_id!="kilo-desktop"||self.app_name!="Kilo.app"||self.bundle_id!="ai.kilo.desktop"
            ||self.package_name!="@kilo-desktop/desktop"||!version(&self.version)||!version(&self.child_version)||self.guards.len()!=4||self.plugins.len()!=12
            ||self.managed_sdk_directory!="~/Library/Application Support/Kilo Desktop/plugins/kilo-server/runtime/kilo-plugins"
            ||![&self.evidence.archive_sha256,&self.evidence.contract_sha256,&self.evidence.source_index_sha256,&self.evidence.global_authority_sha256].iter().all(|v|digest(v)){return Err(());}
        let allowed=["Contents/Info.plist","Contents/MacOS/Kilo",ASAR,CHILD];let mut paths=BTreeSet::new();let mut bytes=self.header.utf8_bytes;
        for guard in &self.guards{
            if !allowed.contains(&guard.path.as_str())||!paths.insert(&guard.path)||guard.bytes==0||!digest(&guard.sha256){return Err(());}
            bytes=bytes.checked_add(guard.bytes).ok_or(())?;
        }
        if self.header.pickle_bytes<8||self.header.pickle_bytes>MAX_HEADER||self.header.utf8_bytes==0
            ||self.header.utf8_bytes>self.header.pickle_bytes-8||!digest(&self.header.sha256){return Err(());}
        let carrier=self.guard(ASAR)?;let child=self.guard(CHILD)?;let data_offset=8+self.header.pickle_bytes;
        let check=|m:&Member|->Result<(),()>{
            if m.path.is_empty()||m.path.len()>512||m.path.contains(['\\',':'])||Path::new(&m.path).is_absolute()
                ||Path::new(&m.path).components().any(|p|!matches!(p,Component::Normal(_)))||m.offset<data_offset||m.bytes==0
                ||m.offset.checked_add(m.bytes).is_none_or(|end|end>carrier.bytes)||!digest(&m.sha256){return Err(());}Ok(())
        };
        if self.package.path!="package.json"||self.main.path!=".vite/build/main.js"||self.package.bytes>MAX_DOCUMENT as u64{return Err(());}
        check(&self.package)?;check(&self.main)?;bytes=bytes.checked_add(self.main.bytes).ok_or(())?;
        if bytes>MAX_HASH{return Err(());}
        let mut ids=BTreeSet::new();let mut members=BTreeSet::new();
        for plugin in &self.plugins{
            if !native_id(&plugin.id)||!ids.insert(&plugin.id)||metadata_name(&plugin.name).as_ref()!=Some(&plugin.name)||!version(&plugin.version)
                ||plugin.member.path!=format!("node_modules/@kilo-desktop/plugin-{}/plugin.json",plugin.id)||!members.insert(&plugin.member.path)
                ||plugin.member.bytes>MAX_DOCUMENT as u64{return Err(());}check(&plugin.member)?;
        }
        if self.skill.name!="kilo-config"||!self.skill.resource_path.starts_with("/$bunfs/root/kilo-config-")||!self.skill.resource_path.ends_with(".md")
            ||self.skill.resource_path.contains("..")||self.skill.bytes==0||self.skill.bytes>MAX_DOCUMENT as u64
            ||self.skill.offset.checked_add(self.skill.bytes).is_none_or(|end|end>child.bytes)
            ||self.child_version_offset.checked_add((self.child_version.encode_utf16().count()as u64+2)*2).is_none_or(|end|end>child.bytes){return Err(());}
        if self.mcp_references.len()>16{return Err(());}
        for reference in &self.mcp_references{
            if !ids.contains(&reference.plugin_id)||metadata_name(&reference.name).as_ref()!=Some(&reference.name)||metadata_name(&reference.gate).as_ref()!=Some(&reference.gate){return Err(());}
        }Ok(())
    }
}
fn contract()->Option<&'static Contract>{
    static VALUE:OnceLock<Option<Contract>>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-kilo-desktop.json")).ok().filter(|c|c.validate().is_ok())).as_ref()
}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
struct Stamp{dev:u64,ino:u64,bytes:u64,mode:u32,links:u64,mtime:i64,mtime_ns:i64,ctime:i64,ctime_ns:i64}
impl Stamp{
    fn capture(value:&fs::Metadata)->Result<Self,()>{
        #[cfg(unix)]{use std::os::unix::fs::MetadataExt;Ok(Self{dev:value.dev(),ino:value.ino(),bytes:value.len(),mode:value.mode(),links:value.nlink(),mtime:value.mtime(),mtime_ns:value.mtime_nsec(),ctime:value.ctime(),ctime_ns:value.ctime_nsec()})}
        #[cfg(not(unix))]{let _=value;Err(())}
    }
}
#[derive(Default)]
struct Budget{hashed:u64,read:usize,visited:usize,components:usize}
impl Budget{
    fn visit(&mut self)->Result<(),()>{self.visited=self.visited.checked_add(1).ok_or(())?;if self.visited>MAX_VISITS{Err(())}else{Ok(())}}
    fn component(&mut self)->Result<(),()>{self.visit()?;self.components=self.components.checked_add(1).ok_or(())?;if self.components>MAX_COMPONENTS{Err(())}else{Ok(())}}
}
struct Bound{path:PathBuf,file:fs::File,stamp:Stamp,directory:bool}
impl Bound{
    fn capture(path:PathBuf,directory:bool,budget:&mut Budget)->Result<Self,()>{
        budget.visit()?;checked_root(&path)?;let metadata=fs::symlink_metadata(&path).map_err(|_|())?;
        if if directory{!metadata.is_dir()}else{!metadata.file_type().is_file()}{return Err(());}
        let stamp=Stamp::capture(&metadata)?;if !directory&&stamp.links!=1{return Err(());}
        let file=open_confined(&path,directory)?;let value=Self{path,file,stamp,directory};value.revalidate()?;Ok(value)
    }
    fn revalidate(&self)->Result<(),()>{
        checked_root(&self.path)?;let m=fs::symlink_metadata(&self.path).map_err(|_|())?;
        if if self.directory{!m.is_dir()}else{!m.file_type().is_file()}{return Err(());}
        if Stamp::capture(&m)?!=self.stamp||Stamp::capture(&self.file.metadata().map_err(|_|())?)?!=self.stamp{return Err(());}Ok(())
    }
    fn slice(&mut self,offset:u64,bytes:u64,budget:&mut Budget)->Result<Vec<u8>,()>{
        budget.visit()?;if self.directory||bytes>MAX_DOCUMENT as u64||offset.checked_add(bytes).is_none_or(|end|end>self.stamp.bytes){return Err(());}
        budget.read=budget.read.checked_add(usize::try_from(bytes).map_err(|_|())?).ok_or(())?;if budget.read>MAX_READ{return Err(());}
        self.revalidate()?;self.file.seek(SeekFrom::Start(offset)).map_err(|_|())?;let mut out=vec![0;bytes as usize];self.file.read_exact(&mut out).map_err(|_|())?;
        self.revalidate()?;Ok(out)
    }
}
#[derive(Clone,PartialEq,Eq)]
struct DigestKey{app:PathBuf,app_stamp:Stamp,path:PathBuf,stamp:Stamp,offset:u64,bytes:u64,expected:String}
#[derive(Default)]
struct DigestCache{entries:VecDeque<DigestKey>}
impl DigestCache{
    fn contains(&mut self,key:&DigestKey)->bool{let Some(i)=self.entries.iter().position(|v|v==key)else{return false;};let v=self.entries.remove(i).expect("located digest key");self.entries.push_back(v);true}
    fn insert(&mut self,key:DigestKey){if self.contains(&key){return;}if self.entries.len()>=MAX_CACHE{self.entries.pop_front();}self.entries.push_back(key);}
}
fn hash_span(file:&mut Bound,app:&Bound,offset:u64,bytes:u64,expected:&str,cache:&Mutex<DigestCache>,budget:&mut Budget)->Result<(),()>{
    budget.visit()?;if offset.checked_add(bytes).is_none_or(|end|end>file.stamp.bytes){return Err(());}
    file.revalidate()?;app.revalidate()?;
    let key=DigestKey{app:app.path.clone(),app_stamp:app.stamp.clone(),path:file.path.clone(),stamp:file.stamp.clone(),offset,bytes,expected:expected.into()};
    if !cache.lock().map_err(|_|())?.contains(&key){
        if bytes>MAX_HASH.saturating_sub(budget.hashed){return Err(());}
        file.file.seek(SeekFrom::Start(offset)).map_err(|_|())?;let mut remaining=bytes;let mut hash=Sha256::new();let mut buffer=[0u8;65536];
        while remaining>0{
            let n=file.file.read(&mut buffer[..remaining.min(65536)as usize]).map_err(|_|())?;if n==0{return Err(());}
            remaining-=n as u64;budget.hashed=budget.hashed.checked_add(n as u64).ok_or(())?;hash.update(&buffer[..n]);
        }
        if hex::encode(hash.finalize())!=expected{return Err(());}file.revalidate()?;app.revalidate()?;cache.lock().map_err(|_|())?.insert(key);
    }file.revalidate()?;app.revalidate()
}
fn host_paths(host:&Host)->Result<Vec<PathBuf>,()>{
    if host.agent_id!="kilo-desktop"||host.adapter!="kilo_desktop"||host.roots.is_empty()||host.roots.len()>2{return Err(());}
    let mut seen=BTreeSet::new();host.roots.iter().map(|root|{
        if root.layout!="kilo_app"||root.presence!=PluginPresence::Bundled||!["/Applications/Kilo.app","~/Applications/Kilo.app"].contains(&root.path.as_str())||!seen.insert(&root.path){return Err(());}
        let path=vscode_extensions::root_path(root);checked_root(&path)?;Ok(path)
    }).collect()
}
fn selected_app(host:&Host)->Result<Option<PathBuf>,()>{
    use crate::application::agent_launch::{info_unlocked,LaunchTarget};
    let allowed=host_paths(host)?;let launch=info_unlocked(&host.agent_id).map_err(|_|())?;if launch.configured_target.is_some(){return Err(());}
    match launch.resolved_target{
        Some(LaunchTarget::App{path,args,new_instance,env})if args.is_empty()&&!new_instance&&env.is_empty()=>{
            let path=PathBuf::from(path);if !allowed.contains(&path){return Err(());}checked_root(&path)?;Ok(Some(path))
        },None=>Ok(None),_=>Err(()),
    }
}
struct Proof{app:Bound,files:BTreeMap<String,Bound>,identity:String}
impl Proof{
    fn capture(host:&Host,c:&Contract,cache:&Mutex<DigestCache>,budget:&mut Budget)->Result<Option<Self>,()>{
        c.validate()?;let Some(path)=selected_app(host)?else{return Ok(None);};
        let mut p=Self{app:Bound::capture(path,true,budget)?,files:BTreeMap::new(),identity:String::new()};
        for guard in &c.guards{
            let mut file=Bound::capture(relative(&p.app.path,&guard.path)?,false,budget)?;if file.stamp.bytes!=guard.bytes{return Err(());}
            hash_span(&mut file,&p.app,0,guard.bytes,&guard.sha256,cache,budget)?;p.files.insert(guard.path.clone(),file);p.revalidate(host)?;
        }
        let info=p.files.get_mut("Contents/Info.plist").ok_or(())?;let bytes=info.stamp.bytes;let bytes=info.slice(0,bytes,budget)?;
        let info=plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(|_|())?;let info=info.as_dictionary().ok_or(())?;
        for(key,value)in [("CFBundleIdentifier",c.bundle_id.as_str()),("CFBundleName","Kilo"),("CFBundleDisplayName","Kilo"),("CFBundleExecutable","Kilo"),("CFBundleShortVersionString",c.version.as_str()),("CFBundleVersion",c.version.as_str())]{
            if info.get(key).and_then(plist::Value::as_string)!=Some(value){return Err(());}
        }
        let integrity=info.get("ElectronAsarIntegrity").and_then(plist::Value::as_dictionary).and_then(|v|v.get("Resources/app.asar")).and_then(plist::Value::as_dictionary).ok_or(())?;
        if integrity.get("algorithm").and_then(plist::Value::as_string)!=Some("SHA256")||integrity.get("hash").and_then(plist::Value::as_string)!=Some(c.header.sha256.as_str()){return Err(());}
        let file=p.files.get_mut(ASAR).ok_or(())?;let prefix=file.slice(0,16,budget)?;
        let words=(0..4).map(|i|Ok(u32::from_le_bytes(prefix[i*4..i*4+4].try_into().map_err(|_|())?))).collect::<Result<Vec<_>,()>>()?;
        if words!=[4,c.header.pickle_bytes as u32,(c.header.pickle_bytes-4)as u32,c.header.utf8_bytes as u32]{return Err(());}
        hash_span(file,&p.app,16,c.header.utf8_bytes,&c.header.sha256,cache,budget)?;
        hash_span(file,&p.app,c.main.offset,c.main.bytes,&c.main.sha256,cache,budget)?;
        let package=p.json(&c.package,budget)?;
        for(key,value)in [("name",c.package_name.as_str()),("version",c.version.as_str()),("main",c.main.path.as_str())]{if package.get(key).and_then(Value::as_str)!=Some(value){return Err(());}}
        let expected:Vec<u8>=format!("\"{}\"",c.child_version).encode_utf16().flat_map(u16::to_le_bytes).collect();
        if p.files.get_mut(CHILD).ok_or(())?.slice(c.child_version_offset,expected.len()as u64,budget)?!=expected{return Err(());}
        p.identity=hex::encode(Sha256::digest(serde_json::to_vec(&(&p.app.path,&p.app.stamp,p.files.iter().map(|(name,f)|(name,&f.stamp)).collect::<Vec<_>>())).map_err(|_|())?));
        p.revalidate(host)?;Ok(Some(p))
    }
    fn json(&mut self,member:&Member,budget:&mut Budget)->Result<Value,()>{
        let bytes=self.files.get_mut(ASAR).ok_or(())?.slice(member.offset,member.bytes,budget)?;
        if hex::encode(Sha256::digest(&bytes))!=member.sha256{return Err(());}serde_json::from_slice(&bytes).map_err(|_|())
    }
    fn revalidate(&self,host:&Host)->Result<(),()>{
        if selected_app(host)?.as_ref()!=Some(&self.app.path){return Err(());}self.app.revalidate()?;
        for file in self.files.values(){file.revalidate()?;}self.app.revalidate()
    }
    fn member_source(&self,path:&str)->String{format!("{}!/{path}",collapse_home(&self.app.path.join(ASAR)))}
    fn sources(&self)->Vec<String>{self.files.values().map(|f|collapse_home(&f.path)).collect()}
}
fn component(items:&mut Vec<PluginComponent>,kind:PluginComponentKind,name:&str,source:String,budget:&mut Budget)->Result<(),()>{
    budget.component()?;items.push(PluginComponent{kind,name:metadata_name(name).ok_or(())?,source_path:source,native_id:None});Ok(())
}
fn observation(host:&Host,proof:&Proof,id:String,name:String,version:String,sources:Vec<String>,mut components:Vec<PluginComponent>,context:&str)->NativePluginObservation{
    components.sort();components.dedup();NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id,name,version,
        revision:format!("public-identity-sha256:{};{context};enablement/registration/runtime=unknown",proof.identity),presence:PluginPresence::Bundled,enabled:None,
        source_paths:sources,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()}
}
fn native_manifest(host:&Host,proof:&mut Proof,c:&Contract,plugin:&NativeManifest,budget:&mut Budget)->Result<NativePluginObservation,()>{
    let value=proof.json(&plugin.member,budget)?;
    for(key,want)in [("id",plugin.id.as_str()),("displayName",plugin.name.as_str()),("version",plugin.version.as_str()),("main","./src/main/index.ts"),("web","./src/web/index.ts")]{if value.get(key).and_then(Value::as_str)!=Some(want){return Err(());}}
    let source=proof.member_source(&plugin.member.path);let mut items=Vec::new();
    if let Some(contributes)=value.get("contributes"){
        let contributes=contributes.as_object().ok_or(())?;
        for field in ["providers","routes","nav","zones","settings"]{
            if let Some(entries)=contributes.get(field){
                let entries=entries.as_array().ok_or(())?;if entries.len()>128{return Err(());}
                for entry in entries{
                    let name=["component","id","path","to","label"].iter().find_map(|key|entry.get(*key).and_then(Value::as_str)).ok_or(())?;
                    component(&mut items,PluginComponentKind::Renderer,name,source.clone(),budget)?;
                }
            }
        }
        if contributes.contains_key("deepLinks"){component(&mut items,PluginComponentKind::TypeDeclaration,"Desktop deep-link handlers",source.clone(),budget)?;}
    }
    let mut gates=Vec::new();
    for reference in c.mcp_references.iter().filter(|v|v.plugin_id==plugin.id){component(&mut items,PluginComponentKind::McpReference,&reference.name,proof.member_source(&c.main.path),budget)?;gates.push(format!("{}:{}",reference.name,reference.gate));}
    let mut sources=proof.sources();sources.push(source);
    Ok(observation(host,proof,plugin.id.clone(),plugin.name.clone(),plugin.version.clone(),sources,items,&format!("native-Desktop-manifest;UI-providers/routes!=MCP/Agent-commands;MCP-reference-gates:{}",gates.join(","))))
}
struct ManagedLeaf{path:PathBuf,stamp:Stamp}
struct ManagedSnapshot{path:PathBuf,directory:Option<Bound>,leaves:Vec<ManagedLeaf>,incomplete:bool}
fn managed_stat(directory:&Bound,name:&std::ffi::OsStr)->Result<Stamp,()>{
    #[cfg(any(target_os="macos",target_os="linux"))]{use rustix::fs::{statat,AtFlags,FileType};
        let s=statat(&directory.file,name,AtFlags::SYMLINK_NOFOLLOW).map_err(|_|())?;
        if FileType::from_raw_mode(s.st_mode)!=FileType::RegularFile{return Err(());}
        Ok(Stamp{dev:s.st_dev as u64,ino:s.st_ino as u64,bytes:u64::try_from(s.st_size).map_err(|_|())?,mode:s.st_mode as u32,links:s.st_nlink as u64,
            mtime:s.st_mtime as i64,mtime_ns:s.st_mtime_nsec as i64,ctime:s.st_ctime as i64,ctime_ns:s.st_ctime_nsec as i64})
    }
    #[cfg(not(any(target_os="macos",target_os="linux")))]{let _=(directory,name);Err(())}
}
impl ManagedSnapshot{
    fn capture(path:PathBuf,budget:&mut Budget)->Result<Self,()>{
        budget.visit()?;checked_root(&path)?;
        let mut value=Self{path:path.clone(),directory:None,leaves:Vec::new(),incomplete:false};
        match fs::symlink_metadata(&path){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(value),Err(_)=>return Err(()),_=>{}}
        let directory=Bound::capture(path.clone(),true,budget)?;
        #[cfg(unix)]{
            use std::os::unix::ffi::OsStrExt;use rustix::fs::Dir;
            let mut count=0usize;
            for entry in Dir::read_from(&directory.file).map_err(|_|())?{
                let entry=entry.map_err(|_|())?;let raw=entry.file_name();if matches!(raw.to_bytes(),b"."|b".."){continue;}
                budget.visit()?;count+=1;if count>MAX_MANAGED_ENTRIES{return Err(());}
                let name=std::str::from_utf8(raw.to_bytes()).map_err(|_|())?;if !name.ends_with(".js"){continue;}
                if metadata_name(name).as_deref()!=Some(name){value.incomplete=true;continue;}
                match managed_stat(&directory,std::ffi::OsStr::new(name)){
                    Ok(stamp)=>value.leaves.push(ManagedLeaf{path:path.join(name),stamp}),Err(())=>value.incomplete=true,
                }
            }
        }
        #[cfg(not(unix))]{return Err(());}
        value.directory=Some(directory);value.leaves.sort_by(|a,b|a.path.cmp(&b.path));value.revalidate()?;Ok(value)
    }
    fn revalidate(&self)->Result<(),()>{
        match &self.directory{
            Some(directory)=>{
                directory.revalidate()?;
                for leaf in &self.leaves{if managed_stat(directory,leaf.path.file_name().ok_or(())?)?!=leaf.stamp{return Err(());}}
                directory.revalidate()
            },
            None=>{checked_root(&self.path)?;match fs::symlink_metadata(&self.path){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),_=>Err(())}},
        }
    }
}
struct Audit{proof:Proof,managed:Option<ManagedSnapshot>,plugins:Vec<NativePluginObservation>,incomplete:bool,hashed_bytes:u64}
impl Audit{fn revalidate(&self,host:&Host)->Result<(),()>{
    self.proof.revalidate(host)?;if let Some(managed)=&self.managed{managed.revalidate()?;}self.proof.revalidate(host)
}}
fn capture(host:&Host,c:&Contract,cache:&Mutex<DigestCache>)->Result<Option<Audit>,()>{
    let mut budget=Budget::default();let Some(mut proof)=Proof::capture(host,c,cache,&mut budget)?else{return Ok(None);};
    let mut plugins=Vec::new();let mut incomplete=false;
    for plugin in &c.plugins{match native_manifest(host,&mut proof,c,plugin,&mut budget){Ok(v)=>plugins.push(v),Err(())=>incomplete=true}}
    let source=format!("{}!{}",collapse_home(&proof.app.path.join(CHILD)),c.skill.resource_path);let mut items=Vec::new();
    component(&mut items,PluginComponentKind::Skill,&c.skill.name,source.clone(),&mut budget)?;let mut sources=proof.sources();sources.push(source);
    plugins.push(observation(host,&proof,"@builtin/skills".into(),"Kilo bundled Skills".into(),c.version.clone(),sources,items,
        &format!("application-owned;child-cli-version:{};whole-carrier-bound-resource-availability;Skill-body/frontmatter=unread;shadowing/selected-engine=unknown",c.child_version)));
    let managed=match ManagedSnapshot::capture(expand_tilde(&c.managed_sdk_directory),&mut budget){
        Ok(managed)=>{
            incomplete|=managed.incomplete;
            for leaf in &managed.leaves{
                let name=leaf.path.file_name().and_then(|v|v.to_str()).ok_or(())?;let source=collapse_home(&leaf.path);let mut items=Vec::new();
                component(&mut items,PluginComponentKind::PluginReference,name,source.clone(),&mut budget)?;
                let stamp=hex::encode(Sha256::digest(serde_json::to_vec(&leaf.stamp).map_err(|_|())?));let mut sources=proof.sources();sources.push(source);
                let mut row=observation(host,&proof,format!("@sdk/runtime/{name}"),name.into(),String::new(),sources,items,
                    &format!("SDK-file-declaration;Arca-source-id!=native-plugin-id;default-userData!=active;owner/installed/native-id=unknown;body/header/exports=unread;metadata-sha256:{stamp}"));
                row.presence=PluginPresence::Declared;plugins.push(row);
            }Some(managed)
        },
        Err(())=>{incomplete=true;None},
    };
    let audit=Audit{proof,managed,plugins,incomplete,hashed_bytes:budget.hashed};audit.revalidate(host)?;Ok(Some(audit))
}
fn observe_contract(host:&Host,c:&Contract,cache:&Mutex<DigestCache>,result:&mut PluginScan)->Result<(),()>{
    let Some(audit)=capture(host,c,cache)?else{return Ok(());};audit.revalidate(host)?;result.unavailable|=audit.incomplete;result.plugins.extend(audit.plugins);Ok(())
}
fn cache()->&'static Mutex<DigestCache>{static CACHE:OnceLock<Mutex<DigestCache>>=OnceLock::new();CACHE.get_or_init(Mutex::default)}
pub(super)fn observe(host:&Host,result:&mut PluginScan){let Some(c)=contract()else{result.unavailable=true;return;};if observe_contract(host,c,cache(),result).is_err(){result.unavailable=true;}}
fn watch_paths_contract(host:&Host,c:&Contract,cache:&Mutex<DigestCache>)->Vec<PathBuf>{
    if c.validate().is_err(){return Vec::new();}let Ok(roots)=host_paths(host)else{return Vec::new();};let mut paths=BTreeSet::new();
    for root in roots{paths.insert(root.clone());for guard in &c.guards{if let Ok(path)=relative(&root,&guard.path){paths.insert(path);}}}
    if let Ok(Some(audit))=capture(host,c,cache){if audit.revalidate(host).is_ok(){if let Some(managed)=audit.managed{
        paths.insert(managed.path);paths.extend(managed.leaves.into_iter().map(|v|v.path));
    }}}paths.into_iter().collect()
}
pub(super)fn watch_paths(host:&Host)->Vec<PathBuf>{contract().map(|c|watch_paths_contract(host,c,cache())).unwrap_or_default()}
/// Guard-only App version; never invokes the App, child CLI, SDK or config service.
pub(super)fn runtime_version(selected:&Path)->Option<String>{
    let c=contract()?;let host=Host{agent_id:c.agent_id.clone(),adapter:"kilo_desktop".into(),docs_url:String::new(),manifest_paths:Vec::new(),default_mcp_files:Vec::new(),mcp_id_prefix:String::new(),resource_groups:Vec::new(),
        roots:["/Applications/Kilo.app","~/Applications/Kilo.app"].into_iter().map(|p|Root{path:p.into(),layout:"kilo_app".into(),presence:PluginPresence::Bundled,complete_marker:None}).collect()};
    let p=Proof::capture(&host,c,cache(),&mut Budget::default()).ok()??;
    if selected!=p.app.path&&selected!=p.app.path.join("Contents/MacOS/Kilo"){return None;}p.revalidate(&host).ok()?;Some(c.version.clone())
}

#[cfg(test)]
mod tests;
