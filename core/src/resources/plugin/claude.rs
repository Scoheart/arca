//! Claude Code user-scope installation ledger and declarative plugin components.
//! No package code, hook, command, monitor, server or bundle is executed.
use super::*;

type Names = BTreeMap<String, PathBuf>;

pub(super) fn config_root() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty())
        .map(PathBuf::from).unwrap_or_else(|| expand_tilde("~/.claude"))
}

pub(super) fn watch_paths() -> Vec<PathBuf> {
    let config = config_root();
    let mut roots = BTreeSet::from([config.join("plugins")]);
    let mut budget = ScanBudget::default();
    let Ok(Some(ledger)) = optional_json(&config, "plugins/installed_plugins.json", &mut budget) else { return roots.into_iter().collect(); };
    if ledger.get("version").and_then(Value::as_u64) != Some(2) { return roots.into_iter().collect(); }
    let Some(plugins) = ledger.get("plugins").and_then(Value::as_object).filter(|plugins| plugins.len() <= MAX_ENTRIES) else { return roots.into_iter().collect(); };
    let known = optional_json(&config, "plugins/known_marketplaces.json", &mut budget).ok().flatten();
    let home = expand_tilde("~/");
    let safe_root = |path: &Path| !config.starts_with(path) && !home.starts_with(path) && checked_root(path).is_ok();
    for (identity, records) in plugins {
        let Some((name, market)) = identity.rsplit_once('@') else { continue; };
        if namespace(name).is_err() || namespace(market).is_err() { continue; }
        let Some(records) = records.as_array().filter(|records| records.len() <= MAX_ENTRIES) else { continue; };
        let users: Vec<_> = records.iter().filter(|record| record.get("scope").and_then(Value::as_str) == Some("user")).collect();
        let [record] = users.as_slice() else { continue; };
        if !record.get("projectPath").is_none_or(Value::is_null) { continue; }
        if let Some(input) = record.get("installPath").and_then(Value::as_str) {
            let path = PathBuf::from(input); if safe_root(&path) { roots.insert(path); }
        }
        if let Some(input) = known.as_ref().and_then(|known| known.get(market)).and_then(|entry| entry.get("installLocation")).and_then(Value::as_str) {
            let path = PathBuf::from(input); if safe_root(&path) { roots.insert(path); }
        }
    }
    roots.into_iter().collect()
}

fn namespace(value: &str) -> Result<String, ()> {
    if value != value.trim() { return Err(()); }
    let name = metadata_name(value).ok_or(())?;
    if name.chars().any(|c| c.is_whitespace() || "@:/\\".contains(c) || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')) { return Err(()); }
    Ok(name)
}

fn component(kind: PluginComponentKind, name: &str, path: &Path, output: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if output.len() >= MAX_COMPONENTS { return Err(()); }
    output.push(PluginComponent { kind, name: metadata_name(name).ok_or(())?, source_path: collapse_home(path), native_id: None });
    Ok(())
}

fn declared_path(root: &Path, input: &str, skill: bool) -> Result<PathBuf, ()> {
    if !input.starts_with("./") && !(skill && input == ".") { return Err(()); }
    let path: PathBuf = relative(root, input)?.components().collect();
    checked_root(&path)?;
    if !path.try_exists().map_err(|_| ())? { return Err(()); }
    Ok(path)
}

fn paths(value: &Value) -> Result<Vec<&str>, ()> {
    match value {
        Value::String(value) => Ok(vec![value]),
        Value::Array(values) if values.len() <= 64 => values.iter().map(|value| value.as_str().ok_or(())).collect(),
        _ => Err(()),
    }
}

fn files(root: &Path, path: &Path, kind: PluginComponentKind, depth: usize,
    output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    budget.visit()?;
    if depth > 8 { return Err(()); }
    checked_root(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value, Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()), Err(_) => return Err(()),
    };
    if metadata.file_type().is_file() {
        let extension = path.extension().and_then(|value| value.to_str());
        let wanted = match kind {
            PluginComponentKind::Skill => path.file_name().is_some_and(|name| name == "SKILL.md"),
            PluginComponentKind::Workflow => extension == Some("js"),
            PluginComponentKind::Theme => extension == Some("json"),
            _ => extension == Some("md"),
        };
        if !wanted { return Err(()); }
        let name = path.strip_prefix(root).map_err(|_| ())?.to_string_lossy();
        return component(kind, &name, path, output);
    }
    if !metadata.file_type().is_dir() { return Err(()); }
    if kind == PluginComponentKind::Skill && fs::symlink_metadata(path.join("SKILL.md")).is_ok() {
        return files(root, &path.join("SKILL.md"), kind, depth + 1, output, budget);
    }
    for entry in directory_entries(path, budget)? {
        if entry.path.file_name().is_some_and(|name| matches!(name.to_str(), Some(".git" | "node_modules" | "target" | "dist" | "__pycache__"))) { continue; }
        if !entry.file && !entry.directory { continue; }
        if kind == PluginComponentKind::Skill {
            if entry.directory && fs::symlink_metadata(entry.path.join("SKILL.md")).is_ok() {
                files(root, &entry.path.join("SKILL.md"), kind.clone(), depth + 1, output, budget)?;
            }
        } else if entry.directory || match kind {
            PluginComponentKind::Workflow => entry.path.extension().is_some_and(|ext| ext == "js"),
            PluginComponentKind::Theme => entry.path.extension().is_some_and(|ext| ext == "json"),
            _ => entry.path.extension().is_some_and(|ext| ext == "md"),
        } { files(root, &entry.path, kind.clone(), depth + 1, output, budget)?; }
    }
    Ok(())
}

