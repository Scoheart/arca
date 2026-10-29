//! Continue CLI's source-bound passive version reader. No plugin observer,
//! JavaScript import, shell, credential/configuration read or version subprocess.
use super::*;
use std::io::{Seek, SeekFrom};

const MAX_RUNTIME_BYTES: usize = 16 * 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 64 * 1024;
const MAX_WRAPPER_BYTES: u64 = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    package_name: String,
    version: String,
    module_type: String,
    command: String,
    entrypoint: String,
    entry_import: String,
    entry_export: String,
    sources: Vec<SourceFile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFile {
    path: String,
    bytes: u64,
    sha256: String,
}

fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-runtime-continue-cli.json"))
        .expect("audited Continue CLI runtime identity contract"))
}

fn clean_relative(value: &str) -> Result<PathBuf, ()> {
    let path = Path::new(value);
    if value.is_empty() || value.len() > 256 || value.contains('\\')
        || value.chars().any(char::is_control) || path.is_absolute()
        || value.split('/').any(|part| part.is_empty() || matches!(part, "." | ".."))
        || path.components().any(|part| !matches!(part, Component::Normal(_))) {
        return Err(());
    }
    Ok(path.to_path_buf())
}

fn absolute_selection(path: &Path) -> Result<(), ()> {
    if !path.is_absolute() || path.components().any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)))
        || path.to_str().is_none_or(|value| value.len() > 4096 || value.chars().any(char::is_control)) {
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
        let physical = fs::canonicalize(selected).map_err(|_| ())?;
        absolute_selection(&physical)?;
        checked_root(&physical)?;
        let physical_metadata = fs::symlink_metadata(&physical).map_err(|_| ())?;
        if !physical_metadata.file_type().is_file() { return Err(()); }
        Ok(Self { selected_metadata, physical, physical_metadata })
    }

    fn revalidate(&self, selected: &Path) -> Result<(), ()> {
        same_metadata(&self.selected_metadata, &fs::symlink_metadata(selected).map_err(|_| ())?)?;
        same_metadata(&self.physical_metadata, &fs::symlink_metadata(&self.physical).map_err(|_| ())?)?;
        if fs::canonicalize(selected).map_err(|_| ())? != self.physical { return Err(()); }
        Ok(())
    }
}

struct PublicMember {
    path: PathBuf,
    file: fs::File,
    metadata: fs::Metadata,
}
impl PublicMember {
    fn read_text(&mut self, limit: u64) -> Result<String, ()> {
        if self.metadata.len() > limit { return Err(()); }
        self.file.seek(SeekFrom::Start(0)).map_err(|_| ())?;
        let mut text = String::new();
        (&mut self.file).take(limit + 1).read_to_string(&mut text).map_err(|_| ())?;
        if text.len() as u64 != self.metadata.len() { return Err(()); }
        self.revalidate()?;
        Ok(text)
    }

    fn revalidate(&self) -> Result<(), ()> {
        same_metadata(&self.metadata, &self.file.metadata().map_err(|_| ())?)?;
        same_metadata(&self.metadata, &fs::symlink_metadata(&self.path).map_err(|_| ())?)?;
        checked_root(&self.path)
    }
}

