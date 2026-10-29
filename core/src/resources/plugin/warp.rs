//! Availability of application-owned Warp Skill resources, not Agent plugin manifests.
//! Public identity guards are read from confined FDs. Skill leaves are stat-only;
//! no Warp process, loader, Skill body, private configuration or MCP state is read.
use super::*;
use std::collections::VecDeque;
use std::io::{Seek, SeekFrom};
use std::sync::Mutex;

const MAX_IDENTITY_BYTES: u64 = 512 * 1024 * 1024;
const MAX_IDENTITY_METADATA_BYTES: usize = 2 * 1024 * 1024;
const MAX_IDENTITY_ENTRIES: usize = 32;
const MAX_DIGEST_CACHE_ENTRIES: usize = 4;
const MAX_RESOURCE_ENTRIES: usize = 512;
const MAX_SKILL_LEAVES: usize = 128;
const MAX_LEAF_BYTES: u64 = 256 * 1024;
const MAX_DECLARED_LEAF_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    schema_version: u32,
    bundle_id: String,
    bundle_name: String,
    executable: String,
    version: String,
    warp_version: String,
    info: Guard,
    main: Guard,
    bundled_version: Guard,
    groups: Vec<Group>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard { path: String, bytes: u64, sha256: String }
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Group { id: String, name: String, root: String }

fn contract() -> Option<&'static Contract> {
    static VALUE: OnceLock<Option<Contract>> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-warp.json"))
        .ok().filter(|value| value.validate().is_ok())).as_ref()
}