fn path_components(root: &Path, value: &Value, kind: PluginComponentKind, explicit_agents: bool,
    output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let mut invalid = false;
    for input in paths(value)? {
        let outcome = (|| {
            let path = declared_path(root, input, kind == PluginComponentKind::Skill)?;
            if explicit_agents && !fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) { return Err(()); }
            if kind == PluginComponentKind::Skill && !fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_dir()) { return Err(()); }
            files(root, &path, kind.clone(), 0, output, budget)
        })();
        invalid |= outcome.is_err();
    }
    if invalid { Err(()) } else { Ok(()) }
}

fn commands(root: &Path, value: &Value, source: &Path, output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let Some(map) = value.as_object() else { return path_components(root, value, PluginComponentKind::Command, false, output, budget); };
    if map.len() > MAX_COMPONENTS { return Err(()); }
    for (name, entry) in map {
        let source_value = entry.get("source"); let content = entry.get("content");
        if source_value.is_some() == content.is_some() { return Err(()); }
        let path = if let Some(input) = source_value {
            let path = declared_path(root, input.as_str().ok_or(())?, false)?;
            if !fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) || path.extension().is_none_or(|ext| ext != "md") { return Err(()); }
            path
        } else { if !content.is_some_and(Value::is_string) { return Err(()); } source.to_path_buf() };
        component(PluginComponentKind::Command, name, &path, output)?;
    }
    Ok(())
}

fn merge_map(value: &Value, source: &Path, wrapped: bool, wrapper: &str, output: &mut Names) -> Result<(), ()> {
    let object = if wrapped { value.get(wrapper).ok_or(())? } else { value }.as_object().ok_or(())?;
    if object.len() > MAX_COMPONENTS { return Err(()); }
    for (name, config) in object {
        if !config.is_object() { return Err(()); }
        output.insert(metadata_name(name).ok_or(())?, source.to_path_buf());
    }
    if output.len() > MAX_COMPONENTS { return Err(()); }
    Ok(())
}

fn merge_hooks(value: &Value, source: &Path, wrapped: bool, output: &mut Names) -> Result<(), ()> {
    let events = if wrapped { value.get("hooks").ok_or(())? } else { value }.as_object().ok_or(())?;
    if events.len() > 64 { return Err(()); }
    for (event, handlers) in events {
        if !handlers.is_array() { return Err(()); }
        output.insert(metadata_name(event).ok_or(())?, source.to_path_buf());
    }
    Ok(())
}

