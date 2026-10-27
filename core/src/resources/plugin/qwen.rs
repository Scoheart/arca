//! Qwen Code extension packages and packaged Skills, observed without importing
//! code, reading component bodies or invoking native store recovery/conversion.
use super::*;

const MANIFEST: &str = "qwen-extension.json";
const METADATA: &str = ".qwen-extension-install.json";
const SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const SCHEMA_PREFIX: &str = "https://agent-plugins.org/schemas/";

fn extensions_root(source: &Root) -> PathBuf {
    match std::env::var_os("QWEN_HOME").filter(|value| !value.is_empty()) {
        Some(value) if value == "~" => expand_tilde("~/").join("extensions"),
        Some(value) => expand_tilde(&value.to_string_lossy()).join("extensions"),
        None => expand_tilde(&source.path),
    }
}

fn name(value: &str) -> Result<String, ()> {
    if value != value.trim() || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) { return Err(()); }
    metadata_name(value).ok_or(())
}

fn standard_name(value: &str) -> bool {
    name(value).is_ok() && value.len() <= 64 && !value.as_bytes().windows(2).any(|pair| pair.iter().all(|b| b".-".contains(b)))
        && value.as_bytes()[0].is_ascii_alphanumeric() && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
        && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b))
}

fn stable_path(root: &Path, input: &str) -> Result<PathBuf, ()> {
    let input = input.replace("${extensionPath}", root.to_str().ok_or(())?)
        .replace("${CLAUDE_PLUGIN_ROOT}", root.to_str().ok_or(())?)
        .replace("${/}", "/").replace("${pathSeparator}", "/");
    if input.contains('$') { return Err(()); }
    let input = Path::new(&input);
    let input = if input.is_absolute() { input.strip_prefix(root).map_err(|_| ())? } else { input };
    let path = relative(root, input.to_str().ok_or(())?)?; checked_root(&path)?; Ok(path)
}

fn add(kind: PluginComponentKind, label: &str, path: &Path, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if components.len() >= MAX_COMPONENTS { return Err(()); }
    components.push(PluginComponent { kind, name: metadata_name(label).ok_or(())?, source_path: collapse_home(path), native_id: None }); Ok(())
}

fn add_file(root: &Path, path: &Path, kind: PluginComponentKind, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if regular_metadata_file(path)? {
        add(kind, &path.strip_prefix(root).map_err(|_| ())?.to_string_lossy(), path, components)?;
    }
    Ok(())
}

fn package_root(installation: &Path, budget: &mut ScanBudget) -> Result<(PathBuf, Option<Arc<Value>>), ()> {
    checked_root(installation)?;
    let metadata = optional_json(installation, METADATA, budget)?;
    let Some(value) = &metadata else { return Ok((installation.to_path_buf(), metadata)); };
    if !value.is_object() { return Err(()); }
    if value.get("type").and_then(Value::as_str) != Some("link") { return Ok((installation.to_path_buf(), metadata)); }
    let path = PathBuf::from(value.get("source").and_then(Value::as_str).ok_or(())?);
    if !path.is_absolute() || expand_tilde("~/").starts_with(&path) { return Err(()); }
    checked_root(&path)?;
    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_dir()) { return Err(()); }
    Ok((path, metadata))
}

fn manifest(root: &Path, budget: &mut ScanBudget) -> Result<(String, Arc<Value>, bool), ()> {
    if let Some(value) = optional_json(root, "plugin.json", budget)? {
        match value.get("$schema").and_then(Value::as_str) {
            Some(SCHEMA) => return Ok(("plugin.json".into(), value, true)),
            Some(schema) if schema.starts_with(SCHEMA_PREFIX) => return Err(()),
            _ => {},
        }
    }
    let value = optional_json(root, MANIFEST, budget)?.ok_or(())?; Ok((MANIFEST.into(), value, false))
}

fn skills(root: &Path, folder: &Path, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    for directory in dirs(folder, budget)? { add_file(root, &directory.join("SKILL.md"), PluginComponentKind::Skill, components)?; }
    Ok(())
}

