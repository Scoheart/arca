//! Shipped Gemini CLI 0.46 extension layout. File declarations are not loaded
//! tools: trust, workspace/CLI overrides and native parsers remain host-owned.
use super::*;

const MANIFEST: &str = "gemini-extension.json";
const METADATA: &str = ".gemini-extension-install.json";

fn extensions_root(source: &Root) -> PathBuf {
    std::env::var_os("GEMINI_CLI_HOME").filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join(".gemini/extensions"))
        .unwrap_or_else(|| expand_tilde(&source.path))
}

fn package_root(installation: &Path, budget: &mut ScanBudget) -> Result<(PathBuf, bool), ()> {
    checked_root(installation)?;
    let Some(metadata) = optional_json(installation, METADATA, budget)? else { return Ok((installation.to_path_buf(), false)); };
    if !metadata.is_object() { return Err(()); }
    if metadata.get("type").and_then(Value::as_str) != Some("link") { return Ok((installation.to_path_buf(), true)); }
    let source = PathBuf::from(metadata.get("source").and_then(Value::as_str).ok_or(())?);
    checked_root(&source)?;
    if !fs::symlink_metadata(&source).is_ok_and(|metadata| metadata.file_type().is_dir()) { return Err(()); }
    Ok((source, true))
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut budget = ScanBudget::default();
    let mut paths = BTreeSet::new(); let home = expand_tilde("~/");
    for source in &host.roots {
        let path = extensions_root(source); paths.insert(path.clone());
        if let Ok(installations) = dirs(&path, &mut budget) {
            for installation in installations {
                if let Ok((root, _)) = package_root(&installation, &mut budget) {
                    // Do not recursively subscribe to HOME or one of its ancestors.
                    if !home.starts_with(&root) { paths.insert(root); }
                }
            }
        }
    }
    paths.into_iter().collect()
}

fn add(kind: PluginComponentKind, name: &str, path: &Path, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if components.len() >= MAX_COMPONENTS { return Err(()); }
    components.push(PluginComponent { kind, name: metadata_name(name).ok_or(())?, source_path: collapse_home(path), native_id: None });
    Ok(())
}

fn file_declarations(root: &Path, folder: &Path, kind: PluginComponentKind, depth: usize,
    components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if depth > 8 { return Err(()); }
    let mut invalid = false;
    for entry in directory_entries(folder, budget)? {
        let name = entry.path.file_name().and_then(|name| name.to_str()).ok_or(())?;
        if !entry.directory && !entry.file {
            invalid |= fs::symlink_metadata(&entry.path).is_ok_and(|metadata| metadata.file_type().is_symlink()); continue;
        }
        if kind == PluginComponentKind::Skill && name.starts_with('.') { continue; }
        if entry.directory {
            if kind == PluginComponentKind::Command || (kind == PluginComponentKind::Skill && depth == 0) {
                invalid |= file_declarations(root, &entry.path, kind.clone(), depth + 1, components, budget).is_err();
            }
            continue;
        }
        let wanted = match kind {
            PluginComponentKind::Skill => name == "SKILL.md",
            PluginComponentKind::Agent => !name.starts_with('_') && name.ends_with(".md"),
            PluginComponentKind::Command | PluginComponentKind::Policy => name.ends_with(".toml"),
            _ => false,
        };
        if wanted {
            let label = entry.path.strip_prefix(root).map_err(|_| ())?.to_string_lossy();
            add(kind.clone(), &label, &entry.path, components)?;
        }
    }
    if invalid { Err(()) } else { Ok(()) }
}