fn configs(root: &Path, value: &Value, source: &Path, kind: PluginComponentKind, depth: usize,
    names: &mut Names, output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    budget.visit()?;
    if depth > 8 { return Err(()); }
    match value {
        Value::Array(parts) if parts.len() <= 64 => {
            let mut invalid = false;
            for part in parts { invalid |= configs(root, part, source, kind.clone(), depth + 1, names, output, budget).is_err(); }
            if invalid { return Err(()); }
        },
        Value::String(input) => {
            if kind == PluginComponentKind::Mcp && (input.ends_with(".mcpb") || input.ends_with(".dxt")) {
                let remote = input.starts_with("https://");
                let path = if remote { source.to_path_buf() } else {
                    let path = declared_path(root, input, false)?;
                    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) { return Err(()); }
                    path
                };
                let label = if remote { "Remote MCP bundle" } else { Path::new(input).file_name().and_then(|name| name.to_str()).ok_or(())? };
                component(PluginComponentKind::McpBundle, label, &path, output)?;
                return Ok(());
            }
            let path = declared_path(root, input, false)?;
            if path.extension().is_none_or(|ext| ext != "json") { return Err(()); }
            let config = optional_json(root, input, budget)?.ok_or(())?;
            match kind {
                PluginComponentKind::Hook => merge_hooks(&config, &path, true, names)?,
                PluginComponentKind::Mcp => merge_map(&config, &path, true, "mcpServers", names)?,
                PluginComponentKind::Lsp => merge_map(&config, &path, false, "", names)?,
                _ => return Err(()),
            }
        },
        Value::Object(_) => if kind == PluginComponentKind::Hook { merge_hooks(value, source, false, names)?; }
            else { merge_map(value, source, false, "", names)?; },
        _ => return Err(()),
    }
    Ok(())
}

fn marketplace_entry(config: &Path, identity: &str, result: &mut PluginScan) -> Result<Option<(PathBuf, Value)>, ()> {
    let (name, market) = identity.rsplit_once('@').ok_or(())?;
    namespace(name)?; namespace(market)?;
    let Some(known) = optional_json(config, "plugins/known_marketplaces.json", &mut result.budget)? else { return Ok(None); };
    if !known.is_object() { return Err(()); }
    let Some(entry) = known.get(market) else { return Ok(None); };
    let root = PathBuf::from(entry.get("installLocation").and_then(Value::as_str).ok_or(())?);
    checked_root(&root)?;
    let Some(catalog) = optional_json(&root, ".claude-plugin/marketplace.json", &mut result.budget)? else { return Err(()); };
    let entries = catalog.get("plugins").and_then(Value::as_array).ok_or(())?;
    if entries.len() > MAX_ENTRIES { return Err(()); }
    let matches: Vec<_> = entries.iter().filter(|entry| entry.get("name").and_then(Value::as_str) == Some(name)).collect();
    match matches.as_slice() { [] => Ok(None), [entry] => Ok(Some((root.join(".claude-plugin/marketplace.json"), (*entry).clone()))), _ => Err(()) }
}