impl Contract {
    fn validate(&self) -> Result<(), ()> {
        let known_roots = ["Contents/Resources/bundled/skills", "Contents/Resources/bundled/mcp_skills/figma"];
        if self.schema_version != 1 || self.bundle_id != "dev.warp.Warp-Stable"
            || self.bundle_name != "Warp" || self.executable != "stable"
            || metadata_name(&self.version).as_deref() != Some(self.version.as_str())
            || metadata_name(&self.warp_version).as_deref() != Some(self.warp_version.as_str())
            || self.info.path != "Contents/Info.plist" || self.main.path != "Contents/MacOS/stable"
            || self.bundled_version.path != "Contents/Resources/bundled/metadata/version.json"
            || self.main.bytes < 40 || self.main.bytes > MAX_IDENTITY_BYTES || self.groups.len() != 2 {
            return Err(());
        }
        for guard in [&self.info, &self.main, &self.bundled_version] {
            if guard.bytes == 0 || guard.sha256.len() != 64 || !guard.sha256.bytes().all(|ch| ch.is_ascii_hexdigit()) { return Err(()); }
        }
        if self.info.bytes > 1_048_576 || self.bundled_version.bytes > 1_048_576 { return Err(()); }
        let mut roots = BTreeSet::new(); let mut ids = BTreeSet::new();
        for group in &self.groups {
            if !known_roots.contains(&group.root.as_str()) || !roots.insert(group.root.as_str())
                || !group.id.starts_with("@builtin/") || !ids.insert(group.id.as_str())
                || metadata_name(&group.id).as_deref() != Some(group.id.as_str())
                || metadata_name(&group.name).as_deref() != Some(group.name.as_str()) { return Err(()); }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct Stamp {
    device: u64, inode: u64, bytes: u64, mode: u32,
    mtime: i64, mtime_nsec: i64, ctime: i64, ctime_nsec: i64,
}
impl Stamp {
    fn capture(metadata: &fs::Metadata) -> Result<Self, ()> {
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            Ok(Self { device: metadata.dev(), inode: metadata.ino(), bytes: metadata.len(), mode: metadata.mode(),
                mtime: metadata.mtime(), mtime_nsec: metadata.mtime_nsec(), ctime: metadata.ctime(), ctime_nsec: metadata.ctime_nsec() })
        }
        #[cfg(not(unix))] { let _ = metadata; Err(()) }
    }
}

struct BoundPath { path: PathBuf, file: fs::File, stamp: Stamp, directory: bool }
impl BoundPath {
    fn capture(path: PathBuf, directory: bool) -> Result<Self, ()> {
        checked_root(&path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
        let expected_kind = if directory { metadata.is_dir() } else { metadata.file_type().is_file() };
        if !expected_kind { return Err(()); }
        let stamp = Stamp::capture(&metadata)?;
        let file = open_confined(&path, directory)?;
        if Stamp::capture(&file.metadata().map_err(|_| ())?)? != stamp { return Err(()); }
        let value = Self { path, file, stamp, directory }; value.revalidate()?; Ok(value)
    }

    fn revalidate(&self) -> Result<(), ()> {
        checked_root(&self.path)?;
        let metadata = fs::symlink_metadata(&self.path).map_err(|_| ())?;
        let expected_kind = if self.directory { metadata.is_dir() } else { metadata.file_type().is_file() };
        if !expected_kind { return Err(()); }
        if Stamp::capture(&metadata)? != self.stamp || Stamp::capture(&self.file.metadata().map_err(|_| ())?)? != self.stamp { return Err(()); }
        Ok(())
    }

    fn guarded_bytes(&mut self, guard: &Guard, budget: &mut IdentityBudget) -> Result<Vec<u8>, ()> {
        budget.visit()?;
        if self.directory || self.stamp.bytes != guard.bytes { return Err(()); }
        let count = usize::try_from(guard.bytes).map_err(|_| ())?;
        budget.metadata(count)?;
        let mut bytes = vec![0; count]; self.file.read_exact(&mut bytes).map_err(|_| ())?;
        self.revalidate()?;
        if hex::encode(Sha256::digest(&bytes)) != guard.sha256 { return Err(()); }
        Ok(bytes)
    }
}

#[derive(Default)]
struct IdentityBudget { entries: usize, metadata_bytes: usize, hashed_bytes: u64 }
impl IdentityBudget {
    fn visit(&mut self) -> Result<(), ()> {
        self.entries = self.entries.checked_add(1).ok_or(())?;
        if self.entries > MAX_IDENTITY_ENTRIES { Err(()) } else { Ok(()) }
    }
    fn metadata(&mut self, count: usize) -> Result<(), ()> {
        self.metadata_bytes = self.metadata_bytes.checked_add(count).ok_or(())?;
        if self.metadata_bytes > MAX_IDENTITY_METADATA_BYTES { Err(()) } else { Ok(()) }
    }
}

fn host_paths(host: &Host) -> Result<Vec<PathBuf>, ()> {
    if host.agent_id != "warp" || host.adapter != "warp" || host.roots.len() > 2 { return Err(()); }
    host.roots.iter().map(|source| {
        if source.layout != "warp_app" { return Err(()); }
        let path = vscode_extensions::root_path(source);
        if !path.is_absolute() || path.file_name().and_then(|part| part.to_str()) != Some("Warp.app")
            || path.to_str().is_none_or(|value| value.len() > 4096 || value.chars().any(char::is_control))
            || path.components().any(|part| matches!(part, Component::ParentDir | Component::Prefix(_))) { return Err(()); }
        Ok(path)
    }).collect()
}

fn select_app(host: &Host) -> Result<Option<PathBuf>, ()> {
    for path in host_paths(host)? {
        checked_root(&path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                let physical = fs::canonicalize(&path).map_err(|_| ())?;
                checked_root(&physical)?;
                return Ok(Some(physical));
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            _ => return Err(()),
        }
    }
    Ok(None)
}

#[derive(Clone, PartialEq, Eq)]
struct DigestKey { app: PathBuf, app_stamp: Stamp, main: PathBuf, main_stamp: Stamp, expected_sha256: String }
#[derive(Default)]
struct DigestCache { entries: VecDeque<DigestKey> }
impl DigestCache {
    fn contains(&mut self, key: &DigestKey) -> bool {
        let Some(index) = self.entries.iter().position(|entry| entry == key) else { return false; };
        let entry = self.entries.remove(index).expect("located cache entry"); self.entries.push_back(entry); true
    }
    fn insert(&mut self, key: DigestKey) {
        if self.contains(&key) { return; }
        if self.entries.len() >= MAX_DIGEST_CACHE_ENTRIES { self.entries.pop_front(); }
        self.entries.push_back(key);
    }
}

struct VerifiedApp { app: BoundPath, info: BoundPath, main: BoundPath, bundled_version: BoundPath }
impl VerifiedApp {
    fn capture(host: &Host, c: &Contract, cache: &Mutex<DigestCache>, budget: &mut IdentityBudget) -> Result<Option<Self>, ()> {
        c.validate()?; budget.visit()?;
        let Some(path) = select_app(host)? else { return Ok(None); };
        let app = BoundPath::capture(path, true)?;
        let mut info = BoundPath::capture(relative(&app.path, &c.info.path)?, false)?;
        let value = plist::Value::from_reader(std::io::Cursor::new(info.guarded_bytes(&c.info, budget)?)).map_err(|_| ())?;
        let fields = value.as_dictionary().ok_or(())?;
        for (key, expected) in [("CFBundleIdentifier", &c.bundle_id), ("CFBundleName", &c.bundle_name),
            ("CFBundleExecutable", &c.executable), ("CFBundleShortVersionString", &c.version),
            ("CFBundleVersion", &c.version), ("WarpVersion", &c.warp_version)] {
            if fields.get(key).and_then(plist::Value::as_string) != Some(expected.as_str()) { return Err(()); }
        }
        let mut bundled_version = BoundPath::capture(relative(&app.path, &c.bundled_version.path)?, false)?;
        let value: Value = serde_json::from_slice(&bundled_version.guarded_bytes(&c.bundled_version, budget)?).map_err(|_| ())?;
        if value.get("warp_version").and_then(Value::as_str) != Some(c.warp_version.as_str()) { return Err(()); }
        budget.visit()?;
        let main = BoundPath::capture(relative(&app.path, &c.main.path)?, false)?;
        let mut proof = Self { app, info, main, bundled_version };
        if proof.main.stamp.bytes != c.main.bytes { return Err(()); }
        budget.metadata(32)?;
        let mut header = [0; 32]; proof.main.file.read_exact(&mut header).map_err(|_| ())?;
        let word = |start| u32::from_le_bytes(header[start..start + 4].try_into().expect("header word"));
        if word(0) != 0xfeedfacf || word(4) != 0x0100000c || word(12) != 2
            || !(1..=1024).contains(&word(16)) || !(8..=65_536).contains(&word(20))
            || u64::from(word(20)) + 32 > c.main.bytes { return Err(()); }
        proof.revalidate(host)?;
        let key = DigestKey { app: proof.app.path.clone(), app_stamp: proof.app.stamp.clone(),
            main: proof.main.path.clone(), main_stamp: proof.main.stamp.clone(), expected_sha256: c.main.sha256.clone() };
        let cached = cache.lock().map_err(|_| ())?.contains(&key);
        if !cached {
            if c.main.bytes > MAX_IDENTITY_BYTES.saturating_sub(budget.hashed_bytes) { return Err(()); }
            proof.main.file.seek(SeekFrom::Start(0)).map_err(|_| ())?;
            let mut digest = Sha256::new(); let mut count = 0u64; let mut buffer = [0; 65_536];
            loop {
                let read = (&mut proof.main.file).take(c.main.bytes.saturating_sub(count) + 1).read(&mut buffer).map_err(|_| ())?;
                if read == 0 { break; }
                count += read as u64; budget.hashed_bytes += read as u64;
                if count > c.main.bytes || budget.hashed_bytes > MAX_IDENTITY_BYTES { return Err(()); }
                digest.update(&buffer[..read]);
            }
            if count != c.main.bytes || hex::encode(digest.finalize()) != c.main.sha256 { return Err(()); }
        }
        // Cache only a main-file digest. Every call still verifies both small
        // public guards and binds all new FDs through final publication CAS.
        proof.revalidate(host)?;
        if !cached { cache.lock().map_err(|_| ())?.insert(key); }
        Ok(Some(proof))
    }

    fn revalidate(&self, host: &Host) -> Result<(), ()> {
        if select_app(host)?.as_ref() != Some(&self.app.path) { return Err(()); }
        self.app.revalidate()?; self.info.revalidate()?; self.main.revalidate()?; self.bundled_version.revalidate()
    }
}

#[derive(Default)]
struct ResourceBudget { entries: usize, leaves: usize, declared_bytes: u64 }
impl ResourceBudget {
    fn visit(&mut self) -> Result<(), ()> {
        self.entries = self.entries.checked_add(1).ok_or(())?;
        if self.entries > MAX_RESOURCE_ENTRIES { Err(()) } else { Ok(()) }
    }
    fn candidate(&mut self) -> Result<(), ()> {
        self.visit()?; self.leaves += 1;
        if self.leaves > MAX_SKILL_LEAVES { Err(()) } else { Ok(()) }
    }
}

fn child_directories(root: &BoundPath, budget: &mut ResourceBudget) -> Result<Vec<PathBuf>, ()> {
    budget.visit()?; root.revalidate()?;
    let mut result = Vec::new();
    #[cfg(unix)] {
        use std::os::unix::ffi::OsStrExt;
        use rustix::fs::{AtFlags, Dir, FileType, statat};
        for entry in Dir::read_from(&root.file).map_err(|_| ())? {
            let entry = entry.map_err(|_| ())?; let name = entry.file_name();
            if matches!(name.to_bytes(), b"." | b"..") { continue; }
            budget.visit()?;
            let kind = match entry.file_type() {
                FileType::Unknown => FileType::from_raw_mode(statat(&root.file, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| ())?.st_mode),
                kind => kind,
            };
            if kind == FileType::Symlink { return Err(()); }
            if kind != FileType::Directory { continue; }
            let label = std::str::from_utf8(name.to_bytes()).map_err(|_| ())?;
            if label.is_empty() || label.len() > 240 || label.starts_with('.')
                || !label.bytes().all(|ch| ch.is_ascii_alphanumeric() || b"._-".contains(&ch)) { return Err(()); }
            result.push(root.path.join(label));
        }
    }
    #[cfg(not(unix))] { let _ = (root, budget); return Err(()); }
    root.revalidate()?; result.sort(); Ok(result)
}

// Relative statat never resolves a replaced intermediate path and never opens
// a Skill file. Unsupported stat ABIs fail closed instead of weakening identity.
fn leaf_stamp(directory: &BoundPath) -> Result<Option<Stamp>, ()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))] {
        use rustix::fs::{AtFlags, FileType, statat};
        let value = match statat(&directory.file, "SKILL.md", AtFlags::SYMLINK_NOFOLLOW) {
            Ok(value) => value, Err(error) if error == rustix::io::Errno::NOENT => return Ok(None), _ => return Err(()),
        };
        if FileType::from_raw_mode(value.st_mode) != FileType::RegularFile { return Err(()); }
        Ok(Some(Stamp { device: value.st_dev as u64, inode: value.st_ino as u64,
            bytes: u64::try_from(value.st_size).map_err(|_| ())?, mode: value.st_mode as u32,
            mtime: value.st_mtime as i64, mtime_nsec: value.st_mtime_nsec as i64,
            ctime: value.st_ctime as i64, ctime_nsec: value.st_ctime_nsec as i64 }))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))] { let _ = directory; Err(()) }
}

