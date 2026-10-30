//! Passive identity for the selected Kilo carrier. Never invokes Node, Bun,
//! the wrapper, postinstall, architecture probes or the target version flag.
use super::super::*;
use std::collections::VecDeque;
use std::io::{Seek, SeekFrom};
use std::sync::Mutex;

const MAX_HASH_BYTES: u64 = 512 * 1024 * 1024;
const MAX_FILES: usize = 256;
const MAX_CACHE: usize = 32;

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Guard { pub path: String, pub bytes: u64, pub sha256: String }
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Contract {
    pub version: String,
    pub package_name: String,
    pub wrapper_guards: Vec<Guard>,
    pub carrier: Guard,
    pub wrapper_archive_sha256: String,
    pub carrier_archive_sha256: String,
    pub compiled_version_binding_sha256: String,
    pub compiled_version_symbol: String,
    pub compiled_version_offset: u64,
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|v| v.is_ascii_digit() || matches!(v,b'a'..=b'f'))
}
impl Contract {
    pub fn validate(&self) -> Result<(), ()> {
        let required = BTreeSet::from(["package.json", "bin/kilo", "postinstall.mjs"]);
        let paths = self.wrapper_guards.iter().map(|g|g.path.as_str()).collect::<BTreeSet<_>>();
        if self.package_name != "@kilocode/cli" || self.carrier.path != "kilo"
            || self.version.split('.').count() != 3 || self.version.split('.').any(|v|v.parse::<u32>().is_err())
            || self.wrapper_guards.len() != required.len() || paths != required
            || ![&self.wrapper_archive_sha256,&self.carrier_archive_sha256,&self.compiled_version_binding_sha256].iter().all(|v|digest(v)) {
            return Err(());
        }
        let mut total = 0u64;
        for guard in self.wrapper_guards.iter().chain(std::iter::once(&self.carrier)) {
            if guard.bytes == 0 || !digest(&guard.sha256) { return Err(()); }
            total = total.checked_add(guard.bytes).ok_or(())?;
        }
        if total > MAX_HASH_BYTES { return Err(()); }
        if self.compiled_version_symbol.is_empty() || self.compiled_version_symbol.len()>64
            || !self.compiled_version_symbol.bytes().all(|v|v.is_ascii_alphanumeric()||matches!(v,b'_'|b'$'))
            || self.compiled_version_symbol.as_bytes()[0].is_ascii_digit() { return Err(()); }
        let marker=self.version_marker();
        if hex::encode(Sha256::digest(marker.as_bytes()))!=self.compiled_version_binding_sha256
            || self.compiled_version_offset.checked_add(marker.encode_utf16().count() as u64*2).is_none_or(|v|v>self.carrier.bytes) {return Err(());}
        Ok(())
    }
    fn version_marker(&self)->String {format!("var {}=\"{}\",",self.compiled_version_symbol,self.version)}
}
fn absolute(path: &Path) -> Result<(), ()> {
    if !path.is_absolute() || path.components().count()>64 || path.to_str().is_none_or(|s|s.len()>4096 || s.chars().any(char::is_control))
        || path.components().any(|v|matches!(v,Component::ParentDir|Component::Prefix(_))) { return Err(()); }
    Ok(())
}
fn environment() -> Result<(), ()> {
    if ["KILO_BIN_PATH","NODE_OPTIONS","NODE_PATH","BUN_OPTIONS"].iter()
        .any(|key|std::env::var_os(key).is_some_and(|v|!v.is_empty())) { return Err(()); }
    Ok(())
}

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

