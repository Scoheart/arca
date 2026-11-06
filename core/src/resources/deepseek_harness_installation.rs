//! Passive proof that the selected, audited Harness installation supplies MCP manager.
//! No settings access: callers may already hold the settings mutation lock.
use serde_json::Value;
use std::{fs, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}};

const VERSION: &str = "0.2.2-rc.1";
const REPOSITORY: &str = "git+https://github.com/deepseek-ai/deepseek-harness.git";
const LIMIT: usize = 2 * 1024 * 1024;
const HEADER_LIMIT: usize = 8 * 1024 * 1024;
const WEB_PATCHES: &[(&str, &str)] = &[
    ("cordis.patch.yml", "746394470e99f57ecb22a95f445d7cdaae1297ce17b8e4799a6c573e93677927"),
    ("presets/standard.patch.yml", "3006cc3780cddaa7fb0089e5854e6bbef4409cf1d425930fd835226b95660afd"),
    ("presets/ptc.patch.yml", "ce02148b1fd575cc93071744180468cd305bdcd50dbfdd3b9b0605c5281d283d"),
    ("presets/minimal.patch.yml", "a2a9ba1c9c392a45466708f18bc0aefbf300577f9b544dc06049f73a8a5b3267"),
    ("presets/cordis.patch.yml", "0e8f611644b3c36249808c4d7eb7a40d78006d04da6b55eef1a42797a5a34355"),
];

fn unavailable() -> String {
    "deepseek_harness_mcp_manager_unverified: creating an inherited MCP manager override requires the selected audited 0.2.2-rc.1 installation and its manager bundle; older, unknown or unavailable installations remain read-only. Update/initialize Harness, or configure its complete native manager override first".into()
}

fn stamp(metadata: &fs::Metadata) -> Result<String, ()> {
    if !metadata.is_file() { return Err(()); }
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        Ok(format!("{}:{}:{}:{}:{}:{}:{}", metadata.dev(), metadata.ino(), metadata.len(), metadata.ctime(), metadata.ctime_nsec(), metadata.mtime(), metadata.mtime_nsec()))
    }
    #[cfg(not(unix))] { Ok(format!("{}:{:?}", metadata.len(), metadata.modified().map_err(|_| ())?)) }
}

struct BoundFile { path: PathBuf, file: fs::File, stamp: String }
impl BoundFile {
    fn open(path: &Path) -> Result<Self, ()> {
        let file = crate::resources::plugin::open_confined_file(path)?;
        let signature = stamp(&file.metadata().map_err(|_| ())?)?;
        Ok(Self { path: path.into(), file, stamp: signature })
    }
    fn recheck(&self) -> Result<(), ()> {
        if stamp(&self.file.metadata().map_err(|_| ())?)? != self.stamp
            || stamp(&fs::symlink_metadata(&self.path).map_err(|_| ())?)? != self.stamp { return Err(()); }
        Ok(())
    }
    fn read(&mut self, offset: u64, length: usize) -> Result<Vec<u8>, ()> {
        let total = self.file.metadata().map_err(|_| ())?.len();
        if length > HEADER_LIMIT || offset.checked_add(length as u64).is_none_or(|end| end > total) { return Err(()); }
        self.file.seek(SeekFrom::Start(offset)).map_err(|_| ())?;
        let mut bytes = vec![0; length];
        self.file.read_exact(&mut bytes).map_err(|_| ())?;
        self.recheck()?;
        Ok(bytes)
    }
}

#[derive(Default)]
struct Proof { files: Vec<BoundFile>, aliases: Vec<(PathBuf, PathBuf)> }
impl Proof {
    fn alias(&mut self, path: &Path) -> Result<PathBuf, ()> {
        let physical = fs::canonicalize(path).map_err(|_| ())?;
        self.aliases.push((path.into(), physical.clone()));
        Ok(physical)
    }
    fn bytes(&mut self, path: &Path) -> Result<Vec<u8>, ()> {
        let mut file = BoundFile::open(path)?;
        let length = usize::try_from(file.file.metadata().map_err(|_| ())?.len()).map_err(|_| ())?;
        if length > LIMIT { return Err(()); }
        let bytes = file.read(0, length)?;
        self.files.push(file);
        Ok(bytes)
    }
    fn json(&mut self, path: &Path) -> Result<Value, ()> { serde_json::from_slice(&self.bytes(path)?).map_err(|_| ()) }
    fn member(&mut self, path: &Path) -> Result<(), ()> {
        let file = BoundFile::open(path)?;
        if file.file.metadata().map_err(|_| ())?.len() == 0 { return Err(()); }
        self.files.push(file); Ok(())
    }
    fn recheck(&self) -> Result<(), ()> {
        for file in &self.files { file.recheck()?; }
        for (alias, physical) in &self.aliases {
            if fs::canonicalize(alias).map_err(|_| ())? != *physical { return Err(()); }
        }
        Ok(())
    }
}

