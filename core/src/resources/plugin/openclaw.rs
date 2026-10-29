//! Source-bound bundled metadata for two independent OpenClaw runtime surfaces.
//! No user ledger/configuration, Skill body, loader import or target execution.
use super::*;
use std::collections::VecDeque;
use std::io::{Seek, SeekFrom};
use std::sync::Mutex;

const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LARGE_BYTES: u64 = 192 * 1024 * 1024;
const MAX_VISITS: usize = 4096;
const MAX_MANIFESTS: usize = 128;
const MAX_SKILLS: usize = 128;
const MAX_SKILL_BYTES: u64 = 256 * 1024;
const MAX_DECLARED_SKILL_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 8;
const WORKER_ROOT: &str = "Contents/Resources/node-worker/arm64/lib/node_modules/openclaw";
const SHARED_GUARDS: [&str; 6] = ["dist/plugins/loader.js", "dist/loader-CLZgaF-g.mjs",
    "dist/loader-runtime-load-DCwhg2IV.mjs", "dist/discovery-BePzXbRd.mjs",
    "dist/manifest-registry-B7FXMCG4.mjs", "dist/bundled-dir-D7bUjZsk.mjs"];
const CLI_GUARDS: [&str; 10] = ["package.json", "dist/build-info.json", "openclaw.mjs",
    "dist/entry.js", "dist/cli/run-main.js", "node-compile-cache.mjs",
    "node-host-launcher.mjs", "node-runtime-recovery.mjs", "node-version.mjs", "node-sqlite.mjs"];
