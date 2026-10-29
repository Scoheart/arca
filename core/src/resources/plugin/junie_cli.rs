//! Junie CLI 3893.6: source-bound passive version and global extension metadata.
//! Never executes the shim/JVM, native services, migration/factory/hooks or body
//! parsers. JAR Skills are fixed ZIP metadata bound by a whole-container digest.
use super::*;
use std::collections::VecDeque;
use std::io::{Seek,SeekFrom};
use std::sync::Mutex;

const MAX_HASH_BYTES:u64=256*1024*1024;
const MAX_METADATA_BYTES:usize=8*1024*1024;
const MAX_DOCUMENT_BYTES:usize=2*1024*1024;
const MAX_VISITS:usize=8192;
const MAX_BOUND:usize=512;
const MAX_ITEMS:usize=4096;
const MAX_PLUGINS:usize=512;
const MAX_DIRECTORY:usize=1024;
const MAX_STAT_BYTES:u64=16*1024*1024;
const MAX_STAT_TOTAL:u64=64*1024*1024;
const MAX_CACHE:usize=32;
const APP:&str="Applications/junie.app";
const GROUP:&str="@builtin/skills";
const ENV_GATES:&[&str]=&["JUNIE_DATA","JUNIE_VERSION","JUNIE_HOME","JUNIE_INSTALL_BASE_URL",
    "JAVA_TOOL_OPTIONS","JDK_JAVA_OPTIONS","_JAVA_OPTIONS"];

