//! Copilot CLI global installation records and materialized distribution metadata.
//! Native runtime helpers, gates, repository/managed overlays and parsers retain
//! ownership of activation. Executable/archive/component bodies are not read;
//! account fields in the installation-state document are never projected.
use super::*;

const SCHEMA_PREFIX: &str = "https://agent-plugins.org/schemas/";
const STANDARD_VERSIONS: [&str; 2] = ["1.0.0", "1.1.0"];
const BUILTIN_PLUGINS: [&str; 2] = ["browser-use", "computer-use"];

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn config_root(host: &Host) -> PathBuf {
    env_path("COPILOT_HOME").unwrap_or_else(|| expand_tilde(&host.roots[0].path))
}

fn package_caches() -> BTreeSet<PathBuf> {
    let home = expand_tilde("~/");
    let xdg = env_path("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache"));
    let platform = if cfg!(target_os = "macos") { home.join("Library/Caches/copilot") }
        else if cfg!(target_os = "windows") { env_path("LOCALAPPDATA").unwrap_or_else(|| home.join(".cache")).join("copilot") }
        else { xdg.join("copilot") };
    let mut roots = BTreeSet::from([platform.join("pkg"), xdg.join("copilot/pkg"), home.join(".copilot/pkg")]);
    for key in ["COPILOT_PKG_CACHE_HOME", "COPILOT_CACHE_HOME", "COPILOT_HOME"] {
        if let Some(path) = env_path(key) { roots.insert(path.join("pkg")); }
    }
    roots
}

fn native_platform() -> String {
    let os = match std::env::consts::OS { "macos" => "darwin", "windows" => "win32", os => os };
    let arch = match std::env::consts::ARCH { "aarch64" => "arm64", "x86_64" => "x64", arch => arch };
    format!("{os}-{arch}")
}

fn namespace(input: &str) -> Result<String, ()> {
    if input.is_empty() || input.len() > 128 || !input.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || !input.as_bytes()[0].is_ascii_alphanumeric() || !input.as_bytes()[input.len() - 1].is_ascii_alphanumeric() { return Err(()); }
    Ok(input.into())
}

fn standard_name(input: &str) -> bool {
    namespace(input).is_ok() && input.len() <= 64 && !input.contains("--") && !input.contains("..")
        && input.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b))
}

fn safe_root(path: &Path, config: &Path) -> Result<(), ()> {
    if !path.is_absolute() || expand_tilde("~/").starts_with(path) || config.starts_with(path) { return Err(()); }
    checked_root(path)
}

fn budget_available(budget: &ScanBudget) -> bool {
    budget.visited <= MAX_SCAN_ENTRIES && budget.references <= MAX_REFERENCES && budget.bytes < MAX_READ_BYTES
}

fn add(kind: PluginComponentKind, name: &str, path: &Path, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if components.len() >= MAX_COMPONENTS { return Err(()); }
    components.push(PluginComponent { kind, name: metadata_name(name).ok_or(())?, source_path: collapse_home(path), native_id: None });
    Ok(())
}

fn manifest(host: &Host, root: &Path, budget: &mut ScanBudget) -> Result<(String, Arc<Value>, Option<&'static str>), ()> {
    if let Some(value) = optional_json(root, "plugin.json", budget)? {
        if let Some(schema) = value.get("$schema").and_then(Value::as_str) {
            for version in STANDARD_VERSIONS {
                if schema == format!("{SCHEMA_PREFIX}{version}/plugin.schema.json") { return Ok(("plugin.json".into(), value, Some(version))); }
            }
            if schema.starts_with(SCHEMA_PREFIX) { return Err(()); }
        }
    }
    for input in &host.manifest_paths {
        if let Some(value) = optional_json(root, input, budget)? { return Ok((input.clone(), value, None)); }
    }
    Err(())
}

fn paths(value: &Value) -> Result<Vec<&str>, ()> {
    match value {
        Value::String(path) => Ok(vec![path]),
        Value::Array(paths) if paths.len() <= 64 => paths.iter().map(|path| path.as_str().ok_or(())).collect(),
        _ => Err(()),
    }
}

