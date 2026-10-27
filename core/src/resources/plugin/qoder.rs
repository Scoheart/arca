//! Qoder IDE's global registrations and shipped packages. Native migrations,
//! enablement, component bodies and daemon execution are never invoked.
use super::*;
use std::cell::RefCell;

const AUTHORED: &str = ".qoder-plugin/plugin.json";
const BUNDLE_MARKET: &str = "qoder-bundler";
const DEFAULT_MARKET: &str = "qoder-marketplace";

fn source_root(source: &Root) -> PathBuf {
    if source.path.starts_with("/Applications/") { crate::paths::system_probe_path(&source.path) }
    else { expand_tilde(&source.path) }
}

fn local_path(input: &str) -> Result<String, ()> {
    let path = Path::new(input);
    if input.is_empty() || input.contains('\\') || input.chars().any(char::is_control) || path.is_absolute()
        || path.components().any(|part| matches!(part, Component::ParentDir | Component::Prefix(_))) { return Err(()); }
    let normalized = path.components().filter(|part| *part != Component::CurDir).collect::<PathBuf>();
    if normalized.as_os_str().is_empty() { return Err(()); }
    Ok(normalized.to_string_lossy().into_owned())
}

fn identity(id: &str) -> Result<(&str, &str), ()> {
    metadata_name(id).ok_or(())?;
    let (name, market) = match id.rsplit_once('@') {
        Some((name, market)) if !name.is_empty() => (name, if market.is_empty() { DEFAULT_MARKET } else { market }),
        _ => (id, DEFAULT_MARKET),
    };
    Ok((name, market))
}

fn foreign(id: &str, path: Option<&str>) -> bool {
    let markets = ["qoderapp-bundler", "qodercli-bundler"];
    id.rsplit_once('@').is_some_and(|(_, market)| markets.contains(&market))
        || path.is_some_and(|path| path.replace('\\', "/").split('/').collect::<Vec<_>>().windows(3)
            .any(|parts| parts[0] == "plugins" && parts[1] == "cache" && markets.contains(&parts[2])))
}

struct Entry { path: String, directory: bool, file: bool }

