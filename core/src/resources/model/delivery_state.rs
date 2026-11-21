use crate::domain::credentials::CredentialReference;
use crate::paths::model_delivery_file;
use crate::safe_write::{acquire_settings_lock, write_private_if_unchanged};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
struct TargetStamp {
    path: PathBuf,
    modified: (u64, u32),
    bytes: u64,
    #[cfg(unix)]
    identity: (u64, u64, u32),
}

impl TargetStamp {
    fn read(path: &Path) -> Option<Self> {
        let metadata = fs::symlink_metadata(path).ok()?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return None;
        }
        let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        Some(Self {
            path: path.to_path_buf(),
            modified: (modified.as_secs(), modified.subsec_nanos()),
            bytes: metadata.len(),
            #[cfg(unix)]
            identity: (metadata.dev(), metadata.ino(), metadata.permissions().mode()),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Applied {
    revision: String,
    files: Vec<TargetStamp>,
}

type Deliveries = BTreeMap<String, Applied>;

fn record_id(agent: &str, profile: &str) -> String {
    format!("{agent}:{profile}")
}

fn read(path: &Path) -> Result<(Option<String>, Deliveries), String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => Some(raw),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(_) => return Err("credential_delivery_state_unavailable: delivery records could not be read".into()),
    };
    let records = raw.as_deref().map(serde_json::from_str).transpose()
        .map_err(|_| "credential_delivery_state_invalid: delivery records are invalid".to_string())?
        .unwrap_or_default();
    Ok((raw, records))
}

fn mutate(change: impl FnOnce(&mut Deliveries)) -> Result<(), String> {
    let path = model_delivery_file();
    let _guard = acquire_settings_lock(&path)?;
    let (original, mut records) = read(&path)?;
    change(&mut records);
    if original.is_none() && records.is_empty() {
        return Ok(());
    }
    let output = serde_json::to_string_pretty(&records)
        .map_err(|_| "credential_delivery_state_invalid: delivery record could not be encoded".to_string())?;
    write_private_if_unchanged(&path, original.as_deref(), &output)
}

pub(super) fn save(agent: &str, profile: &str, reference: Option<&CredentialReference>, paths: &[String]) -> Result<(), String> {
    let id = record_id(agent, profile);
    let applied = reference.map(|reference| {
        let files: Vec<_> = paths.iter().filter_map(|path| TargetStamp::read(Path::new(path))).collect();
        Applied { revision: reference.revision.clone(), files }
    });
    mutate(|records| match applied {
        Some(applied) => { records.insert(id, applied); }
        None => { records.remove(&id); }
    })
}

pub(super) fn clear_agent(agent: &str) -> Result<(), String> {
    let prefix = format!("{agent}:");
    mutate(|records| records.retain(|id, _| !id.starts_with(&prefix)))
}

pub(super) fn matches(agent: &str, profile: &str, reference: &CredentialReference, expected_paths: &[PathBuf]) -> bool {
    let Ok((_, records)) = read(&model_delivery_file()) else { return false };
    let Some(applied) = records.get(&record_id(agent, profile)) else { return false };
    if applied.revision != reference.revision || applied.files.is_empty()
        || !expected_paths.iter().all(|path| applied.files.iter().any(|file| &file.path == path))
    {
        return false;
    }
    applied.files.iter().all(|previous| TargetStamp::read(&previous.path).as_ref() == Some(previous))
}

pub(super) fn uses_revision(revision: &str) -> Result<bool, String> {
    Ok(read(&model_delivery_file())?.1.values()
        .any(|applied| applied.revision == revision))
}