fn commands(root: &Path, folder: &Path, depth: usize, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if depth > 8 { return Err(()); }
    for entry in directory_entries(folder, budget)? {
        if entry.directory { commands(root, &entry.path, depth + 1, components, budget)?; }
        else if entry.file && entry.path.extension().is_some_and(|ext| ext == "md") {
            if !regular_metadata_file(&entry.path)? { return Err(()); }
            let relative = entry.path.strip_prefix(root.join("commands")).map_err(|_| ())?;
            let label = relative.with_extension("").components().map(|part| part.as_os_str().to_string_lossy().replace(':', "_")).collect::<Vec<_>>().join(":");
            add(PluginComponentKind::Command, &label, &entry.path, components)?;
        } else if !entry.file { return Err(()); }
    }
    Ok(())
}

fn mcp(value: &Value, path: &Path, standard: bool, components: &mut Vec<PluginComponent>, unavailable: &mut bool) -> Result<(), ()> {
    let servers = value.as_object().filter(|servers| servers.len() <= MAX_COMPONENTS).ok_or(())?;
    for (key, config) in servers {
        if metadata_name(key).is_none() || matches!(key.as_str(), "__proto__" | "constructor" | "prototype")
            || !config.is_object() || (standard && !standard_mcp_declaration(config)) { *unavailable = true; continue; }
        add(PluginComponentKind::Mcp, key, path, components)?; components.last_mut().ok_or(())?.native_id = Some(key.clone());
    }
    Ok(())
}

fn hook_events(value: &Value, path: &Path, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    let events = value.as_object().filter(|events| events.len() <= 64).ok_or(())?;
    for (event, definitions) in events {
        if !definitions.is_array() { return Err(()); }
        add(PluginComponentKind::Hook, event, path, components)?;
    }
    Ok(())
}

fn hooks(root: &Path, manifest: &Value, source: &str, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let declared = manifest.get("hooks");
    if let Some(Value::Object(_)) = declared { return hook_events(declared.ok_or(())?, &relative(root, source)?, components); }
    let mut path = root.join("hooks/hooks.json");
    match declared {
        Some(Value::String(input)) => { let custom = stable_path(root, input)?; if regular_metadata_file(&custom)? { path = custom; } },
        None | Some(Value::Null | Value::Bool(false)) => {}, _ => return Err(()),
    }
    if let Some(value) = optional_json(root, path.strip_prefix(root).map_err(|_| ())?.to_str().ok_or(())?, budget)? { hook_events(value.get("hooks").unwrap_or(&value), &path, components)?; }
    Ok(())
}

fn workflows(root: &Path, value: Option<&Value>, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let inputs = match value {
        None | Some(Value::Null) => vec!["workflows"], Some(Value::String(input)) => vec![input.as_str()],
        Some(Value::Array(inputs)) if inputs.len() <= 64 => inputs.iter().map(|input| input.as_str().ok_or(())).collect::<Result<_, _>>()?,
        _ => return Err(()),
    };
    for input in inputs {
        let path = stable_path(root, input)?;
        if value.is_some_and(|value| !value.is_null()) && !path.try_exists().map_err(|_| ())? { return Err(()); }
        if regular_metadata_file(&path)? {
            if path.extension().is_none_or(|ext| ext != "js") { return Err(()); }
            add_file(root, &path, PluginComponentKind::Workflow, components)?;
        } else {
            for entry in directory_entries(&path, budget)? {
                if entry.file && entry.path.extension().is_some_and(|ext| ext == "js") { add_file(root, &entry.path, PluginComponentKind::Workflow, components)?; }
                else if !entry.file && !entry.directory { return Err(()); }
            }
        }
    }
    Ok(())
}

