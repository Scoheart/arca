//! Bounded metadata snapshot. Component leaves are stat-only; JSON readers are
//! called only for native configuration or registered package metadata.
use super::super::*;
use super::runtime::Bound;

const MAX_BOUND:usize=512;
const MAX_METADATA:usize=8*1024*1024;
const MAX_DOCUMENT:usize=2*1024*1024;
const MAX_DIRECTORY:usize=1024;
const MAX_DEPTH:usize=64;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Leaf { pub directory:bool, pub file:bool, fingerprint:String }
fn signature(path:&Path)->Result<Option<Leaf>,()> {
    checked_root(path)?;
    let m=match fs::symlink_metadata(path){Ok(v)=>v,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(_)=>return Err(())};
    if !m.is_dir()&&!m.file_type().is_file(){return Err(());}
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        let stamp=(m.dev(),m.ino(),m.len(),m.mode(),m.nlink(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec());
        let fingerprint=hex::encode(Sha256::digest(serde_json::to_vec(&stamp).map_err(|_|())?));
        Ok(Some(Leaf{directory:m.is_dir(),file:m.file_type().is_file(),fingerprint}))
    }
    #[cfg(not(unix))] {Err(())}
}

#[derive(Default)]
pub(super) struct Snapshot {
    bound:Vec<(PathBuf,bool,Bound)>,
    leaves:BTreeMap<PathBuf,Option<Leaf>>,
    missing:BTreeSet<PathBuf>,
    documents:BTreeMap<(PathBuf,DocumentFormat),Arc<Value>>,
    directories:BTreeMap<PathBuf,Vec<(PathBuf,bool,bool)>>,
    scan:ScanBudget,
    read_bytes:usize,
    pub watches:BTreeSet<PathBuf>,
    pub incomplete:bool,
}
impl Snapshot {
    #[cfg(test)]
    pub fn metadata_bytes_read(&self)->usize{self.read_bytes}
    fn limit(&self)->Result<(),()> {
        if self.bound.len()+self.leaves.len()+self.missing.len()>=MAX_BOUND {Err(())}else{Ok(())}
    }
    fn bind(&mut self,path:&Path,directory:bool)->Result<Option<usize>,()> {
        if path.components().count()>MAX_DEPTH{return Err(());}
        self.limit()?;checked_root(path)?;
        if let Some(i)=self.bound.iter().position(|v|v.0==path&&v.1==directory){self.bound[i].2.revalidate()?;return Ok(Some(i));}
        if let Some(parent)=path.parent(){if self.bind(parent,true)?.is_none(){self.missing.insert(path.into());self.limit()?;return Ok(None);}}
        let Some(s)=signature(path)?else{self.missing.insert(path.into());self.limit()?;return Ok(None);};
        if if directory{!s.directory}else{!s.file}{return Err(());}
        let value=Bound::capture(path.into(),directory)?;
        self.bound.push((path.into(),directory,value));self.limit()?;Ok(Some(self.bound.len()-1))
    }
    pub fn stat(&mut self,path:&Path)->Result<Option<Leaf>,()> {
        if path.components().count()>MAX_DEPTH{return Err(());}
        self.limit()?;self.watches.insert(path.into());
        if let Some(parent)=path.parent(){self.bind(parent,true)?;}
        let value=signature(path)?;
        if let Some(before)=self.leaves.get(path){if before!=&value{return Err(());}}
        else{self.leaves.insert(path.into(),value.clone());}
        self.limit()?;Ok(value)
    }
    pub fn json(&mut self,path:&Path,format:DocumentFormat)->Result<Option<Arc<Value>>,()> {
        self.watches.insert(path.into());
        let allowed=match format{
            DocumentFormat::Json|DocumentFormat::JsonBom|DocumentFormat::Jsonc=>matches!(path.extension().and_then(|v|v.to_str()),Some("json"|"jsonc")),
            DocumentFormat::Toml=>path.file_name().is_some_and(|v|v=="config"),
            DocumentFormat::Json5=>false,
        };
        if !allowed{return Err(());}
        let Some(i)=self.bind(path,false)?else{return Ok(None);};
        let key=(path.to_path_buf(),format);
        if let Some(value)=self.documents.get(&key){return Ok(Some(value.clone()));}
        let n=usize::try_from(self.bound[i].2.size()).map_err(|_|())?;
        if n>MAX_DOCUMENT{return Err(());}
        let next=self.read_bytes.checked_add(n).ok_or(())?;
        if next>MAX_METADATA{return Err(());}self.read_bytes=next;
        let raw=self.bound[i].2.read_small(MAX_DOCUMENT)?;
        let text=std::str::from_utf8(&raw).map_err(|_|())?;
        let text=text.strip_prefix('\u{feff}').unwrap_or(text);
        let value=match format {
            DocumentFormat::Json|DocumentFormat::JsonBom=>serde_json::from_str::<Value>(text).map_err(|_|())?,
            DocumentFormat::Jsonc=>{
                check_jsonc_depth(text)?;
                let options=jsonc_parser::ParseOptions{allow_comments:true,allow_trailing_commas:true,allow_loose_object_property_names:false,
                    allow_missing_commas:false,allow_single_quoted_strings:false,allow_hexadecimal_numbers:false,allow_unary_plus_numbers:false};
                jsonc_parser::parse_to_ast(text,&Default::default(),&options).map_err(|_|())?.value.ok_or(())?.into()
            },
            DocumentFormat::Toml=>serde_json::to_value(toml::from_str::<toml::Value>(text).map_err(|_|())?).map_err(|_|())?,
            DocumentFormat::Json5=>return Err(()),
        };
        self.bound[i].2.revalidate()?;
        let value=Arc::new(value);self.documents.insert(key,value.clone());Ok(Some(value))
    }
    pub fn entries(&mut self,path:&Path)->Result<Vec<(PathBuf,bool,bool)>,()> {
        self.watches.insert(path.into());
        let Some(i)=self.bind(path,true)?else{return Ok(Vec::new());};
        if let Some(entries)=self.directories.get(path){return Ok(entries.clone());}
        let entries=directory_entries(path,&mut self.scan)?;
        if entries.len()>MAX_DIRECTORY{return Err(());}
        self.bound[i].2.revalidate()?;
        let entries=entries.into_iter().map(|v|(v.path,v.directory,v.file)).collect::<Vec<_>>();
        self.directories.insert(path.into(),entries.clone());Ok(entries)
    }
    pub fn revalidate(&self)->Result<(),()> {
        for (_,_,bound) in &self.bound{bound.revalidate()?;}
        for (path,before) in &self.leaves{if &signature(path)?!=before{return Err(());}}
        for path in &self.missing{if signature(path)?.is_some(){return Err(());}}
        Ok(())
    }
    pub fn fingerprint(&self)->Result<String,()> {
        self.revalidate()?;
        let files=self.bound.iter().map(|v|v.2.fingerprint()).collect::<Result<Vec<_>,_>>()?;
        let leaves=self.leaves.iter().map(|(p,v)|(p,v.as_ref().map(|v|&v.fingerprint))).collect::<Vec<_>>();
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(files,leaves,&self.missing)).map_err(|_|())?)))
    }
}
