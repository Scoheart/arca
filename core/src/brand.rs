//! Arca brand transition. Legacy names here identify existing user data, not UI.
//! Never rewrite Agent documents or Keychain secrets just to change a brand.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const LEGACY_HOME: &str = ".mux";
pub const LEGACY_HOME_ENV: &str = "MUX_HOME";
const MARKER: &str = ".arca-brand-migration";
const MARKER_CONTENT: &[u8] = b"arca-home-v1\n";

fn override_directory() -> Option<PathBuf> {
    ["ARCA_HOME", LEGACY_HOME_ENV].into_iter().find_map(|name| {
        std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
    })
}

fn roots() -> (PathBuf, PathBuf) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    (home.join(LEGACY_HOME), home.join(".arca"))
}

/// Pure read: before migration, use the existing physical root for recovery.
pub fn data_directory() -> PathBuf {
    if let Some(path) = override_directory() { return path; }
    let (legacy, current) = roots();
    if !current.exists() && legacy.exists() { legacy } else { current }
}

fn metadata(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn conflict(message: &str) -> io::Error { io::Error::new(io::ErrorKind::AlreadyExists, message) }

/// Check both roots before acquiring a lock can create anything.
pub fn validate_roots() -> io::Result<()> {
    if override_directory().is_some() { return Ok(()); }
    let (legacy, current) = roots();
    let old = metadata(&legacy)?;
    let new = metadata(&current)?;
    if new.as_ref().is_some_and(|value| !value.is_dir() || value.file_type().is_symlink()) {
        return Err(conflict("Arca data root must be a real directory"));
    }
    if let Some(old) = old {
        if old.file_type().is_symlink() {
            if new.is_none() || fs::canonicalize(&legacy)? != fs::canonicalize(&current)? {
                return Err(conflict("legacy data root points elsewhere; set ARCA_HOME explicitly"));
            }
        } else if !old.is_dir() || new.is_some() {
            return Err(conflict("both brand data roots exist or legacy root is not a directory; refusing to merge"));
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> io::Result<()> { fs::File::open(path)?.sync_all() }

/// Called only under the shared application lock, after durable recovery.
/// Rename preserves every asset, inode and secret-free recovery file. The old
/// alias preserves absolute Skill links and old credential-helper paths.
pub fn migrate_home() -> io::Result<bool> {
    if override_directory().is_some() { return Ok(false); }
    validate_roots()?;
    let (legacy, current) = roots();
    if metadata(&legacy)?.is_some_and(|value| value.is_dir() && !value.file_type().is_symlink()) {
        let marker = legacy.join(MARKER);
        if marker.exists() {
            if fs::read(&marker)? != MARKER_CONTENT { return Err(conflict("brand migration marker is invalid")); }
        } else {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
            let mut file = options.open(&marker)?;
            file.write_all(MARKER_CONTENT)?;
            file.sync_all()?;
            sync_directory(&legacy)?;
        }
        #[cfg(unix)] {
            use rustix::fs::{renameat_with, RenameFlags, CWD};
            renameat_with(CWD, &legacy, CWD, &current, RenameFlags::NOREPLACE)?;
        }
        #[cfg(not(unix))] { return Err(io::Error::new(io::ErrorKind::Unsupported, "automatic brand migration requires Unix")); }
        sync_directory(current.parent().ok_or_else(|| conflict("missing home directory"))?)?;
    }
    // Resume safely if a crash occurred after the rename but before alias creation.
    if metadata(&legacy)?.is_none() && current.join(MARKER).exists() {
        if fs::read(current.join(MARKER))? != MARKER_CONTENT { return Err(conflict("brand migration marker is invalid")); }
        #[cfg(unix)] std::os::unix::fs::symlink(&current, &legacy)?;
        #[cfg(not(unix))] return Err(io::Error::new(io::ErrorKind::Unsupported, "legacy alias requires Unix"));
        sync_directory(current.parent().ok_or_else(|| conflict("missing home directory"))?)?;
        return Ok(true);
    }
    Ok(false)
}

/// Normalize only the verified former private root; arbitrary user paths stay intact.
pub fn translate_legacy_path(path: PathBuf) -> PathBuf {
    if override_directory().is_some() { return path; }
    let (legacy, current) = roots();
    if let Ok(relative) = path.strip_prefix(&legacy) {
        if current.join(MARKER).is_file()
            && fs::canonicalize(&legacy).ok() == fs::canonicalize(&current).ok()
            && current.is_dir() {
            return current.join(relative);
        }
    }
    path
}

/// Transitional environment aliases. New processes and docs emit ARCA_* only.
pub fn env_os(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).or_else(|| {
        name.strip_prefix("ARCA_").and_then(|suffix| std::env::var_os(format!("MUX_{suffix}")))
    })
}

pub fn env(name: &str) -> Result<String, std::env::VarError> {
    env_os(name).ok_or(std::env::VarError::NotPresent)?
        .into_string().map_err(std::env::VarError::NotUnicode)
}

/// A legacy CLI link is owned only if the enclosing app's public metadata agrees.
pub fn is_former_bundled_cli(path: &Path) -> bool {
    if !path.is_absolute() || path.file_name().is_none_or(|name| name != "mux") { return false; }
    let Some(macos) = path.parent().filter(|parent| parent.file_name().is_some_and(|name| name == "MacOS")) else { return false; };
    let Some(contents) = macos.parent().filter(|parent| parent.file_name().is_some_and(|name| name == "Contents")) else { return false; };
    let Ok(value) = plist::Value::from_file(contents.join("Info.plist")) else { return false; };
    matches!(value.as_dictionary().and_then(|value| value.get("CFBundleIdentifier")).and_then(plist::Value::as_string),
        Some("com.scoheart.mux") | Some("com.scoheart.arca"))
}
