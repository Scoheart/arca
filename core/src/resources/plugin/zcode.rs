//! ZCode's declarative Node adapter observed without calling its config loader,
//! atomic recovery, component materialization, hooks or transport constructors.
use super::*;

const OFFICIAL: &str = "zcode-plugins-official";
const PARTITION: &str = "marketplaces/zcode-plugins-official/bundled-marketplace.json";
const EVENTS: [&str; 7] = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PermissionRequest", "PostToolUse", "PostToolUseFailure", "Stop"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeArtifact {
    version: String, bytes: u64, sha256: String, server_id: String,
    owner_plugin_id: String, consumer_plugin_ids: Vec<String>,
}

fn runtime_artifact() -> &'static RuntimeArtifact {
    static ARTIFACT: OnceLock<RuntimeArtifact> = OnceLock::new();
    ARTIFACT.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-zcode-runtime.json"))
        .expect("audited ZCode runtime descriptor must be valid"))
}

fn name(input: &str) -> Result<String, ()> {
    let input = input.trim();
    if input.is_empty() || input.len() > 128 || !input.as_bytes()[0].is_ascii_alphanumeric()
        || !input.bytes().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || b"._-".contains(&ch)) { return Err(()); }
    Ok(input.to_string())
}

fn root_path(source: &Root) -> PathBuf {
    if source.path.starts_with("/Applications/") { crate::paths::system_probe_path(&source.path) } else { expand_tilde(&source.path) }
}

fn absolute(input: &str, home_relative: bool) -> Result<PathBuf, ()> {
    let path = if home_relative && input.starts_with("~/") { expand_tilde(input) } else { PathBuf::from(input) };
    if input.is_empty() || input.chars().any(char::is_control) || !path.is_absolute() { return Err(()); }
    let mut normalized = PathBuf::new();
    for part in path.components() { match part { Component::ParentDir => { normalized.pop(); }, Component::CurDir => {}, _ => normalized.push(part) } }
    let path = normalized;
    checked_root(&path)?; Ok(path)
}

fn component_path(root: &Path, input: &str) -> Result<PathBuf, ()> {
    if Path::new(input).is_absolute() || input.chars().any(char::is_control) { return Err(()); }
    let mut tail = PathBuf::new();
    for part in Path::new(input).components() { match part {
        Component::Normal(value) => tail.push(value), Component::CurDir => {},
        Component::ParentDir => if !tail.pop() { return Err(()); }, _ => return Err(()),
    }}
    if tail.to_string_lossy().starts_with("..") { return Err(()); }
    let path = root.join(tail); checked_root(&path)?; Ok(path)
}

fn exists(path: &Path) -> Result<bool, ()> {
    checked_root(path)?;
    match fs::symlink_metadata(path) { Ok(_) => Ok(true), Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false), _ => Err(()) }
}

fn recovering(path: &Path) -> Result<bool, ()> {
    let parent = path.parent().ok_or(())?; let leaf = path.file_name().and_then(|v| v.to_str()).ok_or(())?;
    Ok(exists(&parent.join(format!(".{leaf}.transaction.json")))? || exists(&parent.join(format!(".{leaf}.backup")))?)
}

struct Candidate { root: PathBuf, market: String, hint: Option<String>, version: Option<String>, presence: PluginPresence, sources: Vec<String> }

fn location_revision(path: &Path) -> String {
    // Public CLI projections redact source_paths. Keep arbitrary native paths
    // out of the unredacted revision field as well.
    format!("location-sha256:{}",hex::encode(Sha256::digest(path.to_string_lossy().as_bytes())))
}

fn add(kind: PluginComponentKind, label: &str, path: &Path, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if components.len() >= MAX_COMPONENTS { return Err(()); }
    components.push(PluginComponent { kind, name:metadata_name(label).ok_or(())?, source_path:collapse_home(path), native_id:None }); Ok(())
}

fn label(root: &Path, path: &Path) -> Result<String, ()> {
    let tail = path.strip_prefix(root).map_err(|_| ())?;
    Ok(if tail.as_os_str().is_empty() { ".".into() } else { tail.to_string_lossy().into_owned() })
}

fn component_dirs(root: &Path, value: Option<&Value>, default: &str) -> Result<Vec<PathBuf>, ()> {
    let mut paths = BTreeSet::new();
    let path = relative(root, default)?;
    if exists(&path)? { paths.insert(path); }
    let inputs = match value {
        None | Some(Value::Object(_)) => Vec::new(), Some(Value::String(input)) => vec![input.as_str()],
        Some(Value::Array(inputs)) if inputs.len() <= 64 => inputs.iter().filter_map(Value::as_str).collect(), _ => return Err(()),
    };
    for input in inputs { paths.insert(component_path(root,input)?); }
    Ok(paths.into_iter().collect())
}

