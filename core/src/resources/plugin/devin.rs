//! Devin Desktop (the current Windsurf IDE), passive metadata only.
//! No target code, native scanner, migration/store helper or factory is called.
//! Public source guards and metadata/Skill-stat snapshots have separate finite
//! budgets; neither spends the allowance of another native observer.
use super::*;
use std::collections::VecDeque;
use std::io::{Seek, SeekFrom};
use std::sync::Mutex;

const MAX_IDENTITY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_IDENTITY_METADATA: usize = 2 * 1024 * 1024;
const MAX_DIGEST_CACHE: usize = 32;
const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
const MAX_VISITS: usize = 8192;
const MAX_BOUND_PATHS: usize = 1024;
const MAX_DIRECTORY_ENTRIES: usize = 1024;
const MAX_REGISTRATIONS: usize = 512;
const MAX_OWNED_COMPONENTS: usize = 4096;
const MAX_STAT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_STAT_BYTES: u64 = 64 * 1024 * 1024;
const APP_RESOURCES: &str = "Contents/Resources/app";
const SESSION_GROUP: &str = "@builtin/sessions-skills";
const PUBLIC_GUARDS: [&str; 12] = [
    "Contents/Info.plist", "Contents/MacOS/Devin",
    "Contents/Resources/app/package.json", "Contents/Resources/app/product.json",
    "Contents/Resources/app/out/main.js",
    "Contents/Resources/app/out/vs/code/electron-utility/sharedProcess/sharedProcessMain.js",
    "Contents/Resources/app/out/vs/workbench/api/node/extensionHostProcess.js",
    "Contents/Resources/app/out/vs/workbench/workbench.desktop.main.js",
    "Contents/Resources/app/out/vs/sessions/sessions.desktop.main.js",
    "Contents/Resources/app/extensions/windsurf/package.json",
    "Contents/Resources/app/extensions/windsurf/dist/extension.js",
    "Contents/Resources/app/extensions/windsurf-remote-devin/package.json",
];

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard { path: String, bytes: u64, sha256: String }
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundledExtension { id: String, version: String, manifest: Guard }
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    contract_sha256: String, archive_sha256: String, rea_parent: String, rea_summary: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    schema_version: u32, bundle_id: String, bundle_name: String, executable: String,
    version: String, base_version: String, commit: String, data_folder: String,
    user_data: String, package_main: String, guards: Vec<Guard>,
    builtin_extensions: Vec<BundledExtension>, session_skill_root: String,
    session_skills: Vec<String>, evidence: Evidence,
}
fn digest_string(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}
fn simple_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 240 && value != "." && value != ".."
        && value.bytes().all(|ch| ch.is_ascii_alphanumeric() || b"._-".contains(&ch))
}
impl Contract {
    fn validate(&self) -> Result<(), ()> {
        if self.schema_version != 1 || self.bundle_id != "com.exafunction.windsurf"
            || self.bundle_name != "Devin" || self.executable != "Devin"
            || self.data_folder != ".devin" || self.user_data != "Library/Application Support/Devin/User"
            || self.package_main != "./out/main.js"
            || self.session_skill_root != "Contents/Resources/app/out/vs/sessions/skills"
            || self.guards.len() != PUBLIC_GUARDS.len() || self.builtin_extensions.len() != 102
            || self.session_skills.len() != 12 || metadata_name(&self.version).as_deref() != Some(self.version.as_str())
            || metadata_name(&self.base_version).as_deref() != Some(self.base_version.as_str())
            || self.commit.len() != 40 || !self.commit.bytes().all(|ch| ch.is_ascii_hexdigit())
            || !digest_string(&self.evidence.contract_sha256) || !digest_string(&self.evidence.archive_sha256)
            || !self.evidence.rea_parent.starts_with("ev_") || !self.evidence.rea_summary.starts_with("ev_") { return Err(()); }
        let mut paths = BTreeSet::new(); let mut total = 0u64;
        for guard in &self.guards {
            if !PUBLIC_GUARDS.contains(&guard.path.as_str()) || !paths.insert(&guard.path)
                || guard.bytes == 0 || !digest_string(&guard.sha256) { return Err(()); }
            total = total.checked_add(guard.bytes).ok_or(())?;
        }
        if total > MAX_IDENTITY_BYTES { return Err(()); }
        let mut folders = BTreeSet::new(); let mut ids = BTreeSet::new();
        for extension in &self.builtin_extensions {
            vscode_extensions::identifier(&extension.id)?;
            let guard = &extension.manifest;
            let folder = guard.path.strip_prefix("Contents/Resources/app/extensions/")
                .and_then(|tail| tail.strip_suffix("/package.json")).ok_or(())?;
            if !simple_name(folder) || !folders.insert(folder) || !ids.insert(&extension.id)
                || metadata_name(&extension.version).as_deref() != Some(extension.version.as_str())
                || guard.bytes == 0 || guard.bytes > MAX_DOCUMENT_BYTES as u64 || !digest_string(&guard.sha256) { return Err(()); }
        }
        let mut names = BTreeSet::new();
        for name in &self.session_skills { if !simple_name(name) || !names.insert(name) { return Err(()); } }
        Ok(())
    }
}
fn contract() -> Option<&'static Contract> {
    static VALUE: OnceLock<Option<Contract>> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-devin.json"))
        .ok().filter(|value| value.validate().is_ok())).as_ref()
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct Stamp {
    device: u64, inode: u64, bytes: u64, mode: u32,
    mtime: i64, mtime_nsec: i64, ctime: i64, ctime_nsec: i64,
}
impl Stamp {
    fn capture(value: &fs::Metadata) -> Result<Self, ()> {
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            Ok(Self { device:value.dev(), inode:value.ino(), bytes:value.len(), mode:value.mode(),
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
        let stamp = Stamp::capture(&metadata)?; let file = open_confined(&path, directory)?;
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
fn host_paths(host: &Host) -> Result<Vec<PathBuf>, ()> {
    if host.agent_id != "windsurf" || host.adapter != "devin" || host.roots.is_empty() || host.roots.len() > 2 { return Err(()); }
    host.roots.iter().map(|root| {
        if root.layout != "devin_app" || root.presence != PluginPresence::Bundled
            || !["/Applications/Devin.app", "~/Applications/Devin.app"].contains(&root.path.as_str()) { return Err(()); }
        let path = vscode_extensions::root_path(root); checked_root(&path)?; Ok(path)
    }).collect()
}
fn select_app(host: &Host) -> Result<Option<PathBuf>, ()> {
    for path in host_paths(host)? {
        match fs::symlink_metadata(&path) {
            Ok(value) if value.is_dir() => return Ok(Some(path)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}, _ => return Err(()),
        }
    }
    Ok(None)
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
struct IdentityBudget { hashed_bytes: u64, metadata_bytes: usize, visited: usize }
struct VerifiedApp { app: BoundPath, guards: Vec<BoundPath>, commit: String, version: String }
impl VerifiedApp {
    fn capture(host: &Host, c: &Contract, cache: &Mutex<DigestCache>, budget: &mut IdentityBudget) -> Result<Option<Self>, ()> {
        c.validate()?;
        let Some(path) = select_app(host)? else { return Ok(None); };
        let app = BoundPath::capture(path, true)?;
        let mut proof = Self { app, guards:Vec::new(), commit:c.commit.clone(), version:c.version.clone() };
        for guard in &c.guards {
            budget.visited += 1; if budget.visited > 32 { return Err(()); }
            let mut file = BoundPath::capture(relative(&proof.app.path,&guard.path)?,false)?;
            if file.stamp.bytes != guard.bytes { return Err(()); }
            let key = DigestKey { app:proof.app.path.clone(), app_stamp:proof.app.stamp.clone(),
                path:file.path.clone(), stamp:file.stamp.clone(), expected:guard.sha256.clone() };
            let cached = cache.lock().map_err(|_| ())?.contains(&key);
            if !cached {
                if guard.bytes > MAX_IDENTITY_BYTES.saturating_sub(budget.hashed_bytes) { return Err(()); }
                let mut hash = Sha256::new(); let mut count = 0u64; let mut buffer = [0;65_536];
                loop {
                    let read = (&mut file.file).take(guard.bytes.saturating_sub(count)+1).read(&mut buffer).map_err(|_| ())?;
                    if read == 0 { break; } count += read as u64; budget.hashed_bytes += read as u64;
                    if count > guard.bytes || budget.hashed_bytes > MAX_IDENTITY_BYTES { return Err(()); }
                    hash.update(&buffer[..read]);
                }
                if count != guard.bytes || hex::encode(hash.finalize()) != guard.sha256 { return Err(()); }
            }
            file.revalidate()?; proof.app.revalidate()?;
            if select_app(host)?.as_ref() != Some(&proof.app.path) { return Err(()); }
            if !cached { cache.lock().map_err(|_| ())?.insert(key); }
            proof.guards.push(file);
        }
        let info = proof.public_bytes("Contents/Info.plist",budget)?;
        let info = plist::Value::from_reader(std::io::Cursor::new(info)).map_err(|_| ())?;
        let info = info.as_dictionary().ok_or(())?;
        for (key,value) in [("CFBundleIdentifier",&c.bundle_id),("CFBundleName",&c.bundle_name),
            ("CFBundleDisplayName",&c.bundle_name),("CFBundleExecutable",&c.executable),
            ("CFBundleShortVersionString",&c.version),("CFBundleVersion",&c.version)] {
            if info.get(key).and_then(plist::Value::as_string) != Some(value.as_str()) { return Err(()); }
        }
        let package: Value = serde_json::from_slice(&proof.public_bytes("Contents/Resources/app/package.json",budget)?).map_err(|_| ())?;
        let product: Value = serde_json::from_slice(&proof.public_bytes("Contents/Resources/app/product.json",budget)?).map_err(|_| ())?;
        for (key,value) in [("name","Devin"),("version",c.base_version.as_str()),("main",c.package_main.as_str())] {
            if package.get(key).and_then(Value::as_str) != Some(value) { return Err(()); }
        }
        for (key,value) in [("nameShort","Devin"),("nameLong","Devin"),("applicationName","devin-desktop"),
            ("dataFolderName",c.data_folder.as_str()),("windsurfVersion",c.version.as_str()),("commit",c.commit.as_str())] {
            if product.get(key).and_then(Value::as_str) != Some(value) { return Err(()); }
        }
        proof.revalidate(host)?; Ok(Some(proof))
    }
    fn public_bytes(&mut self, relative_path: &str, budget: &mut IdentityBudget) -> Result<Vec<u8>, ()> {
        let path = self.app.path.join(relative_path);
        let file = self.guards.iter_mut().find(|file| file.path == path).ok_or(())?;
        let count = usize::try_from(file.stamp.bytes).map_err(|_| ())?;
        budget.metadata_bytes = budget.metadata_bytes.checked_add(count).ok_or(())?;
        if budget.metadata_bytes > MAX_IDENTITY_METADATA { return Err(()); }
        file.bytes(MAX_DOCUMENT_BYTES)
    }
    fn revalidate(&self, host: &Host) -> Result<(), ()> {
        if select_app(host)?.as_ref() != Some(&self.app.path) { return Err(()); }
        self.app.revalidate()?; for file in &self.guards { file.revalidate()?; } self.app.revalidate()
    }
    fn sources(&self) -> Vec<String> { self.guards.iter().map(|file|collapse_home(&file.path)).collect() }
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
            mode:value.st_mode as u32, mtime:value.st_mtime as i64, mtime_nsec:value.st_mtime_nsec as i64,
            ctime:value.st_ctime as i64, ctime_nsec:value.st_ctime_nsec as i64 }))
    }
    #[cfg(not(any(target_os="macos",target_os="linux")))] { let _=(parent,name); Err(()) }
}

fn clean_relative(root: &Path, input: &str) -> Result<PathBuf, ()> {
    if input.is_empty() || input.len()>4096 || input.chars().any(char::is_control) { return Err(()); }
    // _jo applies URI path normalization before confinement. Internal '..'
    // segments are supported, but cannot remove the selected physical root.
    let mut parts=Vec::new();
    for part in Path::new(input).components() {
        match part {
            Component::Normal(value)=>parts.push(value), Component::CurDir=>{},
            Component::ParentDir=>{parts.pop().ok_or(())?;}, _=>return Err(()),
        }
    }
    let mut path=root.to_path_buf(); for part in parts {path.push(part);} checked_root(&path)?; Ok(path)
}
fn add_component(components: &mut Vec<PluginComponent>, kind: PluginComponentKind, name: &str,
    path: &Path, native_id: Option<String>, budget: &mut MetadataBudget) -> Result<(), ()> {
    budget.component()?;
    components.push(PluginComponent { kind, name:metadata_name(name).ok_or(())?, source_path:collapse_home(path), native_id }); Ok(())
}
fn observation(host: &Host, proof: &VerifiedApp, id: String, name: String, version: String,
    presence: PluginPresence, mut sources: Vec<String>, mut components: Vec<PluginComponent>, context: &str) -> Result<NativePluginObservation, ()> {
    components.sort(); components.dedup(); sources.sort(); sources.dedup();
    let hash = hex::encode(Sha256::digest(serde_json::to_vec(&(&id,&version,&sources,&components,context)).map_err(|_| ())?));
    Ok(NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:id, name, version,
        revision:format!("host:{}; build:{}; {context}; metadata-sha256:{hash}",proof.version,proof.commit),
        presence, enabled:None, source_paths:sources, components, docs_url:host.docs_url.clone(), source_fingerprint:String::new() })
}

