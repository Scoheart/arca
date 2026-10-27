//! Audited host adapters expose metadata, never executable configuration.
use crate::domain::plugin::*;
use crate::domain::types::AgentDefinition;
use crate::resources::mcp::scanner::{collapse_home as collapse_home_string, expand_tilde};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

const MAX_ENTRIES: usize = 4096;
const MAX_COMPONENTS: usize = 4096;
const MAX_SCAN_ENTRIES: usize = 32_768;
const MAX_REFERENCES: usize = 2048;
const MAX_READ_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Host {
    agent_id: String,
    docs_url: String,
    manifest_paths: Vec<String>,
    default_mcp_files: Vec<String>,
    mcp_id_prefix: String,
    roots: Vec<Root>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Root {
    path: String,
    layout: String,
    presence: PluginPresence,
    complete_marker: Option<String>,
}

fn hosts() -> &'static [Host] {
    static HOSTS: OnceLock<Vec<Host>> = OnceLock::new();
    HOSTS.get_or_init(|| serde_json::from_str(include_str!("../../../data/native-plugin-hosts.json"))
        .expect("audited native plugin descriptors must be valid"))
}

#[derive(Default)]
struct ScanBudget {
    visited: usize,
    references: usize,
    bytes: usize,
    json: BTreeMap<PathBuf, Arc<Value>>,
}
impl ScanBudget {
    fn visit(&mut self) -> Result<(), ()> {
        self.visited += 1;
        if self.visited > MAX_SCAN_ENTRIES { Err(()) } else { Ok(()) }
    }
}

#[derive(Default)]
pub(crate) struct PluginScan {
    pub plugins: Vec<NativePluginObservation>,
    pub mcps: Vec<HostMcpObservation>,
    pub unavailable: bool,
    budget: ScanBudget,
}

pub(crate) fn text(path: &Path, limit: u64) -> Result<String, ()> {
    if !fs::symlink_metadata(path).map_err(|_| ())?.file_type().is_file() { return Err(()); }
    let file = open_confined_file(path)?;
    if !file.metadata().map_err(|_| ())?.is_file() { return Err(()); }
    let mut content = String::new();
    file.take(limit + 1).read_to_string(&mut content).map_err(|_| ())?;
    if content.len() as u64 > limit { return Err(()); }
    Ok(content)
}


#[cfg(unix)]
fn open_confined(path: &Path, directory_only: bool) -> Result<fs::File, ()> {
    use rustix::fs::{open, openat, Mode, OFlags};
    let home = expand_tilde("~/");
    let (base, tail) = if let Ok(tail) = path.strip_prefix(&home) {
        (home.as_path(), tail)
    } else { (Path::new("/"), path.strip_prefix("/").map_err(|_| ())?) };
    let parts = tail.components().filter(|part| *part != Component::CurDir).map(|part| match part {
        Component::Normal(name) => Ok(name), _ => Err(()),
    }).collect::<Result<Vec<_>, _>>()?;
    let (last, parents) = parts.split_last().ok_or(())?;
    let mut directory = open(base, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty()).map_err(|_| ())?;
    for parent in parents {
        directory = openat(&directory, *parent, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).map_err(|_| ())?;
    }
    let flags = OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let file = openat(&directory, *last, if directory_only { flags | OFlags::DIRECTORY } else { flags }, Mode::empty()).map_err(|_| ())?;
    Ok(fs::File::from(file))
}
#[cfg(not(unix))]
fn open_confined(path: &Path, directory_only: bool) -> Result<fs::File, ()> {
    checked_root(path)?; let file = fs::File::open(path).map_err(|_| ())?;
    if directory_only && !file.metadata().map_err(|_| ())?.is_dir() { return Err(()); }
    Ok(file)
}

pub(crate) fn open_confined_file(path: &Path) -> Result<fs::File, ()> { open_confined(path, false) }