const DESKTOP_GUARDS: [&str; 6] = ["package.json", "dist/build-info.json", "dist/mac-node-worker.js",
    "dist/worker-DkjCpKLE.mjs", "dist/startup-state-readiness-ucrJBHoD.mjs", "dist/plugin-node-host-BnQgRw6P.mjs"];

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard { path: String, bytes: u64, sha256: String }
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    id: String, manifest: Guard, package: Option<Guard>, skill_leaves: Vec<String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    agent_id: String, layout: String, version: String, commit: String,
    built_at: String, build_id: String, entry: String,
    guards: Vec<Guard>, manifests: Vec<Manifest>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct App {
    bundle_id: String, bundle_name: String, executable: String, build_number: String,
    worker_root: String, info: Guard, main: Guard,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract { schema_version: u32, managed_node: String, app: App, cli: Profile, desktop: Profile }

fn contract() -> Option<&'static Contract> {
    static VALUE: OnceLock<Option<Contract>> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-openclaw.json"))
        .ok().filter(|c| c.validate().is_ok())).as_ref()
}
fn clean(value: &str) -> Result<PathBuf, ()> {
    let value = value.strip_prefix("./").unwrap_or(value);
    if value.is_empty() || value.len() > 512 || value.contains('\\') || value.chars().any(char::is_control)
        || value.split('/').any(|p| p.is_empty() || matches!(p, "." | "..")) { return Err(()); }
    let path=Path::new(value);
    if path.components().any(|p| !matches!(p,Component::Normal(_))) { return Err(()); }
    Ok(path.into())
}
fn guard_valid(guard: &Guard, limit: u64) -> Result<(), ()> {
    clean(&guard.path)?;
    if guard.bytes == 0 || guard.bytes > limit || guard.sha256.len()!=64
        || !guard.sha256.bytes().all(|c|c.is_ascii_hexdigit()) { return Err(()); }
    Ok(())
}
fn safe_text(value: &str) -> Result<String, ()> {
    metadata_name(value).filter(|v|v.as_str()==value).ok_or(())
}
impl Contract {
    fn validate(&self) -> Result<(), ()> {
        if self.schema_version!=1 || !self.managed_node.starts_with("node-v")
            || self.managed_node.len()>64 || self.managed_node.len()<7
            || !self.managed_node.bytes().all(|c|c.is_ascii_alphanumeric()||b".-".contains(&c))
            || self.app.bundle_id!="ai.openclaw.mac" || self.app.bundle_name!="OpenClaw"
            || self.app.executable!="OpenClaw" || self.app.worker_root!=WORKER_ROOT
            || self.app.info.path!="Contents/Info.plist" || self.app.main.path!="Contents/MacOS/OpenClaw" { return Err(()); }
        safe_text(&self.app.build_number)?;guard_valid(&self.app.info,1_048_576)?;guard_valid(&self.app.main,MAX_LARGE_BYTES)?;
        for (profile,id,layout,entry,extra) in [
            (&self.cli,"openclaw","openclaw_cli","openclaw.mjs",CLI_GUARDS.as_slice()),
            (&self.desktop,"openclaw-desktop","openclaw_app","dist/mac-node-worker.js",DESKTOP_GUARDS.as_slice()),
        ] {
            if profile.agent_id!=id || profile.layout!=layout || profile.entry!=entry
                || profile.commit.len()!=40 || !profile.commit.bytes().all(|c|c.is_ascii_hexdigit())
                || profile.manifests.is_empty() || profile.manifests.len()>MAX_MANIFESTS { return Err(()); }
            for text in [&profile.version,&profile.built_at,&profile.build_id] { safe_text(text)?; }
            let required=SHARED_GUARDS.iter().chain(extra).copied().collect::<BTreeSet<_>>();
            if profile.guards.len()!=required.len() { return Err(()); }
            let mut paths=BTreeSet::new();let mut total=0u64;
            for guard in &profile.guards {
                guard_valid(guard,1_048_576)?;
                if !required.contains(guard.path.as_str()) || !paths.insert(guard.path.as_str()) { return Err(()); }
                total=total.checked_add(guard.bytes).ok_or(())?;
            }
            if total>MAX_METADATA_BYTES { return Err(()); }
            let mut ids=BTreeSet::new();let mut leaves=BTreeSet::new();
            for manifest in &profile.manifests {
                if vscode_extensions::identifier(&manifest.id)?!=manifest.id || matches!(manifest.id.as_str(),"."|"..")
                    || !ids.insert(manifest.id.as_str())
                    || manifest.manifest.path!=format!("dist/extensions/{}/openclaw.plugin.json",manifest.id) { return Err(()); }
                guard_valid(&manifest.manifest,256*1024)?;
                if let Some(package)=&manifest.package {
                    guard_valid(package,256*1024)?;
                    if package.path!=format!("dist/extensions/{}/package.json",manifest.id) { return Err(()); }
                }
                for leaf in &manifest.skill_leaves {
                    let path=clean(leaf)?;
                    let prefix=format!("dist/extensions/{}/",manifest.id);
                    if !leaf.starts_with(&prefix) || path.file_name().and_then(|v|v.to_str())!=Some("SKILL.md")
                        || !leaves.insert(leaf.as_str()) || leaves.len()>MAX_SKILLS { return Err(()); }
                }
            }
        }
        Ok(())
    }
    fn profile<'a>(&'a self,host:&Host)->Result<&'a Profile,()> {
        self.validate()?;
        if host.adapter!="openclaw" { return Err(()); }
        let profile=match host.agent_id.as_str() {"openclaw"=>&self.cli,"openclaw-desktop"=>&self.desktop,_=>return Err(())};
        if host.roots.is_empty() || host.roots.len()>2 || host.roots.iter().any(|r|r.layout!=profile.layout) { return Err(()); }
        Ok(profile)
    }
}

