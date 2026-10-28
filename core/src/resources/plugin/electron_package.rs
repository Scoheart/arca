//! Bounded reads of inert Electron package metadata. Never loads an app module.
use super::*;
use std::io::{Seek, SeekFrom};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Guard { pub path: String, pub bytes: u64, pub sha256: String }

fn bytes(file: &mut fs::File, count: usize, budget: &mut ScanBudget) -> Result<Vec<u8>, ()> {
    budget.visit()?;
    if count > MAX_READ_BYTES.saturating_sub(budget.bytes) { return Err(()); }
    let mut data = vec![0; count];
    file.read_exact(&mut data).map_err(|_| ())?;
    budget.bytes += count;
    Ok(data)
}

pub(super) fn app_version(app: &Path, bundle: &str, name: &str, budget: &mut ScanBudget) -> Result<String, ()> {
    let mut file = open_confined_file(&relative(app, "Contents/Info.plist")?)?;
    let length = file.metadata().map_err(|_| ())?.len();
    if length > 1_048_576 { return Err(()); }
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes(&mut file, length as usize, budget)?)).map_err(|_| ())?;
    let info = value.as_dictionary().ok_or(())?;
    if info.get("CFBundleIdentifier").and_then(plist::Value::as_string) != Some(bundle)
        || info.get("CFBundleName").and_then(plist::Value::as_string) != Some(name) { return Err(()); }
    info.get("CFBundleShortVersionString").and_then(plist::Value::as_string).and_then(metadata_name).ok_or(())
}

pub(super) fn guard_asar(path: &Path, guards: &[Guard], budget: &mut ScanBudget) -> Result<(), ()> {
    if guards.is_empty() || guards.len() > 32 { return Err(()); }
    let mut file = open_confined_file(path)?;
    let before = file.metadata().map_err(|_| ())?;
    if !before.is_file() { return Err(()); }
    let prefix = bytes(&mut file, 16, budget)?;
    let word = |offset| u32::from_le_bytes(prefix[offset..offset+4].try_into().expect("four bytes")) as u64;
    let pickle = word(4); let payload = word(8); let length = word(12);
    if word(0) != 4 || pickle % 4 != 0 || payload.checked_add(4) != Some(pickle)
        || length > 8 * 1_048_576 || length.checked_add(8).is_none_or(|n| n > pickle)
        || pickle > length + 11 { return Err(()); }
    let base = pickle.checked_add(8).filter(|n| *n <= before.len()).ok_or(())?;
    let header: Value = serde_json::from_slice(&bytes(&mut file, length as usize, budget)?).map_err(|_| ())?;
    for guard in guards {
        let mut node = &header;
        for part in qoder::local_path(&guard.path)?.split('/') {
            node = node.get("files").and_then(|v| v.get(part)).ok_or(())?;
            if node.get("link").is_some() || node.get("unpacked").is_some_and(|v| v != &Value::Bool(false)) { return Err(()); }
        }
        if node.get("files").is_some() || node.get("size").and_then(Value::as_u64) != Some(guard.bytes) { return Err(()); }
        let offset = node.get("offset").and_then(Value::as_str).ok_or(())?.parse::<u64>().map_err(|_| ())?;
        let offset = base.checked_add(offset).ok_or(())?;
        if offset.checked_add(guard.bytes).is_none_or(|end| end > before.len()) { return Err(()); }
        file.seek(SeekFrom::Start(offset)).map_err(|_| ())?;
        let count = usize::try_from(guard.bytes).map_err(|_| ())?;
        if hex::encode(Sha256::digest(bytes(&mut file, count, budget)?)) != guard.sha256 { return Err(()); }
    }
    let after = file.metadata().map_err(|_| ())?;
    let bound = fs::symlink_metadata(path).map_err(|_| ())?;
    if !bound.file_type().is_file() || before.len() != after.len()
        || before.modified().map_err(|_| ())? != after.modified().map_err(|_| ())? { return Err(()); }
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != bound.dev() || before.ino() != bound.ino()
            || before.ctime() != after.ctime() || before.ctime_nsec() != after.ctime_nsec() { return Err(()); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

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