fn observe_package(host: &Host, installation: &Path, enablement: bool, result: &mut PluginScan) -> Result<NativePluginObservation, ()> {
    result.budget.visit()?;
    let (root, metadata) = package_root(installation, &mut result.budget)?;
    let (source, manifest, standard) = manifest(&root, &mut result.budget)?;
    let name = name(manifest.get("name").and_then(Value::as_str).ok_or(())?)?;
    if standard && !standard_name(&name) { return Err(()); }
    let raw_version = manifest.get("version").filter(|value| !value.is_null() && value.as_str() != Some(""))
        .or_else(|| metadata.as_ref().and_then(|metadata| metadata.get("marketplaceConfig")).and_then(|config| config.get("metadata")).and_then(|metadata| metadata.get("version")).filter(|value| value.as_str() != Some("")));
    let version = if standard {
        match manifest.get("version") {
            None => "1.0.0".into(), Some(Value::String(value)) if value.trim().is_empty() => "1.0.0".into(),
            Some(Value::String(value)) => metadata_name(value).filter(|value| !value.contains('$')).ok_or(())?, _ => return Err(()),
        }
    } else { match raw_version { None => "1.0.0".into(), Some(value) => metadata_name(value.as_str().ok_or(())?).filter(|value| !value.contains('$')).ok_or(())? } };
    let mut components = Vec::new();
    if skills(&root, &root.join("skills"), &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    if standard {
        let observed = (|| {
            if let Some(value) = optional_json(&root, "mcp.json", &mut result.budget)? {
                if value.get("$schema").and_then(Value::as_str) != Some("https://agent-plugins.org/schemas/1.0.0/mcp.schema.json")
                    || !value.as_object().is_some_and(|object| object.keys().all(|key| ["$schema", "mcpServers"].contains(&key.as_str()))) { return Err(()); }
                mcp(value.get("mcpServers").ok_or(())?, &root.join("mcp.json"), true, &mut components, &mut result.unavailable)?;
            }
            Ok::<(), ()>(())
        })(); if observed.is_err() { result.unavailable = true; }
    } else {
        if let Some(value) = manifest.get("mcpServers").filter(|value| !value.is_null()) {
            if mcp(value, &root.join(&source), false, &mut components, &mut result.unavailable).is_err() { result.unavailable = true; }
        }
        let contexts = match manifest.get("contextFileName") {
            None | Some(Value::Null) => Ok(vec!["QWEN.md"]), Some(Value::String(value)) if value.is_empty() => Ok(vec!["QWEN.md"]),
            Some(Value::String(value)) => Ok(vec![value.as_str()]), Some(Value::Array(values)) if values.is_empty() => Ok(vec!["QWEN.md"]),
            Some(Value::Array(values)) if values.len() <= 64 => values.iter().map(|value| value.as_str().ok_or(())).collect(), _ => Err(()),
        };
        match contexts {
            Ok(inputs) => for input in inputs { if stable_path(&root, input).and_then(|path| add_file(&root, &path, PluginComponentKind::Context, &mut components)).is_err() { result.unavailable = true; } },
            Err(()) => result.unavailable = true,
        }
        if commands(&root, &root.join("commands"), 0, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
        match directory_entries(&root.join("agents"), &mut result.budget) {
            Ok(entries) => for entry in entries {
                if entry.file && entry.path.extension().is_some_and(|ext| ext == "md") {
                    if add_file(&root, &entry.path, PluginComponentKind::Agent, &mut components).is_err() { result.unavailable = true; }
                } else if !entry.file && !entry.directory { result.unavailable = true; }
            }, Err(()) => result.unavailable = true,
        }
        if hooks(&root, &manifest, &source, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
        if workflows(&root, manifest.get("workflows"), &mut components, &mut result.budget).is_err() { result.unavailable = true; }
        if let Some(channels) = manifest.get("channels").filter(|value| !value.is_null()) {
            if let Some(channels) = channels.as_object().filter(|channels| channels.len() <= MAX_COMPONENTS) {
                for (channel, config) in channels {
                    let observed = (|| {
                        let input = config.get("entry").and_then(Value::as_str).ok_or(())?;
                        let path = stable_path(&root, input)?;
                        if !regular_metadata_file(&path)? { return Err(()); }
                        add(PluginComponentKind::Channel, channel, &path, &mut components)
                    })(); if observed.is_err() { result.unavailable = true; }
                }
            } else { result.unavailable = true; }
        }
    }
    let mut source_paths = vec![collapse_home(&root.join(&source))];
    if metadata.is_some() { source_paths.push(collapse_home(&installation.join(METADATA))); }
    if enablement { source_paths.push(collapse_home(&installation.parent().ok_or(())?.join("extension-enablement.json"))); }
    components.sort(); components.dedup(); source_paths.sort(); source_paths.dedup();
    Ok(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: name.clone(), name, version,
        revision: collapse_home(installation), presence: PluginPresence::Installed, enabled: None,
        source_paths, components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() })
}

fn bundled_root(source: &Root, budget: &mut ScanBudget) -> Result<Option<PathBuf>, ()> {
    let Some(command) = crate::application::agent_launch::find_command(&source.path) else { return Ok(None); };
    // Resolve only the selected launcher link; package/component reads remain
    // confined to its physical package, with no arbitrary symlink traversal.
    let executable = fs::canonicalize(command).map_err(|_| ())?;
    if !matches!(executable.file_name().and_then(|name| name.to_str()), Some("cli.js" | "cli-entry.js")) { return Err(()); }
    let root = executable.parent().ok_or(())?; checked_root(root)?;
    let package = optional_json(root, "package.json", budget)?.ok_or(())?;
    if package.get("name").and_then(Value::as_str) != Some("@qwen-code/qwen-code") { return Err(()); }
    let bin = package.get("bin").and_then(|value| value.get("qwen")).and_then(Value::as_str).ok_or(())?;
    if relative(root, bin)? != executable || !regular_metadata_file(&executable)? { return Err(()); }
    Ok(Some(root.to_path_buf()))
}

fn bundled(host: &Host, source: &Root, result: &mut PluginScan) -> Result<(), ()> {
    let Some(root) = bundled_root(source, &mut result.budget)? else { return Ok(()); };
    let mut components = Vec::new(); skills(&root, &root.join("bundled"), &mut components, &mut result.budget)?;
    if components.is_empty() { return Ok(()); }
    let package = optional_json(&root, "package.json", &mut result.budget)?.ok_or(())?;
    let version = package.get("version").and_then(Value::as_str).filter(|value| semver::Version::parse(value).is_ok()).ok_or(())?;
    result.plugins.push(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: "@qwen-code/qwen-code:bundled-skills".into(),
        name: "Qwen Code bundled Skills".into(), version: version.into(), revision: collapse_home(&root), presence: PluginPresence::Bundled, enabled: None,
        source_paths: vec![collapse_home(&root.join("package.json"))], components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() }); Ok(())
}

fn store_declarations(host: &Host, extensions: &Path, observed: &BTreeSet<String>, result: &mut PluginScan) -> Result<(), ()> {
    let config = extensions.parent().ok_or(())?;
    let Some(state) = optional_json(config, "extension-store/state.json", &mut result.budget)? else { return Ok(()); };
    if !valid_store(&state) { return Err(()); }
    let policies = state.get("extensions").and_then(Value::as_object).filter(|policies| policies.len() <= MAX_ENTRIES).ok_or(())?;
    for (identity, policy) in policies {
        if identity.len() != 64 || !identity.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { result.unavailable = true; continue; }
        let policy_name = match policy.get("name").and_then(Value::as_str).and_then(|value| name(value).ok()) { Some(name) => name, None => { result.unavailable = true; continue; } };
        if observed.contains(&policy_name) { continue; }
        let declared = policy.get("declarationOnly").and_then(Value::as_bool) == Some(true);
        if !declared { result.unavailable = true; }
        result.plugins.push(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: policy_name.clone(), name: policy_name,
            version: "unknown".into(), revision: format!("state:{identity}"), presence: PluginPresence::Declared, enabled: None,
            source_paths: vec![collapse_home(&config.join("extension-store/state.json"))], components: Vec::new(), docs_url: host.docs_url.clone(), source_fingerprint: String::new() });
    }
    Ok(())
}