fn package(value: &Value, name: &str, directory: &str) -> Result<(), ()> {
    if value.get("name").and_then(Value::as_str) != Some(name)
        || value.get("version").and_then(Value::as_str) != Some(VERSION)
        || value.pointer("/repository/url").and_then(Value::as_str) != Some(REPOSITORY)
        || value.pointer("/repository/directory").and_then(Value::as_str) != Some(directory) { return Err(()); }
    Ok(())
}
fn dependency(value: &Value, name: &str) -> Result<(), ()> {
    if value.get("dependencies").and_then(|v| v.get(name)).and_then(Value::as_str).is_none_or(str::is_empty) { return Err(()); }
    Ok(())
}
fn web_manifest(value: &Value) -> Result<(), ()> {
    package(value, "@deepseek-ai/dsh-web-app", "packages/bundle/web-app")?;
    dependency(value, "@deepseek-ai/dsh-mcp-manager")?;
    let expected = WEB_PATCHES.iter().map(|(path, _)| Value::String(format!("./{path}"))).collect::<Vec<_>>();
    if value.pointer("/dsh/bundle/patch").and_then(Value::as_array) != Some(&expected) { return Err(()); }
    Ok(())
}
fn patch(bytes: &[u8], expected: &str) -> Result<(), ()> {
    use sha2::{Digest, Sha256};
    if hex::encode(Sha256::digest(bytes)) != expected { return Err(()); }
    Ok(())
}

/// Node's installation-anchor node_modules order, without importing Node or
/// accepting a later package after an earlier present but invalid package.
fn resolve_package(root: &Path, name: &str, pool: &Path, proof: &mut Proof) -> Result<PathBuf, ()> {
    for directory in root.ancestors().take(48) {
        if !directory.starts_with(pool) { break; }
        // The node_modules directory itself is its parent's next lookup slot.
        // Checking it here preserves order while keeping the pool boundary.
        let slot = if directory.file_name().is_some_and(|part| part == "node_modules") {
            directory.join(name)
        } else { directory.join("node_modules").join(name) };
        match fs::symlink_metadata(slot.join("package.json")) {
            Ok(_) => {
                let physical = proof.alias(&slot)?;
                if !physical.starts_with(pool) { return Err(()); }
                return Ok(physical);
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err(()),
        }
    }
    Err(())
}

fn cli() -> Result<(), ()> {
    let selected = crate::application::agent_launch::find_command("dsh").ok_or(())?;
    let mut proof = Proof::default();
    let entry = proof.alias(&selected)?;
    if entry.file_name().is_none_or(|name| name != "bin.js")
        || entry.parent().and_then(Path::file_name).is_none_or(|name| name != "lib") { return Err(()); }
    let root = entry.parent().and_then(Path::parent).ok_or(())?;
    if root.file_name().is_none_or(|name| name != "dsh")
        || root.parent().and_then(Path::file_name).is_none_or(|name| name != "@deepseek-ai")
        || root.parent().and_then(Path::parent).and_then(Path::file_name).is_none_or(|name| name != "node_modules") { return Err(()); }
    // pnpm may link packages between its sibling .pnpm trees; permit only the
    // selected installation's enclosing node_modules pool, never an alias to
    // another user directory or Node's global fallback locations.
    let pool = root.ancestors().filter(|directory| directory.file_name().is_some_and(|name| name == "node_modules")).last().ok_or(())?;
    proof.member(&entry)?;
    let app = proof.json(&root.join("package.json"))?;
    package(&app, "@deepseek-ai/dsh", "apps/cli")?;
    if app.pointer("/bin/dsh").and_then(Value::as_str) != Some("lib/bin.js") { return Err(()); }
    for name in ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] { dependency(&app, name)?; }
    let base = resolve_package(root, "@deepseek-ai/dsh-base", pool, &mut proof)?;
    package(&proof.json(&base.join("package.json"))?, "@deepseek-ai/dsh-base", "packages/bundle/base")?;
    let web = resolve_package(root, "@deepseek-ai/dsh-web-app", pool, &mut proof)?;
    web_manifest(&proof.json(&web.join("package.json"))?)?;
    for (path, digest) in WEB_PATCHES { patch(&proof.bytes(&web.join(path))?, digest)?; }
    let manager = resolve_package(&web, "@deepseek-ai/dsh-mcp-manager", pool, &mut proof)?;
    let metadata = proof.json(&manager.join("package.json"))?;
    package(&metadata, "@deepseek-ai/dsh-mcp-manager", "packages/mcp/mcp-manager")?;
    if metadata.get("main").and_then(Value::as_str) != Some("lib/index.js") { return Err(()); }
    proof.member(&manager.join("lib/index.js"))?;
    proof.recheck()?;
    if crate::application::agent_launch::find_command("dsh") != Some(selected) { return Err(()); }
    Ok(())
}

