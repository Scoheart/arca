//! WorkBuddy's registered snapshots and packaged marketplace, without invoking
//! installers, account overlays, integrity code, component bodies or MCPs.
use super::*;

const SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";
const MCP_SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json";
const LEGACY_MANIFESTS: [&str; 3] = [".codebuddy-plugin/plugin.json", ".workbuddy-plugin/plugin.json", ".claude-plugin/plugin.json"];
const MARKETPLACE: &str = ".codebuddy-plugin/marketplace.json";

fn source_root(source: &Root) -> PathBuf {
    if source.path.starts_with("/Applications/") { crate::paths::system_probe_path(&source.path) }
    else { expand_tilde(&source.path) }
}

fn package_path(root: &Path, input: &str) -> Result<PathBuf, ()> {
    // Native path.join normalizes `./`. Keep the confinement/symlink checks,
    // then spell the same physical file once across defaults and declarations.
    Ok(relative(root, input)?.components().collect())
}

fn add(kind: PluginComponentKind, name: &str, path: &Path, output: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if output.len() >= MAX_COMPONENTS { return Err(()); }
    output.push(PluginComponent { kind, name: metadata_name(name).ok_or(())?, source_path: collapse_home(path), native_id: None }); Ok(())
}

fn add_file(root: &Path, path: &Path, kind: PluginComponentKind, output: &mut Vec<PluginComponent>) -> Result<bool, ()> {
    if !regular_metadata_file(path)? { return Ok(false); }
    add(kind, &path.strip_prefix(root).map_err(|_| ())?.to_string_lossy(), path, output)?; Ok(true)
}

fn exists(path: &Path) -> Result<bool, ()> {
    checked_root(path)?;
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true), Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false), Err(_) => Err(()),
    }
}

fn standard_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && !name.contains("..") && !name.contains("--")
        && name.as_bytes()[0].is_ascii_alphanumeric() && name.as_bytes()[name.len() - 1].is_ascii_alphanumeric()
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-".contains(&b))
}

fn standard_manifest(value: &Value) -> bool {
    let Some(object) = value.as_object() else { return false; };
    value.get("$schema").and_then(Value::as_str) == Some(SCHEMA)
        && value.get("name").and_then(Value::as_str).is_some_and(standard_name)
        && object.iter().all(|(key, value)| match key.as_str() {
            "keywords" => value.as_array().is_some_and(|values| values.iter().all(Value::is_string)),
            "author" => value.as_object().is_some_and(|author| author.iter().all(|(key, value)| ["name","email","url"].contains(&key.as_str()) && value.is_string())),
            "extensions" => true, // The native portable reader ignores invalid vendor metadata.
            "$schema" | "name" | "displayName" | "version" | "description" | "homepage" | "repository" | "license" => value.is_string(),
            _ => true, // Non-standard manifest fields are ignored, never components.
        })
}

fn manifest(root: &Path, budget: &mut ScanBudget) -> Result<(String, Arc<Value>, bool), ()> {
    if exists(&root.join("plugin.json"))? {
        let value = optional_json(root, "plugin.json", budget)?.ok_or(())?;
        if !standard_manifest(&value) { return Err(()); }
        return Ok(("plugin.json".into(), value, true));
    }
    for input in LEGACY_MANIFESTS {
        if !exists(&root.join(input))? { continue; }
        let value = optional_json(root, input, budget)?.filter(|value| value.is_object()).ok_or(())?;
        let standard = value.get("$schema").is_some_and(|schema| match schema {
            Value::Null | Value::Bool(false) => false,
            Value::Number(value) => value.as_f64() != Some(0.0),
            Value::String(value) => !value.is_empty(), _ => true,
        });
        if standard && !standard_manifest(&value) { return Err(()); }
        return Ok((input.into(), value, standard));
    }
    Err(())
}

fn feature_directory(root: &Path, folder: &Path, kind: PluginComponentKind, skill: bool,
    output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let mut invalid = false;
    for entry in directory_entries(folder, budget)? {
        let observed = (|| {
            if entry.file && entry.path.extension().is_some_and(|extension| extension == "md") {
                add_file(root, &entry.path, kind.clone(), output)?;
            } else if entry.directory && skill {
                if !add_file(root, &entry.path.join("SKILL.md"), kind.clone(), output)? {
                    add(kind.clone(), &entry.path.strip_prefix(root).map_err(|_| ())?.to_string_lossy(), &entry.path, output)?;
                }
            } else if !entry.file && !entry.directory { return Err(()); }
            Ok::<(), ()>(())
        })(); invalid |= observed.is_err();
    }
    if invalid { Err(()) } else { Ok(()) }
}