fn skills(root: &Path, value: Option<&Value>, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    for path in component_dirs(root, value, "skills")? {
        let marker = path.join("SKILL.md");
        if regular_metadata_file(&marker)? { add(PluginComponentKind::Skill, &label(root,&path)?, &marker, components)?; }
        for entry in directory_entries(&path, budget)? {
            let leaf = entry.path.file_name().and_then(|value| value.to_str()).ok_or(())?;
            if !entry.directory || ["node_modules", "dist", "build", "out", "target", "vendor", "coverage", ".cache", ".next", ".turbo", ".venv", "__pycache__"].contains(&leaf)
                || (leaf.starts_with('.') && leaf != ".system") { continue; }
            let marker = entry.path.join("SKILL.md");
            if regular_metadata_file(&marker)? { add(PluginComponentKind::Skill, &label(root,&entry.path)?, &marker, components)?; }
        }
    }
    Ok(())
}

fn markdown(root: &Path, manifest_path: &Path, value: Option<&Value>, folder: &str, kind: PluginComponentKind, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if let Some(Value::Object(items)) = value {
        if items.len() > MAX_COMPONENTS { return Err(()); }
        for label in items.keys() { if !label.trim().is_empty() { add(kind.clone(), label, manifest_path, components)?; } }
    }
    for path in component_dirs(root, value, folder)? {
        for entry in directory_entries(&path, budget)? {
            if entry.file && entry.path.extension().is_some_and(|value| value == "md") {
                // Paths are inventory labels; native frontmatter naming and
                // runtime de-duplication are not inferred without reading bodies.
                add(kind.clone(), &entry.path.strip_prefix(root).map_err(|_| ())?.to_string_lossy(), &entry.path, components)?;
            }
        }
    }
    Ok(())
}

fn mcp_object(value: &Value, source: &Path, servers: &mut BTreeMap<String, PathBuf>) -> Result<(), ()> {
    let value = value.as_object().ok_or(())?;
    let values = value.get("mcpServers").and_then(Value::as_object).unwrap_or(value);
    if values.len() > MAX_COMPONENTS { return Err(()); }
    for (label, declaration) in values {
        if !declaration.is_object() { continue; }
        metadata_name(label).ok_or(())?; servers.insert(label.clone(), source.to_path_buf());
    }
    if servers.len() > MAX_COMPONENTS { return Err(()); } Ok(())
}

fn mcp_manifest(root: &Path, manifest_path: &Path, value: &Value, servers: &mut BTreeMap<String, PathBuf>, depth: usize, budget: &mut ScanBudget) -> Result<(), ()> {
    budget.visit()?; if depth > 8 { return Err(()); }
    match value {
        Value::String(input) => {
            let path = component_path(root,input)?;
            let value = optional_json(root, path.strip_prefix(root).map_err(|_| ())?.to_str().ok_or(())?, budget)?.ok_or(())?;
            mcp_object(&value, &path, servers)
        },
        Value::Array(items) if items.len() <= 64 => { for item in items { mcp_manifest(root, manifest_path, item, servers, depth+1, budget)?; } Ok(()) },
        _ => mcp_object(value, manifest_path, servers),
    }
}

fn hook_object(value: &Value, path: &Path, wrapped: bool, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    let hooks = if wrapped { value.get("hooks").ok_or(())? } else { value }.as_object().ok_or(())?;
    if hooks.len() > MAX_COMPONENTS { return Err(()); }
    for event in hooks.keys().filter(|event| EVENTS.contains(&event.as_str())) { add(PluginComponentKind::Hook, event, path, components)?; }
    Ok(())
}

fn hooks(root: &Path, manifest_path: &Path, value: Option<&Value>, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let mut files = BTreeSet::new();
    if let Some(value) = optional_json(root, "hooks/hooks.json", budget)? {
        let path = root.join("hooks/hooks.json"); hook_object(&value, &path, true, components)?; files.insert(path);
    }
    let inputs = match value { None => Vec::new(), Some(Value::Array(items)) if items.len() <= 64 => items.iter().collect(), Some(value) => vec![value] };
    for value in inputs {
        if let Some(input) = value.as_str() {
            let path = component_path(root,input)?;
            if files.insert(path.clone()) { let value = optional_json(root, path.strip_prefix(root).map_err(|_| ())?.to_str().ok_or(())?, budget)?.ok_or(())?; hook_object(&value, &path, true, components)?; }
        } else { hook_object(value, manifest_path, false, components)?; }
    }
    Ok(())
}