/// Archive paths are metadata provenance (`archive.zip!/member`), never files
/// extracted into a native installation or an Arca asset directory.
enum Package<'a> {
    Disk(&'a Path),
    Archive { path: &'a Path, zip: RefCell<zip::ZipArchive<fs::File>>, members: BTreeMap<String, (usize, bool)> },
}

impl Package<'_> {
    fn path(&self, input: &str) -> Result<String, ()> {
        let input = local_path(input)?;
        match self {
            Self::Disk(root) => Ok(collapse_home(&relative(root, &input)?)),
            Self::Archive { path, .. } => Ok(format!("{}!/computer-use/{input}", collapse_home(path))),
        }
    }

    fn exists(&self, input: &str) -> Result<bool, ()> {
        let input = local_path(input)?;
        match self {
            Self::Disk(root) => {
                let path = relative(root, &input)?; checked_root(&path)?;
                match fs::symlink_metadata(path) {
                    Ok(value) if value.is_file() || value.is_dir() => Ok(true),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false), _ => Err(()),
                }
            },
            Self::Archive { members, .. } => Ok(members.contains_key(&input)
                || members.keys().any(|name| name.starts_with(&format!("{input}/")))),
        }
    }

    fn file(&self, input: &str) -> Result<bool, ()> {
        let input = local_path(input)?;
        match self {
            Self::Disk(root) => regular_metadata_file(&relative(root, &input)?),
            Self::Archive { members, .. } => Ok(members.get(&input).is_some_and(|(_, directory)| !directory)),
        }
    }

    fn children(&self, input: &str, budget: &mut ScanBudget) -> Result<Vec<Entry>, ()> {
        let input = if input.is_empty() { String::new() } else { local_path(input)? };
        match self {
            Self::Disk(root) => {
                let path = if input.is_empty() { root.to_path_buf() } else { relative(root, &input)? };
                if regular_metadata_file(&path)? { return Ok(Vec::new()); }
                directory_entries(&path, budget)?.into_iter().map(|entry| {
                    if !entry.directory && !entry.file { return Err(()); }
                    Ok(Entry { path: entry.path.strip_prefix(root).map_err(|_| ())?.to_string_lossy().into_owned(), directory: entry.directory, file: entry.file })
                }).collect()
            },
            Self::Archive { members, .. } => {
                budget.visit()?;
                let prefix = if input.is_empty() { String::new() } else { format!("{input}/") };
                let mut entries = BTreeMap::new();
                for (name, (_, directory)) in members {
                    let Some(tail) = name.strip_prefix(&prefix).filter(|tail| !tail.is_empty()) else { continue; };
                    budget.visit()?;
                    let first = tail.split('/').next().ok_or(())?;
                    let path = format!("{prefix}{first}");
                    let directory = *directory || tail.contains('/');
                    entries.entry(path.clone()).or_insert(Entry { path, directory, file: !directory });
                    if entries.len() > MAX_ENTRIES { return Err(()); }
                }
                Ok(entries.into_values().collect())
            },
        }
    }

    fn json(&self, input: &str, budget: &mut ScanBudget) -> Result<Option<Arc<Value>>, ()> {
        let input = local_path(input)?;
        match self {
            Self::Disk(root) => optional_json(root, &input, budget),
            Self::Archive { zip, members, .. } => {
                budget.references += 1;
                if budget.references > MAX_REFERENCES { return Err(()); }
                let key = (PathBuf::from(self.path(&input)?), DocumentFormat::Json);
                if let Some(value) = budget.json.get(&key) { return Ok(Some(value.clone())); }
                let Some((index, directory)) = members.get(&input) else { return Ok(None); };
                if *directory { return Err(()); }
                let mut zip = zip.borrow_mut(); let member = zip.by_index(*index).map_err(|_| ())?;
                let remaining = MAX_READ_BYTES.saturating_sub(budget.bytes);
                if remaining == 0 { return Err(()); }
                let mut content = Vec::new();
                let read = member.take(remaining.min(2_097_153) as u64).read_to_end(&mut content);
                budget.bytes += content.len(); read.map_err(|_| ())?;
                if content.len() > 2_097_152 || content.len() == remaining { return Err(()); }
                let value = Arc::new(serde_json::from_slice::<Value>(&content).map_err(|_| ())?);
                budget.json.insert(key, value.clone()); Ok(Some(value))
            },
        }
    }
}

fn add(package: &Package<'_>, input: &str, kind: PluginComponentKind, name: Option<&str>, output: &mut Vec<PluginComponent>) -> Result<bool, ()> {
    if !package.file(input)? { return Ok(false); }
    if output.len() >= MAX_COMPONENTS { return Err(()); }
    let normalized = local_path(input)?;
    output.push(PluginComponent { kind, name: metadata_name(name.unwrap_or(&normalized)).ok_or(())?, source_path: package.path(input)?, native_id: None });
    Ok(true)
}

fn truthy(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null | Value::Bool(false)))
        && !matches!(value, Some(Value::String(value)) if value.is_empty())
}

fn skill_children(package: &Package<'_>, input: &str, output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    for entry in package.children(input, budget)? {
        let name = entry.path.rsplit('/').next().ok_or(())?;
        if entry.directory && !name.starts_with('.') {
            add(package, &format!("{}/SKILL.md", entry.path), PluginComponentKind::Skill, None, output)?;
        }
    }
    Ok(())
}

fn skills(package: &Package<'_>, value: Option<&Value>, output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let before = output.len();
    if let Some(Value::Array(paths)) = value {
        if paths.len() > 64 { return Err(()); }
        for input in paths.iter().filter_map(Value::as_str) {
            let input = local_path(input)?;
            add(package, &format!("{input}/SKILL.md"), PluginComponentKind::Skill, None, output)?;
        }
    } else {
        let input = value.and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or("skills");
        skill_children(package, input, output, budget)?;
    }
    if output.len() == before && !truthy(value) { skill_children(package, "", output, budget)?; }
    Ok(())
}

