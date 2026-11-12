//! Bounded reads of inert Electron package metadata. Never loads an app module.
use super::*;
use serde_json::Map;
use std::io::{Seek, SeekFrom};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Guard { pub path: String, pub bytes: u64, pub sha256: String }

fn bytes(file: &mut fs::File, count: usize, budget: &mut ScanBudget) -> Result<Vec<u8>, ()> {
    budget.visit()?;
    if count > MAX_VERIFIED_BYTES.saturating_sub(budget.verified_bytes) { return Err(()); }
    let mut data = vec![0; count];
    file.read_exact(&mut data).map_err(|_| ())?;
    budget.verified_bytes += count;
    Ok(data)
}

/// Parse exactly the bytes whose digest was checked, without reopening a path.
pub(super) fn guarded_json(path: &Path, guard: &Guard, budget: &mut ScanBudget) -> Result<Value, ()> {
    if guard.bytes > 2_097_152 { return Err(()); }
    let mut file = open_confined_file(path)?;
    if file.metadata().map_err(|_| ())?.len() != guard.bytes { return Err(()); }
    let data = bytes(&mut file, usize::try_from(guard.bytes).map_err(|_| ())?, budget)?;
    if hex::encode(Sha256::digest(&data)) != guard.sha256 { return Err(()); }
    serde_json::from_slice(&data).map_err(|_| ())
}

pub(super) fn app_info(app: &Path, bundle: &str, name: &str, budget: &mut ScanBudget) -> Result<plist::Dictionary, ()> {
    let mut file = open_confined_file(&relative(app, "Contents/Info.plist")?)?;
    let length = file.metadata().map_err(|_| ())?.len();
    if length > 1_048_576 { return Err(()); }
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes(&mut file, length as usize, budget)?)).map_err(|_| ())?;
    let plist::Value::Dictionary(info) = value else { return Err(()); };
    if info.get("CFBundleIdentifier").and_then(plist::Value::as_string) != Some(bundle)
        || info.get("CFBundleName").and_then(plist::Value::as_string) != Some(name) { return Err(()); }
    Ok(info)
}

pub(super) fn app_version(app: &Path, bundle: &str, name: &str, budget: &mut ScanBudget) -> Result<String, ()> {
    app_info(app, bundle, name, budget)?.get("CFBundleShortVersionString")
        .and_then(plist::Value::as_string).and_then(metadata_name).ok_or(())
}

/// ASAR paths are local archive members. Symlinks and unpacked entries do
/// not count as evidence of a member in this archive.
pub(super) struct AsarArchive {
    path: PathBuf,
    file: fs::File,
    before: fs::Metadata,
    header: Value,
    base: u64,
}

impl AsarArchive {
    pub(super) fn open(path: &Path, expected_header_hash: Option<&str>, budget: &mut ScanBudget) -> Result<Self, ()> {
        let mut file = open_confined_file(path)?;
        let before = file.metadata().map_err(|_| ())?;
        if !before.is_file() { return Err(()); }
        let prefix = bytes(&mut file, 16, budget)?;
        let word = |offset| u32::from_le_bytes(prefix[offset..offset+4].try_into().expect("four bytes")) as u64;
        let pickle = word(4);
        let payload = word(8);
        let length = word(12);
        if word(0) != 4 || pickle % 4 != 0 || payload.checked_add(4) != Some(pickle)
            || length > 8 * 1_048_576 || length.checked_add(8).is_none_or(|n| n > pickle)
            || pickle > length + 11 { return Err(()); }
        let base = pickle.checked_add(8).filter(|n| *n <= before.len()).ok_or(())?;
        let data = bytes(&mut file, length as usize, budget)?;
        if expected_header_hash.is_some_and(|hash| hex::encode(Sha256::digest(&data)) != hash) { return Err(()); }
        let header = serde_json::from_slice(&data).map_err(|_| ())?;
        Ok(Self { path: path.into(), file, before, header, base })
    }

    fn node(&self, path: &str) -> Result<&Value, ()> {
        let mut node = &self.header;
        for part in qoder::local_path(path)?.split('/') {
            node = node.get("files").and_then(|v| v.get(part)).ok_or(())?;
            if node.get("link").is_some() || node.get("unpacked").is_some_and(|v| v != &Value::Bool(false)) { return Err(()); }
        }
        Ok(node)
    }

    pub(super) fn children(&self, path: &str) -> Result<&Map<String, Value>, ()> {
        self.node(path)?.get("files").and_then(Value::as_object).ok_or(())
    }