fn apps(value: Option<&Value>, path: &Path, servers: &BTreeMap<String, PathBuf>, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    let Some(value) = value else { return Ok(()); }; let value = value.as_object().ok_or(())?;
    let Some(surfaces) = value.get("surfaces") else { return Ok(()); }; let surfaces = surfaces.as_array().filter(|items| items.len() <= MAX_COMPONENTS).ok_or(())?;
    let mut ids = BTreeSet::new();
    for surface in surfaces {
        let id = surface.get("id").and_then(Value::as_str).ok_or(())?.trim();
        if id.is_empty() || id.len() > 128 || !id.as_bytes()[0].is_ascii_alphanumeric() || !id.bytes().all(|ch| ch.is_ascii_alphanumeric() || b"_.-".contains(&ch)) { return Err(()); }
        let title = surface.get("title").ok_or(())?;
        let valid_title = title.as_str().is_some_and(|v| !v.trim().is_empty()) || title.as_object().is_some_and(|v| v.values().any(|v| v.as_str().is_some_and(|v| !v.trim().is_empty())));
        let server = surface.get("server").and_then(Value::as_str).ok_or(())?.trim(); let uri = surface.get("resourceUri").and_then(Value::as_str).ok_or(())?.trim();
        if !valid_title || !servers.contains_key(server) || !uri.starts_with("ui://") || uri.encode_utf16().count() > 2048 || surface.get("availability").and_then(Value::as_str) != Some("session") { return Err(()); }
        if ids.insert(id) { add(PluginComponentKind::App, id, path, components)?; }
    }
    Ok(())
}

fn package(host: &Host, candidate: &Candidate, result: &mut PluginScan) -> Result<(), ()> {
    checked_root(&candidate.root)?;
    if recovering(&candidate.root)? { return Err(()); }
    let mut selected = None;
    for input in &host.manifest_paths {
        if exists(&relative(&candidate.root, input)?)? { selected = Some(input); break; }
    }
    let Some(input) = selected else { return if candidate.hint.is_some() { Err(()) } else { Ok(()) }; };
    let path = relative(&candidate.root, input)?;
    let value = optional_json(&candidate.root, input, &mut result.budget)?.ok_or(())?;
    let manifest = value.as_object().ok_or(())?; let name = name(manifest.get("name").and_then(Value::as_str).ok_or(())?)?;
    let version = manifest.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| "0.0.0".into());
    let mut components = Vec::new(); let mut servers = BTreeMap::new();
    match optional_json(&candidate.root, ".mcp.json", &mut result.budget) {
        Ok(Some(value)) => if mcp_object(&value, &candidate.root.join(".mcp.json"), &mut servers).is_err() { result.unavailable = true; },
        Ok(None) => {}, Err(()) => result.unavailable = true,
    }
    if let Some(value) = manifest.get("mcpServers") {
        if mcp_manifest(&candidate.root, &path, value, &mut servers, 0, &mut result.budget).is_err() { result.unavailable = true; }
    }
    for (label, source) in &servers {
        if components.len() >= MAX_COMPONENTS { return Err(()); }
        components.push(PluginComponent { kind:PluginComponentKind::Mcp, name:label.clone(), source_path:collapse_home(source), native_id:Some(format!("plugin:{name}:{label}")) });
    }
    if skills(&candidate.root, manifest.get("skills"), &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    for (field, kind) in [("agents", PluginComponentKind::Agent), ("commands", PluginComponentKind::Command)] {
        if markdown(&candidate.root, &path, manifest.get(field), field, kind, &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    }
    if hooks(&candidate.root, &path, manifest.get("hooks"), &mut components, &mut result.budget).is_err() { result.unavailable = true; }
    if apps(manifest.get("ui"), &path, &servers, &mut components).is_err() { result.unavailable = true; }
    // Native diagnostic-only declarations remain visible as metadata, with no
    // claim that this runtime executes them.
    for (field, kind) in [("channels", PluginComponentKind::Channel), ("lspServers", PluginComponentKind::Lsp), ("outputStyles", PluginComponentKind::OutputStyle), ("settings", PluginComponentKind::Context)] {
        if manifest.contains_key(field) { add(kind, field, &path, &mut components)?; }
    }
    let mut sources = candidate.sources.clone(); sources.push(collapse_home(&path)); sources.sort(); sources.dedup(); components.sort(); components.dedup();
    let revision = format!("{}{}", location_revision(&candidate.root), candidate.version.as_ref().map(|v| format!("; registered-version:{v}")).unwrap_or_default());
    result.plugins.push(NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:format!("{name}@{}",candidate.market), name, version, revision, presence:candidate.presence.clone(), enabled:None,
        source_paths:sources, components, docs_url:host.docs_url.clone(), source_fingerprint:String::new() }); Ok(())
}

fn observe_candidate(host: &Host, candidate: Candidate, result: &mut PluginScan) {
    if package(host, &candidate, result).is_err() {
        result.unavailable = true;
        if let Some(hint) = candidate.hint {
            let mut sources=candidate.sources;sources.push(collapse_home(&candidate.root));
            result.plugins.push(NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:format!("{hint}@{}",candidate.market), name:hint, version:candidate.version.unwrap_or_else(|| "unknown".into()), revision:format!("unresolved:{}",location_revision(&candidate.root)), presence:PluginPresence::Declared, enabled:None, source_paths:sources, components:Vec::new(), docs_url:host.docs_url.clone(), source_fingerprint:String::new() });
        }
    }
}