fn markdown_files(root: &Path, input: &str, kind: PluginComponentKind, depth: usize,
    components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if depth > 8 { return Err(()); }
    let path = relative(root, input)?;
    if regular_metadata_file(&path)? {
        let name = path.file_name().and_then(|name| name.to_str()).ok_or(())?;
        let wanted = if kind == PluginComponentKind::Agent { name.ends_with(".agent.md") } else { name.ends_with(".md") };
        return if wanted { add(kind, input, &path, components) } else { Ok(()) };
    }
    for entry in directory_entries(&path, budget)? {
        let name = entry.path.file_name().and_then(|name| name.to_str()).ok_or(())?;
        if name.starts_with('.') || matches!(name, "node_modules" | "target" | "dist" | "__pycache__") { continue; }
        if !entry.file && !entry.directory { return Err(()); }
        let input = entry.path.strip_prefix(root).map_err(|_| ())?.to_string_lossy();
        markdown_files(root, &input, kind.clone(), depth + 1, components, budget)?;
    }
    Ok(())
}

fn skills(root: &Path, input: &str, standard: bool, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let path = relative(root, input)?; checked_root(&path)?;
    if !standard && regular_metadata_file(&path)? {
        if path.file_name().is_none_or(|name| name != "SKILL.md") { return Err(()); }
        return add(PluginComponentKind::Skill, input, &path, components);
    }
    if !standard && regular_metadata_file(&path.join("SKILL.md"))? {
        return add(PluginComponentKind::Skill, &format!("{input}/SKILL.md"), &path.join("SKILL.md"), components);
    }
    for directory in dirs(&path, budget)? {
        let skill = directory.join("SKILL.md");
        if regular_metadata_file(&skill)? {
            let name = skill.strip_prefix(root).map_err(|_| ())?.to_string_lossy();
            add(PluginComponentKind::Skill, &name, &skill, components)?;
        }
    }
    Ok(())
}

fn servers(value: &Value, path: &Path, kind: PluginComponentKind, standard: bool,
    components: &mut Vec<PluginComponent>, unavailable: &mut bool) -> Result<(), ()> {
    let values = value.as_object().ok_or(())?;
    if values.len() > MAX_COMPONENTS { return Err(()); }
    for (name, config) in values {
        if metadata_name(name).is_none() || matches!(name.as_str(), "__proto__" | "constructor" | "prototype") { *unavailable = true; continue; }
        if !config.is_object() || (standard && !standard_mcp_declaration(config)) { *unavailable = true; continue; }
        add(kind.clone(), name, path, components)?;
        if kind == PluginComponentKind::Mcp { components.last_mut().ok_or(())?.native_id = Some(name.clone()); }
    }
    Ok(())
}

fn component_config(root: &Path, input: &str, key: &str, kind: PluginComponentKind, version: Option<&str>,
    components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    if let Some(value) = optional_json(root, input, &mut result.budget)? {
        if let Some(version) = version {
            let object = value.as_object().ok_or(())?;
            if value.get("$schema").and_then(Value::as_str) != Some(format!("{SCHEMA_PREFIX}{version}/mcp.schema.json").as_str())
                || object.keys().any(|key| !["$schema", "mcpServers"].contains(&key.as_str()))
                || !value.get("mcpServers").is_some_and(Value::is_object) { return Err(()); }
        }
        let definitions = value.get(key).unwrap_or(&value);
        servers(definitions, &relative(root, input)?, kind, version.is_some(), components, &mut result.unavailable)?;
    }
    Ok(())
}

fn named_config(root: &Path, source: &str, manifest: &Value, key: &str, defaults: &[&str], kind: PluginComponentKind,
    components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    match manifest.get(key) {
        Some(Value::String(input)) => {
            if optional_json(root, input, &mut result.budget)?.is_none() { return Err(()); }
            component_config(root, input, key, kind, None, components, result)
        },
        Some(value @ Value::Object(_)) => servers(value.get(key).unwrap_or(value), &relative(root, source)?, kind, false, components, &mut result.unavailable),
        None => {
            for input in defaults { component_config(root, input, key, kind.clone(), None, components, result)?; }
            Ok(())
        },
        _ => Err(()),
    }
}

fn hook_events(value: &Value, path: &Path, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    let events = value.get("hooks").unwrap_or(value).as_object().ok_or(())?;
    if events.len() > 64 { return Err(()); }
    for (name, definitions) in events {
        if !definitions.is_array() { return Err(()); }
        add(PluginComponentKind::Hook, name, path, components)?;
    }
    Ok(())
}