struct Leaf { path: PathBuf, stamp: Stamp, parent: usize }
impl Leaf {
    fn capture(directory: &BoundPath, parent: usize, budget: &mut ResourceBudget) -> Result<Option<Self>, ()> {
        budget.visit()?;
        let Some(stamp) = leaf_stamp(directory)? else { return Ok(None); };
        if stamp.bytes > MAX_LEAF_BYTES { return Err(()); }
        budget.declared_bytes = budget.declared_bytes.checked_add(stamp.bytes).ok_or(())?;
        if budget.declared_bytes > MAX_DECLARED_LEAF_BYTES { return Err(()); }
        Ok(Some(Self { path: directory.path.join("SKILL.md"), stamp, parent }))
    }
    fn revalidate(&self, directory: &BoundPath) -> Result<(), ()> {
        if leaf_stamp(directory)?.as_ref() != Some(&self.stamp) { return Err(()); }
        Ok(())
    }
}

struct ResourceSnapshot { plugins: Vec<NativePluginObservation>, directories: Vec<BoundPath>, leaves: Vec<Leaf> }
impl ResourceSnapshot {
    fn capture(proof: &VerifiedApp, host: &Host, c: &Contract, budget: &mut ResourceBudget) -> Result<Self, ()> {
        let mut snapshot = Self { plugins: Vec::new(), directories: Vec::new(), leaves: Vec::new() };
        for group in &c.groups {
            let directory = BoundPath::capture(relative(&proof.app.path, &group.root)?, true)?;
            let mut components = Vec::new(); let mut leaf_identity = Vec::new();
            for path in child_directories(&directory, budget)? {
                budget.candidate()?;
                let child = BoundPath::capture(path, true)?;
                if let Some(leaf) = Leaf::capture(&child, snapshot.directories.len(), budget)? {
                    let name = child.path.file_name().and_then(|name| name.to_str()).ok_or(())?.to_owned();
                    components.push(PluginComponent { kind: PluginComponentKind::Skill, name: name.clone(),
                        source_path: collapse_home(&leaf.path), native_id: Some(name) });
                    leaf_identity.push((leaf.path.clone(), leaf.stamp.clone())); snapshot.leaves.push(leaf);
                }
                snapshot.directories.push(child);
            }
            let fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&(&proof.app.path, &proof.app.stamp,
                &proof.main.stamp, &c.main.sha256, &group.id, &leaf_identity)).map_err(|_| ())?));
            snapshot.plugins.push(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: group.id.clone(),
                name: group.name.clone(), version: c.version.clone(), revision: format!("artifact-sha256:{}; availability-sha256:{fingerprint}", c.main.sha256),
                presence: PluginPresence::Bundled, enabled: None,
                source_paths: vec![collapse_home(&proof.info.path), collapse_home(&proof.main.path), collapse_home(&proof.bundled_version.path), collapse_home(&directory.path)],
                components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() });
            snapshot.directories.push(directory);
        }
        snapshot.revalidate()?; Ok(snapshot)
    }
    fn revalidate(&self) -> Result<(), ()> {
        for directory in &self.directories { directory.revalidate()?; }
        for leaf in &self.leaves { leaf.revalidate(self.directories.get(leaf.parent).ok_or(())?)?; }
        for directory in &self.directories { directory.revalidate()?; }
        Ok(())
    }
}