fn partition(storage: &Path, result: &mut PluginScan) -> Result<Option<Vec<(PathBuf, String)>>, ()> {
    let Some(value) = optional_json(storage, PARTITION, &mut result.budget)? else { return Ok(None); };
    if value.get("version").and_then(Value::as_u64) != Some(1) || !value.get("manifest").is_some_and(Value::is_object) { return Ok(None); }
    let mut paths = Vec::new(); let Some(items) = value.pointer("/manifest/plugins").and_then(Value::as_array) else { return Ok(Some(paths)); };
    if items.len() > MAX_ENTRIES { return Err(()); }
    for item in items {
        let Some(label) = item.get("name").and_then(Value::as_str) else { continue; };
        let label = match name(label) { Ok(label) => label, Err(()) => { result.unavailable = true; continue; } };
        let Some(input) = item.get("cachePath").and_then(Value::as_str) else { continue; };
        let path = match absolute(input,false) { Ok(path) => path, Err(()) => { result.unavailable = true; continue; } };
        let parent = storage.join("cache").join(OFFICIAL).join(&label);
        if path.strip_prefix(&parent).ok().is_none_or(|tail| tail.as_os_str().is_empty()) { result.unavailable = true; continue; }
        paths.push((path, label));
    }
    Ok(Some(paths))
}

fn registrations(storage: &Path, result: &mut PluginScan) -> Result<Vec<Candidate>, ()> {
    let Some(value) = optional_json(storage, "installed_plugins.json", &mut result.budget)? else { return Ok(Vec::new()); };
    let ledger = collapse_home(&storage.join("installed_plugins.json")); let mut records = Vec::new();
    if let Some(items) = value.get("plugins").and_then(Value::as_object) {
        if items.len() > MAX_ENTRIES { return Err(()); }
        for (id, value) in items {
            let Some((hint, market)) = id.rsplit_once('@').filter(|(a,b)| !a.is_empty() && !b.is_empty()) else { continue; };
            let values = match value { Value::Array(items) if items.len() <= MAX_ENTRIES => items.iter().collect(), _ => vec![value] };
            for value in values {
                if matches!(value.get("scope").and_then(Value::as_str),Some("project" | "local")) { continue; }
                if let Some(path) = value.get("installPath").and_then(Value::as_str).filter(|v| !v.is_empty()) {
                    records.push((hint,market,path,value.get("version").and_then(Value::as_str).unwrap_or("0.0.0")));
                }
            }
        }
    } else if let Some(items) = value.get("plugins").and_then(Value::as_array) {
        if items.len() > MAX_ENTRIES { return Err(()); }
        for value in items {
            if value.get("scope").and_then(Value::as_str) != Some("user") || ["id","name","marketplace","version","installPath","installedAt"].iter().any(|key| !value.get(*key).is_some_and(Value::is_string)) { continue; }
            records.push((value["name"].as_str().ok_or(())?,value["marketplace"].as_str().ok_or(())?,value["installPath"].as_str().ok_or(())?,value["version"].as_str().ok_or(())?));
        }
    }
    if records.len() > MAX_ENTRIES { return Err(()); }
    let mut candidates = Vec::new();
    for (hint,market,input,version) in records {
        let candidate = (|| {
            metadata_name(hint).ok_or(())?; metadata_name(market).ok_or(())?; metadata_name(version).ok_or(())?;
            let safe = |input: &str| input.encode_utf16().map(|ch| if ch < 128 && ((ch as u8).is_ascii_alphanumeric() || b"_.@-".contains(&(ch as u8))) { char::from(ch as u8) } else { '-' }).collect::<String>();
            let root = if input.is_empty() { storage.join("cache").join(safe(market)).join(safe(hint)).join(safe(version)) } else { absolute(input,false)? };
            Ok::<_, ()>(Candidate { root, market:market.into(), hint:Some(hint.into()), version:Some(version.into()), presence:PluginPresence::Installed, sources:vec![ledger.clone()] })
        })();
        match candidate { Ok(candidate) => candidates.push(candidate), Err(()) => result.unavailable = true }
    }
    Ok(candidates)
}