fn feature_path(root: &Path, input: &str, kind: PluginComponentKind, skill: bool,
    output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let path = package_path(root, input)?; checked_root(&path)?;
    if !exists(&path)? { return Ok(()); }
    if regular_metadata_file(&path)? {
        if !path.extension().is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("md")) { return Ok(()); }
        add_file(root, &path, kind, output)?; return Ok(());
    }
    if skill && add_file(root, &path.join("SKILL.md"), kind.clone(), output)? { return Ok(()); }
    feature_directory(root, &path, kind, skill, output, budget)
}

fn features(root: &Path, source: &str, value: Option<&Value>, folder: &str, kind: PluginComponentKind,
    output: &mut Vec<PluginComponent>, result: &mut PluginScan) {
    let skill = kind == PluginComponentKind::Skill;
    if feature_directory(root, &root.join(folder), kind.clone(), skill, output, &mut result.budget).is_err() { result.unavailable = true; }
    if skill && add_file(root, &root.join("SKILL.md"), kind.clone(), output).is_err() { result.unavailable = true; }
    let entries: Vec<&Value> = match value {
        Some(Value::Array(values)) if values.len() <= 64 => values.iter().collect(),
        Some(Value::Array(_)) => { result.unavailable = true; return; },
        Some(value) => vec![value], None => Vec::new(),
    };
    for entry in entries {
        let observed = match entry {
            Value::String(input) if !input.is_empty() => feature_path(root, input, kind.clone(), skill, output, &mut result.budget),
            Value::Object(object) => {
                let name = object.get("name").and_then(Value::as_str);
                match object.get("path").and_then(Value::as_str) {
                    Some(input) => (|| {
                        let path = package_path(root, input)?; checked_root(&path)?;
                        if !exists(&path)? { return Err(()); }
                        add(kind.clone(), name.unwrap_or(input), &path, output)
                    })(),
                    None => name.map(|name| add(kind.clone(), name, &root.join(source), output)).unwrap_or(Ok(())),
                }
            },
            _ => Ok(()), // Native toFeatureEntries ignores booleans/null/other primitives.
        };
        if observed.is_err() { result.unavailable = true; }
    }
}

fn standard_skills(root: &Path, output: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    for path in dirs(&root.join("skills"), budget)? { add_file(root, &path.join("SKILL.md"), PluginComponentKind::Skill, output)?; }
    Ok(())
}

fn standard_server(value: &Value) -> bool {
    if !standard_mcp_declaration(value) { return false; }
    if value.get("type").and_then(Value::as_str) == Some("stdio") {
        let Some(command) = value.get("command").and_then(Value::as_str) else { return false; };
        return command.starts_with("./") || !command.chars().any(|character| character.is_whitespace() || matches!(character, '/' | '\\'));
    }
    let Some(input) = value.get("url").and_then(Value::as_str) else { return false; };
    if input.chars().any(|character| character <= '\u{20}' || character == '\u{7f}' || character == '\\') || input.contains('#') { return false; }
    let Some((scheme, authority)) = input.split_once("://") else { return false; };
    if !["http","https"].iter().any(|candidate| scheme.eq_ignore_ascii_case(candidate)) || authority.split(['/', '?', '#']).next().is_none_or(|value| value.is_empty() || value.contains('@')) { return false; }
    let Ok(url) = url::Url::parse(input) else { return false; };
    let loopback = matches!(url.host_str(), Some("localhost" | "[::1]" | "::1"))
        || url.host_str().and_then(|host| host.parse::<std::net::Ipv4Addr>().ok()).is_some_and(|ip| ip.octets()[0] == 127);
    if !url.username().is_empty() || url.password().is_some() || (url.scheme() != "https" && !loopback) { return false; }
    if let Some(headers) = value.get("headers").and_then(Value::as_object) {
        let mut names = BTreeSet::new();
        if headers.iter().any(|(name, value)| name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || [33,35,36,37,38,39,42,43,45,46,94,95,96,124,126].contains(&b))
            || !names.insert(name.to_ascii_lowercase()) || value.as_str().is_none_or(|value| value.contains(['\r','\n','\0']))) { return false; }
    }
    true
}