/// The production Desktop puts the immutable dsh package tree inside app.asar.
/// Read only fixed inert members; never follow ASAR links/unpacked indirection.
struct Asar { bound: BoundFile, header: Value, payload: u64 }
impl Asar {
    fn open(path: &Path) -> Result<Self, ()> {
        let mut bound = BoundFile::open(path)?;
        let prefix = bound.read(0, 16)?;
        let word = |offset| u32::from_le_bytes(prefix[offset..offset+4].try_into().expect("four bytes")) as u64;
        let pickle = word(4); let payload_size = word(8); let length = word(12);
        if word(0) != 4 || pickle % 4 != 0 || payload_size.checked_add(4) != Some(pickle)
            || length > HEADER_LIMIT as u64 || length.checked_add(8).is_none_or(|n| n > pickle)
            || pickle > length + 11 { return Err(()); }
        let payload = pickle.checked_add(8).ok_or(())?;
        if payload > bound.file.metadata().map_err(|_| ())?.len() { return Err(()); }
        let header = serde_json::from_slice(&bound.read(16, length as usize)?).map_err(|_| ())?;
        Ok(Self { bound, header, payload })
    }
    fn location(&self, path: &str) -> Result<(u64, usize), ()> {
        let mut node = &self.header;
        for part in path.split('/') {
            if part.is_empty() || matches!(part, "." | "..") { return Err(()); }
            node = node.get("files").and_then(|files| files.get(part)).ok_or(())?;
            if node.get("link").is_some() || node.get("unpacked").is_some_and(|v| v != &Value::Bool(false)) { return Err(()); }
        }
        if node.get("files").is_some() { return Err(()); }
        let length = usize::try_from(node.get("size").and_then(Value::as_u64).ok_or(())?).map_err(|_| ())?;
        let relative = node.get("offset").and_then(Value::as_str).ok_or(())?.parse::<u64>().map_err(|_| ())?;
        let offset = self.payload.checked_add(relative).ok_or(())?;
        if offset.checked_add(length as u64).is_none_or(|end| end > self.bound.file.metadata().ok().map(|m| m.len()).unwrap_or(0)) { return Err(()); }
        Ok((offset, length))
    }
    fn bytes(&mut self, path: &str) -> Result<Vec<u8>, ()> { let (offset, length) = self.location(path)?; if length > LIMIT { return Err(()); } self.bound.read(offset, length) }
    fn json(&mut self, path: &str) -> Result<Value, ()> { serde_json::from_slice(&self.bytes(path)?).map_err(|_| ()) }
}

fn selected_desktop() -> Option<PathBuf> {
    let mut roots = vec![crate::paths::system_probe_path("/Applications")];
    if let Some(home) = dirs::home_dir() { roots.push(home.join("Applications")); }
    roots.push(crate::paths::system_probe_path("/System/Applications"));
    roots.into_iter().map(|root| root.join("DeepSeek Harness.app"))
        .find(|path| fs::symlink_metadata(path.join("Contents/Info.plist")).is_ok())
}

