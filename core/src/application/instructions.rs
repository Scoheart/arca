//! Read and edit native user-level Agent documents. No central asset copy,
//! projection, deletion, implicit initialization, or execution of document text.
use crate::domain::error::{CoreError, CoreResult};
pub use crate::domain::instructions::*;
use crate::safe_write::{self, AnchoredPathState, ParentDirectorySnapshot};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fs, path::{Component, Path, PathBuf}, sync::{LazyLock, Mutex}, time::{Duration, Instant}};
use zeroize::Zeroizing;

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_FILES: usize = 256;
const MAX_DEPTH: usize = 8;
const MAX_PLANS: usize = 16;

#[derive(Deserialize)]
struct Registry { profiles: Vec<Profile> }
#[derive(Deserialize)]
struct Profile {
    agent_ids: Vec<String>, sources: Vec<Source>, create: Option<Create>, reload: String,
    native_notes: BTreeMap<String, String>, docs: Vec<String>, max_chars: Option<usize>,
    #[serde(default)] fallback_sources: Vec<usize>,
    #[serde(default)] blocked_env: Vec<String>,
}
#[derive(Deserialize)]
struct Source {
    id: String, root: String, env: Vec<String>, files: Vec<String>, selection: String, editable: bool,
    directory: Option<String>, #[serde(default)] recursive: bool,
    #[serde(default)] extensions: Vec<String>, filename_config: Option<FilenameConfig>,
}
#[derive(Deserialize)]
struct FilenameConfig { path: String, pointer: String }
#[derive(Deserialize)]
struct Create { source: usize, name: String, text: String }
static REGISTRY: LazyLock<Registry> = LazyLock::new(|| serde_json::from_str(include_str!("../../../data/instruction-files.json")).expect("audited native instruction registry"));
struct Located { path: PathBuf, file: InstructionFile }
struct Discovery { view: InstructionInventory, located: Vec<Located>, create_path: Option<PathBuf> }
struct Pending {
    public: InstructionEditPlan, agent: String, file_id: Option<String>, expected: String,
    home: PathBuf, arca_home: PathBuf, text: Zeroizing<String>, expires: Instant,
}
static PLANS: LazyLock<Mutex<BTreeMap<String, Pending>>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));