fn servers(value: &Value, source: &Path, standard: bool,
    output: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    let servers = value.as_object().filter(|servers| servers.len() <= MAX_COMPONENTS).ok_or(())?;
    for (name, config) in servers {
        if metadata_name(name).is_none() || matches!(name.as_str(), "__proto__" | "constructor" | "prototype")
            || !config.is_object() || (standard && !standard_server(config)) { result.unavailable = true; continue; }
        add(PluginComponentKind::Mcp, name, source, output)?;
        output.last_mut().ok_or(())?.native_id = Some(name.clone());
    }
    Ok(())
}

fn mcp_file(root: &Path, input: &str, standard: bool, output: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    let format = if standard { DocumentFormat::Json } else { DocumentFormat::Jsonc };
    let Some(value) = optional_document(root, input, format, &mut result.budget)? else { return Ok(()); };
    if standard && (value.get("$schema").and_then(Value::as_str) != Some(MCP_SCHEMA)
        || !value.as_object().is_some_and(|object| object.keys().all(|key| ["$schema","mcpServers"].contains(&key.as_str())))) { return Err(()); }
    let wrapped = value.get("mcpServers").filter(|value| value.is_object());
    if standard && wrapped.is_none() { return Err(()); }
    if !standard && wrapped.is_none() {
        // Native legacy discovery ignores valid JSON that is not an MCP map.
        // Some shipped packages place Node's package.json beside their server.
        let Some(object) = value.as_object() else { return Ok(()); };
        if !object.values().all(|config| config.is_object() && (config.get("command").is_some() || config.get("url").is_some())) { return Ok(()); }
    }
    servers(wrapped.unwrap_or(&value), &package_path(root, input)?, standard, output, result)
}

fn mcps(root: &Path, manifest: &Value, source: &str, standard: bool,
    output: &mut Vec<PluginComponent>, result: &mut PluginScan) {
    if standard {
        if mcp_file(root, "mcp.json", true, output, result).is_err() { result.unavailable = true; }
        return;
    }
    if let Some(value) = manifest.get("mcpServers") {
        let observed = match value {
            Value::String(input) if !input.is_empty() => mcp_file(root, input, false, output, result),
            Value::Object(_) => servers(value, &root.join(source), false, output, result),
            Value::Null | Value::Bool(false) => Ok(()), _ => Err(()),
        };
        if observed.is_err() { result.unavailable = true; }
    }
    if mcp_file(root, ".mcp.json", false, output, result).is_err() { result.unavailable = true; }
    match directory_entries(&root.join("mcp"), &mut result.budget) {
        Ok(entries) => for entry in entries {
            if entry.file && entry.path.extension().is_some_and(|extension| extension == "json") {
                if mcp_file(root, entry.path.strip_prefix(root).unwrap().to_str().unwrap_or(""), false, output, result).is_err() { result.unavailable = true; }
            } else if !entry.file && !entry.directory { result.unavailable = true; }
        }, Err(()) => result.unavailable = true,
    }
}

fn hook_events(value: &Value, path: &Path, output: &mut Vec<PluginComponent>) -> Result<(), ()> {
    let events = value.get("hooks").unwrap_or(value).as_object().filter(|object| object.len() <= 64).ok_or(())?;
    for (name, definitions) in events {
        if definitions.is_array() { add(PluginComponentKind::Hook, name, path, output)?; }
    }
    Ok(())
}

fn hook_file(root: &Path, input: &str, output: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    let path = package_path(root, input)?;
    if !add_file(root, &path, PluginComponentKind::Hook, output)? { return Ok(()); }
    if path.extension().is_some_and(|extension| extension == "json") {
        if let Some(value) = optional_json(root, input, &mut result.budget)? { hook_events(&value, &path, output)?; }
    }
    Ok(())
}

