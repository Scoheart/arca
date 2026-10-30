//! CodeBuddy global/CN IDE metadata. No target, getter, migration or factory runs.
//! App guards, metadata and stat-only components use independent finite budgets.
use super::*;
use std::collections::VecDeque;
use std::io::{Seek,SeekFrom};
use std::sync::Mutex;

const MAX_IDENTITY_BYTES:u64=128*1024*1024;
const MAX_IDENTITY_METADATA:usize=2*1024*1024;
const MAX_DIGEST_CACHE:usize=32;
const MAX_DOCUMENT_BYTES:usize=2*1024*1024;
const MAX_METADATA_BYTES:usize=8*1024*1024;
const MAX_VISITS:usize=16_384;
const MAX_BOUND_PATHS:usize=2048;
const MAX_DIRECTORY_ENTRIES:usize=1024;
const MAX_REGISTRATIONS:usize=512;
const MAX_OWNED_COMPONENTS:usize=4096;
const MAX_STAT_BYTES:u64=2*1024*1024;
const MAX_TOTAL_STAT_BYTES:u64=64*1024*1024;
const APP_RESOURCES:&str="Contents/Resources/app";
const INNER_ROOT:&str="Contents/Resources/app/extensions/genie";
const ORDINARY_OVERRIDES:&[&str]=&["VSCODE_DEV","VSCODE_EXTENSIONS","VSCODE_PORTABLE","VSCODE_APPDATA"];
const PUBLIC_GUARDS:[&str;10]=[
    "Contents/Info.plist","Contents/MacOS/Electron",
    "Contents/Resources/app/package.json","Contents/Resources/app/product.json",
    "Contents/Resources/app/out/main.js","Contents/Resources/app/extensions/genie/package.json",
    "Contents/Resources/app/extensions/genie/product.json","Contents/Resources/app/extensions/genie/out/extension/index.js",
    "Contents/Resources/app/out/vs/workbench/workbench.desktop.main.js",
    "Contents/Resources/app/out/vs/workbench/api/node/extensionHostProcess.js",
];