pub(crate) fn checked_root(path: &Path) -> Result<(), ()> {
    let home = expand_tilde("~/");
    let (mut current, tail) = if let Ok(tail) = path.strip_prefix(&home) {
        (home.clone(), tail)
    } else {
        (PathBuf::from("/"), path.strip_prefix("/").map_err(|_| ())?)
    };
    for part in tail.components() {
        if matches!(part, Component::ParentDir | Component::Prefix(_)) { return Err(()); }
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(()),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(()),
            _ => {},
        }
    }
    Ok(())
}

fn relative(root: &Path, input: &str) -> Result<PathBuf, ()> {
    let input = Path::new(input);
    if input.as_os_str().is_empty() || input.is_absolute()
        || input.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_))) {
        return Err(());
    }
    let mut path = root.to_path_buf();
    for part in input.components() {
        path.push(part);
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) { return Err(()); }
    }
    Ok(path)
}

fn optional_json(root: &Path, input: &str, budget: &mut ScanBudget) -> Result<Option<Arc<Value>>, ()> {
    budget.references += 1;
    if budget.references > MAX_REFERENCES { return Err(()); }
    let path = relative(root, input)?;
    if let Some(value) = budget.json.get(&path) { return Ok(Some(value.clone())); }
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
        Ok(_) => {
            let remaining = MAX_READ_BYTES.saturating_sub(budget.bytes);
            if remaining == 0 { return Err(()); }
            let file = open_confined_file(&path)?;
            if !file.metadata().map_err(|_| ())?.is_file() { return Err(()); }
            let limit = remaining.min(2_097_153);
            let mut content = Vec::new();
            let read = file.take(limit as u64).read_to_end(&mut content);
            // Charge actual bytes even when I/O, UTF-8 or size validation fails.
            budget.bytes += content.len();
            read.map_err(|_| ())?;
            if content.len() > 2_097_152 || content.len() == remaining { return Err(()); }
            let content = std::str::from_utf8(&content).map_err(|_| ())?;
            let value = Arc::new(serde_json::from_str::<Value>(content).map_err(|_| ())?);
            budget.json.insert(path, value.clone());
            Ok(Some(value))
        },
    }
}

struct ScanEntry { path: PathBuf, directory: bool, file: bool }

fn directory_entries(path: &Path, budget: &mut ScanBudget) -> Result<Vec<ScanEntry>, ()> {
    budget.visit()?;
    checked_root(path)?;
    if !path.try_exists().map_err(|_| ())? { return Ok(Vec::new()); }
    let mut entries = Vec::new();
    #[cfg(unix)] {
        use std::os::unix::ffi::OsStrExt;
        use rustix::fs::{AtFlags, Dir, FileType, statat};
        let file = open_confined(path, true)?;
        for entry in Dir::read_from(&file).map_err(|_| ())? {
            let entry = entry.map_err(|_| ())?;
            let name = entry.file_name();
            if matches!(name.to_bytes(), b"." | b"..") { continue; }
            budget.visit()?;
            if entries.len() >= MAX_ENTRIES { return Err(()); }
            let kind = match entry.file_type() {
                FileType::Unknown => FileType::from_raw_mode(statat(&file, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| ())?.st_mode),
                kind => kind,
            };
            entries.push(ScanEntry { path: path.join(std::ffi::OsStr::from_bytes(name.to_bytes())),
                directory: kind == FileType::Directory, file: kind == FileType::RegularFile });
        }
    }
    #[cfg(not(unix))] {
        for entry in fs::read_dir(path).map_err(|_| ())? {
            budget.visit()?;
            if entries.len() >= MAX_ENTRIES { return Err(()); }
            let entry = entry.map_err(|_| ())?;
            let kind = entry.file_type().map_err(|_| ())?;
            entries.push(ScanEntry { path: entry.path(), directory: kind.is_dir(), file: kind.is_file() });
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn dirs(path: &Path, budget: &mut ScanBudget) -> Result<Vec<PathBuf>, ()> {
    Ok(directory_entries(path, budget)?.into_iter().filter(|entry| entry.directory).map(|entry| entry.path).collect())
}

fn metadata_name(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 240 && !value.chars().any(char::is_control)).then(|| value.to_string())
}