fn markdown(package: &Package<'_>, value: Option<&Value>, fallback: &str, kind: PluginComponentKind, output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if let Some(Value::Array(paths)) = value {
        if paths.len() > 64 { return Err(()); }
        for input in paths.iter().filter_map(Value::as_str) { add(package, input, kind.clone(), None, output)?; }
    } else if kind == PluginComponentKind::Command && value.is_some_and(Value::is_object) {
        let values = value.and_then(Value::as_object).ok_or(())?;
        if values.len() > MAX_COMPONENTS { return Err(()); }
        for (name, declaration) in values {
            if let Some(source) = declaration.get("source").and_then(Value::as_str).filter(|source| !source.is_empty()) {
                if !add(package, source, kind.clone(), Some(name), output)? { return Err(()); }
            }
        }
    } else {
        if truthy(value) && !value.is_some_and(Value::is_string) { return Err(()); }
        let input = value.and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or(fallback);
        for entry in package.children(input, budget)? {
            if entry.file && entry.path.ends_with(".md") { add(package, &entry.path, kind.clone(), None, output)?; }
        }
    }
    Ok(())
}

fn hook_file(package: &Package<'_>, input: &str, output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<bool, ()> {
    let Some(value) = package.json(input, budget)? else { return Ok(false); };
    let events = value.get("hooks").unwrap_or(&value).as_object().ok_or(())?;
    if events.len() > MAX_COMPONENTS { return Err(()); }
    let before = output.len();
    for (name, records) in events {
        let Some(records) = records.as_array() else { continue; };
        if records.len() > MAX_COMPONENTS { return Err(()); }
        let mut present = false;
        for record in records {
            if let Some(handlers) = record.get("hooks").and_then(Value::as_array) {
                if handlers.len() > MAX_COMPONENTS { return Err(()); }
                present |= handlers.iter().any(Value::is_object);
            }
        }
        if present { add(package, input, PluginComponentKind::Hook, Some(name), output)?; }
    }
    Ok(output.len() > before)
}

fn hooks(package: &Package<'_>, value: Option<&Value>, output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if truthy(value) && !value.is_some_and(Value::is_string) { return Err(()); }
    let input = value.and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or("hooks");
    if input.ends_with(".json") { hook_file(package, input, output, budget)?; return Ok(()); }
    let entries = package.children(input, budget)?; let before = output.len();
    for entry in &entries {
        if entry.file && entry.path.ends_with(".json") { hook_file(package, &entry.path, output, budget)?; }
    }
    if before == output.len() {
        for entry in &entries {
            if entry.file && entry.path.ends_with(".md") { add(package, &entry.path, PluginComponentKind::Hook, None, output)?; }
        }
    }
    Ok(())
}

fn mcps(package: &Package<'_>, authored: &Value, id: &str, output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let mut candidates = Vec::new();
    if truthy(authored.get("mcpServers")) {
        candidates.push(authored.get("mcpServers").and_then(Value::as_str).ok_or(())?.to_string());
    }
    candidates.extend(["mcp.json".into(), ".mcp.json".into()]);
    for input in candidates {
        // Native IDE uses first existing, including a malformed candidate.
        if !package.exists(&input)? { continue; }
        let Some(value) = package.json(&input, budget)? else { return Err(()); };
        let Some(servers) = value.get("mcpServers").and_then(Value::as_object) else { return Ok(()); };
        if servers.len() > MAX_COMPONENTS { return Err(()); }
        for (name, config) in servers {
            if !config.is_object() || !["command", "url"].iter().any(|key| config.get(*key).and_then(Value::as_str).is_some_and(|value| !value.is_empty())) { continue; }
            let native_id = config.get("identifier").and_then(Value::as_str).filter(|value| !value.is_empty())
                .map(str::to_string).unwrap_or_else(|| format!("{id}:{name}"));
            // Native identifiers can be longer than component display names.
            if native_id.len() > 1024 || native_id.chars().any(char::is_control) { return Err(()); }
            if !add(package, &input, PluginComponentKind::Mcp, Some(name), output)? { return Err(()); }
            output.last_mut().ok_or(())?.native_id = Some(native_id);
        }
        return Ok(());
    }
    Ok(())
}