fn observe_package(host: &Host, config: &Path, identity: &str, record: &Value, result: &mut PluginScan) -> Result<(), ()> {
    result.budget.visit()?;
    let root = PathBuf::from(record.get("installPath").and_then(Value::as_str).ok_or(())?);
    checked_root(&root)?;
    if !fs::symlink_metadata(&root).is_ok_and(|m| m.file_type().is_dir()) { return Err(()); }
    let manifest_path = root.join(".claude-plugin/plugin.json");
    let disk_manifest = optional_json(&root, ".claude-plugin/plugin.json", &mut result.budget)?;
    let entry = match marketplace_entry(config, identity, result) { Ok(entry) => entry, Err(()) => { result.unavailable = true; None } };
    let fallback_name = identity.rsplit_once('@').ok_or(())?.0;
    let empty = serde_json::json!({"name": fallback_name});
    let (manifest, source) = if let Some(manifest) = &disk_manifest { (manifest.as_ref(), manifest_path.clone()) }
        else if let Some((source, entry)) = &entry { (entry, source.clone()) } else { (&empty, root.clone()) };
    if !manifest.is_object() { return Err(()); }
    let name = namespace(manifest.get("name").and_then(Value::as_str).ok_or(())?)?;
    let display_name = entry.as_ref().and_then(|(_, value)| value.get("displayName")).or_else(|| manifest.get("displayName")).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| name.clone());
    let version = manifest.get("version").or_else(|| record.get("version")).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| "unknown".into());
    let mut sources = vec![collapse_home(&config.join("plugins/installed_plugins.json")), collapse_home(&source)];
    if let Some((source, entry)) = &entry {
        sources.push(collapse_home(source));
        if disk_manifest.is_some() && entry.get("strict") == Some(&Value::Bool(false)) && ["commands", "agents", "skills", "hooks", "outputStyles", "themes"].iter().any(|key| entry.get(key).is_some()) { return Err(()); }
    }
    let mut components = Vec::new();
    // File defaults that are replaced by a corresponding declaration.
    for (key, directory, kind) in [("commands", "commands", PluginComponentKind::Command), ("agents", "agents", PluginComponentKind::Agent), ("outputStyles", "output-styles", PluginComponentKind::OutputStyle), ("workflows", "workflows", PluginComponentKind::Workflow), ("themes", "themes", PluginComponentKind::Theme)] {
        let value = if key == "themes" { manifest.get("experimental").and_then(|value| value.get(key)).or_else(|| manifest.get(key)) } else { manifest.get(key) };
        let outcome = if let Some(value) = value {
            if kind == PluginComponentKind::Command { commands(&root, value, &source, &mut components, &mut result.budget) }
            else { path_components(&root, value, kind.clone(), kind == PluginComponentKind::Agent, &mut components, &mut result.budget) }
        } else { files(&root, &root.join(directory), kind, 0, &mut components, &mut result.budget) };
        if outcome.is_err() { result.unavailable = true; }
    }
    let entry_root_skills = disk_manifest.is_none() && entry.as_ref().is_some_and(|(_, entry)| entry.get("source").and_then(Value::as_str).is_some_and(|path| matches!(path, "." | "./")) && entry.get("skills").is_some());
    if !entry_root_skills && files(&root, &root.join("skills"), PluginComponentKind::Skill, 0, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    if let Some(value) = manifest.get("skills") {
        if path_components(&root, value, PluginComponentKind::Skill, false, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    } else if !root.join("skills").exists() && root.join("SKILL.md").exists() && files(&root, &root.join("SKILL.md"), PluginComponentKind::Skill, 0, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    // Marketplace additions use paths in the selected installed package.
    if disk_manifest.is_some() {
        if let Some((entry_source, entry)) = &entry {
            for (key, kind) in [("commands", PluginComponentKind::Command), ("agents", PluginComponentKind::Agent), ("skills", PluginComponentKind::Skill), ("outputStyles", PluginComponentKind::OutputStyle), ("themes", PluginComponentKind::Theme)] {
                if let Some(value) = entry.get(key) {
                    let outcome = if kind == PluginComponentKind::Command { commands(&root, value, entry_source, &mut components, &mut result.budget) }
                        else { path_components(&root, value, kind.clone(), kind == PluginComponentKind::Agent, &mut components, &mut result.budget) };
                    if outcome.is_err() { result.unavailable = true; }
                }
            }
        }
    }
    for (key, default, kind, wrapped) in [("mcpServers", ".mcp.json", PluginComponentKind::Mcp, true), ("lspServers", ".lsp.json", PluginComponentKind::Lsp, false), ("hooks", "hooks/hooks.json", PluginComponentKind::Hook, true)] {
        let mut names = Names::new();
        match optional_json(&root, default, &mut result.budget) {
            Ok(Some(value)) => {
                let outcome = if kind == PluginComponentKind::Hook { merge_hooks(&value, &root.join(default), true, &mut names) }
                    else { merge_map(&value, &root.join(default), wrapped, key, &mut names) };
                if outcome.is_err() { result.unavailable = true; }
            }, Ok(None) => {}, Err(()) => result.unavailable = true,
        }
        if let Some(value) = manifest.get(key) {
            if (disk_manifest.is_none() && kind == PluginComponentKind::Hook && !value.is_object()) || configs(&root, value, &source, kind.clone(), 0, &mut names, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
        }
        if kind == PluginComponentKind::Hook && disk_manifest.is_some() {
            if let Some((path, entry)) = &entry {
                if let Some(hooks) = entry.get(key) { if merge_hooks(hooks, path, false, &mut names).is_err() { result.unavailable = true; } }
            }
        }
        for (server, path) in names {
            component(kind.clone(), &server, &path, &mut components)?;
            if kind == PluginComponentKind::Mcp { components.last_mut().unwrap().native_id = Some(format!("{}{name}:{server}", host.mcp_id_prefix)); }
        }
    }
    let monitors = manifest.get("experimental").and_then(|value| value.get("monitors")).or_else(|| manifest.get("monitors"));
    let monitor_result = (|| {
        let (values, path) = match monitors {
            Some(Value::Array(values)) => (Value::Array(values.clone()), source.clone()),
            Some(Value::String(input)) => { let path = declared_path(&root, input, false)?; (optional_json(&root, input, &mut result.budget)?.ok_or(())?.as_ref().clone(), path) },
            None => { let path = root.join("monitors/monitors.json"); let Some(values) = optional_json(&root, "monitors/monitors.json", &mut result.budget)? else { return Ok(()); }; (values.as_ref().clone(), path) },
            _ => return Err(()),
        };
        let values = values.as_array().ok_or(())?;
        if values.len() > MAX_COMPONENTS { return Err(()); }
        for value in values { component(PluginComponentKind::Monitor, value.get("name").and_then(Value::as_str).ok_or(())?, &path, &mut components)?; }
        Ok::<(), ()>(())
    })();
    if monitor_result.is_err() { result.unavailable = true; }
    if let Some(input) = manifest.get("types").and_then(Value::as_str) {
        match declared_path(&root, input, false) {
            Ok(path) if input.ends_with(".d.ts") && fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) => { component(PluginComponentKind::TypeDeclaration, input, &path, &mut components)?; },
            _ => result.unavailable = true,
        }
    }
    let eval_dir = manifest.get("experimental").and_then(|value| value.get("evals"));
    let eval_input = match eval_dir { Some(Value::String(input)) => Some(input.as_str()), Some(Value::Array(values)) => values.first().and_then(Value::as_str), None => Some("evals"), _ => { result.unavailable = true; None } };
    if let Some(input) = eval_input {
        match relative(&root, input).and_then(|path| { checked_root(&path)?; Ok(path) }) {
            Ok(path) if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_dir()) => { component(PluginComponentKind::Eval, input, &path, &mut components)?; },
            Ok(_) if eval_dir.is_none() => {}, _ => result.unavailable = true,
        }
    }
    if let Some(channels) = manifest.get("channels") {
        if let Some(channels) = channels.as_array().filter(|values| values.len() <= 64) {
            for channel in channels {
                if let Some(server) = channel.get("server").and_then(Value::as_str) { component(PluginComponentKind::Channel, server, &source, &mut components)?; }
                else { result.unavailable = true; }
            }
        } else { result.unavailable = true; }
    }
    match directory_entries(&root.join("bin"), &mut result.budget) {
        Ok(entries) => for entry in entries { if entry.file {
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; if !fs::symlink_metadata(&entry.path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0) { continue; } }
            component(PluginComponentKind::Executable, entry.path.file_name().and_then(|name| name.to_str()).ok_or(())?, &entry.path, &mut components)?;
        } }, Err(()) => result.unavailable = true,
    }
    components.sort(); components.dedup(); sources.sort(); sources.dedup();
    let revision = record.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| "installed".into());
    result.plugins.push(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: identity.into(), name: display_name, version, revision,
        presence: PluginPresence::Installed, enabled: None, source_paths: sources, components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() });
    Ok(())
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) { observe_registry(host, &config_root(), result); }