fn global_sources(root: &Path, result: &mut PluginScan) -> Result<(Option<PathBuf>, Vec<PathBuf>), ()> {
    let config = match optional_json(root,"cli/config.json",&mut result.budget) {
        Ok(Some(value)) if !value.is_object() => { result.unavailable = true; None },
        Ok(value) => value, Err(()) => { result.unavailable = true; None },
    };
    let env = std::env::var("ZCODE_STORAGE_DIR").ok().filter(|v| !v.trim().is_empty());
    let configured = config.as_ref().and_then(|v| v.pointer("/storage/dir")).and_then(Value::as_str);
    let mut inline = Vec::new();
    if let Some(value) = config.as_ref().and_then(|v| v.pointer("/plugins/dirs")) {
        if let Some(paths) = value.as_array().filter(|v| v.len() <= MAX_ENTRIES) { for input in paths {
            match input.as_str().ok_or(()).and_then(|v| absolute(v,false)) {
                Ok(path) => inline.push(path),
                Err(()) => result.unavailable = true,
            }
        }} else { result.unavailable = true; }
    }
    let base = match env.as_deref().or(configured) {
        Some(input) => match absolute(input,true) { Ok(path) => path, Err(()) => { result.unavailable = true; return Ok((None,inline)); } },
        None => root.to_path_buf(),
    };
    let cli = if base.file_name().is_some_and(|v| v == "cli") { base } else { base.join("cli") };
    Ok((Some(cli.join("plugins")),inline))
}

fn global(host: &Host, root: &Path, result: &mut PluginScan) -> Result<(), ()> {
    let (storage,inline) = global_sources(root,result)?;
    for path in inline { observe_candidate(host,Candidate { root:path, market:"inline".into(), hint:None, version:None, presence:PluginPresence::Local, sources:vec![collapse_home(&root.join("cli/config.json"))] },result); }
    let Some(storage) = storage else { return Ok(()); };
    match registrations(&storage,result) { Ok(candidates) => for candidate in candidates { observe_candidate(host,candidate,result); }, Err(()) => result.unavailable = true }
    let selected = match partition(&storage,result) { Ok(value) => value, Err(()) => { result.unavailable = true; None } };
    if let Some(paths) = selected {
        for (path,label) in paths { observe_candidate(host,Candidate { root:path, market:OFFICIAL.into(), hint:Some(label), version:None, presence:PluginPresence::CacheOnly, sources:vec![collapse_home(&storage.join(PARTITION))] },result); }
    } else {
        for path in dirs(&storage.join("cache").join(OFFICIAL),&mut result.budget)? {
            for path in dirs(&path,&mut result.budget)? { observe_candidate(host,Candidate { root:path, market:OFFICIAL.into(), hint:None, version:None, presence:PluginPresence::CacheOnly, sources:Vec::new() },result); }
        }
    }
    Ok(())
}