fn package(host: &Host, source: &Package<'_>, id: &str, registered_version: Option<&str>, presence: PluginPresence, provenance: &[String], result: &mut PluginScan) -> NativePluginObservation {
    let fallback = identity(id).map(|(name, _)| name).unwrap_or(id);
    let mut plugin = NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: id.into(), name: fallback.into(), version: registered_version.and_then(metadata_name).unwrap_or_else(|| "unknown".into()), revision: source.path(AUTHORED).unwrap_or_else(|_| id.into()), presence, enabled: None, source_paths: provenance.to_vec(), components: Vec::new(), docs_url: host.docs_url.clone(), source_fingerprint: String::new() };
    let observed = (|| {
        result.budget.visit()?;
        let authored = source.json(AUTHORED, &mut result.budget)?;
        let projected = match source.json("plugin.json", &mut result.budget) {
            Ok(value) => value, Err(()) => { result.unavailable = true; None },
        };
        let manifest = authored.as_ref().or(projected.as_ref()).ok_or(())?;
        if !manifest.is_object() { return Err(()); }
        plugin.name = manifest.get("name").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
        if plugin.presence == PluginPresence::Bundled && plugin.name != fallback { return Err(()); }
        if let Some(version) = manifest.get("version").and_then(Value::as_str).and_then(metadata_name) { plugin.version = version; }
        for (input, exists) in [(AUTHORED, authored.is_some()), ("plugin.json", projected.is_some())] {
            if exists { plugin.source_paths.push(source.path(input)?); }
        }
        // Root plugin.json is a native registration projection. The IDE's
        // component-detail and MCP readers consult the authored Qoder manifest.
        let empty = Value::Null; let manifest = authored.as_deref().unwrap_or(&empty);
        let mut invalid = false;
        invalid |= skills(source, manifest.get("skills"), &mut plugin.components, &mut result.budget).is_err();
        for (key, kind) in [("commands", PluginComponentKind::Command), ("agents", PluginComponentKind::Agent), ("rules", PluginComponentKind::Rule)] {
            invalid |= markdown(source, manifest.get(key), key, kind, &mut plugin.components, &mut result.budget).is_err();
        }
        invalid |= hooks(source, manifest.get("hooks"), &mut plugin.components, &mut result.budget).is_err();
        invalid |= mcps(source, manifest, id, &mut plugin.components, &mut result.budget).is_err();
        if invalid { return Err(()); } Ok::<(), ()>(())
    })();
    if observed.is_err() { result.unavailable = true; }
    if plugin.source_paths.len() == provenance.len() { plugin.presence = PluginPresence::Declared; }
    plugin.source_paths.sort(); plugin.source_paths.dedup(); plugin.components.sort(); plugin.components.dedup(); plugin
}

fn selected_registrations(root: &Path, result: &mut PluginScan) -> Result<BTreeMap<String, (Value, String)>, ()> {
    checked_root(root)?;
    let mut selected = BTreeMap::<String, (Value, String)>::new();
    let mut ledger = None;
    for input in ["plugins/installed_plugins_v2.json", "plugins/installed_plugins-v2.json"] {
        if let Some(value) = optional_json(root, input, &mut result.budget)? {
            if value.get("plugins").is_some_and(Value::is_object) { ledger = Some((input, value)); break; }
        }
    }
    if let Some((input, ledger)) = ledger {
        let entries = ledger.get("plugins").and_then(Value::as_object).ok_or(())?;
        if entries.len() > MAX_ENTRIES { return Err(()); }
        for (id, records) in entries {
            let observed = (|| {
                identity(id)?;
                let records = records.as_array().filter(|values| values.len() <= MAX_ENTRIES).ok_or(())?;
                for record in records {
                    result.budget.visit()?;
                    if !record.is_object() { return Err(()); }
                    if record.get("scope").and_then(Value::as_str) == Some("project") { continue; }
                    let path = record.get("installPath").and_then(Value::as_str);
                    if foreign(id, path) { continue; }
                    if selected.get(id).is_some_and(|(record, _)| record.get("scope").and_then(Value::as_str) == Some("managed")) { continue; }
                    selected.insert(id.clone(), (record.clone(), input.into()));
                }
                Ok::<(), ()>(())
            })(); if observed.is_err() { result.unavailable = true; }
        }
    }
    if let Some(legacy) = optional_json(root, "plugins/installed_plugins.json", &mut result.budget)? {
        let entries = legacy.get("plugins").and_then(Value::as_object).ok_or(())?;
        if entries.len() > MAX_ENTRIES { return Err(()); }
        for (id, record) in entries {
            result.budget.visit()?;
            if selected.contains_key(id) || foreign(id, record.get("installPath").and_then(Value::as_str))
                || record.get("scope").and_then(Value::as_str) == Some("project") { continue; }
            if identity(id).is_err() || !record.is_object() { result.unavailable = true; continue; }
            selected.insert(id.clone(), (record.clone(), "plugins/installed_plugins.json".into()));
        }
    }
    Ok(selected)
}