#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
struct Stamp { device:u64,inode:u64,bytes:u64,mode:u32,links:u64,mtime:i64,mtime_ns:i64,ctime:i64,ctime_ns:i64 }
impl Stamp {
    fn capture(metadata:&fs::Metadata)->Result<Self,()> {
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            Ok(Self{device:metadata.dev(),inode:metadata.ino(),bytes:metadata.len(),mode:metadata.mode(),links:metadata.nlink(),
                mtime:metadata.mtime(),mtime_ns:metadata.mtime_nsec(),ctime:metadata.ctime(),ctime_ns:metadata.ctime_nsec()})
        }
        #[cfg(not(unix))] {let _=metadata;Err(())}
    }
}
#[derive(Default)]
struct Budget { visits:usize,read_bytes:u64,hashed_bytes:u64,skill_bytes:u64,skills:usize }
impl Budget {
    fn visit(&mut self)->Result<(),()> {self.visits=self.visits.checked_add(1).ok_or(())?;if self.visits>MAX_VISITS{Err(())}else{Ok(())}}
    fn read(&mut self,count:u64)->Result<(),()> {
        self.read_bytes=self.read_bytes.checked_add(count).ok_or(())?;
        if self.read_bytes>MAX_METADATA_BYTES {Err(())}else{Ok(())}
    }
}
struct Bound { path:PathBuf,file:fs::File,stamp:Stamp,directory:bool }
impl Bound {
    fn capture(path:PathBuf,directory:bool,budget:&mut Budget)->Result<Self,()> {
        budget.visit()?;checked_root(&path)?;
        let metadata=fs::symlink_metadata(&path).map_err(|_|())?;
        if if directory{!metadata.is_dir()}else{!metadata.file_type().is_file()} {return Err(());}
        let stamp=Stamp::capture(&metadata)?;
        if !directory&&stamp.links!=1 {return Err(());}
        let file=open_confined(&path,directory)?;
        if Stamp::capture(&file.metadata().map_err(|_|())?)?!=stamp {return Err(());}
        let value=Self{path,file,stamp,directory};value.revalidate()?;Ok(value)
    }
    fn revalidate(&self)->Result<(),()> {
        checked_root(&self.path)?;
        let metadata=fs::symlink_metadata(&self.path).map_err(|_|())?;
        if if self.directory{!metadata.is_dir()}else{!metadata.file_type().is_file()} {return Err(());}
        if Stamp::capture(&metadata)?!=self.stamp || Stamp::capture(&self.file.metadata().map_err(|_|())?)?!=self.stamp {return Err(());}
        Ok(())
    }
    fn guarded(&mut self,guard:&Guard,budget:&mut Budget)->Result<Vec<u8>,()> {
        if self.directory||self.stamp.bytes!=guard.bytes||guard.bytes>1_048_576 {return Err(());}
        budget.read(guard.bytes)?;self.file.seek(SeekFrom::Start(0)).map_err(|_|())?;
        let mut bytes=vec![0;usize::try_from(guard.bytes).map_err(|_|())?];
        self.file.read_exact(&mut bytes).map_err(|_|())?;self.revalidate()?;
        if hex::encode(Sha256::digest(&bytes))!=guard.sha256{return Err(());}Ok(bytes)
    }
}
#[derive(Clone,PartialEq,Eq)]
struct DigestKey { context:String,root:PathBuf,root_stamp:Stamp,path:PathBuf,stamp:Stamp,expected:String }
#[derive(Default)]
struct DigestCache { entries:VecDeque<DigestKey> }
impl DigestCache {
    fn contains(&mut self,key:&DigestKey)->bool {
        let Some(index)=self.entries.iter().position(|v|v==key)else{return false;};
        let value=self.entries.remove(index).expect("located digest");self.entries.push_back(value);true
    }
    fn insert(&mut self,key:DigestKey) {
        if self.contains(&key){return;}if self.entries.len()>=MAX_CACHE_ENTRIES{self.entries.pop_front();}self.entries.push_back(key);
    }
}
fn cache()->&'static Mutex<DigestCache> {static VALUE:OnceLock<Mutex<DigestCache>>=OnceLock::new();VALUE.get_or_init(Mutex::default)}
fn large_guard(file:&mut Bound,guard:&Guard,root:&Bound,context:&str,cache:&Mutex<DigestCache>,budget:&mut Budget)->Result<(),()> {
    if file.stamp.bytes!=guard.bytes{return Err(());}
    let key=DigestKey{context:context.into(),root:root.path.clone(),root_stamp:root.stamp.clone(),
        path:file.path.clone(),stamp:file.stamp.clone(),expected:guard.sha256.clone()};
    if !cache.lock().map_err(|_|())?.contains(&key) {
        if guard.bytes>MAX_LARGE_BYTES.saturating_sub(budget.hashed_bytes){return Err(());}
        file.file.seek(SeekFrom::Start(0)).map_err(|_|())?;
        let mut count=0u64;let mut digest=Sha256::new();let mut buffer=[0;65_536];
        loop {
            let size=(&mut file.file).take(guard.bytes.saturating_sub(count)+1).read(&mut buffer).map_err(|_|())?;
            if size==0{break;}count+=size as u64;budget.hashed_bytes+=size as u64;
            if count>guard.bytes||budget.hashed_bytes>MAX_LARGE_BYTES{return Err(());}digest.update(&buffer[..size]);
        }
        if count!=guard.bytes||hex::encode(digest.finalize())!=guard.sha256{return Err(());}
        root.revalidate()?;file.revalidate()?;cache.lock().map_err(|_|())?.insert(key);
    }
    root.revalidate()?;file.revalidate()
}