fn hooks(root: &Path, source: &str, value: Option<&Value>, defaults: &[&str], components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let inputs = match value {
        Some(Value::Object(_)) => return hook_events(value.ok_or(())?, &relative(root, source)?, components),
        Some(Value::String(input)) => vec![input.as_str()], None => defaults.to_vec(), _ => return Err(()),
    };
    for input in inputs {
        match optional_json(root, input, budget)? {
            Some(value) => hook_events(&value, &relative(root, input)?, components)?,
            None if value.is_some() => return Err(()), None => {},
        }
    }
    Ok(())
}

fn observe_package(host: &Host, root: &Path, identity: &str, presence: PluginPresence,
    result: &mut PluginScan) -> Result<NativePluginObservation, ()> {
    result.budget.visit()?; checked_root(root)?;
    let (source, manifest, standard) = manifest(host, root, &mut result.budget)?;
    let name = namespace(manifest.get("name").and_then(Value::as_str).ok_or(())?)?;
    if standard.is_some() && !standard_name(&name) { return Err(()); }
    let version = match manifest.get("version") {
        None => "unknown".into(), Some(Value::String(value)) => metadata_name(value).ok_or(())?, _ => return Err(()),
    };
    let mut components = Vec::new();
    let skill_paths = if standard.is_some() { Ok(vec!["skills"]) } else { manifest.get("skills").map(paths).unwrap_or(Ok(vec!["skills"])) };
    match skill_paths {
        Ok(paths) => for input in paths { if skills(root, input, standard.is_some(), &mut components, &mut result.budget).is_err() { result.unavailable = true; } },
        Err(()) => result.unavailable = true,
    }
    if standard.is_none() && manifest.get("skills").is_none() && !root.join("skills").try_exists().map_err(|_| ())? && regular_metadata_file(&root.join("SKILL.md"))? {
        add(PluginComponentKind::Skill, "SKILL.md", &root.join("SKILL.md"), &mut components)?;
    }
    for (key, default, kind) in [("agents", Some("agents"), PluginComponentKind::Agent), ("commands", None, PluginComponentKind::Command), ("rules", None, PluginComponentKind::Rule)] {
        let inputs = if standard.is_some() { Ok(vec![format!("com.github.copilot/{key}")]) }
            else if key == "rules" { Ok(Vec::new()) }
            else { manifest.get(key).map(paths).unwrap_or(Ok(default.into_iter().collect())).map(|paths| paths.into_iter().map(str::to_string).collect()) };
        match inputs {
            Ok(inputs) => for input in inputs { if markdown_files(root, &input, kind.clone(), 0, &mut components, &mut result.budget).is_err() { result.unavailable = true; } },
            Err(()) => result.unavailable = true,
        }
    }
    if let Some(version) = standard {
        if component_config(root, "mcp.json", "mcpServers", PluginComponentKind::Mcp, Some(version), &mut components, result).is_err() { result.unavailable = true; }
        if component_config(root, "com.github.copilot/lsp.json", "lspServers", PluginComponentKind::Lsp, None, &mut components, result).is_err() { result.unavailable = true; }
        if hooks(root, &source, None, &["com.github.copilot/hooks/hooks.json"], &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    } else {
        if named_config(root, &source, &manifest, "mcpServers", &host.default_mcp_files.iter().map(String::as_str).collect::<Vec<_>>(), PluginComponentKind::Mcp, &mut components, result).is_err() { result.unavailable = true; }
        if named_config(root, &source, &manifest, "lspServers", &["lsp.json", ".github/lsp.json", "lsp-config/servers.json"], PluginComponentKind::Lsp, &mut components, result).is_err() { result.unavailable = true; }
        if hooks(root, &source, manifest.get("hooks"), &["hooks.json", "hooks/hooks.json"], &mut components, &mut result.budget).is_err() { result.unavailable = true; }
        if let Some(value) = manifest.get("extensions") {
            let value = value.get("paths").unwrap_or(value);
            match paths(value) {
                Ok(paths) => for input in paths {
                    let outcome = (|| {
                        let path = relative(root, input)?; checked_root(&path)?;
                        if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_dir()) { return Err(()); }
                        add(PluginComponentKind::Extension, input, &path, &mut components)
                    })();
                    if outcome.is_err() { result.unavailable = true; }
                }, Err(()) => result.unavailable = true,
            }
        }
    }
    components.sort(); components.dedup();
    Ok(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: identity.into(), name, version,
        revision: collapse_home(root), presence, enabled: None,
        source_paths: vec![collapse_home(&relative(root, &source)?)], components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() })
}