pub(super) struct Proof { selection:Selection, bound:Vec<Bound>, missing:BTreeSet<PathBuf>, carrier:usize, identity:String }
impl Proof {
    fn bind(&mut self,path:&Path,directory:bool)->Result<usize,()> {
        if let Some(i)=self.bound.iter().position(|v|v.path==path&&v.directory==directory){self.bound[i].revalidate()?;return Ok(i);}
        if self.bound.len()+self.missing.len()>=MAX_FILES{return Err(());}
        let value=Bound::capture(path.into(),directory)?;self.bound.push(value);Ok(self.bound.len()-1)
    }
    fn exists(&mut self,path:&Path)->Result<bool,()> {
        absolute(path)?;checked_root(path)?;
        if self.bound.len()+self.missing.len()>=MAX_FILES{return Err(());}
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink()=>Err(()),
            Ok(_)=>Ok(true),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{self.missing.insert(path.into());Ok(false)},
            Err(_)=>Err(()),
        }
    }
    pub fn capture(selected:&Path,c:&Contract)->Result<Self,()> {
        c.validate()?;
        #[cfg(not(all(target_os="macos",target_arch="aarch64")))] {let _=selected;return Err(());}
        let selection=Selection::capture(selected)?;
        let mut proof=Self{selection,bound:Vec::new(),missing:BTreeSet::new(),carrier:0,identity:String::new()};
        let physical=proof.selection.physical.clone();
        proof.bind(proof.selection.alias.parent().ok_or(())?.to_path_buf().as_path(),true)?;
        proof.bind(physical.parent().ok_or(())?,true)?;
        let entry=proof.bind(&physical,false)?;
        let mut guards=Vec::<(usize,&Guard)>::new();
        if proof.bound[entry].stamp.bytes==c.carrier.bytes {
            proof.carrier=entry;guards.push((entry,&c.carrier));
        } else {
            let root=physical.parent().and_then(Path::parent).ok_or(())?;
            if root.join("bin/kilo")!=physical{return Err(());}
            proof.bind(root,true)?;
            for guard in &c.wrapper_guards {let index=proof.bind(&root.join(&guard.path),false)?;guards.push((index,guard));}
            let cached=root.join("bin/.kilo");
            // An uncached wrapper selects npm packages using the Node process's
            // os.arch(), including Rosetta/x64 candidates. Arca's architecture
            // cannot establish that selection. Do not run Node or guess another
            // carrier. Normal postinstall and an inert static installation can
            // both provide this directly selected, completely guarded cache.
            if !proof.exists(&cached)?{return Err(());}
            let carrier=cached;
            proof.bind(carrier.parent().ok_or(())?,true)?;
            let index=proof.bind(&carrier,false)?;proof.carrier=index;guards.push((index,&c.carrier));
        }
        #[cfg(unix)] {if proof.bound[entry].stamp.mode&0o111==0||proof.bound[proof.carrier].stamp.mode&0o111==0{return Err(());}}
        let context=hex::encode(Sha256::digest(serde_json::to_vec(&(c,&proof.selection.alias,&proof.selection.physical,
            &proof.selection.stamp,&proof.selection.link,proof.bound.iter().map(|v|(&v.path,&v.stamp)).collect::<Vec<_>>(),&proof.missing)).map_err(|_|())?));
        proof.identity=context.clone();let mut hashed=0u64;
        for (index,guard) in guards {
            proof.revalidate()?;let file=&mut proof.bound[index];
            if file.stamp.bytes!=guard.bytes{return Err(());}
            let key=Key{context:context.clone(),path:file.path.clone(),stamp:file.stamp.clone(),expected:guard.sha256.clone()};
            if !cache().lock().map_err(|_|())?.contains(&key) {
                if guard.bytes>MAX_HASH_BYTES.saturating_sub(hashed){return Err(());}
                file.file.seek(SeekFrom::Start(0)).map_err(|_|())?;
                let mut h=Sha256::new();let mut read=0u64;let mut buffer=[0u8;65536];
                loop {
                    let n=(&mut file.file).take(guard.bytes.saturating_sub(read)+1).read(&mut buffer).map_err(|_|())?;
                    if n==0{break;}read+=n as u64;hashed+=n as u64;
                    if read>guard.bytes||hashed>MAX_HASH_BYTES{return Err(());}h.update(&buffer[..n]);
                }
                if read!=guard.bytes||hex::encode(h.finalize())!=guard.sha256{return Err(());}
                file.revalidate()?;proof.revalidate()?;cache().lock().map_err(|_|())?.insert(key);
            }
        }
        let expected=c.version_marker().encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
        if proof.read_metadata(c.compiled_version_offset,expected.len())?!=expected{return Err(());}
        proof.revalidate()?;Ok(proof)
    }
    pub fn revalidate(&self)->Result<(),()> {
        self.selection.revalidate()?;for file in &self.bound{file.revalidate()?;}
        for p in &self.missing {
            checked_root(p)?;
            if !matches!(fs::symlink_metadata(p),Err(e) if e.kind()==std::io::ErrorKind::NotFound){return Err(());}
        }
        self.selection.revalidate()
    }
    pub fn carrier_path(&self)->&Path {&self.bound[self.carrier].path}
    pub fn identity(&self)->&str {&self.identity}
    pub fn watch_paths(&self)->Vec<PathBuf> {
        std::iter::once(self.selection.alias.clone()).chain(self.bound.iter().map(|v|v.path.clone())).chain(self.missing.iter().cloned()).collect()
    }
    pub fn read_metadata(&mut self,offset:u64,count:usize)->Result<Vec<u8>,()> {
        if count>65536||offset.checked_add(count as u64).is_none_or(|end|end>self.bound[self.carrier].stamp.bytes){return Err(());}
        self.revalidate()?;let f=&mut self.bound[self.carrier].file;f.seek(SeekFrom::Start(offset)).map_err(|_|())?;
        let mut bytes=vec![0;count];f.read_exact(&mut bytes).map_err(|_|())?;self.revalidate()?;Ok(bytes)
    }
}