fn registrations(host: &Host, root: &Path, result: &mut PluginScan) -> Result<(), ()> {
    for (id, (record, ledger)) in selected_registrations(root, result)? {
        let (name, market) = identity(&id)?;
        let version = record.get("version").and_then(Value::as_str).filter(|value| !value.is_empty());
        let provenance = vec![collapse_home(&root.join(&ledger))];
        let resolved = (|| {
            let path = match record.get("installPath").and_then(Value::as_str).filter(|value| !value.is_empty()) {
                Some(input) => PathBuf::from(input), None => {
                    let mut path = root.join("plugins/cache");
                    for value in [market, name, version.ok_or(())?] {
                        if value.contains(['/', '\\']) || matches!(value, "" | "." | "..") || value.chars().any(char::is_control) { return Err(()); }
                        path.push(value);
                    }
                    path
                },
            };
            if !path.is_absolute() { return Err(()); } checked_root(&path)?; Ok::<_, ()>(path)
        })();
        let path = match resolved { Ok(path) => path, Err(()) => {
            result.unavailable = true;
            result.plugins.push(NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:id.clone(), name:name.into(), version:version.and_then(metadata_name).unwrap_or_else(|| "unknown".into()), revision:format!("registered:{id}"), presence:PluginPresence::Declared, enabled:None, source_paths:provenance, components:Vec::new(), docs_url:host.docs_url.clone(), source_fingerprint:String::new() });
            continue;
        }};
        let plugin = package(host, &Package::Disk(&path), &id, version, PluginPresence::Installed, &provenance, result);
        result.plugins.push(plugin);
    }
    Ok(())
}

fn bundled(host: &Host, root: &Path, result: &mut PluginScan) -> Result<(), ()> {
    checked_root(root)?;
    let roots: Vec<&Path> = std::iter::once(root).chain(root.parent()).collect();
    let mut packages = BTreeMap::new();
    for root in &roots {
        for path in dirs(&root.join("resources/plugins/bundle-plugins"), &mut result.budget)? {
            let name = path.file_name().and_then(|value| value.to_str()).and_then(metadata_name).ok_or(())?;
            packages.insert(name, path);
        }
    }
    for name in ["better-harness", "security-scan"] {
        for root in &roots {
            let path = root.join("node_modules/@ali").join(name);
            if let Some(manifest) = optional_json(&path, AUTHORED, &mut result.budget)? {
                if manifest.get("name").and_then(Value::as_str) == Some(name) { packages.insert(name.into(), path); }
                break;
            }
        }
    }
    for (name, path) in packages {
        let id = format!("{name}@{BUNDLE_MARKET}");
        let plugin = package(host, &Package::Disk(&path), &id, None, PluginPresence::Bundled, &[], result);
        result.plugins.push(plugin);
    }
    Ok(())
}

