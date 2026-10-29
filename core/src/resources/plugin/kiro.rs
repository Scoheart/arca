//! Kiro IDE's registered global Powers. Installation/catalog initialization,
//! repository cloning, native component parsing and activation remain native.
use super::*;

fn root(source: &Root) -> PathBuf {
    std::env::var_os("KIRO_POWERS_HOME").filter(|value| !value.is_empty())
        .map(PathBuf::from).unwrap_or_else(|| expand_tilde(&source.path))
}

fn installation_name(value: &str) -> Option<String> {
    (!value.is_empty() && value.len() <= 240 && !matches!(value, "." | "..")
        && !value.contains(['/', '\\']) && !value.chars().any(char::is_control)
        && !(value.as_bytes().get(1) == Some(&b':') && value.as_bytes()[0].is_ascii_alphabetic()))
        .then(|| value.to_string())
}

fn standard_name(value: &str, skill: bool) -> bool {
    !value.is_empty() && value.len() <= 64 && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric() && !value.contains("--")
        && !value.contains("..") && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || (!skill && b == b'.'))
}

fn schema_version<'a>(value: &'a str, kind: &str) -> Option<&'a str> {
    let version = value.strip_prefix("https://agent-plugins.org/schemas/")?.strip_suffix(&format!("/{kind}.schema.json"))?;
    let parts = version.split('.').collect::<Vec<_>>();
    (parts.len() == 3 && parts[0] == "1" && parts[1..].iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
        .then_some(version)
}

fn add(kind: PluginComponentKind, label: &str, path: &Path, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if components.len() >= MAX_COMPONENTS { return Err(()); }
    components.push(PluginComponent { kind, name: metadata_name(label).ok_or(())?, source_path: collapse_home(path), native_id: None }); Ok(())
}

fn markdown(root: &Path, folder: &str, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    for entry in directory_entries(&relative(root, folder)?, budget)? {
        if entry.file && entry.path.extension().is_some_and(|extension| extension == "md") {
            if !regular_metadata_file(&entry.path)? { return Err(()); }
            add(PluginComponentKind::Rule, &entry.path.strip_prefix(root).map_err(|_| ())?.to_string_lossy(), &entry.path, components)?;
        } else if !entry.file && !entry.directory { return Err(()); }
    }
    Ok(())
}

fn skills(root: &Path, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    for directory in dirs(&root.join("skills"), budget)? {
        let file = directory.join("SKILL.md");
        if !regular_metadata_file(&file)? { continue; }
        if !directory.file_name().and_then(|name| name.to_str()).is_some_and(|name| standard_name(name, true)) { return Err(()); }
        add(PluginComponentKind::Skill, &file.strip_prefix(root).map_err(|_| ())?.to_string_lossy(), &file, components)?;
    }
    Ok(())
}