fn desktop() -> Result<(), ()> {
    let selected = selected_desktop().ok_or(())?;
    let mut proof = Proof::default();
    let app = proof.alias(&selected)?;
    let bytes = proof.bytes(&app.join("Contents/Info.plist"))?;
    let info = plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(|_| ())?;
    let info = info.as_dictionary().ok_or(())?;
    if info.get("CFBundleIdentifier").and_then(plist::Value::as_string) != Some("com.deepseek.harness")
        || info.get("CFBundleName").and_then(plist::Value::as_string) != Some("DeepSeek Harness")
        || info.get("CFBundleShortVersionString").and_then(plist::Value::as_string) != Some(VERSION) { return Err(()); }
    let mut asar = Asar::open(&app.join("Contents/Resources/app.asar"))?;
    let shell = asar.json("package.json")?;
    if shell.get("name").and_then(Value::as_str) != Some("@deepseek-ai/dsh-desktop")
        || shell.get("version").and_then(Value::as_str) != Some(VERSION)
        || shell.get("dshDesktopAppId").and_then(Value::as_str) != Some("com.deepseek.harness") { return Err(()); }
    let runtime = asar.json("dsh/desktop-runtime.json")?;
    if runtime.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || runtime.pointer("/release/version").and_then(Value::as_str) != Some(VERSION) { return Err(()); }
    let app = asar.json("dsh/node_modules/@deepseek-ai/dsh/package.json")?;
    package(&app, "@deepseek-ai/dsh", "apps/cli")?;
    for name in ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] { dependency(&app, name)?; }
    package(&asar.json("dsh/node_modules/@deepseek-ai/dsh-base/package.json")?, "@deepseek-ai/dsh-base", "packages/bundle/base")?;
    let web_root = "dsh/node_modules/@deepseek-ai/dsh-web-app";
    web_manifest(&asar.json(&format!("{web_root}/package.json"))?)?;
    for (path, digest) in WEB_PATCHES { patch(&asar.bytes(&format!("{web_root}/{path}"))?, digest)?; }
    let manager = "dsh/node_modules/@deepseek-ai/dsh-mcp-manager";
    let metadata = asar.json(&format!("{manager}/package.json"))?;
    package(&metadata, "@deepseek-ai/dsh-mcp-manager", "packages/mcp/mcp-manager")?;
    if metadata.get("main").and_then(Value::as_str) != Some("lib/index.js") { return Err(()); }
    if asar.location("dsh/node_modules/@deepseek-ai/dsh/lib/bin.js")?.1 == 0
        || asar.location(&format!("{manager}/lib/index.js"))?.1 == 0 { return Err(()); }
    asar.bound.recheck()?;
    proof.recheck()?;
    if selected_desktop() != Some(selected) { return Err(()); }
    Ok(())
}

pub(super) fn require_manager(profile: &str) -> Result<(), String> {
    match profile { "web" => cli(), "desktop" => desktop(), _ => Err(()) }.map_err(|_| unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    use serde_json::json;

    #[test]
    fn asar_metadata_members_reject_indirection_bounds_and_changed_archive() {
        let home = TestHome::new("dsh-install-asar");
        let path = home.home.join("app.asar");
        for (member, accepted) in [
            (json!({"offset":"0","size":2}), true),
            (json!({"offset":"0","size":2,"link":"other"}), false),
            (json!({"offset":"0","size":2,"unpacked":true}), false),
            (json!({"offset":"100","size":2}), false),
        ] {
            let header = serde_json::to_vec(&json!({"files":{"package.json":member}})).unwrap();
            let pickle = (header.len() + 11) & !3;
            let mut raw = Vec::new();
            for word in [4, pickle as u32, (pickle - 4) as u32, header.len() as u32] { raw.extend(word.to_le_bytes()); }
            raw.extend(header); raw.resize(pickle + 8, 0); raw.extend(b"{}");
            fs::write(&path, &raw).unwrap();
            let mut archive = Asar::open(&path).unwrap();
            assert_eq!(archive.json("package.json").is_ok(), accepted);
            raw.push(0); fs::write(&path, raw).unwrap();
            assert!(archive.bound.recheck().is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn package_aliases_cannot_escape_the_selected_pool_or_change_after_capture() {
        use std::os::unix::fs::symlink;
        let home = TestHome::new("dsh-install-alias");
        let pool = home.home.join("node_modules");
        let root = pool.join("@deepseek-ai/dsh");
        let outside = home.home.join("unrelated");
        fs::create_dir_all(&root).unwrap(); fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("package.json"), "{}").unwrap();
        let slot = root.join("node_modules/@deepseek-ai/dsh-web-app");
        fs::create_dir_all(slot.parent().unwrap()).unwrap(); symlink(&outside, &slot).unwrap();
        assert!(resolve_package(&root, "@deepseek-ai/dsh-web-app", &pool, &mut Proof::default()).is_err());
        let first = pool.join("first"); let second = pool.join("second");
        fs::create_dir_all(&first).unwrap(); fs::create_dir_all(&second).unwrap();
        fs::remove_file(&slot).unwrap(); symlink(&first, &slot).unwrap();
        let mut proof = Proof::default(); proof.alias(&slot).unwrap();
        fs::remove_file(&slot).unwrap(); symlink(&second, &slot).unwrap();
        assert!(proof.recheck().is_err());
    }
}