struct VerifiedRuntime {
    selection: RuntimeSelection,
    members: Vec<PublicMember>,
}
impl VerifiedRuntime {
    fn capture(c: &Contract, selected: &Path, budget: &mut ScanBudget) -> Result<Self, ()> {
        let entry = clean_relative(&c.entrypoint)?;
        let import = clean_relative(c.entry_import.strip_prefix("./").ok_or(())?)?;
        if import.components().count() != 1 || entry.components().count() < 2 || entry.components().count() > 4
            || c.module_type != "module" || c.package_name.is_empty() || c.package_name.len() > 128
            || c.version.is_empty() || c.version.len() > 64 || c.version == "unknown"
            || c.version.chars().any(|ch| ch.is_control() || ch.is_whitespace())
            || c.command.is_empty() || c.command.len() > 64
            || !c.command.bytes().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_'))
            || c.entry_export.is_empty() || c.entry_export.len() > 64
            || c.entry_export.as_bytes()[0].is_ascii_digit()
            || !c.entry_export.bytes().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$')) {
            return Err(());
        }
        let bundle = entry.parent().ok_or(())?.join(import);
        let bundle_name = bundle.to_str().ok_or(())?;
        let required = BTreeSet::from(["package.json", c.entrypoint.as_str(), bundle_name]);
        if required.len() != 3 || c.sources.len() != 3 { return Err(()); }
        let mut declared = BTreeSet::new();
        let mut total = 0usize;
        for source in &c.sources {
            clean_relative(&source.path)?;
            if !required.contains(source.path.as_str()) || !declared.insert(source.path.as_str())
                || source.bytes == 0 || source.bytes > MAX_RUNTIME_BYTES as u64
                || source.sha256.len() != 64 || !source.sha256.bytes().all(|ch| ch.is_ascii_hexdigit())
                || (source.path == "package.json" && source.bytes > MAX_PACKAGE_BYTES)
                || (source.path == c.entrypoint && source.bytes > MAX_WRAPPER_BYTES) {
                return Err(());
            }
            total = total.checked_add(source.bytes as usize).ok_or(())?;
        }
        if total > MAX_RUNTIME_BYTES || total > MAX_RUNTIME_BYTES.saturating_sub(budget.verified_bytes) {
            return Err(());
        }

        let selection = RuntimeSelection::capture(selected)?;
        if !selection.physical.ends_with(&entry) { return Err(()); }
        let root = selection.physical.ancestors().nth(entry.components().count()).ok_or(())?;
        checked_root(root)?;
        let mut members = Vec::new();
        for source in &c.sources {
            let path = relative(root, &source.path)?;
            // Whole-member SHA-256, no-follow confined open and finite digest
            // accounting. Hold every descriptor through final selection CAS.
            let file = verified_artifact(&path, source.bytes, &source.sha256, budget)?;
            let metadata = file.metadata().map_err(|_| ())?;
            members.push(PublicMember { path, file, metadata });
        }
        let manifest = members.iter_mut().find(|member| member.path == root.join("package.json")).ok_or(())?;
        let value: Value = serde_json::from_str(&manifest.read_text(MAX_PACKAGE_BYTES)?).map_err(|_| ())?;
        if value.get("name").and_then(Value::as_str) != Some(c.package_name.as_str())
            || value.get("version").and_then(Value::as_str) != Some(c.version.as_str())
            || value.get("type").and_then(Value::as_str) != Some(c.module_type.as_str())
            || value.get("bin").and_then(|bin| bin.get(&c.command)).and_then(Value::as_str) != Some(c.entrypoint.as_str()) {
            return Err(());
        }
        let wrapper = members.iter_mut().find(|member| member.path == selection.physical).ok_or(())?;
        same_metadata(&selection.physical_metadata, &wrapper.metadata)?;
        let expected = format!("#!/usr/bin/env node\nimport {{ {} }} from '{}';\nawait {}();\n",
            c.entry_export, c.entry_import, c.entry_export);
        if wrapper.read_text(MAX_WRAPPER_BYTES)? != expected { return Err(()); }
        // The only imported file is guarded in full, independently of private
        // settings, auth, .env, MCPs, hooks or user-provided component bodies.
        if !members.iter().any(|member| member.path == root.join(&bundle)) { return Err(()); }
        Ok(Self { selection, members })
    }

    fn revalidate(&self, selected: &Path) -> Result<(), ()> {
        for member in &self.members { member.revalidate()?; }
        self.selection.revalidate(selected)
    }
}

fn version_for(c: &Contract, selected: &Path) -> Option<String> {
    // Never share the plugin scan's allowance: runtime identity is independent
    // of how many unrelated hosts or private metadata documents were scanned.
    let identity = VerifiedRuntime::capture(c, selected, &mut ScanBudget::default()).ok()?;
    identity.revalidate(selected).ok()?;
    Some(c.version.clone())
}

pub(super) fn runtime_version(selected: &Path) -> Option<String> {
    version_for(contract(), selected)
}

#[cfg(test)]
mod tests;