fn hooks(root: &Path, source: &str, manifest: &Value, output: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    match manifest.get("hooks") {
        Some(Value::String(input)) => {
            // The native UI normalizes an empty hook hint to absence. In particular,
            // shipped Tencent Docs packages use `"hooks": ""` as a placeholder.
            let input = input.strip_prefix("./").or_else(|| input.strip_prefix(".\\"))
                .unwrap_or(input).trim_end_matches(['/', '\\']);
            return if input.is_empty() { Ok(()) } else { hook_file(root, input, output, result) };
        },
        Some(Value::Array(values)) if values.len() <= 64 => {
            for (index, _) in values.iter().enumerate() { add(PluginComponentKind::Hook, &format!("hook declaration {}", index + 1), &root.join(source), output)?; }
            return Ok(());
        },
        Some(Value::Array(_)) => return Err(()),
        Some(Value::Object(_)) => {
            let count = output.len(); hook_events(manifest.get("hooks").ok_or(())?, &root.join(source), output)?;
            if output.len() > count { return Ok(()); }
        },
        _ => {},
    }
    let count = output.len();
    feature_directory(root, &root.join("hooks"), PluginComponentKind::Hook, false, output, &mut result.budget)?;
    if count == output.len() { hook_file(root, "hooks/hooks.json", output, result)?; }
    Ok(())
}

fn lsp(root: &Path, source: &str, manifest: &Value, output: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    let configured = manifest.get("lspServers");
    let (path, value) = match configured {
        Some(Value::Object(value)) if !value.is_empty() => (root.join(source), Arc::new(Value::Object(value.clone()))),
        _ => {
            let custom = configured.and_then(Value::as_str);
            let mut selected = None;
            if let Some(input) = custom {
                if let Some(value) = optional_json(root, input, &mut result.budget)? {
                    if value.as_object().is_some_and(|object| !object.is_empty()) { selected = Some((package_path(root, input)?, value)); }
                }
            }
            match selected { Some(value) => value, None => {
                let Some(value) = optional_json(root, ".lsp.json", &mut result.budget)? else { return Ok(()); };
                (root.join(".lsp.json"), value)
            }}
        },
    };
    let values = value.as_object().filter(|values| values.len() <= MAX_COMPONENTS).ok_or(())?;
    for name in values.keys() { add(PluginComponentKind::Lsp, name, &path, output)?; }
    Ok(())
}

fn package(host: &Host, root: &Path, identity: &str, expected_version: Option<&str>, presence: PluginPresence,
    registry: &Path, result: &mut PluginScan) -> NativePluginObservation {
    let (name, _) = identity.split_once('@').unwrap_or((identity, ""));
    let mut plugin = NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: identity.into(), name: name.into(),
        version: expected_version.unwrap_or("unknown").into(), revision: collapse_home(root), presence, enabled: None,
        source_paths: vec![collapse_home(registry)], components: Vec::new(), docs_url: host.docs_url.clone(), source_fingerprint: String::new() };
    let observed = (|| {
        result.budget.visit()?; checked_root(root)?;
        let (source, manifest, standard) = manifest(root, &mut result.budget)?;
        plugin.source_paths.push(collapse_home(&root.join(&source)));
        if manifest.get("name").and_then(Value::as_str).is_some_and(|value| value != name) { return Err(()); }
        let version = manifest.get("version").and_then(Value::as_str);
        if expected_version.is_some_and(|expected| version.is_some_and(|version| version != expected)) { return Err(()); }
        if let Some(version) = version.and_then(metadata_name) { plugin.version = version; }
        if standard {
            if standard_skills(root, &mut plugin.components, &mut result.budget).is_err() { result.unavailable = true; }
        } else {
            for (field, kind) in [("agents",PluginComponentKind::Agent),("commands",PluginComponentKind::Command),("skills",PluginComponentKind::Skill),("rules",PluginComponentKind::Rule)] {
                features(root, &source, manifest.get(field), field, kind, &mut plugin.components, result);
            }
            if hooks(root, &source, &manifest, &mut plugin.components, result).is_err() { result.unavailable = true; }
            if lsp(root, &source, &manifest, &mut plugin.components, result).is_err() { result.unavailable = true; }
            if manifest.get("category").and_then(Value::as_str) == Some("mcp-app") {
                add(PluginComponentKind::McpBundle, "MCP app package", &root.join(&source), &mut plugin.components)?;
            }
        }
        mcps(root, &manifest, &source, standard, &mut plugin.components, result); Ok::<(), ()>(())
    })(); if observed.is_err() { result.unavailable = true; }
    plugin.components.sort(); plugin.components.dedup(); plugin.source_paths.sort(); plugin.source_paths.dedup(); plugin
}