fn valid_store(state: &Value) -> bool {
    let hash = |value: &str| value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let safe_number = |value: &Value| value.as_u64().is_some_and(|value| value <= 9_007_199_254_740_991);
    let projection = |value: &Value| value.as_object().is_some_and(|entries| entries.len() <= MAX_ENTRIES && entries.values().all(|entry|
        entry.get("overrides").and_then(Value::as_array).is_some_and(|rules| rules.len() <= MAX_ENTRIES && rules.iter().all(Value::is_string))));
    state.get("version").and_then(Value::as_u64) == Some(2)
        && state.get("generation").is_some_and(safe_number)
        && state.get("legacyProjectionHash").and_then(Value::as_str).is_some_and(hash)
        && state.get("legacyProjectionRemainder").is_none_or(projection)
        && state.get("extensions").and_then(Value::as_object).is_some_and(|policies| policies.len() <= MAX_ENTRIES && policies.iter().all(|(identity, policy)| {
            hash(identity) && policy.get("name").and_then(Value::as_str).is_some_and(|value| name(value).is_ok())
                && policy.get("artifactDirectory").is_none_or(|value| value.as_str().is_some_and(|value| !matches!(value, "." | "..") && name(value).is_ok()))
                && policy.get("artifactGeneration").is_none_or(safe_number)
                && policy.get("declarationOnly").is_none_or(|value| value.as_bool() == Some(true))
                && policy.get("preserveActivationOnNextInstall").is_none_or(|value| value.as_bool() == Some(true))
                && matches!(policy.get("defaultActivation").and_then(Value::as_str), Some("enabled" | "disabled"))
                && policy.get("workspaceOverrides").and_then(Value::as_object).is_some_and(|overrides| overrides.len() <= MAX_ENTRIES && overrides.values().all(|value| matches!(value.as_str(), Some("enabled" | "disabled" | "inherit"))))
                && policy.get("skillWorkspaceOverrides").is_none_or(|value| value.as_object().is_some_and(|workspaces| workspaces.len() <= MAX_ENTRIES && workspaces.values().all(|value|
                    value.as_object().is_some_and(|states| states.len() <= MAX_ENTRIES && states.values().all(Value::is_boolean)))))
                && policy.get("legacyPathRules").is_none_or(|value| value.as_array().is_some_and(|rules| rules.len() <= MAX_ENTRIES && rules.iter().all(Value::is_string)))
        }))
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut roots = BTreeSet::new(); let mut budget = ScanBudget::default();
    for source in &host.roots {
        if source.layout == "qwen_bundled_skills" {
            if let Some(launcher) = crate::application::agent_launch::find_command(&source.path) { roots.insert(launcher); }
            if let Ok(Some(root)) = bundled_root(source, &mut budget) { roots.insert(root.join("bundled")); roots.insert(root.join("package.json")); }
        } else if source.layout == "qwen_extensions" {
            let path = extensions_root(source); roots.insert(path.clone());
            if let Some(config) = path.parent() { roots.insert(config.join("extension-store/state.json")); }
            if let Ok(installations) = dirs(&path, &mut budget) {
                for installation in installations { if let Ok((root, _)) = package_root(&installation, &mut budget) { if !expand_tilde("~/").starts_with(&root) { roots.insert(root); } } }
            }
        }
    }
    roots.into_iter().filter(|root| root.is_absolute()).collect()
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    for source in &host.roots {
        if source.layout == "qwen_bundled_skills" { if bundled(host, source, result).is_err() { result.unavailable = true; } continue; }
        if source.layout != "qwen_extensions" { result.unavailable = true; continue; }
        let extensions = extensions_root(source);
        if !extensions.is_absolute() || checked_root(&extensions).is_err() { result.unavailable = true; continue; }
        let enablement = match optional_json(&extensions, "extension-enablement.json", &mut result.budget) {
            Ok(Some(value)) => {
                let valid = value.as_object().is_some_and(|entries| entries.len() <= MAX_ENTRIES && entries.values().all(|entry|
                    entry.get("overrides").and_then(Value::as_array).is_some_and(|rules| rules.len() <= MAX_ENTRIES && rules.iter().all(Value::is_string))));
                if !valid { result.unavailable = true; } true
            }, Ok(None) => false, Err(()) => { result.unavailable = true; false },
        };
        let mut selected = BTreeMap::<String, Option<NativePluginObservation>>::new();
        match directory_entries(&extensions, &mut result.budget) {
            Ok(entries) => for entry in entries {
                if !entry.directory { if !entry.file { result.unavailable = true; } continue; }
                match observe_package(host, &entry.path, enablement, result) {
                    Ok(plugin) => match selected.entry(plugin.plugin_id.clone()) {
                        std::collections::btree_map::Entry::Vacant(entry) => { entry.insert(Some(plugin)); },
                        std::collections::btree_map::Entry::Occupied(mut entry) => { entry.insert(None); result.unavailable = true; },
                    }, Err(()) => result.unavailable = true,
                }
                if result.budget.visited > MAX_SCAN_ENTRIES || result.budget.references > MAX_REFERENCES || result.budget.bytes >= MAX_READ_BYTES { break; }
            }, Err(()) => result.unavailable = true,
        }
        let observed = selected.iter().filter(|(_, value)| value.is_some()).map(|(key, _)| key.clone()).collect(); result.plugins.extend(selected.into_values().flatten());
        if store_declarations(host, &extensions, &observed, result).is_err() { result.unavailable = true; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "qwen-code").unwrap() }
    fn write(root: &Path, input: &str, body: &str) {
        let path = root.join(input); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, body).unwrap();
    }
    fn extension(home: &TestHome, directory: &str, manifest: &str) -> PathBuf {
        let root = home.home.join(".qwen/extensions").join(directory); write(&root, MANIFEST, manifest); root
    }
    fn scan() -> PluginScan { let mut scan = PluginScan::default(); observe(host(), &mut scan); normalize(&mut scan); scan }

    #[test]
    fn qwen_component_depths_and_inline_hook_precedence_preserve_metadata_only_boundary() {
        let home = TestHome::new("qwen-components");
        let root = extension(&home, "fixture", r#"{"name":"fixture","mcpServers":{"server":{"command":"SECRET_SENTINEL"}},"hooks":{"SessionStart":[{"command":"SECRET_SENTINEL"}]},"channels":{"chat":{"entry":"./channel.js","token":"SECRET_SENTINEL"}}}"#);
        for input in ["QWEN.md","skills/one/SKILL.md","skills/deep/nested/SKILL.md","agents/_one.md","agents/nested/ignored.md","commands/.hidden.md","commands/nested/a:b.md","workflows/one.js","workflows/nested/ignored.js","channel.js"] { write(&root, input, "SECRET_SENTINEL"); }
        write(&root, "hooks/hooks.json", r#"{"hooks":{"must-not-load":[{}]}}"#);
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 1); assert_eq!(result.mcps.len(), 1);
        let plugin = &result.plugins[0]; assert_eq!(plugin.version, "1.0.0"); assert_eq!(plugin.enabled, None);
        for kind in [PluginComponentKind::Skill,PluginComponentKind::Agent,PluginComponentKind::Workflow,PluginComponentKind::Channel,PluginComponentKind::Hook,PluginComponentKind::Context] { assert_eq!(plugin.components.iter().filter(|c| c.kind == kind).count(), 1); }
        assert!(plugin.components.iter().any(|c| c.kind == PluginComponentKind::Command && c.name == "nested:a_b"));
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("SECRET_SENTINEL"));
    }

    #[test]
    fn agent_plugins_only_contribute_portable_components_and_unknown_versions_do_not_fall_back() {
        let home = TestHome::new("qwen-standard"); let root = extension(&home, "fixture", r#"{"name":"legacy","mcpServers":{"ignored":{}}}"#);
        write(&root, "plugin.json", &serde_json::json!({"$schema":SCHEMA,"name":"fixture","hooks":{"Ignored":[]},"extensions":{"com.github.copilot":{}}}).to_string());
        write(&root, METADATA, r#"{"marketplaceConfig":{"metadata":{"version":"9.9.9"}}}"#);
        write(&root, "mcp.json", r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"server":{"type":"stdio","command":"SECRET_SENTINEL"}}}"#);
        for input in ["skills/one/SKILL.md","skills/deep/nested/SKILL.md","commands/ignored.md","agents/ignored.md","workflows/ignored.js"] { write(&root, input, "SECRET_SENTINEL"); }
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins[0].name, "fixture"); assert_eq!(result.plugins[0].version, "1.0.0");
        assert_eq!(result.plugins[0].components.len(), 2); assert_eq!(result.mcps[0].server_id, "server");
        write(&root, "plugin.json", r#"{"$schema":"https://agent-plugins.org/schemas/1.1.0/plugin.schema.json","name":"fixture"}"#);
        let result = scan(); assert!(result.unavailable && result.plugins.is_empty() && result.mcps.is_empty());
    }

    #[test]
    fn linked_packages_use_registered_source_and_fallback_versions_without_exporting_source_credentials() {
        let home = TestHome::new("qwen-link"); let stub = extension(&home, "stub", r#"{"name":"stale"}"#); let root = home.home.join("linked");
        write(&root, MANIFEST, r#"{"name":"linked","contextFileName":"${extensionPath}${/}QWEN.md"}"#); write(&root, "QWEN.md", "SECRET_SENTINEL");
        write(&stub, METADATA, &serde_json::json!({"type":"link","source":root,"private":"SECRET_SENTINEL","marketplaceConfig":{"metadata":{"version":"2.3.4"}}}).to_string());
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins[0].name, "linked"); assert_eq!(result.plugins[0].version, "2.3.4");
        assert!(watch_paths(host()).contains(&root)); assert!(!serde_json::to_string(&result.plugins).unwrap().contains("SECRET_SENTINEL"));
        write(&root, MANIFEST, r#"{"name":"linked","contextFileName":"../private.md"}"#);
        assert!(scan().unavailable);
    }

    #[test]
    fn current_store_declarations_are_not_installed_or_activated_and_workspace_values_stay_private() {
        let home = TestHome::new("qwen-store"); let config = home.home.join(".qwen"); let id = "a".repeat(64);
        let state = serde_json::json!({"version":2,"generation":1,"legacyProjectionHash":"b".repeat(64),"extensions":{
            id:{"name":"pending","declarationOnly":true,"defaultActivation":"enabled","workspaceOverrides":{"SECRET_SENTINEL":"disabled"}}
        }});
        write(&config, "extension-store/state.json", &state.to_string());
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 1); assert_eq!(result.plugins[0].presence, PluginPresence::Declared); assert_eq!(result.plugins[0].enabled, None);
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("SECRET_SENTINEL"));
        write(&config, "extension-store/state.json", r#"{"version":2,"generation":1,"extensions":{}}"#);
        let result = scan(); assert!(result.unavailable && result.plugins.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn bundled_skills_come_from_selected_launcher_package_and_no_code_or_markdown_is_executed() {
        use std::os::unix::fs::PermissionsExt;
        let home = TestHome::new("qwen-bundled"); let root = home.home.join("qwen-package");
        write(&root, "package.json", r#"{"name":"@qwen-code/qwen-code","version":"0.26.2","bin":{"qwen":"cli-entry.js"}}"#);
        write(&root, "cli-entry.js", "SECRET_SENTINEL"); fs::set_permissions(root.join("cli-entry.js"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(root.join("cli-entry.js"), home.home.join("bin/qwen")).unwrap();
        write(&root, "bundled/one/SKILL.md", "SECRET_SENTINEL"); write(&root, "bundled/deep/nested/SKILL.md", "SECRET_SENTINEL");
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 1); assert_eq!(result.plugins[0].presence, PluginPresence::Bundled); assert_eq!(result.plugins[0].version, "0.26.2");
        assert_eq!(result.plugins[0].components.len(), 1); assert!(!serde_json::to_string(&result.plugins).unwrap().contains("SECRET_SENTINEL"));
        std::os::unix::fs::symlink(home.home.join("outside"), root.join("bundled/escape")).unwrap();
        write(&root, "package.json", r#"{"name":"different-package","version":"0.26.2","bin":{"qwen":"cli-entry.js"}}"#);
        assert!(scan().plugins.is_empty());
    }
}