fn runtime(host: &Host, path: &Path, artifact: &RuntimeArtifact, result: &mut PluginScan) -> Result<(), ()> {
    // The shared node_repl is created in native host code, not plugin JSON.
    // Match the complete audited artifact before applying that association;
    // strings in an unknown release never become server declarations.
    if !regular_metadata_file(path)? { return Ok(()); }
    result.budget.visit()?;
    if artifact.bytes > MAX_READ_BYTES.saturating_sub(result.budget.bytes) as u64 { return Err(()); }
    let mut file = open_confined_file(path)?; let before = file.metadata().map_err(|_| ())?;
    if !before.is_file() || before.len() != artifact.bytes { return Err(()); }
    let mut digest = Sha256::new(); let mut bytes = 0u64; let mut buffer = [0u8; 65_536];
    loop {
        let count = (&mut file).take(artifact.bytes.saturating_sub(bytes)+1).read(&mut buffer).map_err(|_| ())?;
        if count == 0 { break; }
        bytes += count as u64; result.budget.bytes += count;
        if bytes > artifact.bytes { return Err(()); }
        digest.update(&buffer[..count]);
    }
    if bytes != artifact.bytes || hex::encode(digest.finalize()) != artifact.sha256 { return Err(()); }
    let after = file.metadata().map_err(|_| ())?; let bound = fs::symlink_metadata(path).map_err(|_| ())?;
    if !bound.file_type().is_file() || before.len() != after.len() || before.modified().map_err(|_| ())? != after.modified().map_err(|_| ())? { return Err(()); }
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != bound.dev() || before.ino() != bound.ino() || before.ctime() != after.ctime() || before.ctime_nsec() != after.ctime_nsec() { return Err(()); }
    }
    let source = collapse_home(path);
    for plugin in result.plugins.iter_mut().filter(|plugin| plugin.agent_id == host.agent_id && plugin.presence != PluginPresence::Declared) {
        let component = if plugin.plugin_id == artifact.owner_plugin_id {
            Some(PluginComponent { kind:PluginComponentKind::Mcp, name:artifact.server_id.clone(), source_path:source.clone(), native_id:Some(artifact.server_id.clone()) })
        } else if artifact.consumer_plugin_ids.contains(&plugin.plugin_id) {
            Some(PluginComponent { kind:PluginComponentKind::McpReference, name:artifact.server_id.clone(), source_path:source.clone(), native_id:Some(artifact.server_id.clone()) })
        } else { None };
        if let Some(component) = component {
            if plugin.components.len() >= MAX_COMPONENTS { return Err(()); }
            plugin.components.push(component); plugin.source_paths.push(source.clone());
            plugin.components.sort(); plugin.components.dedup(); plugin.source_paths.sort(); plugin.source_paths.dedup();
            plugin.revision.push_str(&format!("; host-runtime:{}; runtime-sha256:{}",artifact.version,artifact.sha256));
            // Runtime selection requires enabled browser-use or computer-use,
            // a resolved host package and native process/broker support. None
            // of those activation conditions are inferred from this snapshot.
        }
    }
    Ok(())
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    for source in host.roots.iter().filter(|source| source.layout != "zcode_runtime") {
        let root = root_path(source);
        let observed = if source.layout == "zcode_global" { global(host,&root,result) } else if source.layout == "zcode_bundled" {
            (|| { for path in dirs(&root,&mut result.budget)? { observe_candidate(host,Candidate { root:path, market:OFFICIAL.into(), hint:None, version:None, presence:PluginPresence::Bundled, sources:Vec::new() },result); } Ok(()) })()
        } else { Err(()) };
        if observed.is_err() { result.unavailable = true; }
    }
    for source in host.roots.iter().filter(|source| source.layout == "zcode_runtime") {
        if runtime(host,&root_path(source),runtime_artifact(),result).is_err() { result.unavailable = true; }
    }
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut paths = Vec::new(); let mut scan = PluginScan::default();
    for source in &host.roots {
        let root = root_path(source); paths.push(root.clone());
        if let Some(app) = root.ancestors().find(|path| path.extension().is_some_and(|value| value == "app")) { paths.push(app.to_path_buf()); }
        if source.layout == "zcode_global" {
            if let Ok((storage,inline)) = global_sources(&root,&mut scan) {
                paths.extend(inline);
                if let Some(storage) = storage {
                    paths.push(storage.clone());
                    if let Ok(candidates) = registrations(&storage,&mut scan) { paths.extend(candidates.into_iter().map(|item| item.root)); }
                }
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    use serde_json::json;

    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "zcode").unwrap() }
    fn write(root: &Path, input: &str, content: &str) {
        let path=root.join(input); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path,content).unwrap();
    }
    fn install(root: &Path, package: &str, version: &str) {
        write(root,".zcode-plugin/plugin.json",&json!({"name":package,"version":version}).to_string());
    }
    fn scan() -> PluginScan { let mut result=PluginScan::default(); observe(host(),&mut result); normalize(&mut result); result }
    fn storage(home: &TestHome) -> PathBuf { home.home.join(".zcode/cli/plugins") }
    fn cache(home: &TestHome, version: &str) -> PathBuf { storage(home).join("cache").join(OFFICIAL).join("example").join(version) }
    fn registry(home: &TestHome, plugins: Value) { write(&storage(home),"installed_plugins.json",&json!({"version":1,"plugins":plugins}).to_string()); }

    #[test]
    fn authored_names_and_bundled_cache_snapshots_stay_distinct_without_runtime_claims() {
        let home=TestHome::new("zcode-bundle-cache");let bundled=root_path(&host().roots[1]).join("different-folder");install(&bundled,"example","1");
        let cached=cache(&home,"1");install(&cached,"example","1");
        for root in [&bundled,&cached] { write(root,".mcp.json",r#"{"mcpServers":{"docs":{"command":"private-command","env":{"TOKEN":"private-token"}}}}"#); }
        write(&storage(&home),PARTITION,&json!({"version":1,"manifest":{"plugins":[{"name":"example","cachePath":cached}]}}).to_string());
        let result=scan();assert!(!result.unavailable);assert_eq!(result.plugins.len(),2);assert_eq!(result.mcps.len(),2);
        assert!(result.plugins.iter().all(|p|p.plugin_id=="example@zcode-plugins-official"&&p.enabled.is_none()));
        assert!(result.plugins.iter().any(|p|p.presence==PluginPresence::Bundled));assert!(result.plugins.iter().any(|p|p.presence==PluginPresence::CacheOnly));
        assert!(result.mcps.iter().all(|p|p.server_id=="plugin:example:docs"));let output=serde_json::to_string(&result.plugins).unwrap();assert!(!output.contains("private-command"));assert!(!output.contains("private-token"));
    }

    #[test]
    fn installed_maps_exclude_workspace_records_and_retain_registered_versions() {
        let home=TestHome::new("zcode-installed-map");let one=cache(&home,"1");let two=cache(&home,"2");install(&one,"example","1");install(&two,"example","2");
        registry(&home,json!({"example@zcode-plugins-official":[{"scope":"user","installPath":one,"version":"9"},{"scope":"project","installPath":two,"version":"2"}],"other@market":{"scope":"local","installPath":two}}));
        let result=scan();assert!(!result.unavailable);assert_eq!(result.plugins.len(),3);
        let installed=result.plugins.iter().filter(|p|p.presence==PluginPresence::Installed).collect::<Vec<_>>();assert_eq!(installed.len(),1);assert_eq!(installed[0].version,"1");assert!(installed[0].revision.contains("registered-version:9"));
    }

    #[test]
    fn empty_valid_partition_blocks_stale_cache_fallback_and_invalid_wrapper_allows_it() {
        let home=TestHome::new("zcode-partition");install(&cache(&home,"1"),"example","1");
        write(&storage(&home),PARTITION,r#"{"version":1,"manifest":{"plugins":[]}}"#);assert!(scan().plugins.is_empty());
        write(&storage(&home),PARTITION,r#"{"version":2,"manifest":{"plugins":[]}}"#);assert_eq!(scan().plugins.len(),1);
        let outside=home.home.join("outside");install(&outside,"escaped","1");
        write(&storage(&home),PARTITION,&json!({"version":1,"manifest":{"plugins":[{"name":"example","cachePath":outside}]}}).to_string());
        let result=scan();assert!(result.unavailable);assert!(result.plugins.is_empty());
    }

    #[test]
    fn user_storage_and_inline_paths_are_selected_and_watched_without_project_resolution() {
        let home=TestHome::new("zcode-storage");let base=home.home.join("custom/cli");let package=base.join("plugins/cache/zcode-plugins-official/example/1");install(&package,"example","1");
        let inline=home.home.join("local-package");install(&inline,"local","1");
        write(&home.home,".zcode/cli/config.json",&json!({"storage":{"dir":base},"plugins":{"dirs":[inline,"relative-unknown"],"options":{"TOKEN":"private-options"}}}).to_string());
        let result=scan();assert!(result.unavailable);assert_eq!(result.plugins.len(),2);assert!(result.plugins.iter().any(|p|p.presence==PluginPresence::Local));
        let paths=watch_paths(host());assert!(paths.contains(&base.join("plugins")));assert!(paths.contains(&inline));assert!(!paths.contains(&home.home.join("relative-unknown")));
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private-options"));assert!(!base.join("cli").exists());
        assert!(result.plugins.iter().all(|p|!p.revision.contains(home.home.to_str().unwrap())));
        write(&home.home,".zcode/cli/config.json",&json!({"storage":{"dir":"relative-unknown"},"plugins":{"dirs":[inline]}}).to_string());
        let result=scan();assert!(result.unavailable);assert_eq!(result.plugins.len(),1);assert_eq!(result.plugins[0].presence,PluginPresence::Local);assert!(watch_paths(host()).contains(&inline));
    }

    #[test]
    fn root_and_child_skills_combine_defaults_and_authored_paths_without_reading_bodies() {
        let home=TestHome::new("zcode-components");let root=cache(&home,"1");
        write(&root,".zcode-plugin/plugin.json",r#"{"name":"example","skills":["custom"],"commands":{"inline":{"prompt":"private prompt"}}}"#);
        for path in ["skills/SKILL.md","skills/one/SKILL.md","skills/one/examples/demo/SKILL.md","skills/.hidden/SKILL.md","skills/.system/SKILL.md","skills/dist/SKILL.md","custom/two/SKILL.md","commands/normal.md","commands/nested/example.md"] { write(&root,path,"private component body"); }
        let result=scan();assert!(!result.unavailable);let items=&result.plugins[0].components;
        assert_eq!(items.iter().filter(|c|c.kind==PluginComponentKind::Skill).count(),4);assert_eq!(items.iter().filter(|c|c.kind==PluginComponentKind::Command).count(),2);
        let output=serde_json::to_string(&result.plugins).unwrap();assert!(!output.contains("private prompt"));assert!(!output.contains("private component body"));
    }

    #[test]
    fn mcp_merge_retains_last_provenance_native_namespace_and_static_session_apps() {
        let home=TestHome::new("zcode-mcp");let root=cache(&home,"1");
        write(&root,".zcode-plugin/plugin.json",r#"{"name":"example","mcpServers":["extra.json",{"first":{"command":"private inline"}}],"ui":{"surfaces":[{"id":"document-view","title":"View","server":"first","resourceUri":"ui://example/view","availability":"session"}]}}"#);
        write(&root,".mcp.json",r#"{"mcpServers":{"first":{"command":"private default"},"second":{"url":"https://private.invalid"}}}"#);
        write(&root,"extra.json",r#"{"first":{"command":"private file"},"third":{"command":"private command"},"ignore":1}"#);
        let result=scan();assert!(!result.unavailable);assert_eq!(result.mcps.len(),3);let items=&result.plugins[0].components;
        assert!(items.iter().any(|c|c.kind==PluginComponentKind::App&&c.name=="document-view"));
        let first=items.iter().find(|c|c.kind==PluginComponentKind::Mcp&&c.name=="first").unwrap();assert_eq!(first.native_id.as_deref(),Some("plugin:example:first"));assert!(first.source_path.ends_with(".zcode-plugin/plugin.json"));
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private"));
    }

    #[test]
    fn hook_event_names_and_diagnostic_only_declarations_exclude_handlers_and_settings() {
        let home=TestHome::new("zcode-hooks");let root=cache(&home,"1");
        write(&root,".zcode-plugin/plugin.json",r#"{"name":"example","hooks":{"PreToolUse":[{"command":"private handler"}],"Unsupported":[]},"settings":{"secret":"private setting"},"lspServers":"private-lsp"}"#);
        let result=scan();assert!(!result.unavailable);assert_eq!(result.plugins[0].components.len(),3);assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private"));
    }

    #[test]
    fn recovery_markers_keep_an_unresolved_registration_without_renaming_or_deleting() {
        let home=TestHome::new("zcode-recovery");let root=home.home.join("package");install(&root,"example","1");
        write(&home.home,".package.transaction.json",r#"{"stageName":"owned-by-host"}"#);write(&home.home,".package.backup/keep","backup body");
        registry(&home,json!([{"id":"example@market","name":"example","marketplace":"market","version":"1","installPath":root,"installedAt":"date","scope":"user"}]));
        let result=scan();assert!(result.unavailable);assert_eq!(result.plugins.len(),1);assert_eq!(result.plugins[0].presence,PluginPresence::Declared);assert!(result.plugins[0].components.is_empty());
        assert_eq!(fs::read_to_string(home.home.join(".package.backup/keep")).unwrap(),"backup body");assert!(home.home.join(".package.transaction.json").exists());
    }

    #[test]
    fn shared_host_mcp_is_digest_bound_and_never_implies_consumer_activation() {
        let home=TestHome::new("zcode-shared-runtime");
        for name in ["node-repl-host","browser-use","computer-use"] {
            install(&storage(&home).join("cache").join(OFFICIAL).join(name).join("1"),name,"1");
        }
        let path=home.home.join("host.cjs");let content=b"inert audited fixture";fs::write(&path,content).unwrap();
        let artifact=RuntimeArtifact { version:"fixture".into(),bytes:content.len() as u64,sha256:hex::encode(Sha256::digest(content)),server_id:"node_repl".into(),
            owner_plugin_id:format!("node-repl-host@{OFFICIAL}"),consumer_plugin_ids:vec![format!("browser-use@{OFFICIAL}"),format!("computer-use@{OFFICIAL}")] };
        let mut result=scan();runtime(host(),&path,&artifact,&mut result).unwrap();normalize(&mut result);
        assert_eq!(result.plugins.len(),3);assert_eq!(result.mcps.len(),1);assert_eq!(result.mcps[0].server_id,"node_repl");
        assert!(result.plugins.iter().all(|plugin|plugin.enabled.is_none()&&plugin.presence==PluginPresence::CacheOnly));
        assert_eq!(result.plugins.iter().flat_map(|plugin|&plugin.components).filter(|component|component.kind==PluginComponentKind::McpReference).count(),2);
        assert!(!result.mcps.iter().any(|server|server.server_id.contains("computer-use:")));
        fs::write(&path,b"inert changed fixture").unwrap();let mut changed=scan();assert!(runtime(host(),&path,&artifact,&mut changed).is_err());
        normalize(&mut changed);assert!(changed.mcps.is_empty());assert_eq!(changed.plugins.len(),3);
        #[cfg(unix)] { fs::remove_file(&path).unwrap();std::os::unix::fs::symlink(home.home.join("missing"),&path).unwrap();assert!(runtime(host(),&path,&artifact,&mut scan()).is_err()); }
    }

    #[test]
    #[cfg(unix)]
    fn symlinks_escaping_component_paths_and_malformed_first_manifest_never_fall_through() {
        let home=TestHome::new("zcode-confined");let root=cache(&home,"1");install(&root,"example","1");
        write(&home.home,"private.json",r#"{"escaped":{"command":"private command"}}"#);
        std::os::unix::fs::symlink(home.home.join("private.json"),root.join(".mcp.json")).unwrap();
        let result=scan();assert!(result.unavailable);assert!(result.mcps.is_empty());fs::remove_file(root.join(".mcp.json")).unwrap();
        write(&root,".zcode-plugin/plugin.json",r#"{"name":"example","skills":"../outside"}"#);assert!(scan().unavailable);
        write(&root,".zcode-plugin/plugin.json","malformed");write(&root,".claude-plugin/plugin.json",r#"{"name":"wrong-fallback"}"#);
        let result=scan();assert!(result.unavailable);assert!(result.plugins.is_empty());
    }
}