fn observe_registry(host: &Host, config: &Path, result: &mut PluginScan) {
    let ledger = match optional_json(&config, "plugins/installed_plugins.json", &mut result.budget) { Ok(Some(value)) => value, Ok(None) => return, Err(()) => { result.unavailable = true; return; } };
    if ledger.get("version").and_then(Value::as_u64) != Some(2) { result.unavailable = true; return; }
    let Some(plugins) = ledger.get("plugins").and_then(Value::as_object).filter(|values| values.len() <= MAX_ENTRIES) else { result.unavailable = true; return; };
    for (identity, records) in plugins {
        let observed = (|| {
            let (name, market) = identity.rsplit_once('@').ok_or(())?; namespace(name)?; namespace(market)?;
            let records = records.as_array().filter(|values| values.len() <= MAX_ENTRIES).ok_or(())?;
            let users: Vec<_> = records.iter().filter(|record| record.get("scope").and_then(Value::as_str) == Some("user")).collect();
            match users.as_slice() {
                [] => Ok(()),
                [record] if record.get("projectPath").is_none_or(Value::is_null) => observe_package(host, &config, identity, record, result),
                _ => Err(()),
            }
        })();
        if observed.is_err() { result.unavailable = true; }
        if result.budget.visited > MAX_SCAN_ENTRIES || result.budget.references > MAX_REFERENCES || result.budget.bytes >= MAX_READ_BYTES { break; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "claude-code").unwrap() }
    fn package(home: &TestHome, name: &str, manifest: Option<Value>) -> PathBuf {
        let root = home.home.join(name);
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        if let Some(manifest) = manifest { fs::write(root.join(".claude-plugin/plugin.json"), serde_json::to_vec(&manifest).unwrap()).unwrap(); }
        root
    }
    fn ledger(config: &Path, records: Value) {
        fs::create_dir_all(config.join("plugins")).unwrap();
        fs::write(config.join("plugins/installed_plugins.json"), serde_json::to_vec(&serde_json::json!({"version":2,"plugins":{"fixture@market":records}})).unwrap()).unwrap();
    }
    fn user(root: &Path) -> Value { serde_json::json!({"scope":"user","installPath":root,"version":"1.0.0"}) }

    #[test]
    fn registry_selects_exact_user_installation_and_refuses_ambiguous_scope() {
        let home = TestHome::new("claude-plugin-ledger");
        let selected = package(&home, "selected", None);
        let other = package(&home, "plugins/cache/market/fixture/99.0.0", Some(serde_json::json!({"name":"other"})));
        let mut project = user(&other); project["scope"] = Value::String("project".into());
        ledger(&home.home, serde_json::json!([user(&selected), project]));
        let mut scan = PluginScan::default(); observe_registry(host(), &home.home, &mut scan);
        assert!(!scan.unavailable);
        assert_eq!(scan.plugins.len(), 1);
        assert_eq!(scan.plugins[0].plugin_id, "fixture@market");
        assert_eq!(scan.plugins[0].version, "1.0.0");
        assert_eq!(scan.plugins[0].enabled, None);
        ledger(&home.home, serde_json::json!([user(&selected), user(&other)]));
        let mut scan = PluginScan::default(); observe_registry(host(), &home.home, &mut scan);
        assert!(scan.unavailable && scan.plugins.is_empty());
    }

    #[test]
    fn native_merge_replacement_namespace_and_secret_omission_are_preserved() {
        let home = TestHome::new("claude-plugin-components");
        let root = package(&home, "selected", Some(serde_json::json!({
            "name":"namespace","commands":{"about":{"content":"SECRET_SENTINEL"}},
            "mcpServers":["./extra.json",{"same":{"command":"SECRET_SENTINEL"}}],
            "hooks":{"SessionStart":[{"command":"SECRET_SENTINEL"}]},
            "agents":"./custom/reviewer.md","skills":"./custom/skill"
        })));
        for directory in ["commands", "agents", "custom/skill", "skills/base", "custom", "hooks"] { fs::create_dir_all(root.join(directory)).unwrap(); }
        for path in ["commands/ignored.md", "agents/ignored.md", "custom/reviewer.md", "custom/skill/SKILL.md", "skills/base/SKILL.md", "CLAUDE.md"] { fs::write(root.join(path), "SECRET_SENTINEL").unwrap(); }
        fs::write(root.join(".mcp.json"), r#"{"mcpServers":{"same":{"env":{"TOKEN":"SECRET_SENTINEL"}},"default":{}}}"#).unwrap();
        fs::write(root.join("extra.json"), r#"{"mcpServers":{"extra":{},"same":{}}}"#).unwrap();
        fs::write(root.join("hooks/hooks.json"), r#"{"hooks":{"Stop":[],"SessionStart":[]}}"#).unwrap();
        ledger(&home.home, serde_json::json!([user(&root)]));
        let mut scan = PluginScan::default(); observe_registry(host(), &home.home, &mut scan); normalize(&mut scan);
        assert!(!scan.unavailable);
        assert_eq!(scan.mcps.len(), 3);
        assert!(scan.mcps.iter().any(|mcp| mcp.server_id == "plugin:namespace:same" && mcp.source_paths[0].ends_with(".claude-plugin/plugin.json")));
        let components = &scan.plugins[0].components;
        assert_eq!(components.iter().filter(|c| c.kind == PluginComponentKind::Skill).count(), 2);
        assert_eq!(components.iter().filter(|c| c.kind == PluginComponentKind::Hook).count(), 2);
        assert!(!components.iter().any(|c| c.name.contains("ignored") || c.kind == PluginComponentKind::Rule));
        assert!(!serde_json::to_string(&scan.plugins).unwrap().contains("SECRET_SENTINEL"));
    }

    #[test]
    fn marketplace_entry_is_manifest_or_appends_components_with_strict_conflicts() {
        let home = TestHome::new("claude-plugin-marketplace");
        let root = package(&home, "selected", Some(serde_json::json!({"name":"namespace"})));
        let market = home.home.join("market"); fs::create_dir_all(market.join(".claude-plugin")).unwrap();
        fs::create_dir_all(root.join("extras")).unwrap(); fs::write(root.join("extras/entry.md"), "# Command").unwrap();
        ledger(&home.home, serde_json::json!([user(&root)]));
        fs::write(home.home.join("plugins/known_marketplaces.json"), serde_json::to_vec(&serde_json::json!({"market":{"installLocation":market}})).unwrap()).unwrap();
        let write_entry = |strict| fs::write(market.join(".claude-plugin/marketplace.json"), serde_json::to_vec(&serde_json::json!({"plugins":[{"name":"fixture","displayName":"Friendly","strict":strict,"commands":"./extras","mcpServers":{"entry-server":{}}}]})).unwrap()).unwrap();
        write_entry(true);
        let mut scan = PluginScan::default(); observe_registry(host(), &home.home, &mut scan); normalize(&mut scan);
        assert!(!scan.unavailable && scan.mcps.is_empty());
        assert_eq!(scan.plugins[0].name, "Friendly");
        assert_eq!(scan.plugins[0].components.iter().filter(|c| c.kind == PluginComponentKind::Command).count(), 1);
        write_entry(false);
        let mut scan = PluginScan::default(); observe_registry(host(), &home.home, &mut scan);
        assert!(scan.unavailable && scan.plugins.is_empty());
        fs::remove_file(root.join(".claude-plugin/plugin.json")).unwrap();
        let mut scan = PluginScan::default(); observe_registry(host(), &home.home, &mut scan); normalize(&mut scan);
        assert!(!scan.unavailable);
        assert_eq!(scan.mcps[0].server_id, "plugin:fixture:entry-server");
    }

    #[test]
    fn escaping_and_symlink_paths_are_diagnosed_without_reading_or_executing_them() {
        let home = TestHome::new("claude-plugin-containment");
        let root = package(&home, "selected", Some(serde_json::json!({"name":"fixture","skills":"./../outside","commands":{"safe":{"content":"SECRET_SENTINEL"}}})));
        ledger(&home.home, serde_json::json!([user(&root)]));
        let mut scan = PluginScan::default(); observe_registry(host(), &home.home, &mut scan);
        assert!(scan.unavailable);
        assert_eq!(scan.plugins[0].components.len(), 1);
        #[cfg(unix)] {
            let outside = package(&home, "outside", None);
            std::os::unix::fs::symlink(outside, root.join("escape")).unwrap();
            assert!(declared_path(&root, "./escape", true).is_err());
        }
    }
}