#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard{path:String,bytes:u64,sha256:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct BundledExtension{id:String,version:String,manifest:Guard}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillMember{name:String,bytes:u64}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolDeclaration{name:String,source:String,gate:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillGroup{id:String,label:String,root:String,gate:String,skills:Vec<SkillMember>,tools:Vec<ToolDeclaration>}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallRequest{name:String,marketplace:String,auto_enable:bool}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence{contract_sha256:String,source_index_sha256:String,rea_parent:String,rea_summary:String}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile{
    agent_id:String,app_name:String,bundle_id:String,bundle_name:String,
    application_name:String,data_folder:String,archive_sha256:String,
    guards:Vec<Guard>,builtin_extensions:Vec<BundledExtension>,
}
#[derive(Clone,Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract{
    schema_version:u32,version:String,release_label:String,base_version:String,genie_version:String,
    commit:String,executable:String,package_main:String,native_default_folder:String,
    skill_scan_dirs:Vec<String>,profiles:Vec<Profile>,skill_groups:Vec<SkillGroup>,
    declared_plugins:Vec<InstallRequest>,evidence:Evidence,
}
fn digest_string(v:&str)->bool{v.len()==64&&v.bytes().all(|c|c.is_ascii_digit()||b"abcdef".contains(&c))}
fn simple_name(v:&str)->bool{!v.is_empty()&&v.len()<=240&&v!="."&&v!=".."&&v.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c))}
impl Contract{
    fn profile(&self,id:&str)->Option<&Profile>{self.profiles.iter().find(|p|p.agent_id==id)}
    fn validate(&self)->Result<(),()>{
        if self.schema_version!=1||self.executable!="Electron"||self.package_main!="./out/main.js"
            ||self.native_default_folder!=".codebuddy"||self.profiles.len()!=2||self.skill_groups.len()!=2||self.declared_plugins.len()!=9
            ||self.skill_scan_dirs.iter().map(String::as_str).collect::<Vec<_>>()!=[".codebuddy",".workbuddy",".claude",".agents",".openclaw"]
            ||self.commit.len()!=40||!self.commit.bytes().all(|c|c.is_ascii_hexdigit())
            ||[&self.version,&self.base_version,&self.genie_version,&self.release_label].iter().any(|v|metadata_name(v).as_deref()!=Some(v.as_str()))
            ||!digest_string(&self.evidence.contract_sha256)||!digest_string(&self.evidence.source_index_sha256)
            ||!self.evidence.rea_parent.starts_with("ev_")||!self.evidence.rea_summary.starts_with("ev_"){return Err(());}
        let mut ids=BTreeSet::new();
        for p in &self.profiles{
            let identity=match p.agent_id.as_str(){
                "codebuddy-ide"=>("CodeBuddy.app","com.tencent.codebuddy","CodeBuddy","buddy",".codebuddy"),
                "codebuddy-cn-ide"=>("CodeBuddy CN.app","com.tencent.codebuddycn","CodeBuddy CN","buddycn",".codebuddycn"),_=>return Err(()),
            };
            if !ids.insert(&p.agent_id)||(&*p.app_name,&*p.bundle_id,&*p.bundle_name,&*p.application_name,&*p.data_folder)!=identity
                ||!digest_string(&p.archive_sha256)||p.guards.len()!=PUBLIC_GUARDS.len()||p.builtin_extensions.len()!=100{return Err(());}
            let mut paths=BTreeSet::new();let mut bytes=0u64;
            for g in &p.guards{
                if !PUBLIC_GUARDS.contains(&g.path.as_str())||!paths.insert(&g.path)||g.bytes==0||!digest_string(&g.sha256){return Err(());}
                bytes=bytes.checked_add(g.bytes).ok_or(())?;
            }
            if bytes>MAX_IDENTITY_BYTES{return Err(());}
            let mut packages=BTreeSet::new();let mut folders=BTreeSet::new();
            for ext in &p.builtin_extensions{
                vscode_extensions::identifier(&ext.id)?;let g=&ext.manifest;
                let folder=g.path.strip_prefix("Contents/Resources/app/extensions/").and_then(|v|v.strip_suffix("/package.json")).ok_or(())?;
                if !simple_name(folder)||!folders.insert(folder)||!packages.insert(&ext.id)||metadata_name(&ext.version).as_ref()!=Some(&ext.version)
                    ||g.bytes==0||g.bytes>MAX_DOCUMENT_BYTES as u64||!digest_string(&g.sha256){return Err(());}
            }
        }
        let mut groups=BTreeSet::new();
        for group in &self.skill_groups{
            let (root,count,gate,tool_count)=match group.id.as_str(){
                "@builtin/skills"=>("Contents/Resources/app/extensions/genie/out/extension/builtin",3,"availability-only",0),
                "@builtin/wecom-skills"=>("Contents/Resources/app/extensions/genie/out/extension/wecom-skills",12,"isWecomAiBot-request-unknown",1),_=>return Err(()),
            };
            if !groups.insert(&group.id)||group.root!=root||group.skills.len()!=count||group.gate!=gate||group.tools.len()!=tool_count||metadata_name(&group.label).as_ref()!=Some(&group.label){return Err(());}
            let mut names=BTreeSet::new();for skill in &group.skills{
                if !simple_name(&skill.name)||!names.insert(&skill.name)||skill.bytes==0||skill.bytes>MAX_STAT_BYTES{return Err(());}
            }
            for tool in &group.tools{
                if metadata_name(&tool.name).as_ref()!=Some(&tool.name)||tool.source!=format!("{INNER_ROOT}/out/extension/index.js")||tool.gate!=group.gate{return Err(());}
            }
        }
        let mut requests=BTreeSet::new();for request in &self.declared_plugins{
            if !simple_name(&request.name)||request.marketplace!="codebuddy-plugins-official"||!requests.insert(&request.name){return Err(());}
        }Ok(())
    }
}
fn contract()->Option<&'static Contract>{
    static VALUE:OnceLock<Option<Contract>>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-codebuddy-desktop.json")).ok().filter(|c|c.validate().is_ok())).as_ref()
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct Stamp {
    device: u64, inode: u64, bytes: u64, mode: u32, links: u64,
    mtime: i64, mtime_nsec: i64, ctime: i64, ctime_nsec: i64,
}
impl Stamp {
    fn capture(value: &fs::Metadata) -> Result<Self, ()> {
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            Ok(Self { device:value.dev(), inode:value.ino(), bytes:value.len(), mode:value.mode(), links:value.nlink(),
                mtime:value.mtime(), mtime_nsec:value.mtime_nsec(), ctime:value.ctime(), ctime_nsec:value.ctime_nsec() })
        }
        #[cfg(not(unix))] { let _ = value; Err(()) }
    }
}
struct BoundPath { path: PathBuf, file: fs::File, stamp: Stamp, directory: bool }
impl BoundPath {
    fn capture(path: PathBuf, directory: bool) -> Result<Self, ()> {
        checked_root(&path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
        if if directory { !metadata.is_dir() } else { !metadata.file_type().is_file() } { return Err(()); }
        let stamp = Stamp::capture(&metadata)?; if !directory && stamp.links != 1 { return Err(()); }
        let file = open_confined(&path, directory)?;
        let bound = Self { path, file, stamp, directory }; bound.revalidate()?; Ok(bound)
    }
    fn revalidate(&self) -> Result<(), ()> {
        checked_root(&self.path)?;
        let current = fs::symlink_metadata(&self.path).map_err(|_| ())?;
        if if self.directory { !current.is_dir() } else { !current.file_type().is_file() } { return Err(()); }
        if Stamp::capture(&current)? != self.stamp || Stamp::capture(&self.file.metadata().map_err(|_| ())?)? != self.stamp { return Err(()); }
        Ok(())
    }
    fn bytes(&mut self, limit: usize) -> Result<Vec<u8>, ()> {
        if self.directory || self.stamp.bytes > limit as u64 { return Err(()); }
        self.revalidate()?; self.file.seek(SeekFrom::Start(0)).map_err(|_| ())?;
        let mut bytes = Vec::with_capacity(self.stamp.bytes as usize);
        (&mut self.file).take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|_| ())?;
        self.revalidate()?;
        if bytes.len() as u64 != self.stamp.bytes || bytes.len() > limit { return Err(()); }
        Ok(bytes)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct DigestKey { app: PathBuf, app_stamp: Stamp, path: PathBuf, stamp: Stamp, expected: String }
#[derive(Default)]
struct DigestCache { entries: VecDeque<DigestKey> }
impl DigestCache {
    fn contains(&mut self, key: &DigestKey) -> bool {
        let Some(index) = self.entries.iter().position(|entry| entry == key) else { return false; };
        let value = self.entries.remove(index).expect("located digest cache entry"); self.entries.push_back(value); true
    }
    fn insert(&mut self, key: DigestKey) {
        if self.contains(&key) { return; }
        if self.entries.len() == MAX_DIGEST_CACHE { self.entries.pop_front(); }
        self.entries.push_back(key);
    }
}

#[derive(Default)]
struct MetadataBudget { bytes: usize, visited: usize, components: usize, stat_bytes: u64 }
impl MetadataBudget {
    fn visit(&mut self) -> Result<(), ()> { self.visited += 1; if self.visited > MAX_VISITS { Err(()) } else { Ok(()) } }
    fn component(&mut self) -> Result<(), ()> { self.visit()?; self.components += 1; if self.components > MAX_OWNED_COMPONENTS { Err(()) } else { Ok(()) } }
}
#[derive(Clone)]
struct Entry { path: PathBuf, directory: bool, file: bool }
struct StatLeaf { path: PathBuf, parent: usize, stamp: Stamp }
#[derive(Default)]
struct Snapshot {
    plugins: Vec<NativePluginObservation>, bound: Vec<BoundPath>, leaves: Vec<StatLeaf>, missing: BTreeSet<PathBuf>,
    entries: BTreeMap<PathBuf,Vec<Entry>>, watches: BTreeSet<PathBuf>, incomplete: bool,
}
impl Snapshot {
    fn missing(&mut self, path: &Path) -> Result<(), ()> {
        checked_root(path)?; self.missing.insert(path.into()); self.watches.insert(path.into()); Ok(())
    }
    fn bind(&mut self, path: &Path, directory: bool, budget: &mut MetadataBudget) -> Result<Option<usize>, ()> {
        budget.visit()?; checked_root(path)?; self.watches.insert(path.into());
        if let Some(index) = self.bound.iter().position(|file| file.path == path && file.directory == directory) {
            self.bound[index].revalidate()?; return Ok(Some(index));
        }
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => { self.missing(path)?; return Ok(None); },
            Err(_) => return Err(()), _ => {},
        }
        if self.bound.len() >= MAX_BOUND_PATHS { return Err(()); }
        self.bound.push(BoundPath::capture(path.into(),directory)?); Ok(Some(self.bound.len()-1))
    }
    fn read_document(&mut self, path: &Path, jsonc: bool, budget: &mut MetadataBudget) -> Result<Option<Value>, ()> {
        // The native path reader can attempt arbitrary filenames. Our passive
        // subset admits metadata JSON only, so a component override can never
        // redirect it to SKILL.md, a command body, envFile or executable source.
        if !matches!(path.extension().and_then(|value|value.to_str()),Some("json"|"jsonc")) {return Err(());}
        if budget.bytes >= MAX_METADATA_BYTES { return Err(()); }
        let Some(index) = self.bind(path,false,budget)? else { return Ok(None); };
        let size = usize::try_from(self.bound[index].stamp.bytes).map_err(|_| ())?;
        let limit = MAX_DOCUMENT_BYTES.min(MAX_METADATA_BYTES.saturating_sub(budget.bytes));
        if size > limit { return Err(()); }
        let read = self.bound[index].bytes(limit);
        // Account a complete requested read even if UTF-8/parser validation fails.
        budget.bytes = budget.bytes.checked_add(size).ok_or(())?;
        let bytes = read?; let text = std::str::from_utf8(&bytes).map_err(|_| ())?;
        let value = if jsonc {
            check_jsonc_depth(text)?;
            let options = jsonc_parser::ParseOptions { allow_comments:true, allow_trailing_commas:true,
                allow_loose_object_property_names:false, allow_missing_commas:false,
                allow_single_quoted_strings:false, allow_hexadecimal_numbers:false, allow_unary_plus_numbers:false };
            jsonc_parser::parse_to_ast(text,&Default::default(),&options).map_err(|_| ())?.value.ok_or(())?.into()
        } else { serde_json::from_str(text).map_err(|_| ())? };
        Ok(Some(value))
    }
    fn guarded_json(&mut self, app: &Path, guard: &Guard, budget: &mut MetadataBudget) -> Result<Value, ()> {
        let path = relative(app,&guard.path)?;
        let value = self.read_document(&path,false,budget)?.ok_or(())?;
        let index = self.bound.iter().position(|file|file.path==path&&!file.directory).ok_or(())?;
        if self.bound[index].stamp.bytes != guard.bytes { return Err(()); }
        // Small manifest-only buffers; large JS is never allocated here.
        let bytes = self.bound[index].bytes(MAX_DOCUMENT_BYTES)?;
        budget.bytes = budget.bytes.checked_add(bytes.len()).ok_or(())?;
        if budget.bytes > MAX_METADATA_BYTES || hex::encode(Sha256::digest(&bytes)) != guard.sha256 { return Err(()); }
        Ok(value)
    }
    fn directories(&mut self, path: &Path, budget: &mut MetadataBudget) -> Result<Vec<Entry>, ()> {
        let Some(index) = self.bind(path,true,budget)? else { return Ok(Vec::new()); };
        if let Some(entries) = self.entries.get(path) { return Ok(entries.clone()); }
        let mut entries = Vec::new();
        #[cfg(unix)] {
            use std::os::unix::ffi::OsStrExt;
            use rustix::fs::{AtFlags,Dir,FileType,statat};
            let file = open_confined(path,true)?;
            if Stamp::capture(&file.metadata().map_err(|_| ())?)? != self.bound[index].stamp { return Err(()); }
            for entry in Dir::read_from(&file).map_err(|_| ())? {
                let entry = entry.map_err(|_| ())?; let name = entry.file_name();
                if matches!(name.to_bytes(),b"."|b"..") { continue; }
                budget.visit()?; if entries.len() >= MAX_DIRECTORY_ENTRIES { return Err(()); }
                let name_string = std::str::from_utf8(name.to_bytes()).map_err(|_| ())?;
                if name_string.len()>240 || name_string.chars().any(char::is_control) { return Err(()); }
                let kind = match entry.file_type() { FileType::Unknown => FileType::from_raw_mode(statat(&file,name,AtFlags::SYMLINK_NOFOLLOW).map_err(|_| ())?.st_mode), value => value };
                if kind == FileType::Symlink { self.incomplete=true; continue; }
                entries.push(Entry { path:path.join(name_string), directory:kind==FileType::Directory, file:kind==FileType::RegularFile });
            }
        }
        #[cfg(not(unix))] { let _ = index; return Err(()); }
        self.bound[index].revalidate()?; entries.sort_by(|a,b|a.path.cmp(&b.path));
        self.entries.insert(path.into(),entries.clone()); Ok(entries)
    }
    fn stat_file(&mut self, path: &Path, budget: &mut MetadataBudget) -> Result<Option<Stamp>, ()> {
        budget.visit()?; checked_root(path)?; self.watches.insert(path.into());
        if let Some(leaf) = self.leaves.iter().find(|leaf|leaf.path==path) { return Ok(Some(leaf.stamp.clone())); }
        let Some(parent) = self.bind(path.parent().ok_or(())?,true,budget)? else { self.missing(path)?; return Ok(None); };
        let Some(stamp) = stat_at(&self.bound[parent],path.file_name().ok_or(())?)? else { self.missing(path)?; return Ok(None); };
        if stamp.bytes > MAX_STAT_BYTES || self.leaves.len() >= MAX_OWNED_COMPONENTS { return Err(()); }
        budget.stat_bytes = budget.stat_bytes.checked_add(stamp.bytes).ok_or(())?;
        if budget.stat_bytes > MAX_TOTAL_STAT_BYTES { return Err(()); }
        self.leaves.push(StatLeaf { path:path.into(), parent, stamp:stamp.clone() }); Ok(Some(stamp))
    }
    fn revalidate(&self) -> Result<(), ()> {
        for file in &self.bound { file.revalidate()?; }
        for leaf in &self.leaves {
            let parent = self.bound.get(leaf.parent).ok_or(())?;
            if stat_at(parent,leaf.path.file_name().ok_or(())?)?.as_ref() != Some(&leaf.stamp) { return Err(()); }
        }
        for path in &self.missing {
            checked_root(path)?;
            match fs::symlink_metadata(path) { Err(error) if error.kind()==std::io::ErrorKind::NotFound => {}, _=>return Err(()) }
        }
        for file in &self.bound { file.revalidate()?; } Ok(())
    }
}
// FD-relative NOFOLLOW stat never opens a Skill, command, rule or hook body.
fn stat_at(parent: &BoundPath, name: &std::ffi::OsStr) -> Result<Option<Stamp>, ()> {
    #[cfg(any(target_os="macos",target_os="linux"))] {
        use rustix::fs::{AtFlags,FileType,statat};
        let value = match statat(&parent.file,name,AtFlags::SYMLINK_NOFOLLOW) {
            Ok(value)=>value, Err(error) if error==rustix::io::Errno::NOENT=>return Ok(None), _=>return Err(()),
        };
        if FileType::from_raw_mode(value.st_mode)!=FileType::RegularFile { return Err(()); }
        Ok(Some(Stamp { device:value.st_dev as u64, inode:value.st_ino as u64, bytes:u64::try_from(value.st_size).map_err(|_| ())?,
            mode:value.st_mode as u32, links:value.st_nlink as u64, mtime:value.st_mtime as i64, mtime_nsec:value.st_mtime_nsec as i64,
            ctime:value.st_ctime as i64, ctime_nsec:value.st_ctime_nsec as i64 }))
    }
    #[cfg(not(any(target_os="macos",target_os="linux")))] { let _=(parent,name); Err(()) }
}