fn observe_contract(host: &Host, c: &Contract, cache: &Mutex<DigestCache>, result: &mut PluginScan) -> Result<(), ()> {
    // Neither the large identity read nor the stat-only leaf inventory consumes
    // the shared allowance of the other native observers.
    let Some(proof) = VerifiedApp::capture(host, c, cache, &mut IdentityBudget::default())? else { return Ok(()); };
    let snapshot = ResourceSnapshot::capture(&proof, host, c, &mut ResourceBudget::default())?;
    snapshot.revalidate()?; proof.revalidate(host)?;
    result.plugins.extend(snapshot.plugins); Ok(())
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    static CACHE: OnceLock<Mutex<DigestCache>> = OnceLock::new();
    let Some(c) = contract() else { result.unavailable = true; return; };
    if observe_contract(host, c, CACHE.get_or_init(Mutex::default), result).is_err() { result.unavailable = true; }
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let Some(c) = contract() else { return Vec::new(); };
    let Ok(apps) = host_paths(host) else { return Vec::new(); };
    let mut paths = Vec::new();
    for app in apps {
        if checked_root(&app).is_err() { continue; }
        paths.push(app.clone());
        for guard in [&c.info, &c.main, &c.bundled_version] {
            let path = app.join(&guard.path); if checked_root(&path).is_ok() { paths.push(path); }
        }
        for group in &c.groups {
            let path = app.join(&group.root); if checked_root(&path).is_ok() { paths.push(path); }
        }
    }
    paths.sort(); paths.dedup(); paths
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests;