fn installed_records(config: &Path, budget: &mut ScanBudget) -> Result<Vec<Value>, ()> {
    let Some(value) = optional_json(config, "config.json", budget)? else { return Ok(Vec::new()); };
    if !value.is_object() { return Err(()); }
    match value.get("installedPlugins") {
        None => Ok(Vec::new()),
        Some(Value::Array(records)) if records.len() <= MAX_ENTRIES => Ok(records.clone()),
        _ => Err(()),
    }
}

fn record_identity(record: &Value) -> Result<(String, String), ()> {
    let name = namespace(record.get("name").and_then(Value::as_str).ok_or(())?)?;
    let market = match record.get("marketplace") {
        None | Some(Value::Null) => None, Some(Value::String(value)) if value.is_empty() => None,
        Some(Value::String(value)) => Some(namespace(value)?), _ => return Err(()),
    };
    Ok((market.map(|market| format!("{name}@{market}")).unwrap_or_else(|| name.clone()), name))
}

fn record_root(record: &Value, config: &Path) -> Result<PathBuf, ()> {
    let input = record.get("host_provided_root").and_then(Value::as_str).filter(|input| !input.is_empty())
        .or_else(|| record.get("cache_path").and_then(Value::as_str)).ok_or(())?;
    let path = PathBuf::from(input); safe_root(&path, config)?; Ok(path)
}

fn declared(host: &Host, identity: &str, name: &str, version: &str, path: &Path) -> NativePluginObservation {
    NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: identity.into(), name: name.into(), version: version.into(),
        revision: collapse_home(path), presence: PluginPresence::Declared, enabled: None, source_paths: vec![collapse_home(path)],
        components: Vec::new(), docs_url: host.docs_url.clone(), source_fingerprint: String::new() }
}

fn installed(host: &Host, config: &Path, result: &mut PluginScan) -> BTreeSet<String> {
    let mut identities = BTreeSet::new();
    let records = match installed_records(config, &mut result.budget) { Ok(records) => records, Err(()) => { result.unavailable = true; return identities; } };
    let mut roots = BTreeMap::<PathBuf, Option<Value>>::new();
    for record in records {
        let (identity, name) = match record_identity(&record) { Ok(value) => value, Err(()) => { result.unavailable = true; continue; } };
        identities.insert(identity.clone());
        let root = match record_root(&record, config) {
            Ok(root) => root,
            Err(()) => {
                result.unavailable = true;
                result.plugins.push(declared(host, &identity, &name, "unknown", &config.join("config.json"))); continue;
            },
        };
        match roots.entry(root) {
            std::collections::btree_map::Entry::Vacant(entry) => { entry.insert(Some(record)); },
            std::collections::btree_map::Entry::Occupied(mut entry) => { entry.insert(None); result.unavailable = true; },
        }
    }
    for (root, record) in roots {
        if !budget_available(&result.budget) { result.unavailable = true; break; }
        let Some(record) = record else { continue; };
        let (identity, name) = record_identity(&record).expect("previously validated identity");
        match observe_package(host, &root, &identity, PluginPresence::Installed, result) {
            Ok(mut plugin) if plugin.name == name && record.get("version").is_none_or(|version| version.as_str() == Some(plugin.version.as_str())) => {
                plugin.source_paths.push(collapse_home(&config.join("config.json"))); result.plugins.push(plugin);
            },
            _ => {
                result.unavailable = true;
                result.plugins.push(declared(host, &identity, &name, "unknown", &config.join("config.json")));
            },
        }
    }
    identities
}