enum Selection {
    Cli { link:PathBuf,stamp:Stamp,target:PathBuf,text:PathBuf },
    Desktop { app:PathBuf },
}
fn app_selection(host:&Host)->Result<Option<PathBuf>,()> {
    for source in &host.roots {
        if !matches!(source.path.as_str(),"/Applications/OpenClaw.app"|"~/Applications/OpenClaw.app"){return Err(());}
    }
    vscode_extensions::select_app_for_layout(host,"openclaw_app")
}
impl Selection {
    fn capture(host:&Host,p:&Profile,c:&Contract,budget:&mut Budget)->Result<Option<(Self,PathBuf,Vec<Bound>)>,()> {
        if p.agent_id=="openclaw" {
            if host.roots.len()!=1||host.roots[0].path!="~/.openclaw/tools/node"{return Err(());}
            let tools=expand_tilde("~/.openclaw/tools");checked_root(&tools)?;
            let link=tools.join("node");
            let metadata=match fs::symlink_metadata(&link){Ok(v)=>v,Err(e)if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(_)=>return Err(())};
            if !metadata.file_type().is_symlink(){return Err(());}
            let target=tools.join(&c.managed_node);checked_root(&target)?;
            let text=fs::read_link(&link).map_err(|_|())?;
            if text!=target&&text.as_path()!=Path::new(&c.managed_node){return Err(());}
            if fs::canonicalize(&link).map_err(|_|())?!=target{return Err(());}
            let dirs=vec![Bound::capture(tools,true,budget)?,Bound::capture(target.clone(),true,budget)?];
            let selection=Self::Cli{link,stamp:Stamp::capture(&metadata)?,target:target.clone(),text};
            selection.revalidate(host,c)?;
            Ok(Some((selection,target.join("lib/node_modules/openclaw"),dirs)))
        } else {
            let Some(app)=app_selection(host)?else{return Ok(None);};
            let directory=Bound::capture(app.clone(),true,budget)?;
            Ok(Some((Self::Desktop{app:app.clone()},app.join(&c.app.worker_root),vec![directory])))
        }
    }
    fn revalidate(&self,host:&Host,c:&Contract)->Result<(),()> {
        match self {
            Self::Cli{link,stamp,target,text}=>{
                let metadata=fs::symlink_metadata(link).map_err(|_|())?;
                if !metadata.file_type().is_symlink()||Stamp::capture(&metadata)?!=*stamp
                    || fs::read_link(link).map_err(|_|())?!=*text
                    || fs::canonicalize(link).map_err(|_|())?!=*target {return Err(());}
                checked_root(target)?;
                if *target!=expand_tilde("~/.openclaw/tools").join(&c.managed_node){return Err(());}Ok(())
            },
            Self::Desktop{app}=>if app_selection(host)?.as_ref()==Some(app){Ok(())}else{Err(())},
        }
    }
}
struct Verified { selection:Selection,root:Bound,directories:Vec<Bound>,members:Vec<Bound> }
fn json_guard(root:&Path,guard:&Guard,budget:&mut Budget)->Result<(Value,Bound),()> {
    let mut file=Bound::capture(relative(root,&guard.path)?,false,budget)?;
    let value=serde_json::from_slice(&file.guarded(guard,budget)?).map_err(|_|())?;Ok((value,file))
}
impl Verified {
    fn capture(host:&Host,p:&Profile,c:&Contract,cache:&Mutex<DigestCache>,budget:&mut Budget)->Result<Option<Self>,()> {
        if c.profile(host)?.agent_id!=p.agent_id { return Err(()); }
        let Some((selection,path,directories))=Selection::capture(host,p,c,budget)?else{return Ok(None);};
        let root=Bound::capture(path,true,budget)?;
        let mut proof=Self{selection,root,directories,members:Vec::new()};
        if let Selection::Desktop{app}=&proof.selection {
            let mut info=Bound::capture(relative(app,&c.app.info.path)?,false,budget)?;
            let value=plist::Value::from_reader(std::io::Cursor::new(info.guarded(&c.app.info,budget)?)).map_err(|_|())?;
            let fields=value.as_dictionary().ok_or(())?;
            for (key,expected) in [("CFBundleIdentifier",&c.app.bundle_id),("CFBundleName",&c.app.bundle_name),
                ("CFBundleExecutable",&c.app.executable),("CFBundleShortVersionString",&p.version),
                ("CFBundleVersion",&c.app.build_number),("OpenClawGitCommit",&p.commit),
                ("OpenClawBuildTimestamp",&p.built_at),("OpenClawWorkerBuildID",&p.build_id)] {
                if fields.get(key).and_then(plist::Value::as_string)!=Some(expected.as_str()){return Err(());}
            }
            proof.members.push(info);
            let mut main=Bound::capture(relative(app,&c.app.main.path)?,false,budget)?;
            large_guard(&mut main,&c.app.main,&proof.directories[0],&p.agent_id,cache,budget)?;
            proof.members.push(main);
        }
        let mut package=None;let mut build=None;
        for guard in &p.guards {
            let mut file=Bound::capture(relative(&proof.root.path,&guard.path)?,false,budget)?;
            let bytes=file.guarded(guard,budget)?;
            if guard.path=="package.json"{package=Some(serde_json::from_slice::<Value>(&bytes).map_err(|_|())?);}
            if guard.path=="dist/build-info.json"{build=Some(serde_json::from_slice::<Value>(&bytes).map_err(|_|())?);}
            proof.members.push(file);
        }
        let package=package.ok_or(())?;let build=build.ok_or(())?;
        if package.get("name").and_then(Value::as_str)!=Some("openclaw")
            || package.get("version").and_then(Value::as_str)!=Some(p.version.as_str())
            || package.get("type").and_then(Value::as_str)!=Some("module"){return Err(());}
        if p.agent_id=="openclaw" {
            if package.pointer("/bin/openclaw").and_then(Value::as_str)!=Some(p.entry.as_str()){return Err(());}
        } else if package.get("bin").is_some(){return Err(());}
        for (key,expected) in [("version",&p.version),("commit",&p.commit),("builtAt",&p.built_at),("buildId",&p.build_id)] {
            if build.get(key).and_then(Value::as_str)!=Some(expected.as_str()){return Err(());}
        }
        proof.revalidate(host,c)?;Ok(Some(proof))
    }
    fn revalidate(&self,host:&Host,c:&Contract)->Result<(),()> {
        self.selection.revalidate(host,c)?;self.root.revalidate()?;
        for directory in &self.directories{directory.revalidate()?;}
        for file in &self.members{file.revalidate()?;}
        self.selection.revalidate(host,c)
    }
}