fn standard_server(value: &Value, root: &Path) -> bool {
    if !standard_mcp_declaration(value) { return false; }
    if value.get("type").and_then(Value::as_str) == Some("stdio") {
        let Some(command) = value.get("command").and_then(Value::as_str) else { return false; };
        if let Some(input) = command.strip_prefix("./") {
            if relative(root, input).is_err() { return false; }
        } else if command.contains(['/', '\\']) || command.starts_with("..") { return false; }
        return value.get("cwd").and_then(Value::as_str).is_none_or(|cwd| !cwd.contains(".."));
    }
    let Some(input) = value.get("url").and_then(Value::as_str) else { return false; };
    let Ok(url) = url::Url::parse(input) else { return false; };
    if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some()
        || (url.scheme() != "https" && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1" | "[::1]"))) { return false; }
    if let Some(headers) = value.get("headers").and_then(Value::as_object) {
        let mut names = BTreeSet::new();
        if headers.keys().any(|name| !names.insert(name.to_lowercase())) { return false; }
    }
    true
}

fn mcp(host_name: &str, root: &Path, standard: Option<&str>, components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    let format = if standard.is_some() { DocumentFormat::Json } else { DocumentFormat::Jsonc };
    let Some(value) = optional_document(root, "mcp.json", format, &mut result.budget)? else { return Ok(()); };
    if let Some(version) = standard {
        if value.get("$schema").and_then(Value::as_str).and_then(|value| schema_version(value, "mcp")) != Some(version)
            || !value.as_object().is_some_and(|object| object.keys().all(|key| ["$schema", "mcpServers"].contains(&key.as_str()))) { return Err(()); }
    }
    let values = match value.get("mcpServers") {
        None | Some(Value::Null) if standard.is_none() => return Ok(()),
        Some(value) => value.as_object().filter(|values| values.len() <= MAX_COMPONENTS).ok_or(())?, _ => return Err(()),
    };
    for (name, server) in values {
        if metadata_name(name).is_none() || matches!(name.as_str(), "__proto__" | "constructor" | "prototype")
            || !server.is_object() || (standard.is_some() && !standard_server(server, root)) { result.unavailable = true; continue; }
        add(PluginComponentKind::Mcp, name, &root.join("mcp.json"), components)?;
        components.last_mut().ok_or(())?.native_id = Some(format!("power-{host_name}-{name}"));
    }
    Ok(())
}

fn package(host: &Host, base: &Path, name: &str, result: &mut PluginScan) -> Result<NativePluginObservation, ()> {
    result.budget.visit()?;
    let path = relative(&base.join("installed"), name)?;
    let mut plugin = NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: name.into(), name: name.into(),
        version: "unknown".into(), revision: collapse_home(&path), presence: PluginPresence::Declared, enabled: None,
        source_paths: vec![collapse_home(&base.join("installed.json"))], components: Vec::new(), docs_url: host.docs_url.clone(), source_fingerprint: String::new() };
    checked_root(&path)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_dir() => plugin.presence = PluginPresence::Installed,
        _ => { result.unavailable = true; return Ok(plugin); },
    }
    let manifest_path = path.join("plugin.json");
    let has_manifest = match fs::symlink_metadata(&manifest_path) {
        Ok(_) => true, Err(error) if error.kind() == std::io::ErrorKind::NotFound => false, Err(_) => return Err(()),
    };
    if has_manifest {
        plugin.source_paths.push(collapse_home(&manifest_path));
        let parsed = (|| {
            let manifest = optional_json(&path, "plugin.json", &mut result.budget)?.ok_or(())?;
            if !manifest.get("name").and_then(Value::as_str).is_some_and(|name| standard_name(name, false)) { return Err(()); }
            let version = manifest.get("$schema").and_then(Value::as_str).and_then(|value| schema_version(value, "plugin")).ok_or(())?;
            if let Some(display) = manifest.get("displayName").and_then(Value::as_str).and_then(metadata_name) { plugin.name = display; }
            if let Some(value) = manifest.get("version").and_then(Value::as_str).and_then(metadata_name) { plugin.version = value; }
            if skills(&path, &mut plugin.components, &mut result.budget).is_err() { result.unavailable = true; }
            if mcp(name, &path, Some(version), &mut plugin.components, result).is_err() { result.unavailable = true; }
            if markdown(&path, "dev.kiro/steering", &mut plugin.components, &mut result.budget).is_err() { result.unavailable = true; }
            Ok::<(), ()>(())
        })(); if parsed.is_err() { result.unavailable = true; }
    } else {
        let power = path.join("POWER.md");
        if regular_metadata_file(&power)? {
            plugin.source_paths.push(collapse_home(&power)); add(PluginComponentKind::Context, "POWER.md", &power, &mut plugin.components)?;
        }
        if mcp(name, &path, None, &mut plugin.components, result).is_err() { result.unavailable = true; }
        if markdown(&path, "steering", &mut plugin.components, &mut result.budget).is_err() { result.unavailable = true; }
    }
    plugin.components.sort(); plugin.components.dedup(); plugin.source_paths.sort(); plugin.source_paths.dedup(); Ok(plugin)
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    host.roots.iter().flat_map(|source| { let path = root(source); [path.join("installed.json"), path.join("installed")] })
        .filter(|path| path.is_absolute()).collect()
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    for source in &host.roots {
        let base = root(source);
        if source.layout != "kiro_powers" || !base.is_absolute() || checked_root(&base).is_err() { result.unavailable = true; continue; }
        let value = match optional_json(&base, "installed.json", &mut result.budget) {
            Ok(Some(value)) => value, Ok(None) => continue, Err(()) => { result.unavailable = true; continue; },
        };
        if value.get("version").is_some_and(|version| version.as_str() != Some("1.0.0")) { result.unavailable = true; continue; }
        let Some(installed) = value.get("installedPowers").and_then(Value::as_array).filter(|records| records.len() <= MAX_ENTRIES) else { result.unavailable = true; continue; };
        let mut seen = BTreeSet::new();
        for record in installed {
            let Some(name) = record.get("name").and_then(Value::as_str).and_then(installation_name) else { result.unavailable = true; continue; };
            if !seen.insert(name.clone()) { result.unavailable = true; continue; }
            match package(host, &base, &name, result) { Ok(plugin) => result.plugins.push(plugin), Err(()) => result.unavailable = true, }
            if result.budget.visited > MAX_SCAN_ENTRIES || result.budget.references > MAX_REFERENCES || result.budget.bytes >= MAX_READ_BYTES { break; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "kiro").unwrap() }
    fn write(base: &Path, input: &str, body: &str) {
        let path = base.join(input); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, body).unwrap();
    }
    fn scan() -> PluginScan { let mut result = PluginScan::default(); observe(host(), &mut result); normalize(&mut result); result }
    fn register(base: &Path, names: &[&str]) {
        write(base, "installed.json", &serde_json::json!({"version":"1.0.0","installedPowers":names.iter().map(|name|serde_json::json!({"name":name,"source":"SECRET_SENTINEL"})).collect::<Vec<_>>(),"dismissedAutoInstalls":[]}).to_string());
    }

    #[test]
    fn registered_standard_powers_have_portable_skills_vendor_steering_and_native_mcp_ids_only() {
        let home = TestHome::new("kiro-standard"); let base = home.home.join(".kiro/powers"); register(&base, &["registered"]);
        let root = base.join("installed/registered");
        write(&root, "plugin.json", r#"{"$schema":"https://agent-plugins.org/schemas/1.1.0/plugin.schema.json","name":"manifest-name","version":"2.3.4","displayName":"Example Power","extensions":{"com.github.copilot":{"hooks":{"Ignored":[]}}}}"#);
        write(&root, "mcp.json", r#"{"$schema":"https://agent-plugins.org/schemas/1.1.0/mcp.schema.json","mcpServers":{"service":{"type":"stdio","command":"./server.js","env":{"PRIVATE":"SECRET_SENTINEL"}}}}"#);
        for input in ["skills/one/SKILL.md","skills/deep/nested/SKILL.md","dev.kiro/steering/one.md","dev.kiro/steering/nested/ignored.md","steering/ignored.md","agents/ignored.md","commands/ignored.md","POWER.md"] { write(&root, input, "SECRET_SENTINEL"); }
        let result=scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(),1); let plugin=&result.plugins[0];
        assert_eq!(plugin.plugin_id,"registered"); assert_eq!(plugin.name,"Example Power"); assert_eq!(plugin.version,"2.3.4"); assert_eq!(plugin.presence,PluginPresence::Installed); assert_eq!(plugin.enabled,None);
        assert_eq!(plugin.components.len(),3); assert_eq!(result.mcps[0].server_id,"power-registered-service"); assert!(!serde_json::to_string(&result.plugins).unwrap().contains("SECRET_SENTINEL"));
    }

    #[test]
    fn legacy_power_context_is_metadata_only_and_jsonc_mcp_declarations_do_not_make_skills() {
        let home=TestHome::new("kiro-legacy");let base=home.home.join(".kiro/powers");register(&base,&["example"]);let root=base.join("installed/example");
        for input in ["POWER.md","steering/one.md","skills/ignored/SKILL.md","dev.kiro/steering/ignored.md"] {write(&root,input,"SECRET_SENTINEL");}
        write(&root,"mcp.json","{ // native IDE configuration\n\"mcpServers\":{\"service\":{\"command\":\"SECRET_SENTINEL\"},},}");
        let result=scan();assert!(!result.unavailable);assert_eq!(result.plugins[0].version,"unknown");assert_eq!(result.plugins[0].components.len(),3);assert_eq!(result.mcps[0].server_id,"power-example-service");
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("SECRET_SENTINEL"));
    }

    #[test]
    fn missing_records_stay_declared_and_unregistered_directories_or_install_sources_are_not_scanned() {
        let home=TestHome::new("kiro-records");let base=home.home.join(".kiro/powers");register(&base,&["missing"]);
        write(&base,"installed/unregistered/POWER.md","SECRET_SENTINEL");write(&base,"repos/cached/POWER.md","SECRET_SENTINEL");
        let result=scan();assert!(result.unavailable);assert_eq!(result.plugins.len(),1);assert_eq!(result.plugins[0].presence,PluginPresence::Declared);assert!(result.plugins[0].components.is_empty());
        register(&base,&["../escape","C:escape"]);assert!(scan().plugins.is_empty());
    }

    #[test]
    fn invalid_standard_manifest_never_falls_back_and_mcp_schema_versions_must_match() {
        let home=TestHome::new("kiro-precedence");let base=home.home.join(".kiro/powers");register(&base,&["example"]);let root=base.join("installed/example");
        write(&root,"POWER.md","SECRET_SENTINEL");write(&root,"plugin.json",r#"{"$schema":"https://agent-plugins.org/schemas/2.0.0/plugin.schema.json","name":"example"}"#);
        let result=scan();assert!(result.unavailable);assert!(result.plugins[0].components.is_empty());
        write(&root,"plugin.json",r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"example"}"#);
        write(&root,"mcp.json",r#"{"$schema":"https://agent-plugins.org/schemas/1.1.0/mcp.schema.json","mcpServers":{"service":{"type":"stdio","command":"cmd"}}}"#);
        assert!(scan().mcps.is_empty());
    }

    #[test]
    fn malformed_individual_mcp_entries_are_skipped_without_hiding_other_registered_declarations() {
        let home=TestHome::new("kiro-mcp-shapes");let root=home.home.join("package");let mut result=PluginScan::default();let mut components=Vec::new();
        write(&root,"mcp.json",r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"good":{"type":"streamable-http","url":"https://example.com/mcp"},"remote-http":{"type":"sse","url":"http://example.com/mcp"},"absolute":{"type":"stdio","command":"/bin/sh"},"headers":{"type":"sse","url":"https://example.com","headers":{"Authorization":"PRIVATE","authorization":"PRIVATE"}}}}"#);
        mcp("power",&root,Some("1.0.0"),&mut components,&mut result).unwrap();assert!(result.unavailable);assert_eq!(components.len(),1);assert_eq!(components[0].native_id.as_deref(),Some("power-power-good"));
    }

    #[cfg(unix)]
    #[test]
    fn package_symlinks_cannot_redirect_inventory_outside_the_registered_installation() {
        let home=TestHome::new("kiro-symlinks");let base=home.home.join(".kiro/powers");register(&base,&["escape"]);let outside=home.home.join("outside");
        write(&outside,"POWER.md","SECRET_SENTINEL");fs::create_dir_all(base.join("installed")).unwrap();std::os::unix::fs::symlink(&outside,base.join("installed/escape")).unwrap();
        let result=scan();assert!(result.unavailable);assert!(result.plugins.is_empty());
    }
}
