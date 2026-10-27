//! Codex's installed version resolver and manifest formats are independent of Cursor.
use super::*;
use std::cmp::Ordering;

const AGENT_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const AGENT_SCHEMA_PREFIX: &str = "https://agent-plugins.org/schemas/";

pub(super) fn cache_root(source: &Root) -> PathBuf {
    std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join("plugins/cache"))
        .unwrap_or_else(|| expand_tilde(&source.path))
}

fn compare_versions(a: &str, b: &str) -> Ordering {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(a), Ok(b)) => a.cmp(&b), _ => a.cmp(b),
    }
}

fn active_version(paths: Vec<PathBuf>) -> Result<Option<PathBuf>, ()> {
    let paths: Vec<_> = paths.into_iter().filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name|
        !name.is_empty() && !matches!(name, "." | "..") && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
    )).collect();
    if let Some(local) = paths.iter().find(|path| path.file_name().is_some_and(|name| name == "local")) { return Ok(Some(local.clone())); }
    let Some(candidate) = paths.iter().max_by(|a, b| compare_versions(a.file_name().unwrap().to_str().unwrap(), b.file_name().unwrap().to_str().unwrap())) else { return Ok(None); };
    // Mixed semver/string ordering may be non-transitive. Refuse ambiguous
    // maxima rather than guess a selected installation or panic in sorting.
    if paths.iter().any(|path| compare_versions(candidate.file_name().unwrap().to_str().unwrap(), path.file_name().unwrap().to_str().unwrap()).is_lt()) { return Err(()); }
    Ok(Some(candidate.clone()))
}

fn manifest(host: &Host, root: &Path, result: &mut PluginScan) -> Result<(String, Arc<Value>, bool), ()> {
    let root_path = relative(root, "plugin.json")?;
    if fs::symlink_metadata(&root_path).is_ok_and(|m| !m.file_type().is_file()) { return Err(()); }
    match optional_json(root, "plugin.json", &mut result.budget) {
        Ok(Some(value)) => match value.get("$schema").and_then(Value::as_str) {
            Some(AGENT_SCHEMA) => return Ok(("plugin.json".into(), value, true)),
            Some(schema) if schema.starts_with(AGENT_SCHEMA_PREFIX) => return Err(()),
            _ => {},
        },
        Err(()) => result.unavailable = true,
        _ => {},
    }
    for source in &host.manifest_paths {
        if let Some(value) = optional_json(root, source, &mut result.budget)? { return Ok((source.clone(), value, false)); }
    }
    Err(())
}

fn add_file(root: &Path, input: &str, kind: PluginComponentKind, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    let path = relative(root, input)?;
    checked_root(&path)?;
    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file()) { return Err(()); }
    if components.len() >= MAX_COMPONENTS { return Err(()); }
    components.push(PluginComponent { kind, name: metadata_name(input).ok_or(())?, source_path: collapse_home(&path), native_id: None });
    Ok(())
}

fn legacy_paths(value: Option<&Value>, default: &str) -> Result<Vec<String>, ()> {
    let mut paths = match value {
        None => vec![default.to_string()],
        Some(Value::String(path)) => vec![path.clone()],
        Some(Value::Array(paths)) if paths.len() <= 64 => paths.iter().map(|value| value.as_str().map(str::to_string).ok_or(())).collect::<Result<_, _>>()?,
        _ => return Err(()),
    };
    if paths.is_empty() { paths.push(default.to_string()); }
    Ok(paths)
}

fn mcp_components(root: &Path, source: &str, manifest: &Value, standard: bool,
    components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let mut names = BTreeMap::new();
    let declaration = if standard { None } else { manifest.get("mcpServers").filter(|value| !value.is_null()) };
    match declaration {
        Some(Value::Object(servers)) => {
            if servers.len() > MAX_COMPONENTS { return Err(()); }
            for (key, config) in servers {
                if !config.is_object() { return Err(()); }
                names.insert(metadata_name(key).ok_or(())?, relative(root, source)?);
            }
        },
        None | Some(Value::String(_)) => {
            let path = match declaration {
                Some(Value::String(path)) if path.starts_with("./") => path.as_str(),
                Some(_) => return Err(()),
                None if standard => "mcp.json",
                None => ".mcp.json",
            };
            if let Some(config) = optional_json(root, path, budget)? {
                for name in mcp_keys(&config)? { names.insert(name, relative(root, path)?); }
            } else if declaration.is_some() { return Err(()); }
        },
        _ => return Err(()),
    }
    for (name, path) in names {
        if components.len() >= MAX_COMPONENTS { return Err(()); }
        components.push(PluginComponent { kind: PluginComponentKind::Mcp, native_id: Some(name.clone()), name, source_path: collapse_home(&path) });
    }
    Ok(())
}