fn mcp_keys(value: &Value) -> Result<Vec<String>, ()> {
    let Some(servers) = value.get("mcpServers") else { return Ok(Vec::new()); };
    let servers = servers.as_object().ok_or(())?;
    if servers.len() > MAX_COMPONENTS { return Err(()); }
    servers.keys().map(|key| metadata_name(key).ok_or(())).collect()
}

fn add_manifest_mcp(root: &Path, value: &Value, source: &str,
    servers: &mut BTreeMap<String, PathBuf>, depth: usize, budget: &mut ScanBudget) -> Result<(), ()> {
    budget.visit()?;
    if depth > 8 { return Err(()); }
    match value {
        Value::String(path) => {
            let config = optional_json(root, path, budget)?.ok_or(())?;
            for key in mcp_keys(&config)? { servers.insert(key, relative(root, path)?); }
        }
        Value::Array(parts) => {
            if parts.len() > 64 { return Err(()); }
            for part in parts { add_manifest_mcp(root, part, source, servers, depth + 1, budget)?; }
        }
        Value::Object(config) => {
            if config.len() > MAX_COMPONENTS { return Err(()); }
            let names: Vec<String> = if config.contains_key("mcpServers") { mcp_keys(value)? }
                else { config.keys().map(|key| metadata_name(key).ok_or(())).collect::<Result<_, _>>()? };
            for key in names { servers.insert(key, relative(root, source)?); }
        }
        _ => return Err(()),
    }
    if servers.len() > MAX_COMPONENTS { return Err(()); }
    Ok(())
}

fn component_files(root: &Path, path: &Path, kind: &PluginComponentKind, depth: usize,
    result: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if depth > 8 || result.len() >= MAX_COMPONENTS { return Err(()); }
    for entry in directory_entries(path, budget)? {
        let name = entry.path.file_name().ok_or(())?.to_string_lossy().into_owned();
        if matches!(name.as_str(), ".git" | "node_modules" | "target" | "dist" | "__pycache__") { continue; }
        let path = entry.path;
        if entry.directory { component_files(root, &path, kind, depth + 1, result, budget)?; }
        else if entry.file {
            let wanted = match kind {
                PluginComponentKind::Skill => name == "SKILL.md",
                PluginComponentKind::Rule => name.ends_with(".md") || name.ends_with(".mdc"),
                PluginComponentKind::Agent | PluginComponentKind::Command => name.ends_with(".md"),
                PluginComponentKind::Hook => name.ends_with(".json"),
                PluginComponentKind::Mcp => false,
            };
            if wanted {
                let label = if *kind == PluginComponentKind::Skill { path.parent().unwrap_or(&path) } else { &path };
                let label = label.strip_prefix(root).map_err(|_| ())?.to_string_lossy();
                if let Some(name) = metadata_name(&label) {
                    result.push(PluginComponent { kind: kind.clone(), name, source_path: collapse_home(&path), native_id: None });
                }
            }
        }
        if result.len() >= MAX_COMPONENTS { return Err(()); }
    }
    Ok(())
}

fn component_paths(value: Option<&Value>, fallback: &str) -> Vec<String> {
    match value {
        Some(Value::String(path)) => vec![path.clone()],
        Some(Value::Array(paths)) => paths.iter().filter_map(Value::as_str).take(64).map(str::to_string).collect(),
        _ => vec![fallback.to_string()],
    }
}