fn preferences(host: &Host, config: &Path, installed: &BTreeSet<String>, result: &mut PluginScan) -> Result<(), ()> {
    let Some(value) = optional_document(config, "settings.json", DocumentFormat::Jsonc, &mut result.budget)? else { return Ok(()); };
    if !value.is_object() { return Err(()); }
    let Some(plugins) = value.get("enabledPlugins") else { return Ok(()); };
    let plugins = plugins.as_object().filter(|plugins| plugins.len() <= MAX_ENTRIES).ok_or(())?;
    for (ordinal, (specifier, enabled)) in plugins.iter().enumerate() {
        if !enabled.is_boolean() { result.unavailable = true; continue; }
        let safe = if let Some((name, market)) = specifier.split_once('@') { namespace(name).and_then(|_| namespace(market)).map(|_| specifier.clone()) } else { namespace(specifier) };
        let identity = safe.unwrap_or_else(|_| {
            // A source specifier may carry URL credentials. Even its hash uses
            // only path/ordinal; no source, credentials or options escape.
            format!("configured-plugin-{}", &hex::encode(Sha256::digest(format!("{}:{ordinal}", config.display())))[..12])
        });
        if !installed.contains(&identity) { result.plugins.push(declared(host, &identity, &identity, "unknown", &config.join("settings.json"))); }
    }
    Ok(())
}