    fn member(&self, path: &str) -> Result<(u64, u64), ()> {
        let node = self.node(path)?;
        if node.get("files").is_some() { return Err(()); }
        let size = node.get("size").and_then(Value::as_u64).ok_or(())?;
        let offset = node.get("offset").and_then(Value::as_str).ok_or(())?.parse::<u64>().map_err(|_| ())?;
        let offset = self.base.checked_add(offset).ok_or(())?;
        if offset.checked_add(size).is_none_or(|end| end > self.before.len()) { return Err(()); }
        Ok((offset, size))
    }

    pub(super) fn has_file(&self, path: &str) -> bool {
        self.member(path).is_ok_and(|(_, size)| size > 0)
    }

    pub(super) fn json(&mut self, path: &str, budget: &mut ScanBudget) -> Result<Value, ()> {
        let (offset, size) = self.member(path)?;
        if size > 2_097_152 { return Err(()); }
        let hash = self.node(path)?.pointer("/integrity/hash").and_then(Value::as_str).ok_or(())?.to_owned();
        let data = self.read(offset, size, budget)?;
        if hex::encode(Sha256::digest(&data)) != hash { return Err(()); }
        serde_json::from_slice(&data).map_err(|_| ())
    }

    fn read(&mut self, offset: u64, size: u64, budget: &mut ScanBudget) -> Result<Vec<u8>, ()> {
        self.file.seek(SeekFrom::Start(offset)).map_err(|_| ())?;
        bytes(&mut self.file, usize::try_from(size).map_err(|_| ())?, budget)
    }

    pub(super) fn recheck(&self) -> Result<(), ()> {
        let after = self.file.metadata().map_err(|_| ())?;
        let bound = fs::symlink_metadata(&self.path).map_err(|_| ())?;
        if !bound.file_type().is_file() || self.before.len() != after.len()
            || self.before.modified().map_err(|_| ())? != after.modified().map_err(|_| ())? { return Err(()); }
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            if self.before.dev() != bound.dev() || self.before.ino() != bound.ino()
                || self.before.ctime() != after.ctime() || self.before.ctime_nsec() != after.ctime_nsec() { return Err(()); }
        }
        Ok(())
    }
}

pub(super) fn guard_asar(path: &Path, guards: &[Guard], budget: &mut ScanBudget) -> Result<(), ()> {
    if guards.is_empty() || guards.len() > 32 { return Err(()); }
    let mut archive = AsarArchive::open(path, None, budget)?;
    for guard in guards {
        let (offset, size) = archive.member(&guard.path)?;
        if size != guard.bytes
            || hex::encode(Sha256::digest(archive.read(offset, size, budget)?)) != guard.sha256 { return Err(()); }
    }
    archive.recheck()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    #[test]
    fn binary_verification_does_not_consume_metadata_allowance_and_remains_bounded() {
        let home=TestHome::new("electron-verification-budget");let path=home.home.join("payload");fs::write(&path,b"data").unwrap();
        let mut budget=ScanBudget{bytes:MAX_READ_BYTES,..Default::default()};let digest=hex::encode(Sha256::digest(b"data"));
        verify_artifact(&path,4,&digest,&mut budget).unwrap();assert_eq!(budget.bytes,MAX_READ_BYTES);
        budget.verified_bytes=MAX_VERIFIED_BYTES;assert!(verify_artifact(&path,4,&digest,&mut budget).is_err());
    }

    #[test]
    fn asar_guards_reject_links_unpacked_payloads_out_of_bounds_and_wrong_digests() {
        let home = TestHome::new("electron-metadata");
        let path = home.home.join("fixture.asar");
        let guard = Guard { path: "main.js".into(), bytes: 4, sha256: hex::encode(Sha256::digest(b"code")) };
        for (member, data, accepted) in [
            (serde_json::json!({"offset":"0","size":4}), "code", true),
            (serde_json::json!({"offset":"0","size":4,"link":"other.js"}), "code", false),
            (serde_json::json!({"offset":"0","size":4,"unpacked":true}), "code", false),
            (serde_json::json!({"offset":"99","size":4}), "code", false),
            (serde_json::json!({"offset":"0","size":4}), "evil", false),
        ] {
            let header = serde_json::to_vec(&serde_json::json!({"files":{"main.js":member}})).unwrap();
            let pickle = (header.len()+11)&!3;
            let mut raw = Vec::new();
            for word in [4, pickle as u32, (pickle-4) as u32, header.len() as u32] { raw.extend(word.to_le_bytes()); }
            raw.extend(header); raw.resize(pickle+8, 0); raw.extend(data.as_bytes()); fs::write(&path,raw).unwrap();
            assert_eq!(guard_asar(&path,std::slice::from_ref(&guard),&mut ScanBudget::default()).is_ok(),accepted);
        }
    }
}