fn archive(host: &Host, path: &Path, result: &mut PluginScan) -> Result<(), ()> {
    if !regular_metadata_file(path)? { return Ok(()); }
    let file = open_confined_file(path)?;
    if file.metadata().map_err(|_| ())?.len() > 16 * 1024 * 1024 { return Err(()); }
    let mut zip = zip::ZipArchive::new(file).map_err(|_| ())?;
    if zip.len() > MAX_ENTRIES { return Err(()); }
    let mut members = BTreeMap::new(); let mut bytes = 0u64;
    for index in 0..zip.len() {
        result.budget.visit()?;
        let member = zip.by_index(index).map_err(|_| ())?;
        let name = member.name().trim_end_matches('/');
        if name == "computer-use" && member.is_dir() { continue; }
        let name = name.strip_prefix("computer-use/").ok_or(())?;
        let normalized = local_path(name)?;
        if normalized != name || normalized.is_empty() { return Err(()); }
        if member.unix_mode().is_some_and(|mode| ![0, 0o100000, 0o040000].contains(&(mode & 0o170000))) { return Err(()); }
        bytes = bytes.checked_add(member.size()).ok_or(())?;
        if bytes > 8 * 1024 * 1024 || members.insert(normalized, (index, member.is_dir())).is_some() { return Err(()); }
    }
    for name in members.keys() {
        let mut parent = Path::new(name).parent();
        while let Some(path) = parent {
            if members.get(path.to_string_lossy().as_ref()).is_some_and(|(_, directory)| !directory) { return Err(()); }
            parent = path.parent();
        }
    }
    let source = Package::Archive { path, zip: RefCell::new(zip), members };
    let plugin = package(host, &source, "computer-use@qoder-bundler", None, PluginPresence::Bundled, &[collapse_home(path)], result);
    result.plugins.push(plugin); Ok(())
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    for source in &host.roots {
        let root = source_root(source);
        let observed = match source.layout.as_str() {
            "qoder_installed" => registrations(host, &root, result),
            "qoder_resources" => bundled(host, &root, result),
            "qoder_computer_use_archive" => archive(host, &root, result),
            _ => Err(()),
        };
        if observed.is_err() { result.unavailable = true; }
    }
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    host.roots.iter().flat_map(|source| {
        let root = source_root(source);
        if source.layout == "qoder_resources" {
            vec![root.join("resources/plugins/bundle-plugins"), root.join("node_modules/@ali"), root.parent().unwrap_or(&root).join("resources/plugins/bundle-plugins"), root.parent().unwrap_or(&root).join("node_modules/@ali")]
        } else if source.layout == "qoder_installed" {
            let mut paths = vec![root.join("plugins")];
            let mut scan = PluginScan::default();
            if let Ok(records) = selected_registrations(&root, &mut scan) {
                for (record, _) in records.into_values() {
                    if let Some(path) = record.get("installPath").and_then(Value::as_str).map(PathBuf::from) {
                        if path.is_absolute() && checked_root(&path).is_ok() { paths.push(path); }
                    }
                }
            }
            paths
        } else { vec![root] }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    use serde_json::json;
    use std::io::Write;

    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "qoder").unwrap() }
    fn write(root: &Path, input: &str, content: &str) {
        let path = root.join(input); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, content).unwrap();
    }
    fn install(home: &TestHome, path: &str, name: &str, version: &str) -> (PathBuf, Value) {
        let root = home.home.join(path);
        write(&root, AUTHORED, &json!({"name":name,"version":version}).to_string());
        (root.clone(), json!({"scope":"user","installPath":root,"version":version}))
    }
    fn register(home: &TestHome, input: &str, plugins: Value) {
        write(&home.home.join(".qoder/plugins"), input, &json!({"version":2,"plugins":plugins}).to_string());
    }
    fn scan() -> PluginScan { let mut result = PluginScan::default(); observe(host(), &mut result); normalize(&mut result); result }
    fn components(root: &Path) -> (NativePluginObservation, bool) {
        let mut result = PluginScan::default();
        let plugin = package(host(), &Package::Disk(root), "example@market", Some("1"), PluginPresence::Installed, &[], &mut result);
        (plugin, result.unavailable)
    }
    fn zip_file(path: &Path, entries: &[(&str, &str)]) {
        let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, content) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap(); writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn registered_versionless_paths_managed_precedence_and_legacy_complement() {
        let home = TestHome::new("qoder-records");
        let (_, one) = install(&home, ".qoder/plugins/cache/market/example", "example", "1");
        let (_, two) = install(&home, "explicit-local-copy", "example", "2");
        let (_, mut managed) = install(&home, "managed-copy", "example", "3"); managed["scope"] = json!("managed");
        let (_, mut later_managed) = install(&home, "later-managed-copy", "example", "4"); later_managed["scope"] = json!("managed");
        let (_, legacy) = install(&home, "legacy-only", "other", "5");
        register(&home, "installed_plugins_v2.json", json!({"example@market":[one.clone(),two.clone(),managed,later_managed,two]}));
        register(&home, "installed_plugins.json", json!({"example@market":one,"other@market":legacy}));
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 2);
        assert_eq!(result.plugins.iter().find(|p| p.plugin_id == "example@market").unwrap().version, "3");
        assert!(result.plugins.iter().all(|p| p.enabled.is_none() && p.presence == PluginPresence::Installed));
        assert!(!home.home.join(".qoder/settings.json").exists());
    }

    #[test]
    fn hyphenated_fallback_ignores_projects_other_hosts_and_unregistered_cache() {
        let home = TestHome::new("qoder-scopes");
        let (_, record) = install(&home, ".qoder/plugins/cache/market/example/1", "example", "1");
        let (_, mut project) = install(&home, "project", "project", "1"); project["scope"] = json!("project");
        let (_, foreign_path) = install(&home, ".qoder/plugins/cache/qoderapp-bundler/other/1", "other", "1");
        install(&home, ".qoder/plugins/cache/market/stale/9", "stale", "9");
        write(&home.home, ".qoder/plugins/installed_plugins_v2.json", r#"{"version":2}"#);
        register(&home, "installed_plugins-v2.json", json!({"example@market":[record.clone()],"project@market":[project],"desktop@qoderapp-bundler":[record],"other@market":[foreign_path]}));
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 1); assert_eq!(result.plugins[0].plugin_id, "example@market");
    }

    #[test]
    fn skills_use_individual_arrays_or_immediate_folder_children_without_examples() {
        let home = TestHome::new("qoder-skills"); let root = home.home.join("example");
        write(&root, AUTHORED, r#"{"name":"example","version":"1","skills":"./skills/"}"#);
        for path in ["skills/one/SKILL.md", "skills/.hidden/SKILL.md", "skills/one/nested/SKILL.md", "case-studies/example/SKILL.md"] { write(&root, path, "private component body"); }
        let (plugin, unavailable) = components(&root); assert!(!unavailable); assert_eq!(plugin.components.len(), 1);
        write(&root, AUTHORED, r#"{"name":"example","version":"1","skills":["./case-studies/example"]}"#);
        let (plugin, unavailable) = components(&root); assert!(!unavailable); assert_eq!(plugin.components.len(), 1);
        assert_eq!(plugin.components[0].name, "case-studies/example/SKILL.md");
        assert!(!serde_json::to_string(&plugin).unwrap().contains("private component body"));
    }

    #[test]
    fn first_existing_mcp_blocks_fallback_and_keeps_native_identifiers_without_values() {
        let home = TestHome::new("qoder-mcp"); let (root, _) = install(&home, "example", "example", "1");
        write(&root, "mcp.json", "malformed");
        write(&root, ".mcp.json", r#"{"mcpServers":{"not-selected":{"command":"private-command"}}}"#);
        let (plugin, unavailable) = components(&root); assert!(unavailable); assert!(plugin.components.is_empty());
        write(&root, "mcp.json", r#"{"mcpServers":{"one":{"command":"private-command","env":{"TOKEN":"private-token"},"identifier":"native.one"},"two":{"url":"https://private.invalid/token"},"empty":{}}}"#);
        let (plugin, unavailable) = components(&root); assert!(!unavailable); assert_eq!(plugin.components.len(), 2);
        assert_eq!(plugin.components[0].native_id.as_deref(), Some("native.one"));
        assert_eq!(plugin.components[1].native_id.as_deref(), Some("example@market:two"));
        let serialized = serde_json::to_string(&plugin).unwrap();
        for secret in ["private-command","private-token","private.invalid","not-selected"] { assert!(!serialized.contains(secret)); }
    }

    #[test]
    fn projected_component_hints_stay_inert_and_hooks_export_only_event_names() {
        let home = TestHome::new("qoder-projection"); let (root, _) = install(&home, "example", "example", "1");
        write(&root, "plugin.json", r#"{"name":"example","version":"1","skills":["./samples/hidden"],"mcpServers":"./other.json"}"#);
        write(&root, "samples/hidden/SKILL.md", "private skill body");
        write(&root, "other.json", r#"{"mcpServers":{"wrong":{"command":"never"}}}"#);
        write(&root, AUTHORED, r#"{"name":"example","version":"1","hooks":"./events.json","commands":{"named":{"source":"./named.md"}}}"#);
        write(&root, "named.md", "private command body");
        write(&root, "events.json", r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"private handler"}]}],"Empty":[]}}"#);
        let (plugin, unavailable) = components(&root); assert!(!unavailable);
        assert_eq!(plugin.components.iter().map(|c| (&c.kind, c.name.as_str())).collect::<Vec<_>>(), vec![(&PluginComponentKind::Command,"named"),(&PluginComponentKind::Hook,"SessionStart")]);
        assert_eq!(plugin.source_paths.len(), 2);
        let serialized = serde_json::to_string(&plugin).unwrap(); assert!(!serialized.contains("private")); assert!(!serialized.contains("wrong"));
    }

    #[test]
    #[cfg(unix)]
    fn symlinks_and_escaping_component_paths_never_read_target_metadata() {
        let home = TestHome::new("qoder-confined"); let (root, _) = install(&home, "example", "example", "1");
        let outside = home.home.join("outside"); write(&outside, "mcp.json", r#"{"mcpServers":{"escaped":{"command":"secret"}}}"#);
        std::os::unix::fs::symlink(outside.join("mcp.json"), root.join("mcp.json")).unwrap();
        let (plugin, unavailable) = components(&root); assert!(unavailable); assert!(plugin.components.is_empty());
        fs::remove_file(root.join("mcp.json")).unwrap();
        write(&root, AUTHORED, r#"{"name":"example","version":"1","mcpServers":"../outside/mcp.json"}"#);
        let (plugin, unavailable) = components(&root); assert!(unavailable); assert!(plugin.components.is_empty());
    }

    #[test]
    fn computer_use_archive_contributes_metadata_without_extraction_or_component_bodies() {
        let home = TestHome::new("qoder-archive"); let path = home.home.join("plugin.zip");
        zip_file(&path, &[("computer-use/.qoder-plugin/plugin.json",r#"{"name":"computer-use","version":"1","skills":"./skills/","mcpServers":"./.mcp.json"}"#),("computer-use/.mcp.json",r#"{"mcpServers":{"computer-use":{"command":"private executable"}}}"#),("computer-use/skills/computer-use/SKILL.md","private skill body")]);
        let mut result = PluginScan::default(); archive(host(), &path, &mut result).unwrap(); normalize(&mut result);
        assert!(!result.unavailable); assert_eq!(result.plugins.len(), 1); assert_eq!(result.mcps.len(), 1);
        assert_eq!(result.plugins[0].presence, PluginPresence::Bundled); assert_eq!(result.plugins[0].components.len(), 2);
        assert!(result.plugins[0].components.iter().all(|c| c.source_path.contains("plugin.zip!/computer-use/")));
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private")); assert!(!home.home.join("computer-use").exists());
    }

    #[test]
    fn archive_rejects_an_escaping_member_before_observing_a_plugin() {
        let home = TestHome::new("qoder-archive-path"); let path = home.home.join("plugin.zip");
        zip_file(&path, &[("computer-use/../escape.json","{}")]);
        let mut result = PluginScan::default(); assert!(archive(host(), &path, &mut result).is_err()); assert!(result.plugins.is_empty());
        assert!(!home.home.join("escape.json").exists());
    }

    #[test]
    fn registered_external_packages_are_watched_without_scanning_their_parents() {
        let home = TestHome::new("qoder-watch"); let (root, record) = install(&home, "explicit-local-copy", "example", "1");
        register(&home, "installed_plugins_v2.json", json!({"example@market":[record]}));
        let paths = watch_paths(host()); assert!(paths.contains(&root)); assert!(!paths.contains(&home.home));
    }
}