fn distribution(host: &Host, root: &Path, cache_version: Option<&str>, result: &mut PluginScan) -> Result<(), ()> {
    result.budget.visit()?; checked_root(root)?;
    if !root.try_exists().map_err(|_| ())? { return Ok(()); }
    if cache_version.is_some() && !regular_metadata_file(&root.join(".extraction-complete"))? { return Ok(()); }
    for input in ["index.js".to_string(), "app.js".into(), "cli-main.js".into(), format!("prebuilds/{}/runtime.node", native_platform())] {
        if !regular_metadata_file(&relative(root, &input)?)? { return Err(()); }
    }
    let package = optional_json(root, "package.json", &mut result.budget)?.ok_or(())?;
    let version = package.get("version").and_then(Value::as_str).ok_or(())?;
    if package.get("name").and_then(Value::as_str) != Some("@github/copilot") || semver::Version::parse(version).is_err()
        || cache_version.is_some_and(|expected| expected != version) { return Err(()); }
    for name in BUILTIN_PLUGINS {
        let path = relative(root, &format!("plugins/{name}"))?;
        if !path.try_exists().map_err(|_| ())? { continue; }
        let presence = if cache_version.is_some() { PluginPresence::CacheOnly } else { PluginPresence::Bundled };
        let mut plugin = observe_package(host, &path, name, presence, result)?;
        if plugin.name != name { return Err(()); }
        plugin.revision = format!("distribution:{}", collapse_home(root));
        plugin.source_paths.push(collapse_home(&root.join("package.json")));
        if cache_version.is_some() { plugin.source_paths.push(collapse_home(&root.join(".extraction-complete"))); }
        result.plugins.push(plugin);
    }
    Ok(())
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let config = config_root(host);
    let mut roots = package_caches();
    roots.extend([config.join("config.json"), config.join("settings.json"), config.join("installed-plugins")]);
    for key in ["COPILOT_CLI_DIST_DIR", "COPILOT_CLI_RESOLVED_DIST_DIR"] { if let Some(root) = env_path(key) { roots.insert(root); } }
    if let Ok(records) = installed_records(&config, &mut ScanBudget::default()) {
        for record in records { if let Ok(path) = record_root(&record, &config) { roots.insert(path); } }
    }
    roots.into_iter().filter(|root| safe_root(root, &config).is_ok()).collect()
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    if host.roots.len() != 1 || host.roots[0].layout != "copilot_global" { result.unavailable = true; return; }
    let config = config_root(host);
    if !config.is_absolute() || checked_root(&config).is_err() { result.unavailable = true; return; }
    let identities = installed(host, &config, result);
    if preferences(host, &config, &identities, result).is_err() { result.unavailable = true; }
    let mut distributions = BTreeSet::new();
    for key in ["COPILOT_CLI_RESOLVED_DIST_DIR", "COPILOT_CLI_DIST_DIR"] {
        if let Some(root) = env_path(key) {
            if !distributions.insert(root.clone()) { continue; }
            if safe_root(&root, &config).and_then(|_| distribution(host, &root, None, result)).is_err() { result.unavailable = true; }
        }
    }
    for cache in package_caches() {
        if safe_root(&cache, &config).is_err() { result.unavailable = true; continue; }
        for platform in BTreeSet::from(["universal".into(), native_platform()]) {
            let versions = match dirs(&cache.join(platform), &mut result.budget) { Ok(versions) => versions, Err(()) => { result.unavailable = true; continue; } };
            for root in versions {
                if !budget_available(&result.budget) { result.unavailable = true; return; }
                let Some(version) = root.file_name().and_then(|name| name.to_str()).filter(|name| semver::Version::parse(name).is_ok()) else { continue; };
                if !distributions.insert(root.clone()) { continue; }
                if distribution(host, &root, Some(version), result).is_err() { result.unavailable = true; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "copilot-cli").unwrap() }
    fn write(root: &Path, input: &str, content: &str) {
        let path = root.join(input); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, content).unwrap();
    }
    fn ledger(home: &TestHome, root: &Path) {
        write(&home.home.join(".copilot"), "config.json", &serde_json::json!({
            "token":"SECRET_SENTINEL", "installedPlugins":[{"name":"fixture","marketplace":"market","version":"1.0.0","enabled":false,"cache_path":root}]
        }).to_string());
    }
    fn legacy(root: &Path) { write(root, ".plugin/plugin.json", r#"{"name":"fixture","version":"1.0.0"}"#); }
    fn scan() -> PluginScan { let mut result = PluginScan::default(); observe(host(), &mut result); normalize(&mut result); result }

    #[test]
    fn global_records_select_exact_package_and_never_export_preferences_credentials_or_bodies() {
        let home = TestHome::new("copilot-global"); let root = home.home.join("plugins/selected"); legacy(&root); ledger(&home, &root);
        write(&root, "skills/one/SKILL.md", "SECRET_SENTINEL");
        write(&root, "skills/deep/nested/SKILL.md", "SECRET_SENTINEL");
        write(&root, "agents/one.agent.md", "mcp-servers: SECRET_SENTINEL");
        write(&root, "agents/ignored.md", "SECRET_SENTINEL");
        write(&root, ".mcp.json", r#"{"mcpServers":{"declared":{"command":"SECRET_SENTINEL","env":{"KEY":"SECRET_SENTINEL"}}}}"#);
        write(&root, "hooks/hooks.json", r#"{"hooks":{"sessionStart":[{"bash":"SECRET_SENTINEL"}]}}"#);
        write(&home.home.join(".copilot"), "settings.json", r#"{// JSONC native preferences
          "enabledPlugins":{"fixture@market":false,"pending@market":true,"https://user:SECRET_SENTINEL@example.test/repository":true},
        }"#);
        legacy(&home.home.join(".copilot/installed-plugins/market/unregistered"));
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 3); assert_eq!(result.mcps.len(), 1);
        let installed = result.plugins.iter().find(|plugin| plugin.presence == PluginPresence::Installed).unwrap();
        assert_eq!(installed.plugin_id, "fixture@market"); assert_eq!(installed.components.iter().filter(|c| c.kind == PluginComponentKind::Skill).count(), 1);
        assert_eq!(installed.components.iter().filter(|c| c.kind == PluginComponentKind::Agent).count(), 1);
        assert_eq!(result.mcps[0].server_id, "declared"); assert!(result.plugins.iter().all(|plugin| plugin.enabled.is_none()));
        let output = serde_json::to_string(&result.plugins).unwrap(); assert!(!output.contains("SECRET_SENTINEL") && !output.contains("unregistered"));
        assert!(watch_paths(host()).contains(&root)); assert!(!watch_paths(host()).contains(&home.home));
    }

    #[test]
    fn root_standard_manifest_wins_and_bad_mcp_entries_do_not_hide_valid_siblings() {
        let home = TestHome::new("copilot-standard"); let root = home.home.join("plugins/standard"); legacy(&root); ledger(&home, &root);
        write(&root, "plugin.json", r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"fixture","version":"1.0.0","skills":"../SECRET_SENTINEL","mcpServers":{"ignored":{}}}"#);
        write(&root, "mcp.json", r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{
          "valid":{"type":"stdio","command":"SECRET_SENTINEL","cwd":"${PLUGIN_ROOT}"},
          "bad":{"type":"http","url":"SECRET_SENTINEL"},
          "escape":{"type":"stdio","command":"hidden","cwd":"${PLUGIN_ROOT}/../outside"},
          "remote":{"type":"sse","url":"SECRET_SENTINEL"}}}"#);
        for input in ["SKILL.md","skills/one/SKILL.md","skills/deep/nested/SKILL.md","com.github.copilot/agents/review.agent.md","com.github.copilot/commands/check.md","com.github.copilot/rules/team.md"] { write(&root, input, "SECRET_SENTINEL"); }
        let result = scan(); assert!(result.unavailable); assert_eq!(result.plugins.len(), 1); assert_eq!(result.mcps.len(), 2);
        let plugin = &result.plugins[0]; assert!(plugin.source_paths.iter().any(|path| path.ends_with("standard/plugin.json")));
        assert_eq!(plugin.components.iter().filter(|c| c.kind == PluginComponentKind::Skill).count(), 1);
        assert!(plugin.components.iter().any(|c| c.kind == PluginComponentKind::Rule));
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("SECRET_SENTINEL"));
    }

    #[test]
    fn unsupported_standard_version_suppresses_legacy_and_missing_record_packages_stay_declared() {
        let home = TestHome::new("copilot-unsupported"); let root = home.home.join("plugins/unsupported"); legacy(&root); ledger(&home, &root);
        write(&root, "plugin.json", r#"{"$schema":"https://agent-plugins.org/schemas/9.0.0/plugin.schema.json","name":"fixture"}"#);
        write(&root, ".mcp.json", r#"{"mcpServers":{"must-not-load":{}}}"#);
        let result = scan(); assert!(result.unavailable); assert!(result.mcps.is_empty()); assert_eq!(result.plugins[0].presence, PluginPresence::Declared);
        fs::remove_dir_all(root).unwrap(); let result = scan(); assert!(result.unavailable); assert_eq!(result.plugins[0].presence, PluginPresence::Declared);
    }

    fn distribution_fixture(root: &Path, version: &str, complete: bool) {
        write(root, "package.json", &serde_json::json!({"name":"@github/copilot","version":version}).to_string());
        for input in ["index.js".to_string(), "app.js".into(), "cli-main.js".into(), format!("prebuilds/{}/runtime.node", native_platform())] { write(root, &input, "SECRET_SENTINEL"); }
        if complete { write(root, ".extraction-complete", ""); }
        write(root, "plugins/browser-use/.plugin/plugin.json", r#"{"name":"browser-use","version":"0.0.4"}"#);
        write(root, "plugins/browser-use/.mcp.json", r#"{"mcpServers":{"browser-use":{"command":"SECRET_SENTINEL"}}}"#);
    }

    #[test]
    fn historical_distribution_versions_remain_cache_and_incomplete_extractions_are_ignored() {
        let home = TestHome::new("copilot-cache"); let cache = home.home.join("copilot-pkg-cache/pkg").join(native_platform());
        distribution_fixture(&cache.join("1.0.101"), "1.0.101", true);
        distribution_fixture(&cache.join("1.0.104"), "1.0.104", true);
        distribution_fixture(&cache.join("1.0.105"), "1.0.105", false);
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 2); assert_eq!(result.mcps.len(), 1);
        assert!(result.plugins.iter().all(|plugin| plugin.presence == PluginPresence::CacheOnly && plugin.enabled.is_none()));
        assert_ne!(result.plugins[0].revision, result.plugins[1].revision);
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("SECRET_SENTINEL"));
    }

    #[cfg(unix)]
    #[test]
    fn escaping_component_paths_and_symlinked_packages_are_never_traversed() {
        let home = TestHome::new("copilot-confined"); let root = home.home.join("plugins/selected"); ledger(&home, &root);
        write(&root, ".plugin/plugin.json", r#"{"name":"fixture","version":"1.0.0","mcpServers":"../outside.json","extensions":["../outside"]}"#);
        write(&home.home, "plugins/outside.json", r#"{"mcpServers":{"PRIVATE_SECRET_SENTINEL":{"command":"unused"}}}"#);
        let result = scan(); assert!(result.unavailable); assert!(result.mcps.is_empty());
        fs::rename(&root, home.home.join("plugins/real")).unwrap();
        std::os::unix::fs::symlink(home.home.join("plugins/real"), &root).unwrap();
        let result = scan(); assert!(result.unavailable); assert!(result.mcps.is_empty()); assert_eq!(result.plugins[0].presence, PluginPresence::Declared);
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_SECRET_SENTINEL"));
    }
}