#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Guard {path:String,bytes:u64,sha256:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ZipAudit {entries:u64,central_offset:u64,central_bytes:u64,eocd_offset:u64}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct BundledSkill {name:String,entry:String,bytes:u64,compressed_bytes:u64,crc32:u32,header_offset:u64}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {contract_sha256:String,archive_sha256:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    schema_version:u32,version:String,bundle_id:String,bundle_name:String,main_class:String,
    shim:Guard,guards:Vec<Guard>,zip:ZipAudit,skills:Vec<BundledSkill>,evidence:Evidence,
}
fn hash_text(value:&str)->bool {value.len()==64&&value.bytes().all(|c|c.is_ascii_hexdigit()&&!c.is_ascii_uppercase())}
fn native_text(value:&str)->Result<String,()> {
    if value.is_empty()||value.len()>240||value.chars().all(char::is_whitespace)||value.chars().any(char::is_control){Err(())}else{Ok(value.into())}
}
fn segment(value:&str)->Result<(),()> {
    native_text(value)?;
    if matches!(value,"."|"..")||value.contains(['/', '\\'])||Path::new(value).components().any(|p|!matches!(p,Component::Normal(_))){Err(())}else{Ok(())}
}
fn relative_member(root:&Path,input:&str)->Result<PathBuf,()> {
    if input.is_empty()||input.len()>4096||input.contains('\\')||input.chars().any(char::is_control){return Err(());}
    relative(root,input)
}
impl Contract {
    fn jar_path(&self)->String {format!("{APP}/Contents/app/junie-release-{}-macos-aarch64.jar",self.version)}
    fn validate(&self)->Result<(),()> {
        let parts=self.version.split('.').collect::<Vec<_>>();
        if self.schema_version!=1||parts.len()!=2||parts.iter().any(|v|v.parse::<u32>().is_err())
            ||self.bundle_id!="com.intellij.ml.llm.matterhorn.ej.app.cli.standalone"||self.bundle_name!="junie"
            ||self.main_class!="com.intellij.ml.llm.matterhorn.ej.app.cli.standalone.MainKt"
            ||self.shim.path!="shim/junie.sh"||self.shim.bytes==0||self.shim.bytes>131072||!hash_text(&self.shim.sha256)
            ||!hash_text(&self.evidence.contract_sha256)||!hash_text(&self.evidence.archive_sha256){return Err(());}
        let required=BTreeSet::from(["channel".to_string(),"junie".into(),format!("{APP}/Contents/Info.plist"),
            format!("{APP}/Contents/MacOS/junie"),format!("{APP}/Contents/app/junie.cfg"),
            format!("{APP}/Contents/app/.jpackage.xml"),self.jar_path()]);
        if self.guards.len()!=required.len(){return Err(());}
        let mut paths=BTreeSet::new();let mut total=self.shim.bytes;
        for guard in &self.guards {
            if !required.contains(&guard.path)||!paths.insert(&guard.path)||guard.bytes==0||!hash_text(&guard.sha256){return Err(());}
            total=total.checked_add(guard.bytes).ok_or(())?;
        }
        if total>MAX_HASH_BYTES{return Err(());}
        let jar=self.guards.iter().find(|v|v.path==self.jar_path()).ok_or(())?;
        if self.zip.entries<2||self.zip.entries>100000||self.zip.central_offset.checked_add(self.zip.central_bytes).ok_or(())?>self.zip.eocd_offset
            ||self.zip.eocd_offset>=jar.bytes||self.skills.len()!=2{return Err(());}
        let mut names=BTreeSet::new();
        for skill in &self.skills {
            if !["demo-setup","session-history"].contains(&skill.name.as_str())||!names.insert(&skill.name)
                ||skill.entry!=format!("agent-skills/{}/SKILL.md",skill.name)||skill.bytes==0||skill.bytes>256*1024
                ||skill.compressed_bytes>jar.bytes||skill.header_offset>=jar.bytes{return Err(());}
        }
        Ok(())
    }
}
fn contract()->Option<&'static Contract> {
    static C:OnceLock<Option<Contract>>=OnceLock::new();
    C.get_or_init(||serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-junie-cli.json")).ok().filter(|c|c.validate().is_ok())).as_ref()
}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
struct Stamp {dev:u64,ino:u64,bytes:u64,mode:u32,links:u64,mtime:i64,mtime_ns:i64,ctime:i64,ctime_ns:i64}
impl Stamp {
    fn capture(metadata:&fs::Metadata)->Result<Self,()> {
        #[cfg(unix)] {use std::os::unix::fs::MetadataExt;
            Ok(Self{dev:metadata.dev(),ino:metadata.ino(),bytes:metadata.len(),mode:metadata.mode(),links:metadata.nlink(),
                mtime:metadata.mtime(),mtime_ns:metadata.mtime_nsec(),ctime:metadata.ctime(),ctime_ns:metadata.ctime_nsec()})}
        #[cfg(not(unix))] {let _=metadata;Err(())}
    }
}
#[derive(Default)]
struct Budget {visited:usize,read_bytes:usize,hashed_bytes:u64,components:usize,stat_bytes:u64}
impl Budget {
    fn visit(&mut self)->Result<(),()> {self.visited=self.visited.checked_add(1).ok_or(())?;if self.visited>MAX_VISITS{Err(())}else{Ok(())}}
    fn component(&mut self)->Result<(),()> {self.visit()?;self.components+=1;if self.components>MAX_ITEMS{Err(())}else{Ok(())}}
    fn read(&mut self,n:usize)->Result<(),()> {self.read_bytes=self.read_bytes.checked_add(n).ok_or(())?;if self.read_bytes>MAX_METADATA_BYTES{Err(())}else{Ok(())}}
}
struct Bound {path:PathBuf,file:fs::File,stamp:Stamp,directory:bool}
impl Bound {
    fn capture(path:PathBuf,directory:bool,budget:&mut Budget)->Result<Self,()> {
        budget.visit()?;checked_root(&path)?;let metadata=fs::symlink_metadata(&path).map_err(|_|())?;
        if if directory{!metadata.is_dir()}else{!metadata.file_type().is_file()}{return Err(());}
        let stamp=Stamp::capture(&metadata)?;if !directory&&stamp.links!=1{return Err(());}
        let file=open_confined(&path,directory)?;let value=Self{path,file,stamp,directory};value.revalidate()?;Ok(value)
    }
    fn revalidate(&self)->Result<(),()> {
        checked_root(&self.path)?;let m=fs::symlink_metadata(&self.path).map_err(|_|())?;
        if if self.directory{!m.is_dir()}else{!m.file_type().is_file()}{return Err(());}
        if Stamp::capture(&m)?!=self.stamp||Stamp::capture(&self.file.metadata().map_err(|_|())?)?!=self.stamp{Err(())}else{Ok(())}
    }
    fn bytes(&mut self,limit:usize,budget:&mut Budget)->Result<Vec<u8>,()> {
        if self.directory||self.stamp.bytes>limit as u64{return Err(());}
        self.revalidate()?;budget.read(self.stamp.bytes as usize)?;self.file.seek(SeekFrom::Start(0)).map_err(|_|())?;
        let mut bytes=Vec::with_capacity(self.stamp.bytes as usize);(&mut self.file).take(limit as u64+1).read_to_end(&mut bytes).map_err(|_|())?;
        self.revalidate()?;if bytes.len()as u64!=self.stamp.bytes||bytes.len()>limit{return Err(());}Ok(bytes)
    }
}
#[derive(Clone,PartialEq,Eq)]
struct DigestKey {context:String,path:PathBuf,stamp:Stamp,expected:String}
#[derive(Default)]
struct DigestCache {entries:VecDeque<DigestKey>}
impl DigestCache {
    fn contains(&mut self,key:&DigestKey)->bool {let Some(i)=self.entries.iter().position(|v|v==key)else{return false;};let v=self.entries.remove(i).expect("located cache entry");self.entries.push_back(v);true}
    fn insert(&mut self,key:DigestKey) {if self.contains(&key){return;}if self.entries.len()>=MAX_CACHE{self.entries.pop_front();}self.entries.push_back(key);}
}
fn environment()->Result<(),()> {
    if ENV_GATES.iter().any(|key|std::env::var_os(key).is_some_and(|v|!v.is_empty())){return Err(());}
    if std::env::var_os("PATH").is_some_and(|v|v.len()>16384||std::env::split_paths(&v).count()>128){return Err(());}
    Ok(())
}
fn selected_path()->Result<Option<PathBuf>,()> {environment()?;Ok(crate::application::agent_launch::find_command("junie"))}
struct Current {path:PathBuf,target:PathBuf,stamp:Stamp}
impl Current {
    fn capture(data:&Path,version:&Path)->Result<Self,()> {
        checked_root(data)?;let path=data.join("current");let m=fs::symlink_metadata(&path).map_err(|_|())?;
        if !m.file_type().is_symlink(){return Err(());}
        let target=fs::read_link(&path).map_err(|_|())?;
        if target.to_str().is_none_or(|v|v.len()>4096||v.chars().any(char::is_control))||target.components().any(|p|matches!(p,Component::ParentDir|Component::Prefix(_))){return Err(());}
        let physical=if target.is_absolute(){target.clone()}else{data.join(&target)};
        if physical!=version{return Err(());}
        let value=Self{path,target,stamp:Stamp::capture(&m)?};value.revalidate()?;Ok(value)
    }
    fn revalidate(&self)->Result<(),()> {
        checked_root(self.path.parent().ok_or(())?)?;let m=fs::symlink_metadata(&self.path).map_err(|_|())?;
        if !m.file_type().is_symlink()||Stamp::capture(&m)?!=self.stamp||fs::read_link(&self.path).map_err(|_|())?!=self.target{Err(())}else{Ok(())}
    }
}
struct Verified {alias:PathBuf,version:String,current:Current,directories:Vec<Bound>,members:Vec<Bound>,version_root:PathBuf,jar:PathBuf,identity:String}
impl Verified {
    fn capture(selected:&Path,c:&Contract,cache:&Mutex<DigestCache>,budget:&mut Budget)->Result<Self,()> {
        c.validate()?;environment()?;
        let alias=expand_tilde("~/.local/bin/junie");
        if selected!=alias||selected_path()?.as_ref()!=Some(&alias){return Err(());}
        let data=expand_tilde("~/.local/share/junie");let version_root=data.join("versions").join(&c.version);
        let directories=[data.clone(),data.join("versions"),version_root.clone(),version_root.join(APP)].into_iter()
            .map(|p|Bound::capture(p,true,budget)).collect::<Result<Vec<_>,_>>()?;
        let current=Current::capture(&data,&version_root)?;
        let shim=Bound::capture(alias.clone(),false,budget)?;
        if shim.stamp.mode&0o111==0{return Err(());}
        let context=hex::encode(Sha256::digest(serde_json::to_vec(&(&alias,&shim.stamp,&current.stamp,&current.target,
            directories.iter().map(|v|(&v.path,&v.stamp)).collect::<Vec<_>>())).map_err(|_|())?));
        let mut proof=Self{alias,version:c.version.clone(),current,directories,members:Vec::new(),version_root:version_root.clone(),jar:version_root.join(c.jar_path()),identity:String::new()};
        proof.verify_member(shim,&c.shim,&context,cache,budget)?;
        for guard in &c.guards {let bound=Bound::capture(relative_member(&version_root,&guard.path)?,false,budget)?;proof.verify_member(bound,guard,&context,cache,budget)?;}
        let info:plist::Value=plist::Value::from_reader(std::io::Cursor::new(proof.small(&format!("{APP}/Contents/Info.plist"),budget)?)).map_err(|_|())?;
        let info=info.as_dictionary().ok_or(())?;
        for(k,v)in[("CFBundleIdentifier",&c.bundle_id),("CFBundleName",&c.bundle_name),("CFBundleExecutable",&c.bundle_name),
            ("CFBundleVersion",&c.version),("CFBundleShortVersionString",&c.version)] {
            if info.get(k).and_then(plist::Value::as_string)!=Some(v.as_str()){return Err(());}
        }
        let cfg=String::from_utf8(proof.small(&format!("{APP}/Contents/app/junie.cfg"),budget)?).map_err(|_|())?;
        for expected in [format!("app.classpath=$APPDIR/junie-release-{}-macos-aarch64.jar",c.version),format!("app.mainclass={}",c.main_class),format!("java-options=-Djpackage.app-version={}",c.version)] {
            if !cfg.lines().any(|line|line==expected){return Err(());}
        }
        if proof.small("channel",budget)?!=b"release\n"{return Err(());}
        proof.identity=hex::encode(Sha256::digest(serde_json::to_vec(&(context,c,
            proof.members.iter().map(|m|(&m.path,&m.stamp)).collect::<Vec<_>>())).map_err(|_|())?));
        proof.revalidate()?;Ok(proof)
    }
    fn verify_member(&mut self,mut file:Bound,g:&Guard,context:&str,cache:&Mutex<DigestCache>,budget:&mut Budget)->Result<(),()> {
        if file.stamp.bytes!=g.bytes{return Err(());}
        let key=DigestKey{context:context.into(),path:file.path.clone(),stamp:file.stamp.clone(),expected:g.sha256.clone()};
        let cached=cache.lock().map_err(|_|())?.contains(&key);
        if !cached {
            if g.bytes>MAX_HASH_BYTES.saturating_sub(budget.hashed_bytes){return Err(());}
            let mut hash=Sha256::new();let mut count=0u64;let mut buffer=[0;65536];
            loop {let n=(&mut file.file).take(g.bytes.saturating_sub(count)+1).read(&mut buffer).map_err(|_|())?;if n==0{break;}
                count+=n as u64;budget.hashed_bytes+=n as u64;if count>g.bytes||budget.hashed_bytes>MAX_HASH_BYTES{return Err(());}hash.update(&buffer[..n]);}
            if count!=g.bytes||hex::encode(hash.finalize())!=g.sha256{return Err(());}
        }
        file.revalidate()?;self.current.revalidate()?;for d in &self.directories{d.revalidate()?;}
        if !cached{cache.lock().map_err(|_|())?.insert(key);}self.members.push(file);Ok(())
    }
    fn small(&mut self,relative:&str,budget:&mut Budget)->Result<Vec<u8>,()> {
        let path=self.version_root.join(relative);self.members.iter_mut().find(|v|v.path==path).ok_or(())?.bytes(MAX_DOCUMENT_BYTES,budget)
    }
    fn revalidate(&self)->Result<(),()> {
        environment()?;if selected_path()?.as_ref()!=Some(&self.alias){return Err(());}
        self.current.revalidate()?;for d in &self.directories{d.revalidate()?;}for m in &self.members{m.revalidate()?;}self.current.revalidate()
    }
    fn sources(&self)->Vec<String> {let mut v=self.members.iter().map(|v|collapse_home(&v.path)).collect::<Vec<_>>();v.push(collapse_home(&self.current.path));v}
}