fn observe_package(host: &Host, source: &Root, root: &Path, result: &mut PluginScan) -> Result<(), ()> {
    result.budget.visit()?;
    checked_root(root)?;
    if let Some(marker) = &source.complete_marker {
        let marker = relative(root, marker)?;
        if !fs::symlink_metadata(marker).is_ok_and(|metadata| metadata.file_type().is_file()) { return Ok(()); }
    }
    let mut manifest = None;
    for path in &host.manifest_paths {
        if let Some(value) = optional_json(root, path, &mut result.budget)? { manifest = Some((path, value)); break; }
    }
    let Some((manifest_path, manifest)) = manifest else { return Ok(()); };
    let name = metadata_name(manifest.get("name").and_then(Value::as_str).ok_or(())?).ok_or(())?;
    let version = manifest.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| "unknown".into());
    let revision = if source.layout == "marketplace_cache" {
        root.file_name().and_then(|name| name.to_str()).and_then(metadata_name).ok_or(())?
    } else { "local".to_string() };
    let mut components = Vec::new();
    let mut servers = BTreeMap::new();
    for path in &host.default_mcp_files {
        match optional_json(root, path, &mut result.budget) {
            Ok(Some(config)) => match mcp_keys(&config) {
                Ok(keys) => for key in keys { servers.entry(key).or_insert(relative(root, path)?); },
                Err(()) => result.unavailable = true,
            },
            Ok(None) => {},
            Err(_) => result.unavailable = true,
        }
    }
    if let Some(value) = manifest.get("mcpServers") {
        if add_manifest_mcp(root, value, manifest_path, &mut servers, 0, &mut result.budget).is_err() { result.unavailable = true; }
    }
    for (server, path) in servers {
        components.push(PluginComponent { kind: PluginComponentKind::Mcp, native_id: Some(format!("{}{name}-{server}", host.mcp_id_prefix)), name: server, source_path: collapse_home(&path) });
    }
    for (key, folder, kind) in [
        ("skills", "skills", PluginComponentKind::Skill), ("rules", "rules", PluginComponentKind::Rule),
        ("agents", "agents", PluginComponentKind::Agent), ("commands", "commands", PluginComponentKind::Command),
        ("hooks", "hooks", PluginComponentKind::Hook),
    ] {
        for input in component_paths(manifest.get(key), folder) {
            match relative(root, &input) {
                Ok(path) if path.is_file() => components.push(PluginComponent { kind: kind.clone(), name: input, source_path: collapse_home(&path), native_id: None }),
                Ok(path) => if component_files(root, &path, &kind, 0, &mut components, &mut result.budget).is_err() { result.unavailable = true; },
                Err(_) => result.unavailable = true,
            }
        }
    }
    components.sort(); components.dedup();
    let source_paths = vec![collapse_home(&relative(root, manifest_path)?)];
    let fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&(&name, &version, &revision, &source.presence, &components)).map_err(|_| ())?));
    result.plugins.push(NativePluginObservation {
        agent_id: host.agent_id.clone(), plugin_id: name.clone(), name, version, revision,
        presence: source.presence.clone(), enabled: None, source_paths, components,
        docs_url: host.docs_url.clone(), source_fingerprint: fingerprint,
    });
    Ok(())
}