fn identity(value: &str) -> Option<(&str, &str)> {
    let (name, market) = value.split_once('@')?;
    [name, market].iter().all(|value| !value.is_empty() && value.len() <= 240
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))).then_some((name, market))
}

fn sanitize(value: &str, version: bool) -> String {
    let value: String = value.chars().map(|character| if character.is_ascii_alphanumeric()
        || matches!(character, '-' | '_') || (version && character == '.') { character } else { '-' }).collect();
    if value.is_empty() { "unknown".into() } else if matches!(value.as_str(), "." | "..") { "-".into() } else { value }
}

struct Candidate { root: PathBuf, version: String, rank: u8, stamp: String }

fn candidate(config: &Path, name: &str, market: &str, value: &Value) -> Result<Option<Candidate>, ()> {
    if !value.is_object() || ["scope","installPath","version","resolvedVersion","installedAt","lastUpdated"].iter().any(|key| value.get(*key).is_some_and(|value| !value.is_string())) { return Err(()); }
    let rank = match value.get("scope").and_then(Value::as_str) { Some("managed") => 2, Some("user") => 1, _ => return Ok(None) };
    let version = value.get("version").and_then(Value::as_str).filter(|version| !version.is_empty() && version.len() <= 240 && version.is_ascii()).ok_or(())?;
    let root = config.join("plugins/cache").join(sanitize(market,false)).join(sanitize(name,false)).join(sanitize(version,true));
    let installed = PathBuf::from(value.get("installPath").and_then(Value::as_str).ok_or(())?);
    if installed != root || !installed.is_absolute() { return Err(()); }
    checked_root(&root)?;
    if !fs::symlink_metadata(&root).is_ok_and(|metadata| metadata.file_type().is_dir()) { return Err(()); }
    let version = value.get("resolvedVersion").and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or(version);
    let stamp = value.get("lastUpdated").and_then(Value::as_str).filter(|value| !value.is_empty())
        .or_else(|| value.get("installedAt").and_then(Value::as_str)).unwrap_or("");
    Ok(Some(Candidate { root, version: metadata_name(version).ok_or(())?, rank, stamp: stamp.into() }))
}

fn canonical_stamp(value: &str) -> bool {
    value.is_empty() || (value.len() == 24 && value.ends_with('Z') && chrono::DateTime::parse_from_rfc3339(value).is_ok())
}

fn installations(host: &Host, config: &Path, result: &mut PluginScan) -> Result<(), ()> {
    checked_root(config)?;
    let Some(ledger) = optional_json(config, "plugins/installed_plugins.json", &mut result.budget)? else { return Ok(()); };
    let entries = ledger.get("plugins").and_then(Value::as_object).filter(|entries| entries.len() <= MAX_ENTRIES).ok_or(())?;
    let registry = config.join("plugins/installed_plugins.json");
    for (id, records) in entries {
        let Some((name, market)) = identity(id) else { result.unavailable = true; continue; };
        let records: Vec<&Value> = match records { Value::Array(values) if values.len() <= MAX_ENTRIES => values.iter().collect(), Value::Object(_) => vec![records], _ => { result.unavailable = true; continue; } };
        let mut candidates = Vec::new(); let mut global = false;
        for record in records {
            global |= matches!(record.get("scope").and_then(Value::as_str), Some("user" | "managed"));
            match candidate(config, name, market, record) {
                Ok(Some(value)) => candidates.push(value), Ok(None) => {}, Err(()) => result.unavailable = true,
            }
        }
        let rank = candidates.iter().map(|candidate| candidate.rank).max().unwrap_or(0);
        candidates.retain(|candidate| candidate.rank == rank);
        if candidates.len() > 1 && candidates.iter().any(|candidate| !canonical_stamp(&candidate.stamp)) { result.unavailable = true; candidates.clear(); }
        let selected = candidates.into_iter().reduce(|selected, candidate| if candidate.stamp > selected.stamp { candidate } else { selected });
        if let Some(selected) = selected {
            let plugin = package(host, &selected.root, id, Some(&selected.version), PluginPresence::Installed, &registry, result);
            result.plugins.push(plugin);
        } else if global {
            result.plugins.push(NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:id.clone(), name:name.into(), version:"unknown".into(),
                revision:format!("registered:{id}"), presence:PluginPresence::Declared, enabled:None, source_paths:vec![collapse_home(&registry)],
                components:Vec::new(), docs_url:host.docs_url.clone(), source_fingerprint:String::new() });
        }
        if result.budget.visited > MAX_SCAN_ENTRIES || result.budget.references > MAX_REFERENCES || result.budget.bytes >= MAX_READ_BYTES { break; }
    }
    Ok(())
}

