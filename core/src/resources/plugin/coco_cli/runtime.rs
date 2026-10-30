//! Passive selected Node SEA carrier identity; never runs the client or Node.
use super::super::*;
use std::collections::VecDeque;
use std::io::{Seek,SeekFrom};
use std::sync::Mutex;
const MAX_HASH_BYTES:u64=256*1024*1024;
const MAX_CACHE:usize=32;
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Guard{pub path:String,pub bytes:u64,pub sha256:String}
#[derive(Clone,Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Contract{pub version:String,pub carrier:Guard,pub version_file:Guard,pub archive_sha256:String,pub sea_offset:u64,pub sea_bytes:u64,pub sea_header_bytes:usize,pub sea_header_sha256:String}
fn digest(v:&str)->bool{v.len()==64&&v.bytes().all(|b|b.is_ascii_digit()||matches!(b,b'a'..=b'f'))}
impl Contract{
 pub fn validate(&self)->Result<(),()>{
  if self.carrier.path!="cortex"||self.version_file.path!="cortex-version.txt"||self.version.is_empty()||self.version.len()>100||!self.version.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'+'|b'-'|b'_'))
   ||!digest(&self.archive_sha256)||!digest(&self.sea_header_sha256)||self.sea_header_bytes<25||self.sea_header_bytes>4096||self.sea_bytes<self.sea_header_bytes as u64||self.sea_offset.checked_add(self.sea_bytes).is_none_or(|v|v>self.carrier.bytes){return Err(());}
  let total=self.carrier.bytes.checked_add(self.version_file.bytes).ok_or(())?;
  if total>MAX_HASH_BYTES||[&self.carrier,&self.version_file].iter().any(|g|g.bytes==0||!digest(&g.sha256))||self.version_file.bytes!=self.version.len() as u64+1||hex::encode(Sha256::digest(format!("{}\n",self.version).as_bytes()))!=self.version_file.sha256{return Err(());}
  Ok(())
 }
}
fn absolute(path:&Path)->Result<(),()>{
 if !path.is_absolute()||path.components().count()>64||path.to_str().is_none_or(|s|s.len()>4096||s.chars().any(char::is_control))||path.components().any(|c|matches!(c,Component::ParentDir|Component::Prefix(_))){return Err(());}Ok(())
}
fn environment()->Result<(),()>{if ["COCO_DIST_DIR","NODE_OPTIONS","NODE_PATH"].iter().any(|k|std::env::var_os(k).is_some_and(|v|!v.is_empty())){Err(())}else{Ok(())}}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct Stamp { dev:u64, ino:u64, bytes:u64, mode:u32, links:u64, mtime:i64, mtime_ns:i64, ctime:i64, ctime_ns:i64 }
impl Stamp {
    fn capture(value:&fs::Metadata)->Result<Self,()> {
        #[cfg(unix)] { use std::os::unix::fs::MetadataExt;
            Ok(Self {dev:value.dev(),ino:value.ino(),bytes:value.len(),mode:value.mode(),links:value.nlink(),
                mtime:value.mtime(),mtime_ns:value.mtime_nsec(),ctime:value.ctime(),ctime_ns:value.ctime_nsec()}) }
        #[cfg(not(unix))] { let _=value; Err(()) }
    }
}
pub(super) struct Bound { path:PathBuf, file:fs::File, stamp:Stamp, directory:bool }
impl Bound {
    pub fn capture(path:PathBuf,directory:bool)->Result<Self,()> {
        absolute(&path)?;checked_root(&path)?;let m=fs::symlink_metadata(&path).map_err(|_|())?;
        if if directory{!m.is_dir()}else{!m.file_type().is_file()}{return Err(());}
        let stamp=Stamp::capture(&m)?;
        if !directory&&stamp.links!=1{return Err(());}
        // The shared relative opener intentionally rejects an empty tail.
        // Snapshot ancestor anchors also include HOME and / themselves.
        let file=if directory&&(path==expand_tilde("~/")||path==Path::new("/")){
            #[cfg(unix)] {use rustix::fs::{open,Mode,OFlags};
                fs::File::from(open(&path,OFlags::RDONLY|OFlags::DIRECTORY|OFlags::NOFOLLOW|OFlags::CLOEXEC,Mode::empty()).map_err(|_|())?)}
            #[cfg(not(unix))] {return Err(());}
        }else{open_confined(&path,directory)?};
        let value=Self{path,file,stamp,directory};value.revalidate()?;Ok(value)
    }
    pub fn revalidate(&self)->Result<(),()> {
        checked_root(&self.path)?;let m=fs::symlink_metadata(&self.path).map_err(|_|())?;
        if if self.directory{!m.is_dir()}else{!m.file_type().is_file()}{return Err(());}
        if Stamp::capture(&m)?!=self.stamp||Stamp::capture(&self.file.metadata().map_err(|_|())?)?!=self.stamp {return Err(());}
        Ok(())
    }
    pub fn size(&self)->u64 {self.stamp.bytes}
    pub fn fingerprint(&self)->Result<String,()> {
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(&self.path,&self.stamp,self.directory)).map_err(|_|())?)))
    }
    pub fn read_small(&mut self,limit:usize)->Result<Vec<u8>,()> {
        if self.directory||limit>2*1024*1024||self.stamp.bytes>limit as u64{return Err(());}
        self.revalidate()?;self.file.seek(SeekFrom::Start(0)).map_err(|_|())?;
        let mut value=Vec::with_capacity(self.stamp.bytes as usize);(&mut self.file).take(limit as u64+1).read_to_end(&mut value).map_err(|_|())?;
        self.revalidate()?;
        if value.len() as u64!=self.stamp.bytes||value.len()>limit{return Err(());}
        Ok(value)
    }
}
struct Selection { alias:PathBuf, physical:PathBuf, stamp:Stamp, link:Option<PathBuf> }
impl Selection {
    fn capture(alias:&Path)->Result<Self,()> {
        environment()?;absolute(alias)?;checked_root(alias.parent().ok_or(())?)?;
        let m=fs::symlink_metadata(alias).map_err(|_|())?;
        if !m.file_type().is_file()&&!m.file_type().is_symlink(){return Err(());}
        let stamp=Stamp::capture(&m)?;
        let link=if m.file_type().is_symlink(){Some(fs::read_link(alias).map_err(|_|())?)}else{None};
        let physical=fs::canonicalize(alias).map_err(|_|())?;absolute(&physical)?;checked_root(&physical)?;
        let value=Self{alias:alias.into(),physical,stamp,link};value.revalidate()?;Ok(value)
    }
    fn revalidate(&self)->Result<(),()> {
        environment()?;checked_root(self.alias.parent().ok_or(())?)?;
        let m=fs::symlink_metadata(&self.alias).map_err(|_|())?;
        if Stamp::capture(&m)?!=self.stamp||fs::canonicalize(&self.alias).map_err(|_|())?!=self.physical{return Err(());}
        match &self.link {
            Some(link) if m.file_type().is_symlink()&&fs::read_link(&self.alias).map_err(|_|())?==*link=>{},
            None if m.file_type().is_file()=>{}, _=>return Err(()),
        }
        checked_root(&self.physical)
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Key { context:String, path:PathBuf, stamp:Stamp, expected:String }
#[derive(Default)]
struct Cache { entries:VecDeque<Key> }
impl Cache {
    fn contains(&mut self,key:&Key)->bool {
        let Some(i)=self.entries.iter().position(|v|v==key)else{return false;};
        let value=self.entries.remove(i).expect("located cache entry");self.entries.push_back(value);true
    }
    fn insert(&mut self,key:Key) {
        if self.contains(&key){return;}if self.entries.len()>=MAX_CACHE{self.entries.pop_front();}self.entries.push_back(key);
    }
}
fn cache()->&'static Mutex<Cache> {static C:OnceLock<Mutex<Cache>>=OnceLock::new();C.get_or_init(Mutex::default)}

pub(super) struct Proof{selection:Selection,bound:Vec<Bound>,identity:String,version:String}
impl Proof{
 pub fn capture(selected:&Path,c:&Contract)->Result<Self,()>{
  c.validate()?;
  #[cfg(not(all(target_os="macos",target_arch="aarch64")))]{let _=selected;return Err(());}
  let selection=Selection::capture(selected)?;let physical=selection.physical.clone();
  let parent=physical.parent().ok_or(())?;let alias_parent=selection.alias.parent().ok_or(())?;
  let bound=vec![Bound::capture(physical.clone(),false)?,Bound::capture(parent.join(&c.version_file.path),false)?,Bound::capture(parent.into(),true)?,Bound::capture(alias_parent.into(),true)?];
  #[cfg(unix)]if bound[0].stamp.mode&0o111==0{return Err(());}
  let mut p=Self{selection,bound,identity:String::new(),version:c.version.clone()};
  p.identity=hex::encode(Sha256::digest(serde_json::to_vec(&(c,&p.selection.alias,&p.selection.physical,&p.selection.stamp,&p.selection.link,p.bound.iter().map(|v|(&v.path,&v.stamp)).collect::<Vec<_>>())).map_err(|_|())?));
  let mut total=0u64;
  for (i,g) in [(0,&c.carrier),(1,&c.version_file)]{
   p.revalidate()?;let f=&mut p.bound[i];if f.stamp.bytes!=g.bytes{return Err(());}
   let key=Key{context:p.identity.clone(),path:f.path.clone(),stamp:f.stamp.clone(),expected:g.sha256.clone()};
   if !cache().lock().map_err(|_|())?.contains(&key){
    if g.bytes>MAX_HASH_BYTES.saturating_sub(total){return Err(());}f.file.seek(SeekFrom::Start(0)).map_err(|_|())?;
    let mut h=Sha256::new();let mut read=0u64;let mut buffer=[0u8;65536];
    loop{let n=(&mut f.file).take(g.bytes.saturating_sub(read)+1).read(&mut buffer).map_err(|_|())?;if n==0{break;}read+=n as u64;total+=n as u64;if read>g.bytes||total>MAX_HASH_BYTES{return Err(());}h.update(&buffer[..n]);}
    if read!=g.bytes||hex::encode(h.finalize())!=g.sha256{return Err(());}p.revalidate()?;cache().lock().map_err(|_|())?.insert(key);
   }
  }
  let header=p.read_metadata(c.sea_offset,c.sea_header_bytes)?;
  if hex::encode(Sha256::digest(&header))!=c.sea_header_sha256||header.get(..4)!=Some(0x143da20u32.to_le_bytes().as_slice())||header.get(4..8)!=Some(1u32.to_le_bytes().as_slice())||header.get(8)!=Some(&1){return Err(());}
  p.revalidate()?;Ok(p)
 }
 pub fn revalidate(&self)->Result<(),()>{self.selection.revalidate()?;for f in &self.bound{f.revalidate()?;}self.selection.revalidate()}
 pub fn carrier_path(&self)->&Path{&self.bound[0].path}
 pub fn base(&self)->&Path{self.carrier_path().parent().expect("bound parent")}
 pub fn identity(&self)->&str{&self.identity}
 pub fn version(&self)->&str{&self.version}
 pub fn watch_paths(&self)->Vec<PathBuf>{std::iter::once(self.selection.alias.clone()).chain(self.bound.iter().map(|v|v.path.clone())).collect()}
 pub fn read_metadata(&mut self,offset:u64,count:usize)->Result<Vec<u8>,()>{
  if count>65536||offset.checked_add(count as u64).is_none_or(|v|v>self.bound[0].stamp.bytes){return Err(());}
  self.revalidate()?;let f=&mut self.bound[0].file;f.seek(SeekFrom::Start(offset)).map_err(|_|())?;let mut bytes=vec![0;count];f.read_exact(&mut bytes).map_err(|_|())?;self.revalidate()?;Ok(bytes)
 }
}