pub(crate) fn observe(agents: &BTreeMap<String, AgentDefinition>) -> PluginScan {
    let mut result = PluginScan::default();
    for host in hosts() {
        if !agents.get(&host.agent_id).is_some_and(|agent| agent.enabled) { continue; }
        for source in &host.roots {
            let root = expand_tilde(&source.path);
            let mut packages = 0;
            let mut scan_package = |path: &Path, result: &mut PluginScan| {
                packages += 1;
                if packages > MAX_ENTRIES { result.unavailable = true; return false; }
                if observe_package(host, source, path, result).is_err() { result.unavailable = true; }
                result.budget.visited <= MAX_SCAN_ENTRIES && result.budget.references <= MAX_REFERENCES && result.budget.bytes < MAX_READ_BYTES
            };
            match dirs(&root, &mut result.budget) {
                Ok(roots) if source.layout == "flat" => for path in roots { if !scan_package(&path, &mut result) { break; } },
                Ok(markets) if source.layout == "marketplace_cache" => {
                    'markets: for market in markets {
                        let identities = match dirs(&market, &mut result.budget) {
                            Ok(items) => items, Err(()) => { result.unavailable = true; continue; }
                        };
                        for identity in identities {
                            match dirs(&identity, &mut result.budget) {
                                Ok(items) => for path in items { if !scan_package(&path, &mut result) { break 'markets; } },
                                Err(()) => result.unavailable = true,
                            }
                        }
                    }
                }
                _ => result.unavailable = true,
            }
        }
    }
    normalize(&mut result);
    result
}

fn normalize(result: &mut PluginScan) {
    let mut plugins = BTreeMap::<_, NativePluginObservation>::new();
    for plugin in std::mem::take(&mut result.plugins) {
        let key = (plugin.agent_id.clone(), plugin.plugin_id.clone(), plugin.version.clone(), plugin.revision.clone(), plugin.presence.clone());
        if let Some(existing) = plugins.get_mut(&key) {
            existing.source_paths.extend(plugin.source_paths);
            existing.components.extend(plugin.components);
            existing.source_paths.sort(); existing.source_paths.dedup();
            existing.components.sort(); existing.components.dedup();
        } else { plugins.insert(key, plugin); }
    }
    result.plugins = plugins.into_values().collect();
    let mut mcps = BTreeMap::<_, HostMcpObservation>::new();
    for plugin in &mut result.plugins {
        plugin.source_fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&(&plugin.plugin_id, &plugin.version, &plugin.revision, &plugin.presence, &plugin.source_paths, &plugin.components)).expect("metadata serializes")));
        for component in &plugin.components {
            let Some(id) = component.native_id.as_ref() else { continue; };
            if component.kind != PluginComponentKind::Mcp { continue; }
            let key = (plugin.agent_id.clone(), id.clone(), plugin.presence.clone());
            let mcp = mcps.entry(key).or_insert_with(|| HostMcpObservation {
                agent_id: plugin.agent_id.clone(), server_id: id.clone(), name: component.name.clone(),
                provider: plugin.name.clone(), origin: HostResourceOrigin::Plugin, presence: plugin.presence.clone(),
                versions: Vec::new(), source_paths: Vec::new(), docs_url: plugin.docs_url.clone(), source_fingerprint: String::new(),
            });
            mcp.versions.push(plugin.version.clone()); mcp.source_paths.push(component.source_path.clone());
        }
    }
    result.mcps = mcps.into_values().map(|mut mcp| {
        mcp.versions.sort(); mcp.versions.dedup(); mcp.source_paths.sort(); mcp.source_paths.dedup();
        mcp.source_fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&(&mcp.server_id, &mcp.presence, &mcp.versions, &mcp.source_paths)).expect("metadata serializes")));
        mcp
    }).collect();
}

pub(crate) fn watch_paths(agents: &BTreeMap<String, AgentDefinition>) -> Vec<PathBuf> {
    hosts().iter().filter(|host| agents.get(&host.agent_id).is_some_and(|agent| agent.enabled))
        .flat_map(|host| host.roots.iter().map(|root| expand_tilde(&root.path)))
        .collect::<BTreeSet<_>>().into_iter().collect()
}

pub(crate) fn supported_agents(agents: &BTreeMap<String, AgentDefinition>) -> Vec<String> {
    hosts().iter().filter(|host| agents.get(&host.agent_id).is_some_and(|agent| agent.enabled))
        .map(|host| host.agent_id.clone()).collect()
}