fn err(code: &str, message: &str) -> CoreError { CoreError::new(code, message) }
fn hash(bytes: impl AsRef<[u8]>) -> String { hex::encode(Sha256::digest(bytes.as_ref())) }
fn stale() -> CoreError { err("instruction_conflict", "The native file changed. Your draft is preserved; reload or compare it before saving.") }
fn home() -> CoreResult<PathBuf> {
    dirs::home_dir().and_then(|path| fs::canonicalize(path).ok()).ok_or_else(|| err("instruction_home_unavailable", "Cannot resolve the local user home directory."))
}
fn profile(id: &str) -> Option<&'static Profile> { REGISTRY.profiles.iter().find(|p| p.agent_ids.iter().any(|i| i == id)) }
pub fn supported(id: &str) -> bool { profile(id).is_some_and(|p| !p.sources.is_empty()) }
fn absolute(value: &str, home: &Path) -> CoreResult<PathBuf> {
    let path = if value == "~" { home.to_path_buf() } else if let Some(tail) = value.strip_prefix("~/") { home.join(tail) } else { PathBuf::from(value) };
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err(err("instruction_context_unresolved", "The native instruction root is not an unambiguous absolute directory."));
    }
    #[cfg(debug_assertions)]
    if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() && !path.starts_with(home) {
        return Err(err("instruction_test_isolation", "Native file fixtures cannot escape their isolated home."));
    }
    Ok(path)
}
fn root(source: &Source, home: &Path) -> CoreResult<PathBuf> {
    if (source.root == "~/.claude" && std::env::var_os("CLAUDE_CONFIG_DIR").is_some())
        || (source.root == "~/.stepcode/agent" && std::env::var_os("STEPCODE_CONFIG_DIR").is_some()) {
        return Err(err("instruction_context_unresolved", "This custom native root needs a verified local resolver."));
    }
    if source.root == "~/.config/crush" && std::env::var_os("CRUSH_GLOBAL_CONFIG").is_some() {
        return Err(err("instruction_context_unresolved", "Custom Crush global configuration needs native path verification."));
    }
    for name in &source.env {
        if let Some(value) = std::env::var_os(name).filter(|v| !v.is_empty()) {
            return absolute(value.to_str().ok_or_else(|| err("instruction_context_unresolved", "The native root is not valid text."))?, home);
        }
    }
    if source.root == "@goose" {
        if let Ok(value) = std::env::var("GOOSE_PATH_ROOT") { return Ok(absolute(&value, home)?.join("config")); }
        return Ok(if cfg!(target_os = "macos") { home.join("Library/Application Support/Block/goose/config") } else { home.join(".config/goose") });
    }
    if source.root == "@goose-agents" {
        return match std::env::var("GOOSE_PATH_ROOT") { Ok(value) => Ok(absolute(&value, home)?.join(".agents")), Err(_) => Ok(home.join(".agents")) };
    }
    if source.root == "@vtcode" { return Ok(if cfg!(target_os = "macos") { home.join("Library/Application Support/com.vinhnx.vtcode") } else { home.join(".config/vtcode") }); }
    if source.root == "~/.config/mimocode" {
        if let Ok(value) = std::env::var("MIMOCODE_HOME") { return Ok(absolute(&value, home)?.join("config")); }
    }
    if let Some(tail) = source.root.strip_prefix("~/.config/") {
        if let Ok(value) = std::env::var("XDG_CONFIG_HOME") { return Ok(absolute(&value, home)?.join(tail)); }
    }
    absolute(&source.root, home)
}
fn overrides<'a>(settings: &'a crate::settings::Settings, agent_id: &str) -> Option<&'a BTreeMap<String, String>> {
    settings.agent_config_paths.as_ref()?.get(agent_id)?.instruction_paths.as_ref()
}
fn default_location(source: &Source, home: &Path) -> CoreResult<PathBuf> {
    let base = root(source, home)?;
    Ok(match source.directory.as_deref() {
        Some(directory) if directory != "@mode-rules" => base.join(directory),
        _ => base,
    })
}
fn location(source: &Source, home: &Path, paths: Option<&BTreeMap<String, String>>) -> CoreResult<PathBuf> {
    match paths.and_then(|paths| paths.get(&source.id)) {
        Some(path) => absolute(path, home),
        None => default_location(source, home),
    }
}
fn validate_directory(path: &Path) -> CoreResult<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => return Err(err("instruction_paths_invalid", "Instructions directories must use real directory paths, without file or symlink components.")),
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err(err("instruction_paths_invalid", "Cannot inspect this Instructions directory. Existing files were not changed.")),
        }
    }
    Ok(())
}
fn checked_location(source: &Source, home: &Path, paths: Option<&BTreeMap<String, String>>) -> CoreResult<PathBuf> {
    let path = location(source, home, paths)?;
    validate_directory(&path)?;
    Ok(path)
}
fn display_path(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".into(),
        Ok(relative) => format!("~/{}", relative.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}
fn path_configuration(agent_id: &str, settings: &crate::settings::Settings) -> CoreResult<InstructionPathConfiguration> {
    let home = home()?;
    let stored = overrides(settings, agent_id);
    let mut sources = vec![];
    if let Some(profile) = profile(agent_id) {
        for source in &profile.sources {
            let default_path = default_location(source, &home).ok().map(|path| display_path(&path, &home));
            let path = location(source, &home, stored).ok().map(|path| display_path(&path, &home)).unwrap_or_default();
            let kind = if source.directory.as_deref() == Some("@mode-rules") { "mode-rules" } else if source.directory.is_some() { "rules" } else { "files" };
            let patterns = if source.directory.is_some() { source.extensions.iter().map(|ext| format!("*.{ext}")).collect() } else { native_filenames(source, &home).unwrap_or_else(|_| source.files.clone()) };
            sources.push(InstructionPathSource { id: source.id.clone(), kind: kind.into(), path, default_path, overridden: stored.is_some_and(|p| p.contains_key(&source.id)), patterns });
        }
    }
    let revision = hash(serde_json::to_vec(&(agent_id, &home, &sources, stored)).map_err(|_| err("instruction_revision_failed", "Cannot inspect instruction directory preferences."))?);
    Ok(InstructionPathConfiguration { agent_id: agent_id.into(), revision, sources })
}
pub fn get_paths(agent_id: &str) -> CoreResult<InstructionPathConfiguration> {
    super::gate::query(|| {
        let settings = crate::settings::load_settings_strict().map_err(|_| err("instruction_settings_unreadable", "Cannot read Arca directory preferences. Existing files were not changed."))?;
        path_configuration(agent_id, &settings)
    })
}
/// Save editor locations only. This does not initialize, move, copy or delete
/// native files, or alter the Agent's own runtime configuration.
pub fn set_paths(agent_id: &str, expected_revision: &str, paths: Vec<InstructionPathUpdate>) -> CoreResult<InstructionPathConfiguration> {
    super::gate::mutate_independent_core("instruction_paths", || {
        let p = profile(agent_id).filter(|p| !p.sources.is_empty()).ok_or_else(|| err("instruction_unsupported", "This Agent has no verified native instruction directories."))?;
        if paths.is_empty() || paths.len() > p.sources.len() { return Err(err("instruction_paths_invalid", "Select a known Instructions directory to update.")); }
        let home = home()?;
        let mut updates = BTreeMap::new();
        for update in paths {
            if !p.sources.iter().any(|s| s.id == update.id) || updates.contains_key(&update.id) { return Err(err("instruction_paths_invalid", "Instructions source IDs must be known and unique.")); }
            let path = update.path.map(|path| {
                let value = path.trim();
                if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) { return Err(err("instruction_paths_invalid", "Enter an absolute directory or a ~/ path.")); }
                let path = absolute(value, &home)?;
                // Absent directories are valid and remain absent after Save.
                validate_directory(&path)?;
                state(&path.join(".arca-instructions-directory-check"))?;
                Ok(path.to_string_lossy().into_owned())
            }).transpose()?;
            updates.insert(update.id, path);
        }
        crate::settings::mutate_settings_checked(|settings| {
            let current = path_configuration(agent_id, settings).map_err(|e| std::io::Error::other(e.to_string()))?;
            if current.revision != expected_revision { return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "instruction_paths_conflict")); }
            let all = settings.agent_config_paths.get_or_insert_default();
            let preference = all.entry(agent_id.into()).or_default();
            let stored = preference.instruction_paths.get_or_insert_default();
            for (id, path) in updates { if let Some(path) = path { stored.insert(id, path); } else { stored.remove(&id); } }
            if stored.is_empty() { preference.instruction_paths = None; }
            if preference == &crate::settings::AgentConfigPathOverride::default() { all.remove(agent_id); }
            if all.is_empty() { settings.agent_config_paths = None; }
            path_configuration(agent_id, settings).map_err(|e| std::io::Error::other(e.to_string()))
        }).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock { err("instruction_paths_conflict", "Instructions directories changed in another window. Reopen configuration before saving; your files were not changed.") }
            else { err("instruction_paths_save_failed", "Cannot safely save Instructions directory preferences. Native files were not changed.") }
        })
    })
}
fn safe_relative(value: &str) -> bool {
    !value.is_empty() && Path::new(value).components().all(|c| matches!(c, Component::Normal(_)))
}
fn state(path: &Path) -> CoreResult<(ParentDirectorySnapshot, AnchoredPathState)> {
    let parent = safe_write::capture_parent_directory(path).map_err(|_| err("instruction_file_unsafe", "Cannot safely inspect the native file or its parent directory."))?;
    let value = safe_write::read_path_state_anchored_limited(path, &parent, MAX_BYTES)
        .map_err(|_| err("instruction_file_unreadable", "Cannot read this native file safely within the 1 MiB editor limit."))?;
    Ok((parent, value))
}
fn revision(parent: &ParentDirectorySnapshot, state: &AnchoredPathState) -> CoreResult<String> {
    safe_write::fingerprint_anchored_path_state(state, parent).map_err(|_| err("instruction_revision_failed", "Cannot verify the native file revision."))
}
fn filenames(source: &Source, root: &Path) -> CoreResult<Vec<String>> {
    let Some(config) = &source.filename_config else { return Ok(source.files.clone()); };
    let (_, value) = state(&root.join(&config.path))?;
    let AnchoredPathState::File { bytes, .. } = value else {
        return if matches!(value, AnchoredPathState::Missing) { Ok(source.files.clone()) } else { Err(err("instruction_context_unresolved", "The native filename configuration is not a regular file.")) };
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| err("instruction_context_unresolved", "Cannot parse the native context filename configuration; no file will be initialized."))?;
    let Some(names) = value.pointer(&config.pointer) else { return Ok(source.files.clone()); };
    let names = match names {
        serde_json::Value::String(name) => vec![name.clone()],
        serde_json::Value::Array(names) => names.iter().map(|name| name.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>()
            .ok_or_else(|| err("instruction_context_unresolved", "The native context filename list is invalid."))?,
        _ => return Err(err("instruction_context_unresolved", "The native context filename is invalid.")),
    };
    if names.is_empty() || names.iter().any(|n| !safe_relative(n) || n.contains('/') || n.contains('\\')) {
        return Err(err("instruction_context_unresolved", "Custom context paths require native setup; Arca will not guess a file location."));
    }
    Ok(names)
}
fn native_filenames(source: &Source, home: &Path) -> CoreResult<Vec<String>> {
    if source.filename_config.is_none() { return Ok(source.files.clone()); }
    // A directory preference moves the editor's lookup, not the Agent's
    // settings.json. Native context.fileName still owns the filenames.
    filenames(source, &root(source, home)?)
}
fn reader_matches(source: &Source, base: &Path, path: &Path, home: &Path) -> bool {
    if !source.files.is_empty() && native_filenames(source, home).is_ok_and(|names| names.iter().any(|name| base.join(name) == path)) { return true; }
    let Some(directory) = &source.directory else { return false; };
    let Ok(relative) = path.strip_prefix(base) else { return false; };
    if directory == "@mode-rules" && !relative.components().next().is_some_and(|c| c.as_os_str().to_string_lossy().starts_with("rules-")) { return false; }
    (source.recursive || relative.components().count() == 1)
        && source.extensions.iter().any(|ext| path.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&format!(".{ext}"))))
}
fn readers(path: &Path, home: &Path, settings: &crate::settings::Settings) -> Vec<InstructionReader> {
    let definitions = crate::agents::load_agents();
    REGISTRY.profiles.iter().flat_map(|p| p.agent_ids.iter().filter(move |id| p.sources.iter().any(|s| checked_location(s, home, overrides(settings, id)).is_ok_and(|base| reader_matches(s, &base, path, home)))))
        .filter_map(|id| definitions.get(id).map(|d| InstructionReader { id: id.clone(), name: d.name.clone().unwrap_or_else(|| id.clone()) })).collect()
}
fn inspect(path: PathBuf, editable: bool, loading: &str, home: &Path, settings: &crate::settings::Settings, files: &mut Vec<Located>, issues: &mut BTreeSet<String>, seen: &mut BTreeSet<String>) {
    if files.len() >= MAX_FILES { issues.insert("scan-limit".into()); return; }
    let metadata = match fs::symlink_metadata(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if state(&path).is_err() { issues.insert("unsafe-or-unreadable-path".into()); } return;
        }
        Err(_) => { issues.insert("unreadable-file".into()); return; }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() { issues.insert("unsafe-file-type".into()); return; }
    let readable = metadata.len() <= MAX_BYTES;
    let key = {
        #[cfg(unix)] { use std::os::unix::fs::MetadataExt; format!("{}:{}", metadata.dev(), metadata.ino()) }
        #[cfg(not(unix))] { path.to_string_lossy().into_owned() }
    };
    if !seen.insert(key) { return; }
    if readable && state(&path).is_err() { issues.insert("unreadable-file".into()); return; }
    if !readable { issues.insert("editor-size-limit".into()); }
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let purpose = match name.as_str() { "SYSTEM.md" => "system-prompt", "APPEND_SYSTEM.md" => "append-system", "SOUL.md" => "identity", _ => "instructions" }.into();
    files.push(Located { file: InstructionFile { id: hash(path.as_os_str().as_encoded_bytes()), path: path.to_string_lossy().into_owned(), name, purpose, loading: loading.into(), bytes: metadata.len(), editable: editable && readable && cfg!(unix), readers: readers(&path, home, settings) }, path });
}
fn scan(directory: &Path, source: &Source, home: &Path, settings: &crate::settings::Settings, depth: usize, files: &mut Vec<Located>, issues: &mut BTreeSet<String>, seen: &mut BTreeSet<String>, visited: &mut usize) {
    if *visited >= 2048 || depth > MAX_DEPTH || files.len() >= MAX_FILES { issues.insert("scan-limit".into()); return; }
    if state(&directory.join(".arca-read-probe")).is_err() { issues.insert("unsafe-or-unreadable-directory".into()); return; }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => { issues.insert("unreadable-directory".into()); return; }
    };
    for entry in entries {
        *visited += 1;
        if *visited >= 2048 || files.len() >= MAX_FILES { issues.insert("scan-limit".into()); break; }
        let Ok(entry) = entry else { issues.insert("unreadable-directory".into()); continue; };
        let Ok(kind) = entry.file_type() else { issues.insert("unreadable-file".into()); continue; };
        if kind.is_dir() && source.recursive { scan(&entry.path(), source, home, settings, depth + 1, files, issues, seen, visited); }
        else if kind.is_symlink() { issues.insert("symlink-not-followed".into()); }
        else if kind.is_file() && source.extensions.iter().any(|ext| entry.file_name().to_string_lossy().ends_with(&format!(".{ext}"))) {
            inspect(entry.path(), source.editable, "native-rule", home, settings, files, issues, seen);
        }
    }
}
fn discover(agent_id: &str) -> CoreResult<Discovery> {
    let home = home()?;
    let mut view = InstructionInventory { agent_id: agent_id.into(), supported: supported(agent_id), revision: String::new(), files: vec![], create: None, complete: true, issues: vec![], reload: "unknown".into(), native_notes: String::new(), docs: vec![] };
    let Some(p) = profile(agent_id) else { view.revision = hash(agent_id); return Ok(Discovery { view, located: vec![], create_path: None }); };
    view.reload.clone_from(&p.reload); view.docs.clone_from(&p.docs); view.native_notes = p.native_notes.get(agent_id).cloned().unwrap_or_default();
    let blocked_environment = p.blocked_env.iter().any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()));
    // A configured launcher may use a profile/root that differs from Arca's
    // environment. Do not initialize a default file for that different runtime.
    let launch_context = crate::settings::load_settings_strict();
    let context_readable = launch_context.is_ok();
    let settings = launch_context.unwrap_or_default();
    let configured_paths = overrides(&settings, agent_id);
    let mut custom_launch = settings.agent_launch.as_ref().and_then(|all| all.get(agent_id))
        .is_some_and(|preferences| !preferences.has_valid_profile_selection());
    {
        if let Some(target) = settings.agent_launch.as_ref().and_then(|all| all.get(agent_id)).and_then(|p| p.configured_target()) {
            use crate::domain::agents::LaunchTarget;
            let custom = match target {
                LaunchTarget::Cli { args, env, .. } | LaunchTarget::App { args, env, .. } => {
                    args.iter().any(|a| a.starts_with("--profile") || a.starts_with("--config") || a.starts_with("--data-dir") || a.starts_with("--agent-dir") || a.starts_with("--remote") || a.starts_with("--context"))
                        || p.sources.iter().flat_map(|s| &s.env).any(|name| env.get(name).is_some_and(|v| std::env::var(name).as_ref().ok() != Some(v)))
                        || ["XDG_CONFIG_HOME", "CLAUDE_CONFIG_DIR", "STEPCODE_CONFIG_DIR", "GOOSE_PATH_ROOT", "MIMOCODE_HOME"].iter().any(|name| env.get(*name).is_some_and(|v| std::env::var(name).as_ref().ok() != Some(v)))
                        || env.get("HOME").is_some_and(|v| Path::new(v) != home)
                }
                LaunchTarget::Web { .. } => true,
            };
            custom_launch |= custom;
        }
    }
    let mut located = vec![]; let mut issues = BTreeSet::new(); let mut seen = BTreeSet::new(); let mut context = vec![]; let mut roots = vec![]; let mut visited = 0;
    if !context_readable { issues.insert("native-launch-context-unreadable".into()); }
    if blocked_environment { issues.insert("native-custom-sources-unresolved".into()); }
    for (source_index, source) in p.sources.iter().enumerate() {
        let has_primary = !located.is_empty();
        // An explicit directory resolves that editor source even when the
        // launcher uses another root. Unconfigured sources remain unresolved.
        if (custom_launch || blocked_environment) && !configured_paths.is_some_and(|paths| paths.contains_key(&source.id)) {
            issues.insert("native-launch-context-unresolved".into()); roots.push(None); continue;
        }
        let base = match checked_location(source, &home, configured_paths) { Ok(base) => base, Err(_) => { issues.insert("native-root-unresolved".into()); roots.push(None); continue; } };
        context.push(base.to_string_lossy().into_owned()); roots.push(Some(base.clone()));
        let names = match native_filenames(source, &home) { Ok(names) => names, Err(_) => { issues.insert("native-filename-unresolved".into()); source.files.clone() } };
        let mut winner = false;
        for name in names {
            if !safe_relative(&name) { issues.insert("native-filename-unresolved".into()); continue; }
            let path = base.join(name);
            let nonempty = state(&path).ok().is_some_and(|(_, s)| match s { AnchoredPathState::File { bytes, .. } => source.selection == "first-existing" || !String::from_utf8_lossy(&bytes).trim().is_empty(), _ => false });
            let loading = if source.selection == "additive" { "native-file" } else if winner { "fallback" } else if nonempty { "preferred" } else { "empty" };
            let loading = if p.fallback_sources.contains(&source_index) { if has_primary { "shadowed" } else { "compatibility-fallback" } } else { loading };
            inspect(path, source.editable, loading, &home, &settings, &mut located, &mut issues, &mut seen);
            winner |= nonempty;
        }
        if let Some(directory) = &source.directory {
            if directory == "@mode-rules" {
                match fs::read_dir(&base) {
                    Ok(entries) => for entry in entries {
                        visited += 1;
                        if visited >= 2048 { issues.insert("scan-limit".into()); break; }
                        match entry {
                            Ok(entry) if entry.file_name().to_string_lossy().starts_with("rules-") => scan(&entry.path(), source, &home, &settings, 0, &mut located, &mut issues, &mut seen, &mut visited),
                            Ok(_) => {},
                            Err(_) => { issues.insert("unreadable-directory".into()); }
                        }
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                    Err(_) => { issues.insert("unreadable-directory".into()); }
                }
            } else { scan(&base, source, &home, &settings, 0, &mut located, &mut issues, &mut seen, &mut visited); }
        }
    }
    let create_path = p.create.as_ref().and_then(|create| roots.get(create.source).and_then(Option::as_ref).and_then(|root| {
        if create.name == "@context-file" { native_filenames(&p.sources[create.source], &home).ok()?.first().map(|name| root.join(name)) }
        else {
            let source = &p.sources[create.source];
            let name = match source.directory.as_deref() {
                Some(directory) if directory != "@mode-rules" => Path::new(&create.name).strip_prefix(directory).ok()?,
                _ => Path::new(&create.name),
            };
            Some(root.join(name))
        }
    }));
    if !context_readable { for located in &mut located { located.file.editable = false; } }
    // Keep native fixed-file priority, with a stable order for directory rows.
    located.sort_by(|a, b| match (a.file.loading == "native-rule", b.file.loading == "native-rule") {
        (true, true) => a.path.cmp(&b.path), (a, b) => a.cmp(&b),
    });
    view.complete = issues.is_empty(); view.issues = issues.into_iter().collect(); view.files = located.iter().map(|l| l.file.clone()).collect();
    view.revision = hash(serde_json::to_vec(&(&home, context, &view.files, &view.issues, &create_path)).map_err(|_| err("instruction_revision_failed", "Cannot inspect instruction files."))?);
    if view.supported && view.complete && view.files.is_empty() && cfg!(unix) {
        if let (Some(path), Some(create)) = (&create_path, &p.create) {
            view.create = Some(InstructionCreateTarget { path: path.to_string_lossy().into_owned(), initial_text: create.text.clone() });
        }
    }
    Ok(Discovery { view, located, create_path })
}
pub fn list(agent_id: &str) -> CoreResult<InstructionInventory> { super::gate::query(|| discover(agent_id).map(|d| d.view)) }
fn read_inner(agent_id: &str, file_id: &str) -> CoreResult<InstructionSnapshot> {
    let d = discover(agent_id)?;
    let file = d.located.into_iter().find(|l| l.file.id == file_id).ok_or_else(stale)?;
    let (parent, s) = state(&file.path)?;
    let AnchoredPathState::File { bytes, .. } = &s else { return Err(stale()); };
    let text = std::str::from_utf8(bytes).map_err(|_| err("instruction_encoding_unsupported", "This file is not UTF-8. Use its native editor to preserve the original encoding."))?.to_owned();
    Ok(InstructionSnapshot { file: file.file, text, revision: revision(&parent, &s)?, reload: d.view.reload })
}
pub fn read(agent_id: &str, file_id: &str) -> CoreResult<InstructionSnapshot> { super::gate::query(|| read_inner(agent_id, file_id)) }