fn hooks(root: &Path, value: &Value, source: &str, depth: usize,
    components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    budget.visit()?;
    if depth > 8 { return Err(()); }
    match value {
        Value::String(path) if path.starts_with("./") => {
            if let Some(value) = optional_json(root, path, budget)? { hooks(root, &value, path, depth + 1, components, budget)?; }
        },
        Value::Array(parts) if parts.len() <= 64 => for part in parts { hooks(root, part, source, depth + 1, components, budget)?; },
        Value::Object(object) => {
            let events = object.get("hooks").unwrap_or(value).as_object().ok_or(())?;
            if events.len() > 64 { return Err(()); }
            for name in events.keys() {
                if components.len() >= MAX_COMPONENTS { return Err(()); }
                components.push(PluginComponent { kind: PluginComponentKind::Hook, name: metadata_name(name).ok_or(())?, source_path: collapse_home(&relative(root, source)?), native_id: None });
            }
        },
        _ => return Err(()),
    }
    Ok(())
}

fn observe_package(host: &Host, source: &Root, root: &Path, identity: &str, result: &mut PluginScan) -> Result<(), ()> {
    result.budget.visit()?;
    checked_root(root)?;
    let (manifest_path, manifest, standard) = manifest(host, root, result)?;
    let name = metadata_name(manifest.get("name").and_then(Value::as_str).ok_or(())?).ok_or(())?;
    if standard && (name.len() > 64 || name.contains("--") || name.contains("..") || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b)) || !name.as_bytes()[0].is_ascii_alphanumeric() || !name.as_bytes()[name.len()-1].is_ascii_alphanumeric()) { return Err(()); }
    let revision = root.file_name().and_then(|value| value.to_str()).and_then(metadata_name).ok_or(())?;
    let version = manifest.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| "unknown".into());
    let interface = if standard { manifest.get("extensions").and_then(|value| value.get("com.openai")).and_then(|value| value.get("interface")) } else { manifest.get("interface") };
    let display_name = interface.and_then(|value| value.get("displayName")).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| name.clone());
    let mut components = Vec::new();
    if mcp_components(root, &manifest_path, &manifest, standard, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    let skill_paths = if standard { Ok(vec!["./skills".to_string()]) } else { legacy_paths(manifest.get("skills").filter(|value| !value.is_null()), "./skills") };
    match skill_paths {
        Ok(paths) => for input in paths {
            if !input.starts_with("./") { result.unavailable = true; continue; }
            let observed = (|| {
                let path = relative(root, &input)?;
                if standard {
                    for directory in dirs(&path, &mut result.budget)? {
                        let skill = directory.join("SKILL.md");
                        if skill.try_exists().map_err(|_| ())? {
                            let input = skill.strip_prefix(root).map_err(|_| ())?.to_string_lossy();
                            if add_file(root, &input, PluginComponentKind::Skill, &mut components).is_err() { result.unavailable = true; }
                        }
                    }
                } else if path.is_file() { add_file(root, &input, PluginComponentKind::Skill, &mut components)?; }
                else { component_files(root, &path, &PluginComponentKind::Skill, 0, &mut components, &mut result.budget)?; }
                Ok::<(), ()>(())
            })();
            if observed.is_err() { result.unavailable = true; }
        },
        Err(()) => result.unavailable = true,
    }
    if !standard {
        let migrated = root.join(".codex-plugin/migrated-command-skills");
        if component_files(root, &migrated, &PluginComponentKind::Skill, 0, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    }
    let extension = if standard { manifest.get("extensions").and_then(|value| value.get("com.openai")) } else { Some(manifest.as_ref()) };
    let hook_value = extension.and_then(|value| value.get("hooks")).filter(|value| !value.is_null()).cloned().unwrap_or(Value::String("./hooks/hooks.json".into()));
    if hooks(root, &hook_value, &manifest_path, 0, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    let app_paths = match extension.and_then(|value| value.get("apps")) {
        None | Some(Value::Null) => Ok(vec!["./.app.json".to_string()]),
        Some(Value::String(path)) if path.starts_with("./") => Ok(vec![path.clone()]),
        _ => Err(()),
    };
    match app_paths {
        Ok(paths) => for path in paths {
            let observed = (|| {
                if let Some(config) = optional_json(root, &path, &mut result.budget)? {
                    let apps = config.get("apps").and_then(Value::as_object).ok_or(())?;
                    if apps.len() > MAX_COMPONENTS { return Err(()); }
                    for name in apps.keys() {
                        if components.len() >= MAX_COMPONENTS { return Err(()); }
                        components.push(PluginComponent { kind: PluginComponentKind::App, name: metadata_name(name).ok_or(())?, source_path: collapse_home(&relative(root, &path)?), native_id: None });
                    }
                }
                Ok::<(), ()>(())
            })();
            if observed.is_err() { result.unavailable = true; }
        },
        Err(()) => result.unavailable = true,
    }
    if let Some(path) = extension.and_then(|value| value.get("onboardingSkill")).and_then(Value::as_str) {
        if add_file(root, path, PluginComponentKind::Skill, &mut components).is_err() { result.unavailable = true; }
    }
    components.sort(); components.dedup();
    result.plugins.push(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: identity.into(), name: display_name,
        version, revision, presence: source.presence.clone(), enabled: None,
        source_paths: vec![collapse_home(&relative(root, &manifest_path)?)], components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() });
    Ok(())
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    for source in &host.roots {
        if source.layout != "codex_active_cache" { result.unavailable = true; continue; }
        let markets = match dirs(&cache_root(source), &mut result.budget) { Ok(paths) => paths, Err(()) => { result.unavailable = true; continue; } };
        for market in markets {
            let packages = match dirs(&market, &mut result.budget) { Ok(paths) => paths, Err(()) => { result.unavailable = true; continue; } };
            for package in packages {
                let observed = (|| {
                    let Some(root) = active_version(dirs(&package, &mut result.budget)?)? else { return Ok(()); };
                    let identity = format!("{}@{}", package.file_name().and_then(|name| name.to_str()).and_then(metadata_name).ok_or(())?, market.file_name().and_then(|name| name.to_str()).and_then(metadata_name).ok_or(())?);
                    observe_package(host, source, &root, &identity, result)
                })();
                if observed.is_err() { result.unavailable = true; }
                if result.budget.visited > MAX_SCAN_ENTRIES || result.budget.references > MAX_REFERENCES || result.budget.bytes >= MAX_READ_BYTES { break; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "codex").unwrap() }
    fn package(home: &TestHome) -> PathBuf {
        let root = home.home.join("fixture");
        fs::create_dir_all(root.join(".codex-plugin")).unwrap();
        root
    }

    #[test]
    fn native_active_version_uses_local_then_semver_then_string_without_exposing_old_versions() {
        let paths = |names: &[&str]| names.iter().map(|name| PathBuf::from(name)).collect();
        assert_eq!(active_version(paths(&["2.0.0", "10.0.0"])).unwrap(), Some(PathBuf::from("10.0.0")));
        assert_eq!(active_version(paths(&["local", "99.0.0"])).unwrap(), Some(PathBuf::from("local")));
        assert_eq!(active_version(paths(&["abc", "xyz"])).unwrap(), Some(PathBuf::from("xyz")));
        assert!(active_version(paths(&["bad version"])).unwrap().is_none());
    }

    #[test]
    fn legacy_components_follow_codex_paths_and_do_not_export_executable_configuration() {
        let home = TestHome::new("codex-plugin-components");
        let root = package(&home);
        fs::write(root.join(".codex-plugin/plugin.json"), r#"{"name":"fixture","version":"1.0.0","skills":"./skills","mcpServers":"./selected.json","apps":"./.app.json"}"#).unwrap();
        fs::write(root.join(".mcp.json"), r#"{"mcpServers":{"ignored":{}}}"#).unwrap();
        fs::write(root.join("selected.json"), r#"{"mcpServers":{"selected":{"command":"SECRET_SENTINEL","env":{"TOKEN":"SECRET_SENTINEL"}}}}"#).unwrap();
        fs::write(root.join(".app.json"), r#"{"apps":{"mail":{"id":"PRIVATE_CONFIG_SENTINEL"}}}"#).unwrap();
        for directory in ["skills/fixture", "agents", "rules", "hooks"] { fs::create_dir_all(root.join(directory)).unwrap(); }
        fs::write(root.join("skills/fixture/SKILL.md"), "# Skill").unwrap();
        fs::write(root.join("agents/ignored.md"), "# Not a Codex plugin component").unwrap();
        fs::write(root.join("rules/ignored.md"), "# Not a Codex plugin component").unwrap();
        fs::write(root.join("hooks/hooks.json"), r#"{"hooks":{"SessionStart":[{"command":"SECRET_SENTINEL"}]}}"#).unwrap();
        let mut scan = PluginScan::default();
        observe_package(host(), &host().roots[0], &root, "fixture@market", &mut scan).unwrap();
        normalize(&mut scan);
        assert!(!scan.unavailable);
        assert_eq!(scan.plugins.len(), 1);
        assert_eq!(scan.mcps.len(), 1);
        assert_eq!(scan.mcps[0].server_id, "selected");
        let components = &scan.plugins[0].components;
        assert!(components.iter().any(|component| component.kind == PluginComponentKind::App && component.name == "mail"));
        assert!(components.iter().any(|component| component.kind == PluginComponentKind::Hook && component.name == "SessionStart"));
        assert!(!components.iter().any(|component| matches!(component.kind, PluginComponentKind::Agent | PluginComponentKind::Rule)));
        let output = serde_json::to_string(&scan.plugins).unwrap();
        assert!(!output.contains("SECRET_SENTINEL") && !output.contains("PRIVATE_CONFIG_SENTINEL"));
        assert_eq!(scan.plugins[0].presence, PluginPresence::Installed);
        assert_eq!(scan.plugins[0].enabled, None);
    }

    #[test]
    fn standard_schema_is_fixed_and_unsupported_schema_does_not_fall_through() {
        let home = TestHome::new("codex-standard-plugin");
        let root = package(&home);
        fs::write(root.join(".codex-plugin/plugin.json"), r#"{"name":"legacy","version":"1.0.0"}"#).unwrap();
        let manifest = |schema: &str| serde_json::json!({"$schema":schema,"name":"standard","version":"1.0.0","skills":"../outside","mcpServers":{"ignored":{}}});
        fs::write(root.join("plugin.json"), serde_json::to_vec(&manifest(AGENT_SCHEMA)).unwrap()).unwrap();
        fs::write(root.join("mcp.json"), r#"{"mcpServers":{"standard":{}}}"#).unwrap();
        for path in ["skills/direct", "skills/nested/deeper"] { fs::create_dir_all(root.join(path)).unwrap(); fs::write(root.join(path).join("SKILL.md"), "# Fixture").unwrap(); }
        let mut scan = PluginScan::default();
        observe_package(host(), &host().roots[0], &root, "fixture@market", &mut scan).unwrap();
        normalize(&mut scan);
        assert!(!scan.unavailable);
        assert_eq!(scan.mcps[0].name, "standard");
        assert_eq!(scan.plugins[0].components.iter().filter(|component| component.kind == PluginComponentKind::Skill).count(), 1);
        fs::write(root.join("plugin.json"), serde_json::to_vec(&manifest("https://agent-plugins.org/schemas/2.0.0/plugin.schema.json")).unwrap()).unwrap();
        let mut scan = PluginScan::default();
        assert!(observe_package(host(), &host().roots[0], &root, "fixture@market", &mut scan).is_err());
        assert!(scan.plugins.is_empty());
    }
}