#[derive(Clone)]
struct Entry {path:PathBuf,directory:bool,file:bool}
struct Leaf {path:PathBuf,parent:usize,stamp:Stamp}
#[derive(Default)]
struct Snapshot {plugins:Vec<NativePluginObservation>,bound:Vec<Bound>,leaves:Vec<Leaf>,missing:BTreeSet<PathBuf>,
    entries:BTreeMap<PathBuf,Vec<Entry>>,watches:BTreeSet<PathBuf>,incomplete:bool}
impl Snapshot {
    fn bind(&mut self,path:&Path,directory:bool,b:&mut Budget)->Result<Option<usize>,()> {
        b.visit()?;checked_root(path)?;self.watches.insert(path.into());
        if let Some(i)=self.bound.iter().position(|v|v.path==path&&v.directory==directory){self.bound[i].revalidate()?;return Ok(Some(i));}
        match fs::symlink_metadata(path) {Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{self.missing.insert(path.into());return Ok(None);},Err(_)=>return Err(()),_=>{}}
        if self.bound.len()>=MAX_BOUND{return Err(());}self.bound.push(Bound::capture(path.into(),directory,b)?);Ok(Some(self.bound.len()-1))
    }
    fn document(&mut self,path:&Path,jsonc:bool,b:&mut Budget)->Result<Option<Value>,()> {
        if path.extension().and_then(|v|v.to_str())!=Some("json"){return Err(());}
        let Some(i)=self.bind(path,false,b)?else{return Ok(None);};
        let limit=MAX_DOCUMENT_BYTES.min(MAX_METADATA_BYTES.saturating_sub(b.read_bytes));let bytes=self.bound[i].bytes(limit,b)?;
        let text=std::str::from_utf8(&bytes).map_err(|_|())?;
        let value=if jsonc {check_jsonc_depth(text)?;let options=jsonc_parser::ParseOptions{allow_comments:true,allow_trailing_commas:true,
            allow_loose_object_property_names:false,allow_missing_commas:false,allow_single_quoted_strings:false,allow_hexadecimal_numbers:false,allow_unary_plus_numbers:false};
            jsonc_parser::parse_to_ast(text,&Default::default(),&options).map_err(|_|())?.value.ok_or(())?.into()
        }else{serde_json::from_str(text).map_err(|_|())?};Ok(Some(value))
    }
    fn list(&mut self,path:&Path,b:&mut Budget)->Result<Vec<Entry>,()> {
        let Some(index)=self.bind(path,true,b)?else{return Ok(Vec::new());};
        if let Some(v)=self.entries.get(path){return Ok(v.clone());}
        let mut rows=Vec::new();
        #[cfg(unix)] {
            use std::os::unix::ffi::OsStrExt;use rustix::fs::{AtFlags,Dir,FileType,statat};
            let file=open_confined(path,true)?;if Stamp::capture(&file.metadata().map_err(|_|())?)?!=self.bound[index].stamp{return Err(());}
            for entry in Dir::read_from(&file).map_err(|_|())? {
                let entry=entry.map_err(|_|())?;let name=entry.file_name();if matches!(name.to_bytes(),b"."|b".."){continue;}
                b.visit()?;if rows.len()>=MAX_DIRECTORY{return Err(());}let label=std::str::from_utf8(name.to_bytes()).map_err(|_|())?;native_text(label)?;
                let kind=match entry.file_type(){FileType::Unknown=>FileType::from_raw_mode(statat(&file,name,AtFlags::SYMLINK_NOFOLLOW).map_err(|_|())?.st_mode),v=>v};
                if kind==FileType::Symlink{self.incomplete=true;continue;}
                rows.push(Entry{path:path.join(label),directory:kind==FileType::Directory,file:kind==FileType::RegularFile});
            }
        }
        #[cfg(not(unix))] {let _=index;return Err(());}
        self.bound[index].revalidate()?;rows.sort_by(|a,b|a.path.cmp(&b.path));self.entries.insert(path.into(),rows.clone());Ok(rows)
    }
    fn stat(&mut self,path:&Path,b:&mut Budget)->Result<Option<Stamp>,()> {
        b.visit()?;checked_root(path)?;self.watches.insert(path.into());
        if let Some(v)=self.leaves.iter().find(|v|v.path==path){return Ok(Some(v.stamp.clone()));}
        let Some(parent)=self.bind(path.parent().ok_or(())?,true,b)?else{self.missing.insert(path.into());return Ok(None);};
        let Some(stamp)=stat_at(&self.bound[parent],path.file_name().ok_or(())?)?else{self.missing.insert(path.into());return Ok(None);};
        b.stat_bytes=b.stat_bytes.checked_add(stamp.bytes).ok_or(())?;
        if stamp.bytes>MAX_STAT_BYTES||b.stat_bytes>MAX_STAT_TOTAL||self.leaves.len()>=MAX_ITEMS{return Err(());}
        self.leaves.push(Leaf{path:path.into(),parent,stamp:stamp.clone()});Ok(Some(stamp))
    }
    fn revalidate(&self)->Result<(),()> {
        for v in &self.bound{v.revalidate()?;}
        for v in &self.leaves{if stat_at(self.bound.get(v.parent).ok_or(())?,v.path.file_name().ok_or(())?)?.as_ref()!=Some(&v.stamp){return Err(());}}
        for path in &self.missing{checked_root(path)?;match fs::symlink_metadata(path){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},_=>return Err(())}}
        for v in &self.bound{v.revalidate()?;}Ok(())
    }
    fn availability(&self,root:Option<&Path>,ledger:&Path)->Result<String,()> {
        let files=self.bound.iter().filter(|v|v.path==ledger||root.is_some_and(|p|v.path.starts_with(p))).map(|v|(&v.path,&v.stamp)).collect::<Vec<_>>();
        let leaves=self.leaves.iter().filter(|v|root.is_some_and(|p|v.path.starts_with(p))).map(|v|(&v.path,&v.stamp)).collect::<Vec<_>>();
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(files,leaves)).map_err(|_|())?)))
    }
}
fn stat_at(parent:&Bound,name:&std::ffi::OsStr)->Result<Option<Stamp>,()> {
    #[cfg(any(target_os="macos",target_os="linux"))] {use rustix::fs::{AtFlags,FileType,statat};
        let v=match statat(&parent.file,name,AtFlags::SYMLINK_NOFOLLOW){Ok(v)=>v,Err(e)if e==rustix::io::Errno::NOENT=>return Ok(None),_=>return Err(())};
        if FileType::from_raw_mode(v.st_mode)!=FileType::RegularFile{return Err(());}
        Ok(Some(Stamp{dev:v.st_dev as u64,ino:v.st_ino as u64,bytes:u64::try_from(v.st_size).map_err(|_|())?,mode:v.st_mode as u32,links:v.st_nlink as u64,
            mtime:v.st_mtime as i64,mtime_ns:v.st_mtime_nsec as i64,ctime:v.st_ctime as i64,ctime_ns:v.st_ctime_nsec as i64}))}
    #[cfg(not(any(target_os="macos",target_os="linux")))] {let _=(parent,name);Err(())}
}
fn component(items:&mut Vec<PluginComponent>,kind:PluginComponentKind,name:String,path:String,id:Option<String>,b:&mut Budget)->Result<(),()> {
    b.component()?;native_text(&name)?;items.push(PluginComponent{kind,name,source_path:path,native_id:id});Ok(())
}
fn observation(host:&Host,proof:&Verified,id:String,name:String,presence:PluginPresence,mut sources:Vec<String>,mut components:Vec<PluginComponent>,context:&str,version:String)->Result<NativePluginObservation,()> {
    components.sort();components.dedup();sources.sort();sources.dedup();
    let digest=hex::encode(Sha256::digest(serde_json::to_vec(&(&id,&sources,&components,context)).map_err(|_|())?));
    Ok(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id,name,version,revision:format!("runtime:{}; identity-sha256:{}; {context}; metadata-sha256:{digest}",proof.version,proof.identity),presence,enabled:None,source_paths:sources,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()})
}
fn builtins(host:&Host,c:&Contract,proof:&Verified,s:&mut Snapshot,b:&mut Budget)->Result<(),()> {
    let mut components=Vec::new();
    for skill in &c.skills {component(&mut components,PluginComponentKind::Skill,skill.name.clone(),
        format!("{}!/{}",collapse_home(&proof.jar),skill.entry),None,b)?;}
    s.plugins.push(observation(host,proof,GROUP.into(),"Junie bundled Skills".into(),PluginPresence::Bundled,proof.sources(),components,
        "application-owned; whole-JAR-bound ZIP metadata; body/frontmatter/activation=unknown",c.version.clone())?);Ok(())
}
struct Record {market:String,name:String}
fn string_or_null(value:Option<&Value>)->bool {value.is_none_or(|v|v.is_null()||v.is_string())}
fn records(value:&Value)->Result<Vec<Record>,()> {
    let markets=value.as_object().filter(|v|v.len()<=128).ok_or(())?;let mut out=Vec::new();
    for(market,value)in markets {
        native_text(market)?;let row=value.as_object().ok_or(())?;
        if !string_or_null(row.get("url")){return Err(());}
        if let Some(source)=row.get("source").filter(|v|!v.is_null()) {
            let source=source.as_object().ok_or(())?;
            match source.get("type").and_then(Value::as_str) {
                Some("git")=>if !source.get("url").is_some_and(Value::is_string)||!string_or_null(source.get("ref")){return Err(());},
                Some("local")=>if !source.get("path").is_some_and(Value::is_string){return Err(());},
                Some("url")=>if !source.get("url").is_some_and(Value::is_string){return Err(());},_=>return Err(()),
            }
        }
        if let Some(extensions)=row.get("extensions") {
            let extensions=extensions.as_array().filter(|v|v.len()<=MAX_PLUGINS).ok_or(())?;
            for name in extensions {let name=name.as_str().ok_or(())?;native_text(name)?;if out.len()>=MAX_PLUGINS{return Err(());}
                out.push(Record{market:market.clone(),name:name.into()});}
        }
    }
    Ok(out)
}
fn file_availability(root:&Path,subdir:&str,kind:PluginComponentKind,items:&mut Vec<PluginComponent>,s:&mut Snapshot,b:&mut Budget)->Result<(),()> {
    for entry in s.list(&root.join(subdir),b)? {
        if !entry.file||entry.path.extension().and_then(|v|v.to_str())!=Some("md"){continue;}
        if s.stat(&entry.path,b)?.is_some(){let filename=entry.path.file_name().and_then(|v|v.to_str()).ok_or(())?;
            component(items,kind.clone(),filename.into(),collapse_home(&entry.path),None,b)?;}
    }
    Ok(())
}
fn owned_skills(root:&Path,items:&mut Vec<PluginComponent>,s:&mut Snapshot,b:&mut Budget)->Result<(),()> {
    for entry in s.list(&root.join("skills"),b)? {
        if !entry.directory{continue;}let leaf=entry.path.join("SKILL.md");
        if s.stat(&leaf,b)?.is_some(){let name=entry.path.file_name().and_then(|v|v.to_str()).ok_or(())?;
            component(items,PluginComponentKind::Skill,name.into(),collapse_home(&leaf),None,b)?;}
    }
    Ok(())
}
fn mcp_names(value:&Value)->Result<Vec<String>,()> {
    let root=value.as_object().ok_or(())?;let map=if let Some(v)=root.get("mcpServers"){v.as_object().ok_or(())?}else{root};
    if map.len()>MAX_PLUGINS{return Err(());}let mut out=Vec::new();
    for(name,value)in map {
        native_text(name)?;let v=value.as_object().ok_or(())?;
        for key in ["command","url"]{if !string_or_null(v.get(key)){return Err(());}}
        if let Some(args)=v.get("args").filter(|v|!v.is_null()){if !args.as_array().is_some_and(|v|v.len()<=MAX_ITEMS&&v.iter().all(|v|v.is_string()||v.is_null())){return Err(());}}
        for key in ["env","headers"] {if let Some(map)=v.get(key).filter(|v|!v.is_null()){if !map.as_object().is_some_and(|v|v.len()<=MAX_ITEMS&&v.values().all(|v|v.is_string()||v.is_null())){return Err(());}}}
        if v.get("enabled").is_some_and(|v|!v.is_boolean())||v.get("oauth").is_some_and(|v|!v.is_null()&&!v.is_object()){return Err(());}
        // Full OAuth acceptance belongs to the native codec. This projection
        // uses only a declared name/source and never evaluates auth or env.
        if v.get("url").is_some_and(Value::is_string)||v.get("command").is_some_and(Value::is_string){out.push(name.clone());}
    }
    Ok(out)
}
fn owned_mcp(root:&Path,items:&mut Vec<PluginComponent>,s:&mut Snapshot,b:&mut Budget)->Result<(),()> {
    let mut files=s.list(&root.join("mcp"),b)?.into_iter().filter(|v|v.file&&v.path.extension().and_then(|v|v.to_str())==Some("json")).map(|v|v.path).collect::<Vec<_>>();
    files.push(root.join(".mcp.json"));let mut names=BTreeSet::new();
    for file in files {
        match s.document(&file,true,b) {
            Ok(Some(value))=>match mcp_names(&value){Ok(values)=>for name in values{names.insert((name,file.clone()));},Err(())=>s.incomplete=true},
            Ok(None)=>{},Err(())=>s.incomplete=true,
        }
    }
    // Keep each declaration's source, including conflicts. Native Files.list
    // order and the later custom/local/project merge are not observed here;
    // a stable display order must never masquerade as the active winner.
    for(name,path)in names {component(items,PluginComponentKind::McpReference,name.clone(),collapse_home(&path),Some(name),b)?;}Ok(())
}
fn extensions(host:&Host,proof:&Verified,s:&mut Snapshot,b:&mut Budget)->Result<(),()> {
    let root=expand_tilde("~/.junie/extensions");let ledger=root.join("extensions.json");
    let Some(value)=s.document(&ledger,false,b)?else{return Ok(());};let records=records(&value)?;
    for row in records {
        b.visit()?;let id=format!("{}@{}",row.name,row.market);let mut items=Vec::new();let mut sources=vec![collapse_home(&ledger)];let mut observed_root=None;
        let physical=(||{segment(&row.name)?;segment(&row.market)?;let path=relative_member(&root,&format!("{}/{}",row.market,row.name))?;
            s.bind(&path,true,b)?.ok_or(())?;Ok::<_,()>(path)})();
        let presence=match physical {
            Ok(path)=>{
                observed_root=Some(path.clone());
                sources.push(collapse_home(&path));
                for result in [owned_skills(&path,&mut items,s,b),file_availability(&path,"commands",PluginComponentKind::Command,&mut items,s,b),
                    file_availability(&path,"agents",PluginComponentKind::Agent,&mut items,s,b),file_availability(&path,"guidelines",PluginComponentKind::Rule,&mut items,s,b),owned_mcp(&path,&mut items,s,b)]{
                    if result.is_err(){s.incomplete=true;}
                }
                let hook=path.join("hooks/hooks.json");match s.stat(&hook,b){Ok(Some(_))=>component(&mut items,PluginComponentKind::TypeDeclaration,"Hooks file reference (not evaluated)".into(),collapse_home(&hook),None,b)?,Ok(None)=>{},Err(())=>s.incomplete=true}
                PluginPresence::Installed
            },Err(())=>{s.incomplete=true;PluginPresence::Declared},
        };
        // USER list membership+physical cache, not runtime enablement. No
        // plugin.json/frontmatter/version lookup or arbitrary source IO.
        let availability=s.availability(observed_root.as_deref(),&ledger)?;
        s.plugins.push(observation(host,proof,id,row.name,presence,sources,items,
            &format!("user extension membership; file availability; body/frontmatter/activation/project/policy=unknown; plugin-version=unknown; availability:{availability}"),String::new())?);
    }
    Ok(())
}
fn valid_host(host:&Host)->Result<(),()> {
    if host.agent_id!="junie-cli"||host.adapter!="junie_cli"||host.roots.len()!=1{return Err(());}
    let root=&host.roots[0];if root.path!="~/.local/bin/junie"||root.layout!="junie_shim"||root.presence!=PluginPresence::Installed{Err(())}else{Ok(())}
}
struct Audit {proof:Verified,snapshot:Snapshot}
impl Audit {fn revalidate(&self)->Result<(),()> {self.snapshot.revalidate()?;self.proof.revalidate()?;self.snapshot.revalidate()}}
fn capture(host:&Host,c:&Contract,cache:&Mutex<DigestCache>)->Result<Option<Audit>,()> {
    valid_host(host)?;let Some(selected)=selected_path()?else{return Ok(None);};
    let proof=Verified::capture(&selected,c,cache,&mut Budget::default())?;let mut snapshot=Snapshot::default();let mut budget=Budget::default();
    builtins(host,c,&proof,&mut snapshot,&mut budget)?;
    if extensions(host,&proof,&mut snapshot,&mut budget).is_err(){snapshot.incomplete=true;}
    let audit=Audit{proof,snapshot};audit.revalidate()?;Ok(Some(audit))
}
fn observe_contract(host:&Host,c:&Contract,cache:&Mutex<DigestCache>,result:&mut PluginScan)->Result<(),()> {
    let Some(audit)=capture(host,c,cache)?else{return Ok(());};audit.revalidate()?;result.unavailable|=audit.snapshot.incomplete;result.plugins.extend(audit.snapshot.plugins);Ok(())
}
fn cache()->&'static Mutex<DigestCache> {static C:OnceLock<Mutex<DigestCache>>=OnceLock::new();C.get_or_init(Mutex::default)}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {let Some(c)=contract()else{result.unavailable=true;return;};if observe_contract(host,c,cache(),result).is_err(){result.unavailable=true;}}
fn version_contract(selected:&Path,c:&Contract,cache:&Mutex<DigestCache>)->Option<String> {let proof=Verified::capture(selected,c,cache,&mut Budget::default()).ok()?;proof.revalidate().ok()?;Some(proof.version)}
pub(super) fn runtime_version(selected:&Path)->Option<String> {version_contract(selected,contract()?,cache())}
fn watch_contract(host:&Host,c:&Contract,cache:&Mutex<DigestCache>)->Vec<PathBuf> {
    if valid_host(host).is_err()||c.validate().is_err(){return Vec::new();}let mut paths=BTreeSet::new();
    let alias=expand_tilde("~/.local/bin/junie");let data=expand_tilde("~/.local/share/junie");let version=data.join("versions").join(&c.version);
    for path in [alias,data.clone(),data.join("versions"),version.clone()]{if checked_root(&path).is_ok(){paths.insert(path);}}
    for guard in &c.guards{let path=version.join(&guard.path);if checked_root(&path).is_ok(){paths.insert(path);}}
    if environment().is_ok(){let root=expand_tilde("~/.junie/extensions");for path in [root.clone(),root.join("extensions.json")]{if checked_root(&path).is_ok(){paths.insert(path);}}}
    if let Ok(Some(audit))=capture(host,c,cache){if audit.revalidate().is_ok(){paths.insert(audit.proof.current.path);paths.extend(audit.snapshot.watches);}}
    paths.into_iter().collect()
}
pub(super) fn watch_paths(host:&Host)->Vec<PathBuf> {contract().map(|c|watch_contract(host,c,cache())).unwrap_or_default()}

#[cfg(test)]
mod tests;