fn collapse_home(path: &Path) -> String { collapse_home_string(path.to_string_lossy().as_ref()) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    fn package(home: &TestHome, name: &str) -> PathBuf {
        let root = home.home.join("plugins").join(name);
        fs::create_dir_all(root.join(".cursor-plugin")).unwrap();
        fs::write(root.join(".cursor-plugin/plugin.json"), r#"{"name":"fixture","version":"1.0.0"}"#).unwrap();
        root
    }

    #[test]
    fn incomplete_cache_is_not_a_complete_package_and_completed_cache_is_not_installed() {
        let home = TestHome::new("plugin-cache-presence");
        let root = package(&home, "revision-a");
        let host = &hosts()[0]; let source = &host.roots[0];
        let mut scan = PluginScan::default();
        observe_package(host, source, &root, &mut scan).unwrap();
        assert!(scan.plugins.is_empty());
        fs::write(root.join(".cache-complete"), "").unwrap();
        observe_package(host, source, &root, &mut scan).unwrap();
        assert_eq!(scan.plugins[0].presence, PluginPresence::CacheOnly);
        assert_eq!(scan.plugins[0].enabled, None);
        let value = serde_json::to_value(&scan.plugins[0]).unwrap();
        assert!(value.get("asset").is_none());
        assert!(value.get("available_actions").is_none());
    }

    #[test]
    fn manifest_mcp_overrides_first_wins_defaults_without_exporting_secrets() {
        let home = TestHome::new("plugin-mcp-precedence");
        let root = package(&home, "local");
        fs::write(root.join(".mcp.json"), r#"{"mcpServers":{"same":{"url":"https://first.invalid","headers":{"Authorization":"SECRET_SENTINEL"}}}}"#).unwrap();
        fs::write(root.join("mcp.json"), r#"{"mcpServers":{"same":{"command":"SECRET_SENTINEL"},"second":{"command":"SECRET_SENTINEL"}}}"#).unwrap();
        fs::write(root.join(".cursor-plugin/plugin.json"), r#"{"name":"fixture","version":"1.0.0","mcpServers":{"same":{"url":"https://override.invalid"}}}"#).unwrap();
        let mut scan = PluginScan::default();
        observe_package(&hosts()[0], &hosts()[0].roots[1], &root, &mut scan).unwrap();
        normalize(&mut scan);
        assert_eq!(scan.mcps.len(), 2);
        let same = scan.mcps.iter().find(|mcp| mcp.server_id == "plugin-fixture-same").unwrap();
        assert!(same.source_paths[0].ends_with(".cursor-plugin/plugin.json"));
        let encoded = serde_json::to_string(&(&scan.plugins, &scan.mcps)).unwrap();
        assert!(!encoded.contains("SECRET_SENTINEL"));
        assert!(!encoded.contains("https://override.invalid"));
        assert!(!encoded.contains("available_actions"));
    }

    #[test]
    fn component_paths_remain_confined_and_other_packages_survive_invalid_references() {
        let home = TestHome::new("plugin-path-boundary");
        let root = package(&home, "safe");
        assert!(relative(&root, "../outside.json").is_err());
        assert!(relative(&root, "/tmp/outside.json").is_err());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(home.home.join("outside.json"), root.join("linked.json")).unwrap();
            assert!(relative(&root, "linked.json").is_err());
            assert!(text(&root.join("linked.json"), 100).is_err());
        }
        assert!(text(&root, 100).is_err());
        fs::write(root.join(".cursor-plugin/plugin.json"), r#"{"name":"fixture","mcpServers":"../outside.json"}"#).unwrap();
        let mut scan = PluginScan::default();
        observe_package(&hosts()[0], &hosts()[0].roots[1], &root, &mut scan).unwrap();
        assert!(scan.unavailable);
        assert_eq!(scan.plugins.len(), 1);
        assert!(scan.plugins[0].components.is_empty());
    }

    #[test]
    fn alias_sources_and_versions_are_distinct_from_host_mcp_identity() {
        let home = TestHome::new("plugin-version-aliases");
        let first = package(&home, "revision-a"); let second = package(&home, "revision-b");
        for root in [&first, &second] {
            fs::write(root.join(".cache-complete"), "").unwrap();
            fs::write(root.join("mcp.json"), r#"{"mcpServers":{"same":{"command":"fixture"}}}"#).unwrap();
        }
        let mut scan = PluginScan::default();
        for root in [&first, &second] { observe_package(&hosts()[0], &hosts()[0].roots[0], root, &mut scan).unwrap(); }
        normalize(&mut scan);
        assert_eq!(scan.plugins.len(), 2);
        assert_eq!(scan.mcps.len(), 1);
        assert_eq!(scan.mcps[0].source_paths.len(), 2);
        assert_eq!(scan.mcps[0].presence, PluginPresence::CacheOnly);
    }

    #[test]
    fn local_packages_with_distinct_versions_do_not_merge() {
        let home = TestHome::new("plugin-local-versions");
        let first = package(&home, "one"); let second = package(&home, "two");
        fs::write(second.join(".cursor-plugin/plugin.json"), r#"{"name":"fixture","version":"2.0.0"}"#).unwrap();
        let mut scan = PluginScan::default();
        for root in [&first, &second] { observe_package(&hosts()[0], &hosts()[0].roots[1], root, &mut scan).unwrap(); }
        normalize(&mut scan);
        assert_eq!(scan.plugins.iter().map(|plugin| plugin.version.as_str()).collect::<Vec<_>>(), ["1.0.0", "2.0.0"]);
        assert!(scan.plugins.iter().all(|plugin| plugin.source_paths.len() == 1));
    }

    #[test]
    fn repeated_references_share_json_and_stop_at_the_scan_budget() {
        let home = TestHome::new("plugin-reference-budget");
        let root = package(&home, "bounded");
        fs::write(root.join("mcp.json"), r#"{"mcpServers":{"fixture":{}}}"#).unwrap();
        let mut budget = ScanBudget::default();
        let first = optional_json(&root, "mcp.json", &mut budget).unwrap().unwrap();
        let bytes = budget.bytes;
        for _ in 1..MAX_REFERENCES {
            let next = optional_json(&root, "mcp.json", &mut budget).unwrap().unwrap();
            assert!(Arc::ptr_eq(&first, &next));
        }
        assert_eq!(budget.bytes, bytes);
        assert!(optional_json(&root, "mcp.json", &mut budget).is_err());
        budget.visited = MAX_SCAN_ENTRIES;
        assert!(dirs(&root, &mut budget).is_err());
        let mut exhausted = ScanBudget { bytes: MAX_READ_BYTES, ..ScanBudget::default() };
        assert!(optional_json(&root, "mcp.json", &mut exhausted).is_err());
    }

    #[test]
    fn rejected_file_bytes_are_still_charged() {
        let home = TestHome::new("plugin-invalid-read-budget");
        let root = package(&home, "invalid");
        fs::write(root.join("bad.json"), [0xff; 32]).unwrap();
        let mut budget = ScanBudget::default();
        assert!(optional_json(&root, "bad.json", &mut budget).is_err());
        assert_eq!(budget.bytes, 32);
        fs::write(root.join("large.json"), vec![b' '; 2_097_153]).unwrap();
        assert!(optional_json(&root, "large.json", &mut budget).is_err());
        assert_eq!(budget.bytes, 32 + 2_097_153);
    }

    #[test]
    fn invalid_mcp_shape_does_not_hide_valid_package_components() {
        let home = TestHome::new("plugin-partial-components");
        let root = package(&home, "partial");
        fs::create_dir_all(root.join("skills/valid")).unwrap();
        fs::write(root.join("skills/valid/SKILL.md"), "# Fixture").unwrap();
        fs::write(root.join(".mcp.json"), r#"{"mcpServers":[]}"#).unwrap();
        fs::write(root.join("mcp.json"), r#"{"mcpServers":{"valid":{}}}"#).unwrap();
        let mut scan = PluginScan::default();
        observe_package(&hosts()[0], &hosts()[0].roots[1], &root, &mut scan).unwrap();
        normalize(&mut scan);
        assert!(scan.unavailable);
        assert_eq!(scan.plugins.len(), 1);
        assert!(scan.plugins[0].components.iter().any(|component| component.kind == PluginComponentKind::Skill));
        assert_eq!(scan.mcps.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_package_ancestors_are_rejected_before_reading() {
        let home = TestHome::new("plugin-ancestor-symlink");
        let root = package(&home, "real");
        let alias = home.home.join("alias");
        std::os::unix::fs::symlink(root.parent().unwrap(), &alias).unwrap();
        let escaped = alias.join("real");
        assert!(checked_root(&escaped).is_err());
        assert!(text(&escaped.join(".cursor-plugin/plugin.json"), 4096).is_err());
        let mut scan = PluginScan::default();
        assert!(observe_package(&hosts()[0], &hosts()[0].roots[1], &escaped, &mut scan).is_err());
        assert!(scan.plugins.is_empty());
    }
}