fn bundled(host: &Host, root: &Path, result: &mut PluginScan) -> Result<(), ()> {
    checked_root(root)?;
    let Some(market) = optional_json(root, MARKETPLACE, &mut result.budget)? else { return Ok(()); };
    if market.get("name").and_then(Value::as_str) != Some("workbuddy-builtin") { return Err(()); }
    let entries = market.get("plugins").and_then(Value::as_array).filter(|entries| entries.len() <= MAX_ENTRIES).ok_or(())?;
    let mut seen = BTreeSet::new();
    for entry in entries {
        let observed = (|| {
            let name = entry.get("name").and_then(Value::as_str).ok_or(())?;
            let id = format!("{name}@workbuddy-builtin"); identity(&id).ok_or(())?;
            if !seen.insert(id.clone()) { return Err(()); }
            if let Some(platforms) = entry.get("platforms") {
                let platforms = platforms.as_array().filter(|values| values.iter().all(Value::is_string)).ok_or(())?;
                let platform = if cfg!(target_os="macos") { "darwin" } else if cfg!(target_os="windows") { "win32" } else { std::env::consts::OS };
                if !platforms.is_empty() && !platforms.iter().any(|value| value.as_str() == Some(platform)) { return Ok(()); }
            }
            let source = entry.get("source").and_then(Value::as_str).ok_or(())?;
            let path = package_path(root, source)?; checked_root(&path)?;
            if path == root || !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_dir()) { return Err(()); }
            let plugin = package(host, &path, &id, None, PluginPresence::Bundled, &root.join(MARKETPLACE), result);
            result.plugins.push(plugin); Ok::<(), ()>(())
        })(); if observed.is_err() { result.unavailable = true; }
    }
    Ok(())
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    host.roots.iter().map(source_root).filter(|root| root.is_absolute()).collect()
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    for source in &host.roots {
        let root = source_root(source);
        if !root.is_absolute() { result.unavailable = true; continue; }
        let observed = match source.layout.as_str() {
            "workbuddy_installed" => installations(host, &root, result),
            "workbuddy_bundled" => bundled(host, &root, result),
            _ => Err(()),
        };
        if observed.is_err() { result.unavailable = true; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "workbuddy").unwrap() }
    fn write(root: &Path, input: &str, body: &str) {
        let path = root.join(input); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, body).unwrap();
    }
    fn install(home: &TestHome, name: &str, version: &str, scope: &str) -> (PathBuf, Value) {
        let root = home.home.join(".workbuddy-ai/plugins/cache/market").join(name).join(version);
        write(&root, LEGACY_MANIFESTS[0], &serde_json::json!({"name":name,"version":version}).to_string());
        let record = serde_json::json!({"scope":scope,"installPath":root,"version":version,"installedAt":"2026-10-28T00:00:00.000Z"});
        (root, record)
    }
    fn register(home: &TestHome, values: Value) {
        write(&home.home, ".workbuddy-ai/plugins/installed_plugins.json", &serde_json::json!({"plugins":values}).to_string());
    }
    fn scan() -> PluginScan { let mut result = PluginScan::default(); observe(host(), &mut result); normalize(&mut result); result }

    #[test]
    fn empty_native_hook_hints_are_absent_and_do_not_report_unavailable_packages() {
        let home = TestHome::new("workbuddy-empty-hooks"); let (root, record) = install(&home, "docs", "1", "user");
        register(&home, serde_json::json!({"docs@market":[record]}));
        for hint in ["", "./", ".\\"] {
            write(&root, LEGACY_MANIFESTS[0], &serde_json::json!({"name":"docs","version":"1","hooks":hint}).to_string());
            write(&root, "hooks/hooks.json", r#"{"hooks":{"must-not-fall-back":[]}}"#);
            let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 1);
            assert!(result.plugins[0].components.is_empty()); assert!(result.mcps.is_empty());
        }
    }

    #[test]
    fn legacy_features_keep_paths_and_mcp_names_without_exporting_commands_or_bodies() {
        let home = TestHome::new("workbuddy-legacy"); let (root, record) = install(&home, "example", "1", "user");
        register(&home, serde_json::json!({"example@market":record}));
        write(&root, LEGACY_MANIFESTS[0], r#"{"name":"example","version":"1","agents":["./agents/one.md","./declared/agent.md"],"commands":["./commands/one.md"],"mcpServers":{"mcpServers":{"command":"SECRET_SENTINEL"},"shared":{"command":"SECRET_SENTINEL"}},"hooks":{"SessionStart":[{"hooks":[{"command":"SECRET_SENTINEL"}]}]},"lspServers":{"language":{"command":"SECRET_SENTINEL"}}}"#);
        for input in ["agents/one.md","declared/agent.md","commands/one.md","skills/one/SKILL.md","rules/one.md"] { write(&root, input, "SECRET_SENTINEL"); }
        write(&root, ".mcp.json", "{ // native JSONC\n\"mcpServers\":{\"shared\":{\"url\":\"SECRET_SENTINEL\"},},}");
        write(&root, "mcp/other.json", r#"{"third":{"command":"SECRET_SENTINEL"}}"#);
        write(&root, "mcp/package.json", r#"{"name":"native-server","version":"1.0.0","private":true,"bin":{"server":"SECRET_SENTINEL"}}"#);
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 1); assert_eq!(result.mcps.len(), 3);
        let plugin = &result.plugins[0]; assert_eq!(plugin.presence, PluginPresence::Installed); assert_eq!(plugin.enabled, None);
        assert_eq!(plugin.components.iter().filter(|component| component.kind == PluginComponentKind::Agent).count(), 2);
        for kind in [PluginComponentKind::Skill, PluginComponentKind::Command, PluginComponentKind::Rule, PluginComponentKind::Hook, PluginComponentKind::Lsp] {
            assert_eq!(plugin.components.iter().filter(|component| component.kind == kind).count(), 1);
        }
        assert!(result.mcps.iter().any(|mcp| mcp.server_id == "mcpServers"));
        assert!(!serde_json::to_string(&(&result.plugins, &result.mcps)).unwrap().contains("SECRET_SENTINEL"));
    }

    #[test]
    fn native_global_scope_and_timestamp_precedence_select_only_registered_snapshots() {
        let home = TestHome::new("workbuddy-records");
        let (_, mut user) = install(&home, "example", "9", "user"); user["lastUpdated"] = Value::from("2026-10-29T00:00:00.000Z");
        let (_, first) = install(&home, "example", "1", "managed"); let (_, second) = install(&home, "example", "2", "managed");
        install(&home, "unregistered", "9", "user"); let (_, local) = install(&home, "local", "1", "project");
        register(&home, serde_json::json!({"example@market":[user,first,second],"local@market":[local]}));
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 1); assert_eq!(result.plugins[0].version, "1");
        let mut ledger: Value = serde_json::from_str(&fs::read_to_string(home.home.join(".workbuddy-ai/plugins/installed_plugins.json")).unwrap()).unwrap();
        ledger["plugins"]["example@market"][2]["lastUpdated"] = Value::from("2026-10-28T00:00:01.000Z");
        register(&home, ledger["plugins"].clone()); assert_eq!(scan().plugins[0].version, "2");
    }

    #[test]
    fn unavailable_and_ambiguous_registered_packages_stay_declared_without_cache_guessing() {
        let home = TestHome::new("workbuddy-declared");
        let (_, first) = install(&home, "ambiguous", "1", "user"); let (_, mut second) = install(&home, "ambiguous", "2", "user");
        second["lastUpdated"] = Value::from("yesterday");
        let (missing, record) = install(&home, "missing", "1", "user"); fs::remove_dir_all(missing).unwrap();
        register(&home, serde_json::json!({"ambiguous@market":[first,second],"missing@market":[record]}));
        let result = scan(); assert!(result.unavailable); assert_eq!(result.plugins.len(), 2);
        assert!(result.plugins.iter().all(|plugin| plugin.presence == PluginPresence::Declared && plugin.components.is_empty() && plugin.enabled.is_none()));
    }

    #[test]
    fn standard_manifest_precedence_restricts_components_and_rejects_unknown_schemas() {
        let home = TestHome::new("workbuddy-standard"); let (root, record) = install(&home, "example", "1", "user");
        register(&home, serde_json::json!({"example@market":[record]}));
        write(&root, "plugin.json", &serde_json::json!({"$schema":SCHEMA,"name":"example","version":"1","hooks":{"Ignored":[]},"extensions":{"vendor":{"private":"SECRET_SENTINEL"}}}).to_string());
        for input in ["skills/one/SKILL.md","skills/deep/nested/SKILL.md","agents/ignored.md","rules/ignored.md","hooks/ignored.md"] { write(&root, input, "SECRET_SENTINEL"); }
        write(&root, ".mcp.json", r#"{"mcpServers":{"legacy":{"command":"SECRET_SENTINEL"}}}"#);
        write(&root, "mcp.json", &serde_json::json!({"$schema":MCP_SCHEMA,"mcpServers":{
            "good":{"type":"stdio","command":"./server.js","env":{"PRIVATE":"SECRET_SENTINEL"}},
            "bad-http":{"type":"sse","url":"http://example.com/mcp"},
            "bad-headers":{"type":"sse","url":"https://example.com","headers":{"A":"SECRET_SENTINEL","a":"SECRET_SENTINEL"}}
        }}).to_string());
        let result = scan(); assert!(result.unavailable); assert_eq!(result.plugins[0].components.len(), 2);
        assert_eq!(result.mcps.len(), 1); assert_eq!(result.mcps[0].server_id, "good");
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("SECRET_SENTINEL"));
        write(&root, "plugin.json", r#"{"$schema":"https://agent-plugins.org/schemas/2.0.0/plugin.schema.json","name":"example"}"#);
        let result = scan(); assert!(result.unavailable); assert!(result.plugins[0].components.is_empty()); assert!(result.mcps.is_empty());
    }

    #[test]
    fn packaged_marketplace_and_installed_snapshot_remain_distinct_and_mcp_apps_have_no_invented_ids() {
        let home = TestHome::new("workbuddy-bundled"); let root = home.home.join(".workbuddy-ai/plugins/cache/workbuddy-builtin/app/1");
        register(&home, serde_json::json!({"app@workbuddy-builtin":[{"scope":"user","version":"1","installPath":root}]}));
        write(&root, LEGACY_MANIFESTS[0], r#"{"name":"app","version":"1","category":"mcp-app"}"#);
        let bundle = home.home.join("bundle");
        write(&bundle, MARKETPLACE, r#"{"name":"workbuddy-builtin","plugins":[{"name":"app","source":"./mcps/app"}]}"#);
        write(&bundle, "mcps/app/.codebuddy-plugin/plugin.json", r#"{"name":"app","version":"2","category":"mcp-app"}"#);
        write(&bundle, "unlisted/.codebuddy-plugin/plugin.json", r#"{"name":"unlisted"}"#);
        let mut result = scan(); bundled(host(), &bundle, &mut result).unwrap(); normalize(&mut result);
        assert!(!result.unavailable); assert_eq!(result.plugins.len(), 2);
        assert!(result.plugins.iter().all(|plugin| plugin.plugin_id == "app@workbuddy-builtin"));
        assert!(result.plugins.iter().any(|plugin| plugin.presence == PluginPresence::Bundled && plugin.version == "2"));
        assert!(result.plugins.iter().any(|plugin| plugin.presence == PluginPresence::Installed && plugin.version == "1"));
        assert!(result.plugins.iter().all(|plugin| plugin.components.len() == 1 && plugin.components[0].kind == PluginComponentKind::McpBundle && plugin.components[0].native_id.is_none()));
        assert!(result.mcps.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn registered_and_declared_symlinks_cannot_import_outside_package_metadata() {
        let home = TestHome::new("workbuddy-symlinks"); let (root, record) = install(&home, "example", "1", "user");
        register(&home, serde_json::json!({"example@market":[record]}));
        let outside = home.home.join("outside"); write(&outside, "private.md", "SECRET_SENTINEL");
        std::os::unix::fs::symlink(&outside, root.join("agents")).unwrap();
        let result = scan(); assert!(result.unavailable); assert!(result.plugins[0].components.is_empty());
        fs::remove_dir_all(&root).unwrap(); std::os::unix::fs::symlink(&outside, &root).unwrap();
        let result = scan(); assert!(result.unavailable); assert_eq!(result.plugins[0].presence, PluginPresence::Declared);
    }
}