fn extension_components(root: &Path, value: &Value, snapshot: &mut Snapshot, budget: &mut MetadataBudget) -> Result<Vec<PluginComponent>, ()> {
    let path=root.join("package.json"); let id=vscode_extensions::package_id(value)?;
    let mut components=Vec::new(); add_component(&mut components,PluginComponentKind::Extension,&id,&path,Some(id.clone()),budget)?;
    for field in ["main","browser"] {
        if let Some(input)=value.get(field) {
            let source=(||{
                let input=input.as_str().ok_or(())?; let mut entry=clean_relative(root,input)?;
                let mut materialized=snapshot.stat_file(&entry,budget)?.is_some();
                if !materialized && entry.extension().is_none() { entry.set_extension("js"); materialized=snapshot.stat_file(&entry,budget)?.is_some(); }
                Ok::<_,()>(materialized.then_some(entry))
            })();
            match source {
                Ok(Some(entry))=>add_component(&mut components,PluginComponentKind::Code,field,&entry,Some(field.into()),budget)?,
                Ok(None)=>add_component(&mut components,PluginComponentKind::Code,"Entrypoint declaration (not materialized)",&path,Some(field.into()),budget)?,
                Err(())=>{snapshot.incomplete=true;add_component(&mut components,PluginComponentKind::Code,"Entrypoint declaration (source unresolved)",&path,Some(field.into()),budget)?;},
            }
        }
    }
    for (field,key,kind) in [("languageModelTools","name",PluginComponentKind::Tool),
        ("mcpServerDefinitionProviders","id",PluginComponentKind::McpReference)] {
        if let Some(items)=value.get("contributes").and_then(|item|item.get(field)) {
            let Some(items)=items.as_array().filter(|items|items.len()<=MAX_REGISTRATIONS) else {snapshot.incomplete=true;continue;};
            for item in items {
                let Some(name)=item.get(key).and_then(Value::as_str).and_then(metadata_name) else {snapshot.incomplete=true;continue;};
                add_component(&mut components,kind.clone(),&name,&path,Some(name.clone()),budget)?;
            }
        }
    }
    // Ordinary contributes.commands are IDE commands, never Agent commands.
    Ok(components)
}
fn extension(host: &Host, proof: &VerifiedApp, root: &Path, value: &Value, sources: Vec<String>,
    presence: PluginPresence, snapshot: &mut Snapshot, budget: &mut MetadataBudget) -> Result<NativePluginObservation, ()> {
    let id=vscode_extensions::package_id(value)?;
    let version=value.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    let name=value.get("displayName").and_then(Value::as_str).filter(|value|!value.starts_with('%'))
        .and_then(metadata_name).unwrap_or_else(||id.clone());
    let components=extension_components(root,value,snapshot,budget)?;
    let context=if presence==PluginPresence::Installed {
        "VSCode-extension; default-profile-registration; active-profile=unknown; activation=unknown"
    }else{"VSCode-extension; activation=unknown"};
    observation(host,proof,id,name,version,presence,sources,components,context)
}