fn observe_package(host: &Host, installation: &Path, enablement: bool, result: &mut PluginScan) -> Result<NativePluginObservation, ()> {
    result.budget.visit()?;
    let (root, has_metadata) = package_root(installation, &mut result.budget)?;
    let manifest = optional_json(&root, MANIFEST, &mut result.budget)?.ok_or(())?;
    let name = manifest.get("name").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    if manifest.get("name").and_then(Value::as_str) != Some(name.as_str()) || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { return Err(()); }
    let version = manifest.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    let manifest_path = root.join(MANIFEST);
    let mut components = Vec::new();
    if let Some(servers) = manifest.get("mcpServers") {
        if let Some(servers) = servers.as_object().filter(|servers| servers.len() <= MAX_COMPONENTS) {
            for (server, config) in servers {
                if matches!(server.as_str(), "__proto__" | "constructor" | "prototype") { continue; }
                if !config.is_object() { result.unavailable = true; continue; }
                add(PluginComponentKind::Mcp, server, &manifest_path, &mut components)?;
                components.last_mut().unwrap().native_id = Some(metadata_name(server).ok_or(())?);
            }
        } else { result.unavailable = true; }
    }
    let context = match manifest.get("contextFileName") {
        None | Some(Value::Null) => Ok(vec!["GEMINI.md"]),
        Some(Value::String(path)) if path.is_empty() => Ok(vec!["GEMINI.md"]),
        Some(Value::String(path)) => Ok(vec![path.as_str()]),
        Some(Value::Array(paths)) if paths.len() <= 64 => paths.iter().map(|value| value.as_str().ok_or(())).collect(),
        _ => Err(()),
    };
    match context {
        Ok(paths) => for input in paths {
            let outcome = (|| {
                // No arbitrary environment interpolation or workspace inference.
                if input.contains("${") { return Err(()); }
                let path = relative(&root, input)?; checked_root(&path)?;
                match fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.file_type().is_file() => add(PluginComponentKind::Context, input, &path, &mut components),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(()),
                }
            })();
            if outcome.is_err() { result.unavailable = true; }
        }, Err(()) => result.unavailable = true,
    }
    for (directory, kind) in [("skills", PluginComponentKind::Skill), ("agents", PluginComponentKind::Agent), ("commands", PluginComponentKind::Command), ("policies", PluginComponentKind::Policy)] {
        if file_declarations(&root, &root.join(directory), kind, 0, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    }
    match optional_json(&root, "hooks/hooks.json", &mut result.budget) {
        Ok(Some(config)) => if let Some(events) = config.get("hooks").and_then(Value::as_object).filter(|events| events.len() <= 64) {
            for (event, definitions) in events {
                if matches!(event.as_str(), "__proto__" | "constructor" | "prototype") { continue; }
                if !definitions.is_array() { result.unavailable = true; continue; }
                add(PluginComponentKind::Hook, event, &root.join("hooks/hooks.json"), &mut components)?;
            }
        } else { result.unavailable = true; },
        Ok(None) => {}, Err(()) => result.unavailable = true,
    }
    if let Some(themes) = manifest.get("themes") {
        if let Some(themes) = themes.as_array().filter(|themes| themes.len() <= MAX_COMPONENTS) {
            for theme in themes {
                if let Some(name) = theme.get("name").and_then(Value::as_str) { add(PluginComponentKind::Theme, name, &manifest_path, &mut components)?; }
                else { result.unavailable = true; }
            }
        } else { result.unavailable = true; }
    }
    if let Some(plan) = manifest.get("plan") {
        if plan.is_object() { add(PluginComponentKind::Plan, "Plan configuration", &manifest_path, &mut components)?; }
        else { result.unavailable = true; }
    }
    let mut source_paths = vec![collapse_home(&manifest_path)];
    if has_metadata { source_paths.push(collapse_home(&installation.join(METADATA))); }
    if enablement { source_paths.push(collapse_home(&installation.parent().ok_or(())?.join("extension-enablement.json"))); }
    components.sort(); components.dedup(); source_paths.sort(); source_paths.dedup();
    Ok(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: name.clone(), name,
        revision: version.clone(), version, presence: PluginPresence::Installed, enabled: None,
        source_paths, components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() })
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let mut selected = BTreeMap::<String, Option<NativePluginObservation>>::new();
    for source in &host.roots {
        if source.layout != "gemini_extensions" { result.unavailable = true; continue; }
        let root = extensions_root(source);
        let enablement = match optional_json(&root, "extension-enablement.json", &mut result.budget) {
            Ok(Some(value)) => {
                let valid = value.as_object().is_some_and(|entries| entries.len() <= MAX_ENTRIES && entries.values().all(|entry|
                    entry.get("overrides").and_then(Value::as_array).is_some_and(|rules| rules.len() <= MAX_ENTRIES && rules.iter().all(Value::is_string))));
                if !valid { result.unavailable = true; } valid
            }, Ok(None) => false, Err(()) => { result.unavailable = true; false },
        };
        match directory_entries(&root, &mut result.budget) {
            Ok(entries) => for entry in entries {
                if entry.directory {
                    match observe_package(host, &entry.path, enablement, result) {
                        Ok(plugin) => match selected.entry(plugin.plugin_id.clone()) {
                            std::collections::btree_map::Entry::Vacant(entry) => { entry.insert(Some(plugin)); },
                            std::collections::btree_map::Entry::Occupied(mut entry) => { entry.insert(None); result.unavailable = true; },
                        }, Err(()) => result.unavailable = true,
                    }
                } else if fs::symlink_metadata(&entry.path).is_ok_and(|metadata| metadata.file_type().is_symlink()) { result.unavailable = true; }
                if result.budget.visited > MAX_SCAN_ENTRIES || result.budget.references > MAX_REFERENCES || result.budget.bytes >= MAX_READ_BYTES { break; }
            }, Err(()) => result.unavailable = true,
        }
    }
    result.plugins.extend(selected.into_values().flatten());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "gemini").unwrap() }
    fn package(home: &TestHome, directory: &str, name: &str) -> PathBuf {
        let root = home.home.join(".gemini/extensions").join(directory);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(MANIFEST), serde_json::to_vec(&serde_json::json!({"name":name,"version":"1.0.0"})).unwrap()).unwrap(); root
    }
    #[test]
    fn declarations_follow_native_file_depth_and_never_export_bodies() {
        let home = TestHome::new("gemini-native-components");
        let root = package(&home, "fixture", "fixture");
        let manifest = serde_json::json!({"name":"fixture","version":"1.0.0","mcpServers":{"server":{"command":"SECRET_SENTINEL","env":{"TOKEN":"SECRET_SENTINEL"}},"constructor":{}},"themes":[{"name":"theme","description":"SECRET_SENTINEL"}],"plan":{"directory":"PRIVATE_CONFIG_SENTINEL"}});
        fs::write(root.join(MANIFEST), serde_json::to_vec(&manifest).unwrap()).unwrap();
        for directory in ["skills/one", "skills/nested/deeper", "agents/nested", "commands/nested", "policies/nested", "hooks"] { fs::create_dir_all(root.join(directory)).unwrap(); }
        for path in ["GEMINI.md", "CLAUDE.md", "skills/SKILL.md", "skills/one/SKILL.md", "skills/nested/deeper/SKILL.md", "agents/one.md", "agents/_ignored.md", "agents/nested/ignored.md", "commands/nested/one.toml", "commands/ignored.md", "policies/one.toml", "policies/nested/ignored.toml"] { fs::write(root.join(path), "SECRET_SENTINEL").unwrap(); }
        fs::write(root.join("hooks/hooks.json"), r#"{"hooks":{"BeforeTool":[{"command":"SECRET_SENTINEL"}]}}"#).unwrap();
        fs::write(root.join(METADATA), r#"{"type":"git","source":"https://SECRET_SENTINEL.invalid/repository"}"#).unwrap();
        let mut scan = PluginScan::default(); observe(host(), &mut scan); normalize(&mut scan);
        assert!(!scan.unavailable); assert_eq!(scan.plugins.len(), 1); assert_eq!(scan.mcps.len(), 1);
        let components = &scan.plugins[0].components;
        assert_eq!(components.iter().filter(|c| c.kind == PluginComponentKind::Skill).count(), 2);
        assert_eq!(components.iter().filter(|c| c.kind == PluginComponentKind::Agent).count(), 1);
        assert_eq!(components.iter().filter(|c| c.kind == PluginComponentKind::Command).count(), 1);
        assert_eq!(components.iter().filter(|c| c.kind == PluginComponentKind::Policy).count(), 1);
        assert_eq!(components.iter().filter(|c| c.kind == PluginComponentKind::Context).count(), 1);
        assert_eq!(scan.mcps[0].server_id, "server"); assert_eq!(scan.plugins[0].enabled, None);
        let output = serde_json::to_string(&scan.plugins).unwrap();
        assert!(!output.contains("SECRET_SENTINEL") && !output.contains("PRIVATE_CONFIG_SENTINEL"));
    }
    #[test]
    fn linked_installation_uses_metadata_target_instead_of_stale_stub_manifest() {
        let home = TestHome::new("gemini-linked-installation");
        let stub = package(&home, "stub", "stale");
        let target = home.home.join("linked"); fs::create_dir_all(&target).unwrap();
        fs::write(target.join(MANIFEST), r#"{"name":"linked","version":"2.0.0"}"#).unwrap();
        fs::write(stub.join(METADATA), serde_json::to_vec(&serde_json::json!({"type":"link","source":target})).unwrap()).unwrap();
        let mut scan = PluginScan::default(); observe(host(), &mut scan);
        assert!(!scan.unavailable); assert_eq!(scan.plugins[0].plugin_id, "linked"); assert_eq!(scan.plugins[0].version, "2.0.0");
        assert!(scan.plugins[0].source_paths.iter().any(|path| path.ends_with("stub/.gemini-extension-install.json")));
        assert!(watch_paths(host()).contains(&target));
        fs::remove_file(target.join(MANIFEST)).unwrap();
        let mut scan = PluginScan::default(); observe(host(), &mut scan);
        assert!(scan.unavailable && scan.plugins.is_empty());
    }
    #[test]
    fn duplicate_names_and_broken_enablement_never_become_guessed_active_plugins() {
        let home = TestHome::new("gemini-native-ambiguity");
        package(&home, "first", "fixture"); package(&home, "second", "fixture");
        let mut scan = PluginScan::default(); observe(host(), &mut scan);
        assert!(scan.unavailable && scan.plugins.is_empty());
        fs::remove_dir_all(home.home.join(".gemini/extensions/second")).unwrap();
        fs::write(home.home.join(".gemini/extensions/extension-enablement.json"), r#"{"fixture":{"overrides":false}}"#).unwrap();
        let mut scan = PluginScan::default(); observe(host(), &mut scan);
        assert!(scan.unavailable); assert_eq!(scan.plugins.len(), 1); assert_eq!(scan.plugins[0].enabled, None);
    }
    #[test]
    fn escaping_context_and_symlinks_are_diagnostics_and_not_configuration_reads() {
        let home = TestHome::new("gemini-native-containment");
        let root = package(&home, "fixture", "fixture");
        fs::write(root.join(MANIFEST), r#"{"name":"fixture","version":"1.0.0","contextFileName":"../outside","mcpServers":{"safe":{}}}"#).unwrap();
        let mut scan = PluginScan::default(); observe(host(), &mut scan); normalize(&mut scan);
        assert!(scan.unavailable); assert_eq!(scan.mcps.len(), 1);
        assert!(!scan.plugins[0].components.iter().any(|c| c.kind == PluginComponentKind::Context));
        #[cfg(unix)] {
            fs::create_dir_all(root.join("commands")).unwrap();
            std::os::unix::fs::symlink(home.home.join("outside"), root.join("commands/escape.toml")).unwrap();
            let mut scan = PluginScan::default(); observe(host(), &mut scan);
            assert!(scan.unavailable);
        }
    }
}