/// Textareas normalize line breaks. Keep original endings on unchanged lines,
/// retain the original BOM, and use the document's first ending on edited lines.
pub(super) fn preserve_text(original: &str, edited: &str) -> String {
    let original_bom = original.starts_with('\u{feff}');
    let old = original.trim_start_matches('\u{feff}'); let new = edited.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let normalized = old.replace("\r\n", "\n");
    if normalized == new { return original.into(); }
    let endings: Vec<_> = old.split_inclusive('\n').map(|l| if l.ends_with("\r\n") { "\r\n" } else if l.ends_with('\n') { "\n" } else { "" }).collect();
    let default = endings.iter().find(|e| !e.is_empty()).copied().unwrap_or("\n");
    let diff = similar::TextDiff::configure().deadline(Instant::now() + Duration::from_millis(100)).diff_lines(&normalized, &new);
    let mut result = if original_bom { "\u{feff}".into() } else { String::new() };
    for change in diff.iter_all_changes() {
        if change.tag() == similar::ChangeTag::Delete { continue; }
        let value = change.value(); result.push_str(value.strip_suffix('\n').unwrap_or(value));
        if value.ends_with('\n') { result.push_str(change.old_index().and_then(|i| endings.get(i)).copied().filter(|e| !e.is_empty()).unwrap_or(default)); }
    }
    result
}
fn validate_text(p: &Profile, path: &Path, before: Option<&str>, text: &str) -> CoreResult<()> {
    if text.len() as u64 > MAX_BYTES || text.contains('\0') { return Err(err("instruction_editor_limit", "The editor accepts UTF-8 text up to 1 MiB without NUL bytes.")); }
    if let Some(limit) = p.max_chars {
        let count = text.encode_utf16().count();
        if count > limit && before.is_none_or(|b| count >= b.encode_utf16().count()) {
            return Err(err("instruction_native_limit", "This edit exceeds the native instruction limit. No content was truncated."));
        }
    }
    if path.extension().is_some_and(|e| e == "mdc") && text.trim_start_matches('\u{feff}').starts_with("---\n") {
        let text = text.trim_start_matches('\u{feff}');
        let end = text[4..].find("\n---").ok_or_else(|| err("instruction_frontmatter_invalid", "Close the native YAML frontmatter before saving."))?;
        serde_yaml::from_str::<serde_yaml::Value>(&text[4..4 + end]).map_err(|_| err("instruction_frontmatter_invalid", "The native YAML frontmatter is invalid; its source was preserved."))?;
    }
    Ok(())
}
fn prepare(agent_id: &str, file_id: Option<&str>, expected: &str, text: &str) -> CoreResult<InstructionEditPlan> {
    if text.len() as u64 > MAX_BYTES { return Err(err("instruction_editor_limit", "The native file editor accepts at most 1 MiB of UTF-8 text.")); }
    let d = discover(agent_id)?; let p = profile(agent_id).ok_or_else(|| err("instruction_unsupported", "This Agent has no verified native instruction file contract."))?;
    let (path, content) = if let Some(id) = file_id {
        let snap = read_inner(agent_id, id)?;
        if !snap.file.editable { return Err(err("instruction_read_only", "This native file is available for inspection only.")); }
        if snap.revision != expected { return Err(stale()); }
        let text = preserve_text(&snap.text, text); let path = PathBuf::from(&snap.file.path);
        validate_text(p, &path, Some(&snap.text), &text)?; (path, text)
    } else {
        if d.view.revision != expected || d.view.create.is_none() { return Err(stale()); }
        let path = d.create_path.ok_or_else(stale)?;
        validate_text(p, &path, None, text)?; (path, text.to_owned())
    };
    let operation_id = uuid::Uuid::new_v4().to_string();
    let public = InstructionEditPlan { candidate_hash: hash(content.as_bytes()), operation_id: operation_id.clone(), path: path.to_string_lossy().into_owned(), creating: file_id.is_none() };
    let pending = Pending { public: public.clone(), agent: agent_id.into(), file_id: file_id.map(str::to_owned), expected: expected.into(), home: home()?, arca_home: crate::paths::arca_dir(), text: Zeroizing::new(content), expires: Instant::now() + Duration::from_secs(600) };
    let mut plans = PLANS.lock().map_err(|_| err("instruction_busy", "The native file editor is busy."))?;
    plans.retain(|_, p| p.expires > Instant::now());
    if plans.len() >= MAX_PLANS { return Err(err("instruction_busy", "Too many pending file edits. Retry after the current save finishes.")); }
    plans.insert(operation_id, pending); Ok(public)
}
pub fn prepare_save(agent_id: &str, file_id: &str, expected_revision: &str, text: &str) -> CoreResult<InstructionEditPlan> {
    super::gate::query(|| prepare(agent_id, Some(file_id), expected_revision, text))
}
pub fn prepare_create(agent_id: &str, inventory_revision: &str, text: &str) -> CoreResult<InstructionEditPlan> {
    super::gate::query(|| prepare(agent_id, None, inventory_revision, text))
}
pub fn commit(operation_id: &str, candidate_hash: &str) -> CoreResult<InstructionSnapshot> {
    super::gate::mutate_independent_core("instruction_file_edit", || {
        let plan = PLANS.lock().map_err(|_| err("instruction_busy", "The native file editor is busy."))?.remove(operation_id)
            .ok_or_else(|| err("instruction_plan_expired", "This file edit plan expired. Save again to prepare a fresh plan."))?;
        if plan.expires <= Instant::now() || plan.public.candidate_hash != candidate_hash || plan.home != home()? || plan.arca_home != crate::paths::arca_dir() { return Err(stale()); }
        let d = discover(&plan.agent)?; let path = PathBuf::from(&plan.public.path);
        if let Some(id) = &plan.file_id {
            let current = read_inner(&plan.agent, id)?;
            if current.revision != plan.expected || current.file.path != plan.public.path { return Err(stale()); }
        } else if d.view.create.is_none() || d.view.revision != plan.expected || d.create_path.as_ref() != Some(&path) { return Err(stale()); }
        let (parent, current) = state(&path)?;
        if plan.file_id.is_some() && revision(&parent, &current)? != plan.expected { return Err(stale()); }
        let scope = crate::paths::arca_dir().join("instruction-edits").join(hash(path.as_os_str().as_encoded_bytes())).join("claims");
        let result = safe_write::with_file_edit_claims(&scope, || {
            match &current {
                AnchoredPathState::File { bytes, mode, identity } if plan.file_id.is_some() => {
                    if bytes.as_slice() == plan.text.as_bytes() { return Ok(()); }
                    // Keep one private Keychain rollback snapshot per document;
                    // neither the claim journal nor a plan persists prompt text.
                    crate::resources::model::persist_private_file_snapshot(&format!("instruction-{}", hash(path.as_os_str().as_encoded_bytes())), &path, bytes)?;
                    safe_write::write_bytes_if_hash_unchanged_in_parent(&path, &parent, Some((&hash(bytes), *mode, *identity)), plan.text.as_bytes(), *mode)
                }
                AnchoredPathState::Missing if plan.file_id.is_none() => safe_write::write_bytes_if_hash_unchanged_in_parent(&path, &parent, None, plan.text.as_bytes(), Some(0o600)),
                _ => Err("native file changed".into()),
            }
        });
        result.map_err(|error| {
            if error.contains("recovery_required") { err("instruction_file_recovery_required", "This file has an interrupted edit requiring recovery. Other Agent resources remain available; your private backup was retained.") }
            else if error.contains("changed") { stale() }
            else { err("instruction_save_failed", "Cannot safely back up or save this native file. Your draft was preserved.") }
        })?;
        read_inner(&plan.agent, &hash(path.as_os_str().as_encoded_bytes()))
    })
}