fn bundled(host: &Host, proof: &VerifiedApp, c: &Contract, snapshot: &mut Snapshot, budget: &mut MetadataBudget) -> Result<(), ()> {
    let root=proof.app.path.join(APP_RESOURCES).join("extensions");
    let entries=snapshot.directories(&root,budget)?;
    let expected:BTreeSet<_>=c.builtin_extensions.iter().filter_map(|extension|Path::new(&extension.manifest.path).parent().and_then(Path::file_name).and_then(|name|name.to_str())).collect();
    for entry in entries { if entry.directory && !expected.contains(entry.path.file_name().and_then(|name|name.to_str()).ok_or(())?) { snapshot.incomplete=true; } }
    for package in &c.builtin_extensions {
        let item=(||{
            let value=snapshot.guarded_json(&proof.app.path,&package.manifest,budget)?;
            if vscode_extensions::package_id(&value)?!=package.id || value.get("version").and_then(Value::as_str)!=Some(package.version.as_str()) {return Err(());}
            let manifest=proof.app.path.join(&package.manifest.path); let mut sources=proof.sources(); sources.push(collapse_home(&manifest));
            extension(host,proof,manifest.parent().ok_or(())?,&value,sources,PluginPresence::Bundled,snapshot,budget)
        })();
        match item { Ok(item)=>snapshot.plugins.push(item),Err(())=>snapshot.incomplete=true }
    }
    let skill_root=proof.app.path.join(&c.session_skill_root); let names:BTreeSet<_>=c.session_skills.iter().map(String::as_str).collect();
    for entry in snapshot.directories(&skill_root,budget)? {
        if entry.directory && !names.contains(entry.path.file_name().and_then(|name|name.to_str()).ok_or(())?) {snapshot.incomplete=true;}
    }
    let mut components=Vec::new(); let mut identities=Vec::new();
    for name in &c.session_skills {
        let path=clean_relative(&skill_root,name)?.join("SKILL.md");
        match snapshot.stat_file(&path,budget) {
            Ok(Some(stamp))=>{ add_component(&mut components,PluginComponentKind::Skill,name,&path,None,budget)?; identities.push((path,stamp)); },
            _=>snapshot.incomplete=true,
        }
    }
    let availability=hex::encode(Sha256::digest(serde_json::to_vec(&identities).map_err(|_| ())?));
    let mut sources=proof.sources(); sources.push(collapse_home(&skill_root));
    snapshot.plugins.push(observation(host,proof,SESSION_GROUP.into(),"Devin bundled session Skills".into(),c.version.clone(),
        PluginPresence::Bundled,sources,components,&format!("application-owned; Skill-body-unread; availability:{availability}"))?);
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct DefaultRoots { extensions: PathBuf, plugins: PathBuf }
fn default_roots(c: &Contract) -> Result<DefaultRoots, ()> {
    // These are passive default namespaces. The running IDE's argv/env/profile
    // is unobserved. Never follow arbitrary process overrides or legacy roots.
    for key in ["VSCODE_DEV","VSCODE_EXTENSIONS","VSCODE_PORTABLE","VSCODE_APPDATA"] {
        if std::env::var_os(key).is_some_and(|value|!value.is_empty()) {return Err(());}
    }
    let roots=DefaultRoots {extensions:expand_tilde(&format!("~/{}/extensions",c.data_folder)),
        plugins:expand_tilde(&format!("~/{}/agent-plugins",c.user_data))};
    // Each registry gets its own confined read. An unsafe sibling namespace
    // must not invalidate already bound metadata from the other source.
    Ok(roots)
}
fn uri_object(root: &Path, value: &Value) -> Result<PathBuf, ()> {
    let object=value.as_object().ok_or(())?;
    if object.get("scheme").and_then(Value::as_str)!=Some("file") {return Err(());}
    for key in ["authority","query","fragment"] {
        if object.get(key).is_some_and(|value|!value.as_str().is_some_and(str::is_empty)) {return Err(());}
    }
    let path=object.get("path").and_then(Value::as_str).ok_or(())?;
    scoped_absolute(root,Path::new(path))
}
fn scoped_absolute(root: &Path, path: &Path) -> Result<PathBuf, ()> {
    if !path.is_absolute() || path.to_str().is_none_or(|value|value.len()>4096||value.chars().any(char::is_control)) {return Err(());}
    let tail=path.strip_prefix(root).map_err(|_| ())?.to_str().ok_or(())?;
    if tail.is_empty() {return Err(());}
    clean_relative(root,tail)
}
fn plugin_uri(root: &Path, input: &str) -> Result<PathBuf, ()> {
    if input.len()>8192 || input.contains('\\') || input.chars().any(char::is_control) {return Err(());}
    // Reject traversal before URL normalization can erase it. Decode exactly
    // once; encoded separators/parents receive the same path-component check.
    let url=url::Url::parse(input).map_err(|_| ())?;
    if url.scheme()!="file" || url.host_str().is_some_and(|host|!host.is_empty())
        || url.query().is_some() || url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {return Err(());}
    let after_scheme=input.strip_prefix("file://").ok_or(())?;
    let raw_path=after_scheme.strip_prefix('/').map(|tail|format!("/{tail}")).ok_or(())?;
    let raw=url::Url::parse(&format!("file://{raw_path}")).map_err(|_| ())?;
    let path=raw.to_file_path().map_err(|_| ())?;
    // URL normalizes dot segments, so also inspect the original encoded text.
    for segment in raw_path.split('/') {
        let lower=segment.to_ascii_lowercase().replace("%2e",".");
        if lower==".." || lower.contains("%2f") || lower.contains("%5c") {return Err(());}
    }
    scoped_absolute(root,&path)
}
fn provenance(host: &Host, proof: &VerifiedApp, id: String, name: String, version: String,
    ledger: &Path, reason: &str) -> Result<NativePluginObservation, ()> {
    observation(host,proof,id,name,version,PluginPresence::Declared,vec![collapse_home(ledger)],Vec::new(),reason)
}
fn ordinary_registrations(host: &Host, proof: &VerifiedApp, roots: &DefaultRoots, snapshot: &mut Snapshot, budget: &mut MetadataBudget) -> Result<(), ()> {
    let ledger=roots.extensions.join("extensions.json");
    let Some(value)=snapshot.read_document(&ledger,false,budget)? else {return Ok(());};
    let rows=value.as_array().filter(|rows|rows.len()<=MAX_REGISTRATIONS).ok_or(())?;
    for row in rows {
        budget.visit()?;
        let item=(||{
            let id=vscode_extensions::identifier(row.pointer("/identifier/id").and_then(Value::as_str).ok_or(())?)?;
            let version=row.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
            let location=row.get("location").ok_or(())?;
            if !(location.as_str().is_some_and(|value|!value.is_empty()) || location.as_object().is_some_and(|value|value.get("path").is_some_and(Value::is_string)&&value.get("scheme").is_some_and(Value::is_string)))
                || row.get("relativeLocation").is_some_and(|value|!value.is_string()) {return Err(());}
            let path=if let Some(input)=row.get("relativeLocation").and_then(Value::as_str).filter(|value|!value.is_empty()) {
                clean_relative(&roots.extensions,input)
            } else {uri_object(&roots.extensions,location)};
            let observed=path.and_then(|root|{
                let manifest=root.join("package.json");
                let value=snapshot.read_document(&manifest,false,budget)?.ok_or(())?;
                if !vscode_extensions::package_id(&value)?.eq_ignore_ascii_case(&id)
                    || value.get("version").and_then(Value::as_str)!=Some(version.as_str()) {return Err(());}
                let mut sources=proof.sources(); sources.extend([collapse_home(&ledger),collapse_home(&manifest)]);
                extension(host,proof,&root,&value,sources,PluginPresence::Installed,snapshot,budget)
            });
            match observed { Ok(value)=>Ok(value),Err(())=>{
                snapshot.incomplete=true;
                provenance(host,proof,id.clone(),id,version,&ledger,"default-profile-registration; physical-source-unresolved; active-profile=unknown")
            }}
        })();
        match item {Ok(item)=>snapshot.plugins.push(item),Err(())=>snapshot.incomplete=true}
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Format { Open, Claude, Copilot }
impl Format {
    fn manifest(self) -> &'static str {match self {Self::Open=>".plugin/plugin.json",Self::Claude=>".claude-plugin/plugin.json",Self::Copilot=>"plugin.json"}}
    fn hook(self) -> &'static str {match self {Self::Copilot=>"hooks.json",_=>"hooks/hooks.json"}}
    fn label(self) -> &'static str {match self {Self::Open=>"openPlugin",Self::Claude=>"claude",Self::Copilot=>"copilot"}}
}
fn exists(snapshot: &mut Snapshot, path: &Path, budget: &mut MetadataBudget) -> Result<bool, ()> {
    budget.visit()?; checked_root(path)?; snapshot.watches.insert(path.into());
    match fs::symlink_metadata(path) {
        Ok(_)=>Ok(true),Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{snapshot.missing(path)?;Ok(false)},_=>Err(()),
    }
}
fn format(root: &Path, snapshot: &mut Snapshot, budget: &mut MetadataBudget) -> Result<Format, ()> {
    if exists(snapshot,&root.join(Format::Open.manifest()),budget)? {return Ok(Format::Open);}
    if root.components().any(|part|matches!(part,Component::Normal(value) if value==".claude"))
        || exists(snapshot,&root.join(Format::Claude.manifest()),budget)? {return Ok(Format::Claude);}
    Ok(Format::Copilot)
}
fn component_paths_native(root: &Path, field: Option<&Value>, fallback: &str, snapshot: &mut Snapshot) -> Vec<PathBuf> {
    let mut paths=Vec::new(); let mut exclusive=false;
    let values:Vec<&str>=match field {
        Some(Value::String(value))=>vec![value.as_str()],
        Some(Value::Array(values))=>values.iter().filter_map(Value::as_str).collect(),
        Some(Value::Object(value))=>if let Some(values)=value.get("paths").and_then(Value::as_array) {
            exclusive=value.get("exclusive").and_then(Value::as_bool)==Some(true);
            values.iter().filter_map(Value::as_str).collect()
        }else{Vec::new()},_=>Vec::new(),
    };
    if !exclusive {paths.push(root.join(fallback));}
    for input in values {
        let input=input.trim(); if input.is_empty(){continue;}
        match clean_relative(root,input) {Ok(path)=>paths.push(path),Err(())=>snapshot.incomplete=true}
    }
    paths
}
fn skill_candidates(root: &Path, paths: Vec<PathBuf>, components: &mut Vec<PluginComponent>, snapshot: &mut Snapshot, budget: &mut MetadataBudget) -> Result<(), ()> {
    let mut found=BTreeSet::new();
    for path in paths {
        // Native Pjo first tests path/SKILL.md, then only direct children.
        let direct=path.join("SKILL.md");
        match snapshot.stat_file(&direct,budget) {
            Ok(Some(_))=>{found.insert(direct);continue;},Ok(None)=>{},Err(())=>{snapshot.incomplete=true;continue;},
        }
        match snapshot.directories(&path,budget) {
            Ok(entries)=>for entry in entries {
                if !entry.directory {continue;}
                let leaf=entry.path.join("SKILL.md");
                match snapshot.stat_file(&leaf,budget) {Ok(Some(_))=>{found.insert(leaf);},Ok(None)=>{},Err(())=>snapshot.incomplete=true}
            },Err(())=>snapshot.incomplete=true,
        }
    }
    if found.is_empty() {
        let path=root.join("SKILL.md"); if snapshot.stat_file(&path,budget)?.is_some(){found.insert(path);}
    }
    for path in found {
        let name=path.parent().and_then(Path::file_name).and_then(|name|name.to_str()).ok_or(())?;
        add_component(components,PluginComponentKind::Skill,name,&path,None,budget)?;
    }
    Ok(())
}
fn markdown_candidates(paths: Vec<PathBuf>, kind: PluginComponentKind, components: &mut Vec<PluginComponent>, snapshot: &mut Snapshot, budget: &mut MetadataBudget) -> Result<(), ()> {
    let mut seen=BTreeSet::new();
    let name=|path:&Path|->Option<String>{
        let input=path.file_name()?.to_str()?; let lower=input.to_ascii_lowercase();
        let suffixes:&[&str]=if kind==PluginComponentKind::Rule {&[".instructions.md",".mdc",".md"]}else{&[".md"]};
        suffixes.iter().find(|suffix|lower.ends_with(**suffix)).and_then(|suffix|metadata_name(&input[..input.len()-suffix.len()]))
    };
    for path in paths {
        match fs::symlink_metadata(&path) {
            Ok(value) if value.file_type().is_file()=>{
                if let Some(label)=name(&path) {if seen.insert(label.clone()) && snapshot.stat_file(&path,budget)?.is_some(){add_component(components,kind.clone(),&label,&path,Some(label.clone()),budget)?;}}
            },
            Ok(value) if value.is_dir()=>for entry in snapshot.directories(&path,budget)? {
                if !entry.file {continue;}
                if let Some(label)=name(&entry.path) {if seen.insert(label.clone()) && snapshot.stat_file(&entry.path,budget)?.is_some(){add_component(components,kind.clone(),&label,&entry.path,Some(label.clone()),budget)?;}}
            },
            Err(error) if error.kind()==std::io::ErrorKind::NotFound=>{snapshot.missing(&path)?;},_=>return Err(()),
        }
    }
    Ok(())
}
fn inline(value: Option<&Value>) -> bool {value.is_some_and(|value|value.as_object().is_some_and(|value|!value.contains_key("paths")))}
fn native_mcp(value: &Value) -> bool {
    let Some(object)=value.as_object() else {return false;};
    let kind=object.get("type").and_then(Value::as_str).filter(|value|!value.is_empty());
    let nonempty=|key:&str|object.get(key).and_then(Value::as_str).is_some_and(|value|!value.is_empty());
    match kind {
        Some("stdio")=>nonempty("command"),Some("http"|"sse")=>nonempty("url"),None=>nonempty("command")||nonempty("url"),_=>false,
    }
}
fn mcp_references(value: &Value, path: &Path, wrapped: bool, seen: &mut BTreeSet<String>, components: &mut Vec<PluginComponent>, budget: &mut MetadataBudget) -> Result<(), ()> {
    let value=if wrapped {value.get("mcpServers").unwrap_or(value)}else{value};
    let Some(servers)=value.as_object() else {return Ok(());};
    if servers.len()>MAX_REGISTRATIONS {return Err(());}
    for (name,value) in servers {
        budget.visit()?;
        if native_mcp(value) && seen.insert(name.clone()) {add_component(components,PluginComponentKind::McpReference,name,path,Some(name.clone()),budget)?;}
    }
    Ok(())
}
fn agent_components(root: &Path, manifest: &Value, manifest_path: &Path, format: Format, snapshot: &mut Snapshot, budget: &mut MetadataBudget) -> Vec<PluginComponent> {
    let mut components=Vec::new();
    for (field,kind) in [("commands",PluginComponentKind::Command),("agents",PluginComponentKind::Agent),("rules",PluginComponentKind::Rule)] {
        let paths=component_paths_native(root,manifest.get(field),field,snapshot);
        if markdown_candidates(paths,kind,&mut components,snapshot,budget).is_err(){snapshot.incomplete=true;}
    }
    let paths=component_paths_native(root,manifest.get("skills"),"skills",snapshot);
    if skill_candidates(root,paths,&mut components,snapshot,budget).is_err(){snapshot.incomplete=true;}
    let hooks=manifest.get("hooks");
    let hook_result=(||{
        if inline(hooks) {
            // Hook command/event parsing is not closed by this observer. Keep
            // only the declaration's provenance, never its executable fields.
            add_component(&mut components,PluginComponentKind::TypeDeclaration,"Hook declaration (not evaluated)",manifest_path,None,budget)?;
        }else{
            for path in component_paths_native(root,hooks,format.hook(),snapshot) {
                if snapshot.stat_file(&path,budget)?.is_some(){add_component(&mut components,PluginComponentKind::TypeDeclaration,"Hook file reference (not evaluated)",&path,None,budget)?;}
            }
        }
        Ok::<(),()>(())
    })(); if hook_result.is_err(){snapshot.incomplete=true;}
    let field=manifest.get("mcpServers"); let mut seen=BTreeSet::new();
    let mcp_result=(||{
        if inline(field) {mcp_references(field.ok_or(())?,manifest_path,false,&mut seen,&mut components,budget)?;}
        else {
            for path in component_paths_native(root,field,".mcp.json",snapshot) {
                match snapshot.read_document(&path,true,budget) {
                    Ok(Some(value))=>{if mcp_references(&value,&path,true,&mut seen,&mut components,budget).is_err(){snapshot.incomplete=true;}},
                    Ok(None)=>{},Err(())=>snapshot.incomplete=true,
                }
            }
        }
        Ok::<(),()>(())
    })(); if mcp_result.is_err(){snapshot.incomplete=true;}
    components
}
fn installed_agent_plugins(host: &Host, proof: &VerifiedApp, roots: &DefaultRoots, snapshot: &mut Snapshot, budget: &mut MetadataBudget) -> Result<(), ()> {
    let ledger=roots.plugins.join("installed.json");
    let Some(value)=snapshot.read_document(&ledger,false,budget)? else {return Ok(());};
    // The native reader is permissive, but this observer supports only the
    // current version-1 format emitted by its writer. No migration fallback.
    if value.get("version").and_then(Value::as_u64)!=Some(1) {return Err(());}
    let rows=value.get("installed").and_then(Value::as_array).filter(|rows|rows.len()<=MAX_REGISTRATIONS).ok_or(())?;
    for row in rows {
        budget.visit()?;
        let item=(||{
            let uri=row.get("pluginUri").and_then(Value::as_str).ok_or(())?;
            let marketplace=row.get("marketplace").and_then(Value::as_str).ok_or(())?;
            if uri.len()>8192 || marketplace.len()>4096 {return Err(());}
            // URI is the native identity. Hash it rather than emitting remote
            // queries, fragments or credentials in an unresolved row.
            let id=format!("@agent-plugin/{}",hex::encode(Sha256::digest(uri.as_bytes())));
            let record_name=row.get("name").and_then(Value::as_str).and_then(metadata_name);
            let observed=(||{
                let root=plugin_uri(&roots.plugins,uri)?;
                let Some(_)=snapshot.bind(&root,true,budget)? else {return Err(());};
                let format=format(&root,snapshot,budget)?; let path=root.join(format.manifest());
                let manifest=snapshot.read_document(&path,true,budget)?.filter(Value::is_object).ok_or(())?;
                let name=record_name.clone().or_else(||manifest.get("name").and_then(Value::as_str).and_then(metadata_name))
                    .or_else(||root.file_name().and_then(|name|name.to_str()).and_then(metadata_name)).ok_or(())?;
                let version=manifest.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_default();
                let components=agent_components(&root,&manifest,&path,format,snapshot,budget);
                let mut sources=proof.sources(); sources.extend([collapse_home(&ledger),collapse_home(&path)]);
                let provenance=hex::encode(Sha256::digest(marketplace.as_bytes()));
                observation(host,proof,id.clone(),name,version,PluginPresence::Installed,sources,components,
                    &format!("Agent-plugin:{}; default-profile-record; active-profile=unknown; Skill-body-unread; marketplace:{provenance}",format.label()))
            })();
            match observed {Ok(item)=>Ok(item),Err(())=>{
                snapshot.incomplete=true;
                provenance(host,proof,id,record_name.unwrap_or_else(||"Unresolved Agent plugin".into()),String::new(),&ledger,
                    "Agent-plugin-installed-record; physical-source-unresolved; active-profile=unknown")
            }}
        })();
        match item {Ok(item)=>snapshot.plugins.push(item),Err(())=>snapshot.incomplete=true}
    }
    Ok(())
}

struct Audit { proof: VerifiedApp, snapshot: Snapshot, defaults: Option<DefaultRoots> }
impl Audit {
    fn revalidate(&self, host: &Host) -> Result<(), ()> {
        self.snapshot.revalidate()?; self.proof.revalidate(host)?;
        if let Some(defaults)=&self.defaults {
            // Recheck the selected default namespace through the same env
            // gate; do not publish a scan across a changed source selection.
            for key in ["VSCODE_DEV","VSCODE_EXTENSIONS","VSCODE_PORTABLE","VSCODE_APPDATA"] {
                if std::env::var_os(key).is_some_and(|value|!value.is_empty()){return Err(());}
            }
            if defaults.extensions!=expand_tilde("~/.devin/extensions")
                || defaults.plugins!=expand_tilde("~/Library/Application Support/Devin/User/agent-plugins") {return Err(());}
        }
        self.snapshot.revalidate()?; self.proof.revalidate(host)
    }
}
fn capture(host: &Host, c: &Contract, cache: &Mutex<DigestCache>) -> Result<Option<Audit>, ()> {
    let Some(proof)=VerifiedApp::capture(host,c,cache,&mut IdentityBudget::default())? else {return Ok(None);};
    let mut snapshot=Snapshot::default(); let mut budget=MetadataBudget::default();
    bundled(host,&proof,c,&mut snapshot,&mut budget)?;
    let defaults=match default_roots(c) {
        Ok(roots)=>{
            if ordinary_registrations(host,&proof,&roots,&mut snapshot,&mut budget).is_err(){snapshot.incomplete=true;}
            if installed_agent_plugins(host,&proof,&roots,&mut snapshot,&mut budget).is_err(){snapshot.incomplete=true;}
            Some(roots)
        },Err(())=>{snapshot.incomplete=true;None},
    };
    let audit=Audit {proof,snapshot,defaults}; audit.revalidate(host)?; Ok(Some(audit))
}
fn observe_contract(host: &Host, c: &Contract, cache: &Mutex<DigestCache>, result: &mut PluginScan) -> Result<(), ()> {
    let Some(audit)=capture(host,c,cache)? else {return Ok(());};
    audit.revalidate(host)?; result.unavailable|=audit.snapshot.incomplete; result.plugins.extend(audit.snapshot.plugins); Ok(())
}
fn cache() -> &'static Mutex<DigestCache> {
    static CACHE: OnceLock<Mutex<DigestCache>>=OnceLock::new(); CACHE.get_or_init(Mutex::default)
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let Some(c)=contract() else {result.unavailable=true;return;};
    if observe_contract(host,c,cache(),result).is_err(){result.unavailable=true;}
}
fn watch_paths_contract(host: &Host, c: &Contract, cache: &Mutex<DigestCache>) -> Vec<PathBuf> {
    let Ok(apps)=host_paths(host) else {return Vec::new();};
    let mut paths=BTreeSet::new();
    for app in apps {
        paths.insert(app.clone());
        for guard in &c.guards {let path=app.join(&guard.path); if checked_root(&path).is_ok(){paths.insert(path);}}
        for extension in &c.builtin_extensions {let path=app.join(&extension.manifest.path);if checked_root(&path).is_ok(){paths.insert(path);}}
        let skills=app.join(&c.session_skill_root); if checked_root(&skills).is_ok(){paths.insert(skills);}
    }
    if let Ok(roots)=default_roots(c) {
        for path in [roots.extensions.clone(),roots.extensions.join("extensions.json"),roots.plugins.clone(),roots.plugins.join("installed.json")] {
            if checked_root(&path).is_ok(){paths.insert(path);}
        }
    }
    if let Ok(Some(audit))=capture(host,c,cache) {if audit.revalidate(host).is_ok(){paths.extend(audit.snapshot.watches);}}
    paths.into_iter().collect()
}
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    contract().map(|c|watch_paths_contract(host,c,cache())).unwrap_or_default()
}

#[cfg(test)]
mod tests;