fn host_paths(host:&Host)->Result<Vec<PathBuf>,()>{
    let app=match host.agent_id.as_str(){"codebuddy-ide"=>"CodeBuddy.app","codebuddy-cn-ide"=>"CodeBuddy CN.app",_=>return Err(())};
    if host.adapter!="codebuddy_desktop"||host.roots.is_empty()||host.roots.len()>2{return Err(());}
    let allowed=[format!("/Applications/{app}"),format!("~/Applications/{app}")];let mut seen=BTreeSet::new();
    host.roots.iter().map(|root|{
        if root.layout!="codebuddy_app"||root.presence!=PluginPresence::Bundled||!allowed.contains(&root.path)||!seen.insert(&root.path){return Err(());}
        let path=vscode_extensions::root_path(root);checked_root(&path)?;Ok(path)
    }).collect()
}
fn select_app(host:&Host)->Result<Option<PathBuf>,()>{
    use crate::application::agent_launch::{info_unlocked,LaunchTarget};
    let allowed=host_paths(host)?;let launch=info_unlocked(&host.agent_id).map_err(|_|())?;
    if launch.configured_target.is_some(){return Err(());}
    match launch.resolved_target{
        Some(LaunchTarget::App{path,args,new_instance,env})if args.is_empty()&&!new_instance&&env.is_empty()=>{
            let path=PathBuf::from(path);if !allowed.contains(&path){return Err(());}checked_root(&path)?;Ok(Some(path))
        },
        None=>Ok(None),_=>Err(()),
    }
}
#[derive(Default)]
struct IdentityBudget{hashed_bytes:u64,metadata_bytes:usize,visited:usize}
struct VerifiedApp{app:BoundPath,guards:Vec<BoundPath>,version:String,commit:String}
impl VerifiedApp{
    fn capture(host:&Host,c:&Contract,p:&Profile,cache:&Mutex<DigestCache>,budget:&mut IdentityBudget)->Result<Option<Self>,()>{
        c.validate()?;if host.agent_id!=p.agent_id{return Err(());}
        let Some(path)=select_app(host)?else{return Ok(None);};
        let mut proof=Self{app:BoundPath::capture(path,true)?,guards:Vec::new(),version:c.version.clone(),commit:c.commit.clone()};
        for guard in &p.guards{
            budget.visited+=1;if budget.visited>32{return Err(());}
            let mut file=BoundPath::capture(relative(&proof.app.path,&guard.path)?,false)?;
            if file.stamp.bytes!=guard.bytes{return Err(());}
            let key=DigestKey{app:proof.app.path.clone(),app_stamp:proof.app.stamp.clone(),path:file.path.clone(),stamp:file.stamp.clone(),expected:guard.sha256.clone()};
            let cached=cache.lock().map_err(|_|())?.contains(&key);
            if !cached{
                if guard.bytes>MAX_IDENTITY_BYTES.saturating_sub(budget.hashed_bytes){return Err(());}
                let mut hash=Sha256::new();let mut count=0u64;let mut buffer=[0u8;65_536];
                loop{
                    let n=(&mut file.file).take(guard.bytes.saturating_sub(count)+1).read(&mut buffer).map_err(|_|())?;
                    if n==0{break;}count+=n as u64;budget.hashed_bytes+=n as u64;
                    if count>guard.bytes||budget.hashed_bytes>MAX_IDENTITY_BYTES{return Err(());}
                    hash.update(&buffer[..n]);
                }
                if count!=guard.bytes||hex::encode(hash.finalize())!=guard.sha256{return Err(());}
            }
            file.revalidate()?;proof.app.revalidate()?;
            if select_app(host)?.as_ref()!=Some(&proof.app.path){return Err(());}
            if !cached{cache.lock().map_err(|_|())?.insert(key);}
            proof.guards.push(file);
        }
        let info=plist::Value::from_reader(std::io::Cursor::new(proof.public_bytes("Contents/Info.plist",budget)?)).map_err(|_|())?;
        let info=info.as_dictionary().ok_or(())?;
        for(key,value)in [("CFBundleIdentifier",&p.bundle_id),("CFBundleName",&p.bundle_name),("CFBundleDisplayName",&p.bundle_name),
            ("CFBundleExecutable",&c.executable),("CFBundleShortVersionString",&c.version),("CFBundleVersion",&c.version)]{
            if info.get(key).and_then(plist::Value::as_string)!=Some(value.as_str()){return Err(());}
        }
        let package:Value=serde_json::from_slice(&proof.public_bytes("Contents/Resources/app/package.json",budget)?).map_err(|_|())?;
        let product:Value=serde_json::from_slice(&proof.public_bytes("Contents/Resources/app/product.json",budget)?).map_err(|_|())?;
        for(key,value)in [("name",p.bundle_name.as_str()),("version",c.base_version.as_str()),("main",c.package_main.as_str())]{
            if package.get(key).and_then(Value::as_str)!=Some(value){return Err(());}
        }
        for(key,value)in [("nameShort",p.bundle_name.as_str()),("nameLong",p.bundle_name.as_str()),("applicationName",p.application_name.as_str()),
            ("dataFolderName",p.data_folder.as_str()),("genieVersion",c.version.as_str()),("version",c.base_version.as_str()),("commit",c.commit.as_str())]{
            if product.get(key).and_then(Value::as_str)!=Some(value){return Err(());}
        }
        let genie:Value=serde_json::from_slice(&proof.public_bytes(&format!("{INNER_ROOT}/package.json"),budget)?).map_err(|_|())?;
        if vscode_extensions::package_id(&genie)?!="Tencent-Cloud.coding-copilot"
            ||genie.get("version").and_then(Value::as_str)!=Some(c.genie_version.as_str())
            ||genie.get("main").and_then(Value::as_str)!=Some("out/extension/index.js"){return Err(());}
        let native:Value=serde_json::from_slice(&proof.public_bytes(&format!("{INNER_ROOT}/product.json"),budget)?).map_err(|_|())?;
        // Inner ClientProductProvider finds this product before any outer IDE
        // product. Its default has no dataFolderName: both Genie roots fall
        // back to .codebuddy. No current remote/cache product is read here.
        if native.get("dataFolderName").is_some()||native.get("skillScanDirs")!=Some(&serde_json::to_value(&c.skill_scan_dirs).map_err(|_|())?){return Err(());}
        let requests=native.get("builtInMarketPlugins").and_then(Value::as_array).ok_or(())?;
        if requests.len()!=c.declared_plugins.len(){return Err(());}
        for request in &c.declared_plugins{
            if !requests.iter().any(|r|r.get("name").and_then(Value::as_str)==Some(request.name.as_str())
                &&r.get("marketplaceName").and_then(Value::as_str)==Some(request.marketplace.as_str())
                &&r.get("scope").and_then(Value::as_str)==Some("user")&&r.get("autoEnable").and_then(Value::as_bool)==Some(request.auto_enable)){return Err(());}
        }
        let markets=native.get("builtInMarketplaces").and_then(Value::as_object).ok_or(())?;
        if markets.len()!=2||!["codebuddy-plugins-official","cb_teams_marketplace"].iter().all(|name|markets.get(*name).is_some_and(|m|m.get("source").and_then(Value::as_str)==Some("zip"))){return Err(());}
        proof.revalidate(host)?;Ok(Some(proof))
    }
    fn public_bytes(&mut self,path:&str,budget:&mut IdentityBudget)->Result<Vec<u8>,()>{
        let path=self.app.path.join(path);let file=self.guards.iter_mut().find(|f|f.path==path).ok_or(())?;
        let count=usize::try_from(file.stamp.bytes).map_err(|_|())?;budget.metadata_bytes=budget.metadata_bytes.checked_add(count).ok_or(())?;
        if budget.metadata_bytes>MAX_IDENTITY_METADATA{return Err(());}
        file.bytes(MAX_DOCUMENT_BYTES)
    }
    fn sources(&self)->Vec<String>{self.guards.iter().map(|f|collapse_home(&f.path)).collect()}
    fn revalidate(&self,host:&Host)->Result<(),()>{
        if select_app(host)?.as_ref()!=Some(&self.app.path){return Err(());}
        self.app.revalidate()?;for file in &self.guards{file.revalidate()?;}self.app.revalidate()
    }
}
fn clean_relative(root:&Path,input:&str)->Result<PathBuf,()>{
    if input.is_empty()||input.len()>4096||input.contains('\\')||input.contains("://")||input.chars().any(char::is_control){return Err(());}
    let input=Path::new(input);if input.is_absolute()||input.components().any(|p|matches!(p,Component::ParentDir|Component::Prefix(_)|Component::RootDir)){return Err(());}
    let mut path=root.to_path_buf();for part in input.components(){if let Component::Normal(name)=part{path.push(name);}}
    checked_root(&path)?;Ok(path)
}
fn confined_absolute(root:&Path,input:&str)->Result<PathBuf,()>{
    let path=Path::new(input);if !path.is_absolute(){return Err(());}
    let tail=path.strip_prefix(root).map_err(|_|())?.to_str().ok_or(())?;
    if tail.is_empty(){checked_root(root)?;Ok(root.to_path_buf())}else{clean_relative(root,tail)}
}
fn component(components:&mut Vec<PluginComponent>,kind:PluginComponentKind,name:&str,path:&Path,native_id:Option<String>,budget:&mut MetadataBudget)->Result<(),()>{
    budget.component()?;components.push(PluginComponent{kind,name:metadata_name(name).ok_or(())?,source_path:collapse_home(path),native_id});Ok(())
}
fn observation(host:&Host,proof:&VerifiedApp,id:String,name:String,version:String,presence:PluginPresence,
    mut sources:Vec<String>,mut components:Vec<PluginComponent>,context:&str)->Result<NativePluginObservation,()>{
    sources.sort();sources.dedup();components.sort();components.dedup();
    let hash=hex::encode(Sha256::digest(serde_json::to_vec(&(&id,&version,&sources,&components,context)).map_err(|_|())?));
    Ok(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id,name,version,revision:format!("host:{};build:{};{context};metadata-sha256:{hash}",proof.version,proof.commit),
        presence,enabled:None,source_paths:sources,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()})
}
fn extension_components(root:&Path,value:&Value,budget:&mut MetadataBudget)->Result<Vec<PluginComponent>,()>{
    let mut components=Vec::new();let path=root.join("package.json");let id=vscode_extensions::package_id(value)?;
    component(&mut components,PluginComponentKind::Extension,&id,&path,Some(id.clone()),budget)?;
    for(field,key,kind)in [("languageModelTools","name",PluginComponentKind::Tool),("mcpServerDefinitionProviders","id",PluginComponentKind::McpReference)]{
        if let Some(items)=value.get("contributes").and_then(|v|v.get(field)){
            for item in items.as_array().filter(|a|a.len()<=MAX_REGISTRATIONS).ok_or(())?{
                let name=item.get(key).and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
                component(&mut components,kind.clone(),&name,&path,Some(name.clone()),budget)?;
            }
        }
    }
    // contributes.commands are ordinary editor commands. No entrypoint runs,
    // no Tool gating expression is evaluated, no MCP provider is instantiated.
    Ok(components)
}
fn extension(host:&Host,proof:&VerifiedApp,root:&Path,value:&Value,sources:Vec<String>,presence:PluginPresence,budget:&mut MetadataBudget)->Result<NativePluginObservation,()>{
    let id=vscode_extensions::package_id(value)?;let version=value.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    let name=value.get("displayName").and_then(Value::as_str).filter(|v|!v.starts_with('%')).and_then(metadata_name).unwrap_or_else(||id.clone());
    observation(host,proof,id,name,version,presence,sources,extension_components(root,value,budget)?,"ordinary-IDE-extension;default-profile!=active;Tool-gate=unknown;activation=unknown")
}
fn bundled(host:&Host,proof:&VerifiedApp,c:&Contract,p:&Profile,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<(),()>{
    let root=proof.app.path.join(APP_RESOURCES).join("extensions");
    let expected:BTreeSet<_>=p.builtin_extensions.iter().filter_map(|e|Path::new(&e.manifest.path).parent().and_then(Path::file_name).and_then(|v|v.to_str())).collect();
    for entry in snapshot.directories(&root,budget)?{
        let name=entry.path.file_name().and_then(|v|v.to_str()).ok_or(())?;
        if entry.directory&&name!="node_modules"&&!expected.contains(name){snapshot.incomplete=true;}
    }
    for e in &p.builtin_extensions{
        let item=(||{
            let value=snapshot.guarded_json(&proof.app.path,&e.manifest,budget)?;
            if vscode_extensions::package_id(&value)?!=e.id||value.get("version").and_then(Value::as_str)!=Some(e.version.as_str()){return Err(());}
            let manifest=proof.app.path.join(&e.manifest.path);let mut sources=proof.sources();sources.push(collapse_home(&manifest));
            extension(host,proof,manifest.parent().ok_or(())?,&value,sources,PluginPresence::Bundled,budget)
        })();match item{Ok(item)=>snapshot.plugins.push(item),Err(())=>snapshot.incomplete=true}
    }
    for group in &c.skill_groups{
        let root=proof.app.path.join(&group.root);let expected:BTreeSet<_>=group.skills.iter().map(|s|s.name.as_str()).collect();
        for entry in snapshot.directories(&root,budget)?{if entry.directory&&!expected.contains(entry.path.file_name().and_then(|v|v.to_str()).ok_or(())?){snapshot.incomplete=true;}}
        let mut components=Vec::new();let mut identities=Vec::new();
        for skill in &group.skills{
            let path=clean_relative(&root,&skill.name)?.join("SKILL.md");
            match snapshot.stat_file(&path,budget){
                Ok(Some(stamp))if stamp.bytes==skill.bytes=>{component(&mut components,PluginComponentKind::Skill,&skill.name,&path,None,budget)?;identities.push((path,stamp));},
                _=>snapshot.incomplete=true,
            }
        }
        let availability=hex::encode(Sha256::digest(serde_json::to_vec(&identities).map_err(|_|())?));let mut sources=proof.sources();sources.push(collapse_home(&root));
        for tool in &group.tools{
            let source=clean_relative(&proof.app.path,&tool.source)?;
            sources.push(collapse_home(&source));component(&mut components,PluginComponentKind::Tool,&tool.name,&source,None,budget)?;
        }
        snapshot.plugins.push(observation(host,proof,group.id.clone(),format!("{} {}",p.bundle_name,group.label),c.version.clone(),PluginPresence::Bundled,sources,components,
            &format!("application-owned;Skill-body/frontmatter=unread;Skill-native-name=unknown;Tool-registration=unknown;gate:{};loading=unknown;availability:{availability}",group.gate))?);
    }
    let source=proof.app.path.join(INNER_ROOT).join("product.json");
    for request in &c.declared_plugins{
        let id=format!("{}@{}",request.name,request.marketplace);let mut sources=proof.sources();sources.push(collapse_home(&source));let mut components=Vec::new();
        component(&mut components,PluginComponentKind::PluginReference,&request.name,&source,Some(id.clone()),budget)?;
        snapshot.plugins.push(observation(host,proof,id,request.name.clone(),String::new(),PluginPresence::Declared,sources,components,
            &format!("public-product-install-request;scope=user;requested-auto-enable:{};installation=unknown;activation=unknown",request.auto_enable))?);
    }Ok(())
}

#[derive(Clone,PartialEq,Eq)]
struct DefaultRoots{extensions:PathBuf,native:PathBuf,marketplaces:PathBuf}
fn default_roots(p:&Profile)->Result<DefaultRoots,()>{
    if ![".codebuddy",".codebuddycn"].contains(&p.data_folder.as_str()){return Err(());}
    let native=expand_tilde("~/.codebuddy");
    Ok(DefaultRoots{extensions:expand_tilde(&format!("~/{}/extensions",p.data_folder)),marketplaces:native.join("plugins/marketplaces"),native})
}
fn ordinary_allowed()->bool{!ORDINARY_OVERRIDES.iter().any(|key|std::env::var_os(key).is_some_and(|v|!v.is_empty()))}
fn uri_object(root:&Path,value:&Value)->Result<PathBuf,()>{
    let object=value.as_object().ok_or(())?;
    if object.get("scheme").and_then(Value::as_str)!=Some("file"){return Err(());}
    for key in ["authority","query","fragment"]{if object.get(key).is_some_and(|v|!v.as_str().is_some_and(str::is_empty)){return Err(());}}
    confined_absolute(root,object.get("path").and_then(Value::as_str).ok_or(())?)
}
fn reference(host:&Host,proof:&VerifiedApp,id:String,name:String,version:String,presence:PluginPresence,source:&Path,context:&str,budget:&mut MetadataBudget)->Result<NativePluginObservation,()>{
    let mut components=Vec::new();component(&mut components,PluginComponentKind::PluginReference,&name,source,Some(id.clone()),budget)?;
    let mut sources=proof.sources();sources.push(collapse_home(source));observation(host,proof,id,name,version,presence,sources,components,context)
}
fn ordinary_registrations(host:&Host,proof:&VerifiedApp,roots:&DefaultRoots,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<(),()>{
    let ledger=roots.extensions.join("extensions.json");let Some(value)=snapshot.read_document(&ledger,false,budget)?else{return Ok(());};
    for row in value.as_array().filter(|v|v.len()<=MAX_REGISTRATIONS).ok_or(())?{
        budget.visit()?;
        let item=(||{
            let id=vscode_extensions::identifier(row.pointer("/identifier/id").and_then(Value::as_str).ok_or(())?)?;
            let version=row.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
            let resolved=(||{
                let location=row.get("location").ok_or(())?;
                if !(location.as_str().is_some_and(|v|!v.is_empty())||location.as_object().is_some_and(|v|v.get("path").is_some_and(Value::is_string)&&v.get("scheme").is_some_and(Value::is_string)))
                    ||row.get("relativeLocation").is_some_and(|v|!v.is_string()){return Err(());}
                if row.pointer("/identifier/uuid").is_some_and(|v|!v.is_null()&&!v.is_string()){return Err(());}
                let root=if let Some(path)=row.get("relativeLocation").and_then(Value::as_str).filter(|v|!v.is_empty()){
                    clean_relative(&roots.extensions,path)?
                }else{uri_object(&roots.extensions,location)?};
                let path=root.join("package.json");let manifest=snapshot.read_document(&path,false,budget)?.ok_or(())?;
                if !vscode_extensions::package_id(&manifest)?.eq_ignore_ascii_case(&id)||manifest.get("version").and_then(Value::as_str)!=Some(version.as_str()){return Err(());}
                let mut sources=proof.sources();sources.extend([collapse_home(&ledger),collapse_home(&path)]);
                extension(host,proof,&root,&manifest,sources,PluginPresence::Installed,budget)
            })();
            match resolved{Ok(item)=>Ok(item),Err(())=>{
                snapshot.incomplete=true;reference(host,proof,id.clone(),id,version,PluginPresence::Declared,&ledger,
                    "ordinary-default-profile-registration;physical-source-unresolved;active-profile=unknown",budget)
            }}
        })();match item{Ok(item)=>snapshot.plugins.push(item),Err(())=>snapshot.incomplete=true}
    }Ok(())
}
fn path_shape(path:&Path)->Result<Option<bool>,()>{
    checked_root(path)?;match fs::symlink_metadata(path){
        Ok(m)if m.is_dir()=>Ok(Some(true)),Ok(m)if m.file_type().is_file()=>Ok(Some(false)),
        Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),_=>Err(()),
    }
}
fn native_manifest(root:&Path,file:&str,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<Option<(PathBuf,Value)>,()>{
    // First existing directory/file wins. Malformed preferred JSON does not
    // permit reading the lower-priority format as a fabricated fallback.
    for directory in [".codebuddy-plugin",".claude-plugin"]{
        let path=root.join(directory).join(file);
        if let Some(value)=snapshot.read_document(&path,false,budget)?{return Ok(Some((path,value)));}
    }Ok(None)
}
fn native_falsy(value:&Value)->bool{value.is_null()||value.as_bool()==Some(false)||value.as_f64()==Some(0.0)||value.as_str().is_some_and(str::is_empty)}
fn empty_declaration(value:&Value)->bool{native_falsy(value)||value.as_array().is_some_and(Vec::is_empty)}
fn feature_paths(root:&Path,value:Option<&Value>,fallback:&str,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Vec<PathBuf>{
    let inputs=match value{
        None=>vec![Value::String(fallback.into())],Some(v)if empty_declaration(v)=>vec![Value::String(fallback.into())],
        Some(Value::String(v))=>vec![Value::String(v.clone())],Some(Value::Array(v))if v.len()<=MAX_REGISTRATIONS=>v.clone(),
        Some(_)=>{snapshot.incomplete=true;return Vec::new();},
    };
    inputs.iter().filter_map(|v|{
        if budget.visit().is_err(){snapshot.incomplete=true;return None;}
        let path=if let Some(path)=v.as_str(){Some(path)}else{
            let selected=["file","path"].iter().find_map(|key|v.get(*key).filter(|value|!native_falsy(value)));
            selected.and_then(Value::as_str)
        };
        let Some(path)=path else{snapshot.incomplete=true;return None;};
        match clean_relative(root,path){Ok(p)=>Some(p),Err(())=>{snapshot.incomplete=true;None}}
    }).collect()
}
fn skill_file(path:&Path,components:&mut Vec<PluginComponent>,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<bool,()>{
    if snapshot.stat_file(path,budget)?.is_none(){return Ok(false);}
    let file=path.file_name().and_then(|v|v.to_str()).ok_or(())?;
    let title=if ["SKILL.md","skill.md"].contains(&file){path.parent().and_then(Path::file_name).and_then(|v|v.to_str()).ok_or(())?}else{file};
    component(components,PluginComponentKind::Skill,title,path,None,budget)?;Ok(true)
}
fn skill_candidates(paths:Vec<PathBuf>,components:&mut Vec<PluginComponent>,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<(),()>{
    let mut pending:Vec<_>=paths.into_iter().map(|p|(p,0usize)).collect();let mut seen=BTreeSet::new();
    while let Some((path,depth))=pending.pop(){
        budget.visit()?;if depth>20{return Err(());}if !seen.insert(path.clone()){continue;}
        match path_shape(&path)?{
            None=>{snapshot.missing(&path)?;},Some(false)=>{if path.extension().and_then(|v|v.to_str())==Some("md"){skill_file(&path,components,snapshot,budget)?;}},
            Some(true)=>{
                if skill_file(&path.join("SKILL.md"),components,snapshot,budget)?||skill_file(&path.join("skill.md"),components,snapshot,budget)?{continue;}
                for entry in snapshot.directories(&path,budget)?{
                    if entry.directory&&entry.path.file_name().is_none_or(|v|v!="node_modules"){pending.push((entry.path,depth+1));}
                }
            },
        }
    }Ok(())
}
fn markdown_candidates(paths:Vec<PathBuf>,kind:PluginComponentKind,components:&mut Vec<PluginComponent>,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<(),()>{
    let mut pending:Vec<_>=paths.into_iter().map(|p|(p,0usize)).collect();let mut seen=BTreeSet::new();
    while let Some((path,depth))=pending.pop(){
        budget.visit()?;if !seen.insert(path.clone()){continue;}
        match path_shape(&path)?{
            None=>{snapshot.missing(&path)?;},Some(true)=>{
                if depth>=2{continue;}for entry in snapshot.directories(&path,budget)?{
                    if entry.directory||entry.file{pending.push((entry.path,depth+1));}
                }
            },Some(false)=>{
                let ext=path.extension().and_then(|v|v.to_str());
                if ext!=Some("md")&&!(kind==PluginComponentKind::Rule&&ext==Some("mdc")){continue;}
                if snapshot.stat_file(&path,budget)?.is_some(){let name=path.file_name().and_then(|v|v.to_str()).ok_or(())?;component(components,kind.clone(),name,&path,None,budget)?;}
            },
        }
    }Ok(())
}
fn mcp_map(value:&Value,path:&Path,marketplace:&str,plugin:&str,components:&mut Vec<PluginComponent>,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<usize,()>{
    let value=value.get("mcpServers").filter(|v|!native_falsy(v)).unwrap_or(value);let map=value.as_object().filter(|m|m.len()<=MAX_REGISTRATIONS).ok_or(())?;
    for(name,config)in map{
        budget.visit()?;
        if !config.is_object(){snapshot.incomplete=true;continue;}
        let name=metadata_name(name).ok_or(())?;let id=format!("{marketplace}/{plugin}/{name}");
        component(components,PluginComponentKind::McpReference,&name,path,Some(id),budget)?;
    }Ok(map.len())
}
fn mcp_file(root:&Path,path:&Path,body_paths:Option<&BTreeSet<PathBuf>>,marketplace:&str,plugin:&str,components:&mut Vec<PluginComponent>,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<usize,()>{
    if body_paths.is_none_or(|paths|component_payload_is_opaque(root,path,paths))||!matches!(path.extension().and_then(|v|v.to_str()),Some("json"|"jsonc")){
        snapshot.incomplete=true;let name=path.file_name().and_then(|v|v.to_str()).ok_or(())?;
        component(components,PluginComponentKind::McpReference,name,path,None,budget)?;
        // An unread override has unknown keys. Do not claim that it is empty
        // and then read a different fallback source on its behalf.
        return Ok(1);
    }
    match snapshot.read_document(path,false,budget)?{Some(value)=>mcp_map(&value,path,marketplace,plugin,components,snapshot,budget),None=>Ok(0)}
}
fn plugin_mcps(root:&Path,manifest:&Value,manifest_path:&Path,body_paths:Option<&BTreeSet<PathBuf>>,marketplace:&str,plugin:&str,components:&mut Vec<PluginComponent>,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<(),()>{
    let field=manifest.get("mcpServers");
    if let Some(value)=field.filter(|v|v.as_object().is_some_and(|m|!m.is_empty())){
        mcp_map(value,manifest_path,marketplace,plugin,components,snapshot,budget)?;return Ok(());
    }
    let files=match field{None=>Vec::new(),Some(v)if empty_declaration(v)=>Vec::new(),Some(Value::String(v))=>vec![v.clone()],Some(Value::Array(v))=>{
        if v.len()>MAX_REGISTRATIONS{return Err(());}v.iter().filter_map(Value::as_str).map(str::to_string).collect()
    },Some(Value::Object(v))if v.is_empty()=>Vec::new(),Some(_)=>return Err(())};
    let mut keys=0usize;
    for file in files{
        let path=match clean_relative(root,&file){Ok(path)=>path,Err(())=>{
            snapshot.incomplete=true;keys=keys.checked_add(1).ok_or(())?;
            component(components,PluginComponentKind::McpReference,"Unresolved MCP file reference",manifest_path,None,budget)?;continue;
        }};
        match mcp_file(root,&path,body_paths,marketplace,plugin,components,snapshot,budget){Ok(count)=>keys=keys.checked_add(count).ok_or(())?,Err(())=>snapshot.incomplete=true}
    }
    if keys==0&&mcp_file(root,&root.join(".mcp.json"),body_paths,marketplace,plugin,components,snapshot,budget).is_err(){snapshot.incomplete=true;}
    // Preserve declarations from each physical file. Native readdir ordering
    // is not observed, so conflicting server names do not acquire a winner.
    for entry in snapshot.directories(&root.join("mcp"),budget)?{
        if entry.file&&entry.path.extension().and_then(|v|v.to_str())==Some("json"){
            if mcp_file(root,&entry.path,body_paths,marketplace,plugin,components,snapshot,budget).is_err(){snapshot.incomplete=true;}
        }
    }Ok(())
}
fn hook_payload_paths(root:&Path,manifest:&Value,budget:&mut MetadataBudget)->Result<BTreeSet<PathBuf>,()>{
    fn collect(root:&Path,value:&Value,paths:&mut BTreeSet<PathBuf>,budget:&mut MetadataBudget,depth:usize,visited:&mut usize)->Result<(),()>{
        budget.visit()?;*visited+=1;if depth>8||*visited>MAX_REGISTRATIONS{return Err(());}
        if native_falsy(value){return Ok(());}
        match value{
            Value::String(value)=>{paths.insert(clean_relative(root,value)?);},
            Value::Array(values)=>{if values.len()>MAX_REGISTRATIONS{return Err(());}for value in values{collect(root,value,paths,budget,depth+1,visited)?;}},
            Value::Object(value)=>for field in ["file","path"]{
                if let Some(value)=value.get(field).filter(|v|!native_falsy(v)){paths.insert(clean_relative(root,value.as_str().ok_or(())?)?);}
            },
            value if native_falsy(value)=>{},_=>return Err(()),
        }Ok(())
    }
    let mut paths=BTreeSet::from([root.join("hooks/hooks.json")]);
    let mut visited=0;
    if let Some(value)=manifest.get("hooks"){collect(root,value,&mut paths,budget,0,&mut visited)?;}Ok(paths)
}
fn native_components(root:&Path,manifest:&Value,manifest_path:&Path,marketplace:&str,plugin:&str,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Vec<PluginComponent>{
    let mut components=Vec::new();
    // Resolve only explicit body filenames before considering any MCP source.
    // Incomplete body provenance keeps file sources opaque, never readable by
    // reclassifying a hook/account payload as MCP metadata.
    let body_paths=match hook_payload_paths(root,manifest,budget){Ok(paths)=>Some(paths),Err(())=>{snapshot.incomplete=true;None}};
    let skills=feature_paths(root,manifest.get("skills"),"skills",snapshot,budget);
    if skill_candidates(skills,&mut components,snapshot,budget).is_err(){snapshot.incomplete=true;}
    if manifest.get("skills").is_none_or(empty_declaration){
        let root_skill=(||{if !skill_file(&root.join("SKILL.md"),&mut components,snapshot,budget)?{skill_file(&root.join("skill.md"),&mut components,snapshot,budget)?;}Ok::<_,()>(())})();
        if root_skill.is_err(){snapshot.incomplete=true;}
    }
    for(field,fallback,kind)in [("commands","commands",PluginComponentKind::Command),("agents","agents",PluginComponentKind::Agent),("rules","rules",PluginComponentKind::Rule)]{
        let paths=feature_paths(root,manifest.get(field),fallback,snapshot,budget);if markdown_candidates(paths,kind,&mut components,snapshot,budget).is_err(){snapshot.incomplete=true;}
    }
    if plugin_mcps(root,manifest,manifest_path,body_paths.as_ref(),marketplace,plugin,&mut components,snapshot,budget).is_err(){snapshot.incomplete=true;}
    if let Some(hooks)=manifest.get("hooks"){
        let result=if hooks.as_object().is_some_and(|m|!m.is_empty()){component(&mut components,PluginComponentKind::Hook,"Hooks",manifest_path,None,budget)}else if hooks.is_object(){Ok(())}else{
            let paths=feature_paths(root,Some(hooks),"hooks/hooks.json",snapshot,budget);
            (||{for path in paths{if snapshot.stat_file(&path,budget)?.is_some(){let name=path.file_name().and_then(|v|v.to_str()).ok_or(())?;component(&mut components,PluginComponentKind::Hook,name,&path,None,budget)?;}}Ok::<_,()>(())})()
        };if result.is_err(){snapshot.incomplete=true;}
    }
    // Native loadHooksFromExtension also inspects its fixed default after any
    // explicit declaration. Only stat that file; never open/execute the hook.
    let path=root.join("hooks/hooks.json");match snapshot.stat_file(&path,budget){Ok(Some(_))=>{if component(&mut components,PluginComponentKind::Hook,"hooks.json",&path,None,budget).is_err(){snapshot.incomplete=true;}},Ok(None)=>{},Err(())=>snapshot.incomplete=true}
    if let Some(value)=manifest.get("lspServers").and_then(Value::as_object).filter(|v|!v.is_empty()){
        if value.len()>MAX_REGISTRATIONS{snapshot.incomplete=true;}else{for name in value.keys(){if component(&mut components,PluginComponentKind::Lsp,name,manifest_path,None,budget).is_err(){snapshot.incomplete=true;break;}}}
    }else{
        // LSP files are availability references; their body/schema is not
        // needed for this view and cannot become MCP server observations.
        let field=manifest.get("lspServers").filter(|v|!v.as_object().is_some_and(|m|m.is_empty()));
        let paths=feature_paths(root,field,".lsp.json",snapshot,budget);
        let files=(||{let mut paths=paths;for entry in snapshot.directories(&root.join("lsp"),budget)?{if entry.file&&entry.path.extension().and_then(|v|v.to_str())==Some("json"){paths.push(entry.path);}}Ok::<_,()>(paths)})();
        match files{Ok(paths)=>for path in paths{match snapshot.stat_file(&path,budget){Ok(Some(_))=>{let name=path.file_name().and_then(|v|v.to_str()).unwrap_or("LSP");if component(&mut components,PluginComponentKind::Lsp,name,&path,None,budget).is_err(){snapshot.incomplete=true;}},Ok(None)=>{},Err(())=>snapshot.incomplete=true}},Err(())=>snapshot.incomplete=true}
    }
    components.sort();components.dedup();components
}

fn native_name(value:&str)->Result<String,()>{
    let name=metadata_name(value).ok_or(())?;if name!=value{return Err(());}Ok(name)
}
fn text_field(value:&Value,key:&str)->Result<Option<String>,()>{
    match value.get(key){None|Some(Value::Null)=>Ok(None),Some(Value::String(v))if v.is_empty()=>Ok(None),Some(Value::String(v))=>Ok(Some(native_name(v)?)),Some(_)=>Err(())}
}
fn memberships(roots:&DefaultRoots,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<BTreeMap<String,bool>,()>{
    let Some(settings)=snapshot.read_document(&roots.native.join("settings.json"),false,budget)?else{return Ok(BTreeMap::new());};
    let Some(value)=settings.get("enabledPlugins")else{return Ok(BTreeMap::new());};
    let Some(map)=value.as_object()else{if !value.is_null(){snapshot.incomplete=true;}return Ok(BTreeMap::new());};
    if map.len()>MAX_REGISTRATIONS{return Err(());}let mut result=BTreeMap::new();
    for(id,enabled)in map{
        let entry=(||{native_name(id)?;let(name,market)=id.split_once('@').ok_or(())?;native_name(name)?;native_name(market)?;enabled.as_bool().ok_or(())})();
        match entry{Ok(enabled)=>{result.insert(id.clone(),enabled);},Err(())=>snapshot.incomplete=true}
    }Ok(result)
}
struct Marketplace{storage:String,name:String,root:PathBuf,manifest_path:Option<PathBuf>,manifest:Option<Value>}
fn marketplaces(roots:&DefaultRoots,known:&Value,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<Vec<Marketplace>,()>{
    let rows=known.as_object().filter(|m|m.len()<=MAX_REGISTRATIONS).ok_or(())?;let mut result=Vec::new();
    for(storage,row)in rows{
        budget.visit()?;
        let item=(||{
            let storage=native_name(storage)?;
            let kind=row.pointer("/source/source").and_then(Value::as_str).ok_or(())?;
            if !["directory","github","git","url","zip"].contains(&kind){return Err(());}
            let input=row.get("installLocation").and_then(Value::as_str).filter(|v|!v.is_empty()).ok_or(())?;
            let root=confined_absolute(&roots.marketplaces,input)?;
            if snapshot.bind(&root,true,budget)?.is_none(){return Err(());}
            let manifest=native_manifest(&root,"marketplace.json",snapshot,budget)?;
            let (name,path,value)=match manifest{
                Some((path,value))=>{
                    let name=match value.get("name"){None|Some(Value::Null)=>storage.clone(),Some(Value::String(v))if v.is_empty()=>storage.clone(),Some(Value::String(v))=>native_name(v)?,Some(_)=>return Err(())};
                    (name,Some(path),Some(value))
                },None=>(storage.clone(),None,None),
            };
            Ok::<_,()>(Marketplace{storage,name,root,manifest_path:path,manifest:value})
        })();match item{Ok(market)=>result.push(market),Err(())=>snapshot.incomplete=true}
    }Ok(result)
}
fn git_string(input:&str)->bool{
    // Exact supported native forms from module 729907: HTTP(S), git, ssh
    // ending .git (optional query/fragment), or user@host:path.git. Generic
    // protocol strings such as file:// or HTTPS ZIPs are not this source type.
    if input.chars().any(|c|c.is_whitespace()||c.is_control()||c=='\u{feff}'){return false;}
    let main=input.split(['?','#']).next().unwrap_or(input);let lower=main.to_ascii_lowercase();
    for scheme in ["https://","http://","git://","ssh://"]{
        if let Some(body)=lower.strip_prefix(scheme){return body.strip_suffix(".git").is_some_and(|v|!v.is_empty());}
    }
    let Some((user,tail))=main.split_once('@')else{return false;};let Some((host,path))=tail.split_once(':')else{return false;};
    !user.is_empty()&&!user.contains('@')&&!host.is_empty()&&!host.contains(':')&&path.strip_suffix(".git").is_some_and(|v|!v.is_empty())
}
fn plugin_destination(market:&Marketplace,catalog:&Value)->Result<PathBuf,()>{
    let name=catalog.get("name").and_then(Value::as_str).ok_or(())?;native_name(name)?;
    let source=catalog.get("source").ok_or(())?;
    let local=if let Some(input)=source.as_str(){
        if git_string(input){None}else if input.contains("://"){return Err(());}else{Some(input)}
    }else{match source.get("source").and_then(Value::as_str){
        Some("local")=>Some(source.get("path").and_then(Value::as_str).ok_or(())?),
        Some("github"|"url"|"zip")=>None,_=>return Err(()),
    }};
    match local{
        Some(path)if Path::new(path).is_absolute()=>confined_absolute(&market.root,path),
        Some(path)=>clean_relative(&market.root,path),
        None=>{
            if name.len()>240||name.contains('/')||name.contains('\\')||matches!(name,"."|".."){return Err(());}
            clean_relative(&market.root,&format!("plugins/{name}"))
        },
    }
}
fn native_candidate(host:&Host,proof:&VerifiedApp,market:&Marketplace,catalog:&Value,registry:&Path,settings:&Path,
    snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<NativePluginObservation,()>{
    let catalog_name=catalog.get("name").and_then(Value::as_str).map(native_name).transpose()?.ok_or(())?;
    let catalog_version=text_field(catalog,"version")?.unwrap_or_default();
    let base_context=format!("native-default-namespace;storage-name:{};marketplace-name:{};catalog-name:{};effective-enabled=unknown;active-product-root=unknown",
        market.storage,market.name,catalog_name);
    let resolved=(||{
        let root=plugin_destination(market,catalog)?;
        if snapshot.bind(&root,true,budget)?.is_none(){return Err(());}
        let (path,manifest)=native_manifest(&root,"plugin.json",snapshot,budget)?.ok_or(())?;
        let name=match manifest.get("name"){None=>catalog_name.clone(),Some(Value::String(v))=>native_name(v)?,Some(_)=>return Err(())};
        let(version,version_source)=match manifest.get("version"){
            None=>(catalog_version.clone(),if catalog_version.is_empty(){"unknown"}else{"catalog"}),
            Some(Value::Null)=>(String::new(),"unknown"),Some(Value::String(v))if v.is_empty()=>(String::new(),"unknown"),
            Some(Value::String(v))=>(native_name(v)?,"native-manifest"),Some(_)=>return Err(()),
        };
        let id=format!("{}@{}",name,market.name);
        let components=native_components(&root,&manifest,&path,&market.name,&name,snapshot,budget);
        let mut sources=proof.sources();sources.extend([collapse_home(registry),collapse_home(settings),collapse_home(&path)]);
        if let Some(path)=&market.manifest_path{sources.push(collapse_home(path));}
        observation(host,proof,id,name,version,PluginPresence::CacheOnly,sources,components,
            &format!("{base_context};current-confined-native-manifest;version-source:{version_source};Skill-body/frontmatter=unread;filename-availability;activation=unknown"))
    })();
    match resolved{Ok(item)=>Ok(item),Err(())=>{
        snapshot.incomplete=true;reference(host,proof,format!("{}@{}",catalog_name,market.name),catalog_name,catalog_version,
            PluginPresence::Declared,market.manifest_path.as_deref().unwrap_or(registry),&format!("{base_context};physical-source-unresolved;reference-only"),budget)
    }}
}
fn cache_references(host:&Host,proof:&VerifiedApp,market:&Marketplace,physical:&BTreeSet<String>,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<(),()>{
    let path=market.root.join(".plugins-cache.json");let Some(value)=snapshot.read_document(&path,false,budget)?else{return Ok(());};
    if value.get("version").and_then(Value::as_u64)!=Some(1)||value.get("marketplaceName").is_some_and(|v|v.as_str()!=Some(market.name.as_str())){return Err(());}
    let rows=value.get("plugins").and_then(Value::as_array).filter(|v|v.len()<=MAX_REGISTRATIONS).ok_or(())?;
    for row in rows{
        budget.visit()?;let name=row.get("name").and_then(Value::as_str).map(native_name).transpose()?.ok_or(())?;
        let id=format!("{}@{}",name,market.name);if physical.contains(&id){continue;}
        let version=text_field(row,"version")?.unwrap_or_default();
        // Never follow cached installedPath/skills/commands/MCP paths. Cache
        // validation and actual registration remain different facts.
        snapshot.plugins.push(reference(host,proof,id,name,version,PluginPresence::CacheOnly,&path,
            &format!("native-capability-cache:1;storage-name:{};marketplace-name:{};current-membership/manifest=unverified;cached-component-paths=unread;effective-enabled=unknown",market.storage,market.name),budget)?);
    }Ok(())
}
fn native_global(host:&Host,proof:&VerifiedApp,roots:&DefaultRoots,snapshot:&mut Snapshot,budget:&mut MetadataBudget)->Result<(),()>{
    let settings=roots.native.join("settings.json");let registry=roots.native.join("plugins/known_marketplaces.json");
    let registered=memberships(roots,snapshot,budget)?;let mut unresolved=registered.clone();
    let known=snapshot.read_document(&registry,false,budget)?;
    let markets=match known{Some(value)=>marketplaces(roots,&value,snapshot,budget)?,None=>Vec::new()};
    let mut names=BTreeMap::<String,usize>::new();for market in &markets{*names.entry(market.name.clone()).or_default()+=1;}
    for market in markets{
        let mut candidates=Vec::new();
        if let Some(manifest)=&market.manifest{
            match manifest.get("plugins").and_then(Value::as_array).filter(|v|v.len()<=MAX_REGISTRATIONS){
                Some(rows)=>for row in rows{
                    budget.visit()?;match native_candidate(host,proof,&market,row,&registry,&settings,snapshot,budget){Ok(item)=>candidates.push(item),Err(())=>snapshot.incomplete=true}
                },None=>snapshot.incomplete=true,
            }
        }
        let mut counts=BTreeMap::<String,usize>::new();for item in &candidates{if item.presence==PluginPresence::CacheOnly{*counts.entry(item.plugin_id.clone()).or_default()+=1;}}
        let mut physical=BTreeSet::new();
        for mut item in candidates{
            if item.presence==PluginPresence::CacheOnly{
                physical.insert(item.plugin_id.clone());
                if let Some(enabled)=registered.get(&item.plugin_id){
                    if names.get(&market.name)==Some(&1)&&counts.get(&item.plugin_id)==Some(&1){
                        item.presence=PluginPresence::Installed;item.revision.push_str(&format!(";scope=user;user-configured-enabled:{enabled};effective-enabled=unknown"));unresolved.remove(&item.plugin_id);
                    }else{snapshot.incomplete=true;item.revision.push_str(";membership-resolution=ambiguous");}
                }
            }
            snapshot.plugins.push(item);
        }
        if cache_references(host,proof,&market,&physical,snapshot,budget).is_err(){snapshot.incomplete=true;}
    }
    for(id,enabled)in unresolved{
        let name=id.split_once('@').map(|(name,_)|name).ok_or(())?.to_string();
        snapshot.plugins.push(reference(host,proof,id,name,String::new(),PluginPresence::Declared,&settings,
            &format!("native-user-membership;user-configured-enabled:{enabled};physical-source-unresolved;effective-enabled=unknown;reference-only"),budget)?);
        snapshot.incomplete=true;
    }Ok(())
}

struct Audit{proof:VerifiedApp,snapshot:Snapshot,roots:DefaultRoots,ordinary:bool}
impl Audit{
    fn revalidate(&self,host:&Host)->Result<(),()>{
        self.snapshot.revalidate()?;self.proof.revalidate(host)?;
        let data_folder=match host.agent_id.as_str(){"codebuddy-ide"=>".codebuddy","codebuddy-cn-ide"=>".codebuddycn",_=>return Err(())};
        let native=expand_tilde("~/.codebuddy");
        if self.roots.extensions!=expand_tilde(&format!("~/{data_folder}/extensions"))||self.roots.native!=native
            ||self.roots.marketplaces!=native.join("plugins/marketplaces")||self.ordinary!=ordinary_allowed(){return Err(());}
        self.snapshot.revalidate()?;self.proof.revalidate(host)
    }
}
fn capture(host:&Host,c:&Contract,cache:&Mutex<DigestCache>)->Result<Option<Audit>,()>{
    let p=c.profile(&host.agent_id).ok_or(())?;
    let Some(proof)=VerifiedApp::capture(host,c,p,cache,&mut IdentityBudget::default())?else{return Ok(None);};
    let mut snapshot=Snapshot::default();let mut budget=MetadataBudget::default();
    bundled(host,&proof,c,p,&mut snapshot,&mut budget)?;
    let roots=default_roots(p)?;let ordinary=ordinary_allowed();
    if ordinary{if ordinary_registrations(host,&proof,&roots,&mut snapshot,&mut budget).is_err(){snapshot.incomplete=true;}}else{snapshot.incomplete=true;}
    if native_global(host,&proof,&roots,&mut snapshot,&mut budget).is_err(){snapshot.incomplete=true;}
    // The revision binds all metadata and stat-only availability to the actual
    // FD/path stamps, without exposing unrelated native configuration values.
    let files:Vec<_>=snapshot.bound.iter().map(|f|(&f.path,&f.stamp)).collect();
    let leaves:Vec<_>=snapshot.leaves.iter().map(|f|(&f.path,&f.stamp)).collect();
    let digest=hex::encode(Sha256::digest(serde_json::to_vec(&(files,leaves,&snapshot.missing)).map_err(|_|())?));
    for plugin in &mut snapshot.plugins{plugin.revision.push_str(&format!(";snapshot-sha256:{digest}"));}
    let audit=Audit{proof,snapshot,roots,ordinary};audit.revalidate(host)?;Ok(Some(audit))
}
fn observe_contract(host:&Host,c:&Contract,cache:&Mutex<DigestCache>,result:&mut PluginScan)->Result<(),()>{
    let Some(audit)=capture(host,c,cache)?else{return Ok(());};audit.revalidate(host)?;
    result.unavailable|=audit.snapshot.incomplete;result.plugins.extend(audit.snapshot.plugins);Ok(())
}
fn cache()->&'static Mutex<DigestCache>{static CACHE:OnceLock<Mutex<DigestCache>>=OnceLock::new();CACHE.get_or_init(Mutex::default)}
pub(super)fn observe(host:&Host,result:&mut PluginScan){
    let Some(c)=contract()else{result.unavailable=true;return;};if observe_contract(host,c,cache(),result).is_err(){result.unavailable=true;}
}
fn watch_paths_contract(host:&Host,c:&Contract,cache:&Mutex<DigestCache>)->Vec<PathBuf>{
    if c.validate().is_err(){return Vec::new();}
    let Some(p)=c.profile(&host.agent_id)else{return Vec::new();};let Ok(apps)=host_paths(host)else{return Vec::new();};let mut paths=BTreeSet::new();
    for app in apps{
        paths.insert(app.clone());for guard in &p.guards{let path=app.join(&guard.path);if checked_root(&path).is_ok(){paths.insert(path);}}
        for e in &p.builtin_extensions{let path=app.join(&e.manifest.path);if checked_root(&path).is_ok(){paths.insert(path);}}
        for group in &c.skill_groups{let path=app.join(&group.root);if checked_root(&path).is_ok(){paths.insert(path);}}
    }
    // User namespaces are watched only after the selected App's public guards
    // pass. Unknown versions do not reach a private registry or native helper.
    if let Ok(Some(audit))=capture(host,c,cache){if audit.revalidate(host).is_ok(){paths.extend(audit.snapshot.watches);}}
    paths.into_iter().collect()
}
pub(super)fn watch_paths(host:&Host)->Vec<PathBuf>{contract().map(|c|watch_paths_contract(host,c,cache())).unwrap_or_default()}

/// Optional App-version route: guard-only, never a config/registry read.
pub(super)fn runtime_version(selected:&Path)->Option<String>{
    let c=contract()?;
    for p in &c.profiles{
        let host=Host{agent_id:p.agent_id.clone(),adapter:"codebuddy_desktop".into(),docs_url:String::new(),manifest_paths:Vec::new(),default_mcp_files:Vec::new(),mcp_id_prefix:String::new(),resource_groups:Vec::new(),
            roots:[format!("/Applications/{}",p.app_name),format!("~/Applications/{}",p.app_name)].into_iter().map(|path|Root{path,layout:"codebuddy_app".into(),presence:PluginPresence::Bundled,complete_marker:None}).collect()};
        let paths=host_paths(&host).ok()?;
        if !paths.iter().any(|path|selected==path||selected==path.join("Contents/MacOS/Electron")){continue;}
        let proof=VerifiedApp::capture(&host,c,p,cache(),&mut IdentityBudget::default()).ok()??;
        if selected!=proof.app.path&&selected!=proof.app.path.join("Contents/MacOS/Electron"){return None;}
        proof.revalidate(&host).ok()?;return Some(proof.version);
    }None
}

#[cfg(test)]
mod tests;