struct Leaf { path:PathBuf,stamp:Stamp }
impl Leaf {
    fn capture(path:PathBuf,budget:&mut Budget)->Result<Self,()> {
        budget.visit()?;checked_root(&path)?;
        let metadata=fs::symlink_metadata(&path).map_err(|_|())?;let stamp=Stamp::capture(&metadata)?;
        if !metadata.file_type().is_file()||stamp.links!=1||stamp.bytes>MAX_SKILL_BYTES{return Err(());}
        budget.skills=budget.skills.checked_add(1).ok_or(())?;
        budget.skill_bytes=budget.skill_bytes.checked_add(stamp.bytes).ok_or(())?;
        if budget.skills>MAX_SKILLS||budget.skill_bytes>MAX_DECLARED_SKILL_BYTES{return Err(());}
        Ok(Self{path,stamp})
    }
    fn revalidate(&self)->Result<(),()> {
        checked_root(&self.path)?;let metadata=fs::symlink_metadata(&self.path).map_err(|_|())?;
        if !metadata.file_type().is_file()||Stamp::capture(&metadata)?!=self.stamp{return Err(());}Ok(())
    }
}
struct PluginSnapshot { plugin:NativePluginObservation,members:Vec<Bound>,directories:Vec<Bound>,leaves:Vec<Leaf> }
impl PluginSnapshot {
    fn revalidate(&self)->Result<(),()> {
        for directory in &self.directories{directory.revalidate()?;}
        for file in &self.members{file.revalidate()?;}
        for leaf in &self.leaves{leaf.revalidate()?;}
        for directory in &self.directories{directory.revalidate()?;}Ok(())
    }
}
fn optional_text(value:&Value,key:&str)->Result<Option<String>,()> {
    value.get(key).filter(|v|!v.is_null()).map(|v|v.as_str().ok_or(()).and_then(safe_text)).transpose()
}
fn plugin_snapshot(host:&Host,p:&Profile,description:&Manifest,proof:&Verified,budget:&mut Budget)->Result<PluginSnapshot,()> {
    let root=&proof.root.path;
    let (manifest,file)=json_guard(root,&description.manifest,budget)?;
    if manifest.get("id").and_then(Value::as_str)!=Some(description.id.as_str())
        || !manifest.get("configSchema").is_some_and(Value::is_object){return Err(());}
    let mut members=vec![file];let mut sources=vec![collapse_home(&members[0].path)];
    let package=if let Some(guard)=&description.package {
        let (value,file)=json_guard(root,guard,budget)?;sources.push(collapse_home(&file.path));members.push(file);value
    }else{Value::Null};
    let name=optional_text(&manifest,"name")?.or(optional_text(&package,"displayName")?)
        .or(optional_text(&package,"name")?).unwrap_or_else(||description.id.clone());
    let version=optional_text(&manifest,"version")?.or(optional_text(&package,"version")?).unwrap_or_else(||"unknown".into());
    let mut components=vec![PluginComponent{kind:PluginComponentKind::Extension,name:description.id.clone(),
        source_path:collapse_home(&members[0].path),native_id:Some(description.id.clone())}];
    let package_dir=members[0].path.parent().ok_or(())?;
    let mut directories=Vec::new();let mut declared=Vec::new();
    if let Some(skills)=manifest.get("skills").filter(|v|!v.is_null()) {
        let skills=skills.as_array().filter(|v|v.len()<=16).ok_or(())?;
        for skill in skills {
            let path=clean(skill.as_str().ok_or(())?)?;
            let full=relative(package_dir,path.to_str().ok_or(())?)?;
            directories.push(Bound::capture(full.clone(),true,budget)?);declared.push(full);
        }
    }
    let mut leaves=Vec::new();
    for input in &description.skill_leaves {
        let path=relative(root,input)?;
        let parent=path.parent().ok_or(())?;
        let owner=declared.iter().any(|dir|parent.parent()==Some(dir.as_path()));
        if !owner{return Err(());}
        directories.push(Bound::capture(parent.to_path_buf(),true,budget)?);
        let name=parent.file_name().and_then(|v|v.to_str()).and_then(metadata_name).ok_or(())?;
        let leaf=Leaf::capture(path,budget)?;
        components.push(PluginComponent{kind:PluginComponentKind::Skill,name:name.clone(),
            source_path:collapse_home(&leaf.path),native_id:Some(name)});
        leaves.push(leaf);
    }
    if let Some(servers)=manifest.get("mcpServers").filter(|v|!v.is_null()) {
        let servers=servers.as_object().filter(|v|v.len()<=128).ok_or(())?;
        for key in servers.keys() {
            budget.visit()?;let name=safe_text(key)?;
            components.push(PluginComponent{kind:PluginComponentKind::McpReference,name:name.clone(),
                source_path:collapse_home(&members[0].path),native_id:Some(name)});
        }
    }
    components.sort();components.dedup();
    let leaf_stamps=leaves.iter().map(|leaf|(&leaf.path,&leaf.stamp)).collect::<Vec<_>>();
    let digest=hex::encode(Sha256::digest(serde_json::to_vec(&(&p.agent_id,&p.commit,
        &description.manifest.sha256,&description.package.as_ref().map(|v|&v.sha256),&components,&leaf_stamps)).map_err(|_|())?));
    for file in &proof.members {
        if file.path.ends_with("package.json")||file.path.ends_with("dist/build-info.json")||file.path.ends_with("Contents/Info.plist")
            ||file.path.ends_with("Contents/MacOS/OpenClaw")||file.path==proof.root.path.join(&p.entry){
            sources.push(collapse_home(&file.path));
        }
    }
    sources.sort();sources.dedup();
    let plugin=NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:description.id.clone(),name,version,
        revision:format!("host:{}; build:{}; metadata-sha256:{digest}",p.version,p.commit),
        presence:PluginPresence::Bundled,enabled:None,source_paths:sources,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()};
    let snapshot=PluginSnapshot{plugin,members,directories,leaves};snapshot.revalidate()?;Ok(snapshot)
}
fn observe_contract(host:&Host,c:&Contract,cache:&Mutex<DigestCache>,result:&mut PluginScan)->Result<(),()> {
    let p=c.profile(host)?;let mut budget=Budget::default();
    let Some(proof)=Verified::capture(host,p,c,cache,&mut budget)?else{return Ok(());};
    let mut snapshots=Vec::new();let mut incomplete=false;
    for description in &p.manifests {
        match plugin_snapshot(host,p,description,&proof,&mut budget) {
            Ok(snapshot)=>snapshots.push(snapshot),Err(())=>incomplete=true,
        }
    }
    for snapshot in &snapshots{snapshot.revalidate()?;}proof.revalidate(host,c)?;
    for snapshot in &snapshots{snapshot.revalidate()?;}proof.revalidate(host,c)?;
    result.plugins.extend(snapshots.into_iter().map(|v|v.plugin));result.unavailable|=incomplete;Ok(())
}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {
    let Some(c)=contract()else{result.unavailable=true;return;};
    if observe_contract(host,c,cache(),result).is_err(){result.unavailable=true;}
}
fn watch_paths_contract(host:&Host,c:&Contract)->Result<Vec<PathBuf>,()> {
    let p=c.profile(host)?;let mut paths=BTreeSet::new();let mut roots=Vec::new();
    if p.agent_id=="openclaw" {
        if host.roots.len()!=1||host.roots[0].path!="~/.openclaw/tools/node"{return Err(());}
        let tools=expand_tilde("~/.openclaw/tools");checked_root(&tools)?;paths.insert(tools.clone());
        // A rejected active link is watched through its confined parent only;
        // never register a watcher that could follow it to an unrelated tree.
        if Selection::capture(host,p,c,&mut Budget::default()).is_err(){return Ok(paths.into_iter().collect());}
        paths.insert(tools.join("node"));
        let physical=tools.join(&c.managed_node);paths.insert(physical.clone());roots.push(physical.join("lib/node_modules/openclaw"));
    }else{
        for source in &host.roots {
            if !matches!(source.path.as_str(),"/Applications/OpenClaw.app"|"~/Applications/OpenClaw.app"){return Err(());}
            let app=vscode_extensions::root_path(source);checked_root(&app)?;paths.insert(app.clone());
            paths.insert(app.join(&c.app.info.path));paths.insert(app.join(&c.app.main.path));roots.push(app.join(&c.app.worker_root));
        }
    }
    for root in roots {
        checked_root(&root)?;
        paths.insert(root.clone());
        for guard in &p.guards{paths.insert(root.join(&guard.path));}
        for manifest in &p.manifests{
            paths.insert(root.join(&manifest.manifest.path));paths.insert(root.join(&manifest.manifest.path).parent().ok_or(())?.to_path_buf());
            if let Some(package)=&manifest.package{paths.insert(root.join(&package.path));}
            for leaf in &manifest.skill_leaves{paths.insert(root.join(leaf));}
        }
    }
    if paths.len()>1024{return Err(());}Ok(paths.into_iter().collect())
}
pub(super) fn watch_paths(host:&Host)->Vec<PathBuf> {contract().and_then(|c|watch_paths_contract(host,c).ok()).unwrap_or_default()}
fn version_for(host:&Host,c:&Contract,selected:&Path)->Option<String> {
    if host.agent_id!="openclaw"||!selected.is_absolute()
        || selected.components().any(|p|matches!(p,Component::ParentDir|Component::Prefix(_))){return None;}
    let before=fs::symlink_metadata(selected).ok()?;
    if !before.file_type().is_file()&&!before.file_type().is_symlink(){return None;}
    let stamp=Stamp::capture(&before).ok()?;let physical=fs::canonicalize(selected).ok()?;
    let proof=Verified::capture(host,&c.cli,c,cache(),&mut Budget::default()).ok()??;
    if physical!=proof.root.path.join(&c.cli.entry){return None;}
    proof.revalidate(host,c).ok()?;
    if Stamp::capture(&fs::symlink_metadata(selected).ok()?).ok()?!=stamp||fs::canonicalize(selected).ok()?!=physical{return None;}
    Some(c.cli.version.clone())
}
pub(super) fn runtime_version(selected:&Path)->Option<String> {
    let c=contract()?;let host=hosts().iter().find(|h|h.agent_id=="openclaw"&&h.adapter=="openclaw")?;
    version_for(host,c,selected)
}
#[cfg(test)]
mod tests;
