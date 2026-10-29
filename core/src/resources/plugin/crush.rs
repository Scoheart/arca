//! Passive identity for one audited Crush artifact. No native-plugin observer.
//! Never executes Crush, shell configuration, helpers, MCPs or version probes.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    version: String,
    binary_bytes: u64,
    binary_sha256: String,
}

fn contract() -> Option<&'static Contract> {
    static VALUE: OnceLock<Option<Contract>> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-runtime-crush.json")).ok()).as_ref()
}

fn absolute_selection(path: &Path) -> Result<(), ()> {
    if !path.is_absolute()
        || path.components().any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)))
        || path.to_str().is_none_or(|value| value.chars().any(char::is_control)) {
        return Err(());
    }
    Ok(())
}

fn same_metadata(before: &fs::Metadata, after: &fs::Metadata) -> Result<(), ()> {
    if before.file_type() != after.file_type() || before.len() != after.len()
        || before.modified().map_err(|_| ())? != after.modified().map_err(|_| ())? {
        return Err(());
    }
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino()
            || before.ctime() != after.ctime() || before.ctime_nsec() != after.ctime_nsec() {
            return Err(());
        }
    }
    Ok(())
}

struct RuntimeSelection {
    selected_metadata: fs::Metadata,
    physical: PathBuf,
    physical_metadata: fs::Metadata,
}

impl RuntimeSelection {
    fn capture(selected: &Path) -> Result<Self, ()> {
        absolute_selection(selected)?;
        let selected_metadata = fs::symlink_metadata(selected).map_err(|_| ())?;
        if !selected_metadata.file_type().is_file() && !selected_metadata.file_type().is_symlink() {
            return Err(());
        }
        // Resolve only the explicitly selected alias; never search another install.
        let physical = fs::canonicalize(selected).map_err(|_| ())?;
        absolute_selection(&physical)?;
        checked_root(&physical)?;
        let physical_metadata = fs::symlink_metadata(&physical).map_err(|_| ())?;
        if !physical_metadata.file_type().is_file() { return Err(()); }
        Ok(Self { selected_metadata, physical, physical_metadata })
    }

    fn revalidate(&self, selected: &Path, file: &fs::File) -> Result<(), ()> {
        same_metadata(&self.selected_metadata, &fs::symlink_metadata(selected).map_err(|_| ())?)?;
        same_metadata(&self.physical_metadata, &fs::symlink_metadata(&self.physical).map_err(|_| ())?)?;
        same_metadata(&self.physical_metadata, &file.metadata().map_err(|_| ())?)?;
        if fs::canonicalize(selected).map_err(|_| ())? != self.physical { return Err(()); }
        Ok(())
    }
}

fn verified_runtime(c: &Contract, selected: &Path, budget: &mut ScanBudget) -> Result<PathBuf, ()> {
    let selection = RuntimeSelection::capture(selected)?;
    // This helper streams the whole file through the separate finite digest
    // budget, confines physical opens, and checks the opened-file/path identity.
    // Keep that descriptor alive while checking the caller's selection again.
    let file = verified_artifact(&selection.physical, c.binary_bytes, &c.binary_sha256, budget)?;
    selection.revalidate(selected, &file)?;
    Ok(selection.physical)
}

fn version_for(c: &Contract, selected: &Path) -> Option<String> {
    verified_runtime(c, selected, &mut ScanBudget::default()).ok().map(|_| c.version.clone())
}

pub(super) fn runtime_version(selected: &Path) -> Option<String> {
    version_for(contract()?, selected)
}

#[cfg(test)]
mod tests;
