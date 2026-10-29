//! Qoder CLI metadata. Native installation/migration, hooks, artifact extraction,
//! managed-organization gates and effective activation remain owned by Qoder.
use super::*;
use super::qoder::Package;
use std::cell::RefCell;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract { package: String, version: String, entry: String, bytes: u64, sha256: String }
fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-qoder-cli.json")).expect("audited Qoder CLI contract"))
}
fn config_root() -> PathBuf {
    std::env::var_os("QODER_CONFIG_DIR").filter(|v| !v.is_empty())
        .map(|v| expand_tilde(&v.to_string_lossy())).unwrap_or_else(|| expand_tilde("~/.qoder"))
}
fn plugins_root() -> PathBuf {
    std::env::var_os("QODER_PLUGIN_CACHE_DIR").filter(|v| !v.is_empty())
        .map(|v| expand_tilde(&v.to_string_lossy())).unwrap_or_else(|| config_root().join("plugins"))
}
fn add(p: &Package<'_>, input: &str, kind: PluginComponentKind, name: Option<&str>, out: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if out.len() >= MAX_COMPONENTS { return Err(()); }
    if p.file(input)? {
        out.push(PluginComponent { kind, name: metadata_name(name.unwrap_or(input)).ok_or(())?, source_path: p.path(input)?, native_id: None });
    }
    Ok(())
}
pub(super) fn files(p: &Package<'_>, input: &str, kind: PluginComponentKind, depth: usize, out: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if depth > 8 { return Err(()); }
    if !input.is_empty() && p.file(input)? { return add(p, input, kind, None, out); }
    for e in p.children(input, budget)? {
        let name = e.path.rsplit('/').next().ok_or(())?;
        if name.starts_with('.') || matches!(name, "node_modules" | "dist" | "target" | "__pycache__") { continue; }
        if e.directory { files(p, &e.path, kind.clone(), depth + 1, out, budget)?; }
        else if e.file && match kind {
            PluginComponentKind::Skill => name == "SKILL.md",
            PluginComponentKind::Workflow => name.ends_with(".js") || name.ends_with(".mjs"),
            PluginComponentKind::Executable => true,
            _ => name.ends_with(".md"),
        } { add(p, &e.path, kind.clone(), None, out)?; }
    }
    Ok(())
}
fn components(p: &Package<'_>, value: Option<&Value>, fallback: &str, kind: PluginComponentKind, out: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => files(p, fallback, kind, 0, out, budget),
        Some(Value::String(input)) => files(p, input, kind, 0, out, budget),
        Some(Value::Array(inputs)) if inputs.len() <= 64 => {
            for input in inputs { files(p, input.as_str().ok_or(())?, kind.clone(), 0, out, budget)?; } Ok(())
        },
        Some(Value::Object(commands)) if kind == PluginComponentKind::Command && commands.len() <= MAX_COMPONENTS => {
            for (name, command) in commands {
                // Inline content is intentionally not copied into inventory.
                if let Some(input) = command.get("source").and_then(Value::as_str) { add(p, input, kind.clone(), Some(name), out)?; }
            } Ok(())
        },
        _ => Err(()),
    }
}
fn hook_value(p: &Package<'_>, value: &Value, source: &str, out: &mut Vec<PluginComponent>) -> Result<(), ()> {
    let events = value.get("hooks").unwrap_or(value).as_object().filter(|v| v.len() <= MAX_COMPONENTS).ok_or(())?;
    for (name, records) in events {
        let records = records.as_array().filter(|v| v.len() <= MAX_COMPONENTS).ok_or(())?;
        if records.iter().any(|r| r.get("hooks").and_then(Value::as_array).is_some_and(|v| v.iter().any(Value::is_object))) {
            add(p, source, PluginComponentKind::Hook, Some(name), out)?;
        }
    }
    Ok(())
}
fn hooks(p: &Package<'_>, value: &Value, source: &str, depth: usize, out: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if depth > 8 { return Err(()); }
    match value {
        Value::String(input) => {
            let value = p.json(input, budget)?.ok_or(())?; hook_value(p, &value, input, out)
        },
        Value::Array(values) if values.len() <= 64 => {
            for value in values { hooks(p, value, source, depth + 1, out, budget)?; } Ok(())
        },
        Value::Object(_) => hook_value(p, value, source, out),
        _ => Err(()),
    }
}
fn sdk_mcp(value: &Value) -> bool {
    value.is_object()
        && ["command", "url", "type"].iter().all(|key| value.get(*key).is_none_or(Value::is_string))
        && value.get("args").is_none_or(|v| v.as_array().is_some_and(|items| items.iter().all(Value::is_string)))
        && value.get("env").is_none_or(|v| v.as_object().is_some_and(|items| items.values().all(Value::is_string)))
}
fn mcp(p: &Package<'_>, value: &Value, source: &str, depth: usize, require_connection: bool, servers: &mut BTreeMap<String, PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if depth > 8 { return Err(()); }
    match value {
        Value::String(input) => {
            let doc = p.json(input, budget)?.ok_or(())?;
            mcp(p, doc.get("mcpServers").unwrap_or(&doc), input, depth + 1, require_connection, servers, budget)
        },
        Value::Array(values) if values.len() <= 64 => {
            for value in values { mcp(p, value, source, depth + 1, require_connection, servers, budget)?; } Ok(())
        },
        Value::Object(values) if values.len() <= MAX_COMPONENTS => {
            for (name, value) in values {
                let name = metadata_name(name).ok_or(())?;
                let accepted = if require_connection { value.is_object() && ["command", "url"].iter().any(|key| value.get(*key).and_then(Value::as_str).is_some_and(|v| !v.is_empty())) } else { sdk_mcp(value) };
                if !accepted {
                    servers.remove(&name); continue;
                }
                servers.insert(name.clone(), PluginComponent { kind: PluginComponentKind::Mcp, name: name.clone(), source_path: p.path(source)?, native_id: Some(name) });
            } Ok(())
        },
        _ => Err(()),
    }
}
fn package(host: &Host, p: &Package<'_>, id: &str, presence: PluginPresence, provenance: &[String], result: &mut PluginScan) -> Result<NativePluginObservation, ()> {
    let mut manifest = None;
    for folder in [".qoder-plugin", ".claude-plugin"] {
        let input = format!("{folder}/plugin.json");
        if let Some(value) = p.json(&input, &mut result.budget)? { manifest = Some((input, value)); break; }
    }
    package_inner(host, p, id, presence, provenance, manifest, true, result)
}

pub(super) fn sdk_package(host: &Host, p: &Package<'_>, id: &str, presence: PluginPresence, provenance: &[String], manifest: Option<(String, Arc<Value>)>, result: &mut PluginScan) -> Result<NativePluginObservation, ()> {
    package_inner(host, p, id, presence, provenance, manifest, false, result)
}

fn package_inner(host: &Host, p: &Package<'_>, id: &str, presence: PluginPresence, provenance: &[String], manifest: Option<(String, Arc<Value>)>, require_connection: bool, result: &mut PluginScan) -> Result<NativePluginObservation, ()> {
    // The CLI ignores root projection manifests and supports convention-only
    // packages. Installation identity remains the exact global ledger key.
    let (source, value) = manifest.unwrap_or_else(|| (String::new(), Arc::new(serde_json::json!({}))));
    let name = value.get("displayName").or_else(|| value.get("name")).and_then(Value::as_str).and_then(metadata_name)
        .unwrap_or_else(|| id.split('@').next().unwrap_or(id).into());
    let version = value.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| "local".into());
    let mut out = Vec::new();
    for (key, folder, kind) in [
        ("skills","skills",PluginComponentKind::Skill), ("agents","agents",PluginComponentKind::Agent),
        ("commands","commands",PluginComponentKind::Command), ("outputStyles","output-styles",PluginComponentKind::OutputStyle),
        ("workflows","workflows",PluginComponentKind::Workflow), ("bin","bin",PluginComponentKind::Executable),
    ] {
        let declaration = if key == "bin" { None } else { value.get(key) };
        if components(p, declaration, folder, kind, &mut out, &mut result.budget).is_err() { result.unavailable = true; }
    }
    let hook = value.get("hooks").cloned().unwrap_or(Value::String("hooks/hooks.json".into()));
    if (value.get("hooks").is_some() || p.file("hooks/hooks.json")?) && hooks(p, &hook, &source, 0, &mut out, &mut result.budget).is_err() { result.unavailable = true; }
    let mut servers = BTreeMap::new();
    // Native CLI selects its first existing conventional file, then overlays
    // explicit declarations. This deliberately differs from the IDE adapter.
    for input in [".mcp.json", "mcp.json"] {
        if p.exists(input)? {
            if mcp(p, &Value::String(input.into()), input, 0, require_connection, &mut servers, &mut result.budget).is_err() { result.unavailable = true; }
            break;
        }
    }
    if let Some(v) = value.get("mcpServers") {
        if mcp(p, v, &source, 0, require_connection, &mut servers, &mut result.budget).is_err() { result.unavailable = true; }
    }
    out.extend(servers.into_values()); out.sort(); out.dedup();
    let mut sources = provenance.to_vec();
    if !source.is_empty() { sources.push(p.path(&source)?); }
    Ok(NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:metadata_name(id).ok_or(())?, name, version,
        revision:format!("source:{}", sources.join(";")), presence, enabled:None, source_paths:sources, components:out,
        docs_url:host.docs_url.clone(), source_fingerprint:String::new() })
}
fn installed(host: &Host, result: &mut PluginScan) -> Result<(), ()> {
    let root = plugins_root();
    if !root.is_absolute() || expand_tilde("~/").starts_with(&root) { return Err(()); }
    checked_root(&root)?;
    let Some(state) = optional_json(&root, "installed_plugins_v2.json", &mut result.budget)? else { return Ok(()); };
    if state.get("version").and_then(Value::as_u64) != Some(2) { return Err(()); }
    let plugins = state.get("plugins").and_then(Value::as_object).filter(|v| v.len() <= MAX_ENTRIES).ok_or(())?;
    for (id, records) in plugins {
        if metadata_name(id).is_none() { result.unavailable = true; continue; }
        let Some(records) = records.as_array().filter(|v| v.len() <= MAX_ENTRIES) else { result.unavailable = true; continue; };
        for (ordinal, record) in records.iter().enumerate() {
            result.budget.visit()?;
            if !matches!(record.get("scope").and_then(Value::as_str), Some("user" | "managed")) { continue; }
            let Some(path) = record.get("installPath").and_then(Value::as_str).map(PathBuf::from)
                .filter(|p| p.is_absolute() && !expand_tilde("~/").starts_with(p)) else { result.unavailable = true; continue; };
            if checked_root(&path).is_err() { result.unavailable = true; continue; }
            let provenance = vec![collapse_home(&root.join("installed_plugins_v2.json"))];
            if !path.try_exists().map_err(|_| ())? {
                result.plugins.push(NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:id.clone(), name:id.clone(),
                    version:"unknown".into(), revision:format!("registered:{ordinal}"), presence:PluginPresence::Declared, enabled:None,
                    source_paths:provenance, components:Vec::new(), docs_url:host.docs_url.clone(), source_fingerprint:String::new() }); continue;
            }
            match package(host, &Package::Disk(&path), id, PluginPresence::Installed, &provenance, result) {
                Ok(mut item) => { item.revision = format!("registered:{ordinal}:{}", collapse_home(&path)); result.plugins.push(item); },
                Err(()) => result.unavailable = true,
            }
        }
    }
    Ok(())
}
fn sites(host: &Host, root: &Path, presence: PluginPresence, result: &mut PluginScan) -> Result<(), ()> {
    let Some(artifact) = optional_json(root, "bundle/vendor/sites/artifact.json", &mut result.budget)? else { return Ok(()); };
    if artifact.get("target").and_then(Value::as_str) != Some("universal") { return Err(()); }
    let path = root.join("bundle/vendor/sites/sites.zip");
    let bytes = artifact.get("size").and_then(Value::as_u64).filter(|v| *v <= 16 * 1024 * 1024).ok_or(())?;
    let digest = artifact.get("sha256").and_then(Value::as_str).filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())).ok_or(())?;
    verify_artifact(&path, bytes, digest, &mut result.budget)?;
    let file = open_confined_file(&path)?; let mut zip = zip::ZipArchive::new(file).map_err(|_| ())?;
    if zip.len() > MAX_ENTRIES { return Err(()); }
    let mut members = BTreeMap::new(); let mut total = 0u64;
    for i in 0..zip.len() {
        result.budget.visit()?; let entry = zip.by_index(i).map_err(|_| ())?;
        let name = entry.name().trim_end_matches('/');
        let normalized = super::qoder::local_path(name)?;
        if name.is_empty() || name.contains('\\') || normalized != name
            || entry.unix_mode().is_some_and(|m| ![0,0o100000,0o040000].contains(&(m & 0o170000))) { return Err(()); }
        total = total.checked_add(entry.size()).ok_or(())?;
        if total > 8 * 1024 * 1024 || members.insert(name.into(), (i, entry.is_dir())).is_some() { return Err(()); }
    }
    for name in members.keys() {
        for parent in Path::new(name).ancestors().skip(1) {
            if members.get(parent.to_string_lossy().as_ref()).is_some_and(|(_,dir)| !dir) { return Err(()); }
        }
    }
    let package_source = Package::Archive { path:&path, prefix:"", zip:RefCell::new(zip), members };
    let item = package(host, &package_source, "qoder-sites@qodercli-bundler", presence,
        &[collapse_home(&path), collapse_home(&root.join("bundle/vendor/sites/artifact.json"))], result)?;
    result.plugins.push(item); Ok(())
}
fn distribution(host: &Host, root: &Path, presence: PluginPresence, result: &mut PluginScan) -> Result<(), ()> {
    checked_root(root)?;
    let Some(meta) = optional_json(root, "package.json", &mut result.budget)? else { return Ok(()); };
    let c = contract();
    if meta.get("name").and_then(Value::as_str) != Some(&c.package) || meta.get("version").and_then(Value::as_str) != Some(&c.version) { return Ok(()); }
    verify_artifact(&relative(root, &c.entry)?, c.bytes, &c.sha256, &mut result.budget)?;
    let folder = root.join("bundle/builtin"); let p = Package::Disk(&folder); let mut out = Vec::new();
    files(&p, "", PluginComponentKind::Skill, 0, &mut out, &mut result.budget)?;
    if !out.is_empty() { result.plugins.push(NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:"@builtin/skills".into(),
        name:"Qoder CLI packaged Skills".into(), version:c.version.clone(), revision:collapse_home(root), presence:presence.clone(), enabled:None,
        source_paths:vec![collapse_home(&root.join("package.json"))], components:out, docs_url:host.docs_url.clone(), source_fingerprint:String::new() }); }
    let security = root.join("bundle/vendor/qoder-security");
    if regular_metadata_file(&security.join(".qoder-plugin/plugin.json"))? {
        let item = package(host, &Package::Disk(&security), "security-scan@qodercli-bundler", presence.clone(), &[collapse_home(&root.join("package.json"))], result)?;
        result.plugins.push(item);
    }
    sites(host, root, presence, result)
}
fn distributions(host: &Host, result: &mut PluginScan) -> Result<(), ()> {
    // A package directly selected by a native npm launcher is bundled. Other
    // wrappers are inert; matching npm caches remain explicitly cache-only.
    if let Some(command) = crate::application::agent_launch::find_command("qodercli") {
        let executable = fs::canonicalize(command).map_err(|_| ())?;
        if executable.ends_with("bundle/qodercli.js") {
            let root = executable.parent().and_then(Path::parent).ok_or(())?;
            return distribution(host, root, PluginPresence::Bundled, result);
        }
    }
    let caches = expand_tilde("~/.npm/_npx");
    for cache in dirs(&caches, &mut result.budget)? {
        distribution(host, &cache.join("node_modules/@qoder-ai/qodercli"), PluginPresence::CacheOnly, result)?;
    }
    Ok(())
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    if installed(host, result).is_err() { result.unavailable = true; }
    if distributions(host, result).is_err() { result.unavailable = true; }
}
pub(super) fn watch_paths(_: &Host) -> Vec<PathBuf> {
    let root = plugins_root();
    if !root.is_absolute() || expand_tilde("~/").starts_with(&root) { return Vec::new(); }
    let mut paths = vec![root.clone()]; let mut budget = ScanBudget::default();
    if let Ok(Some(state)) = optional_json(&root, "installed_plugins_v2.json", &mut budget) {
        if let Some(plugins) = state.get("plugins").and_then(Value::as_object).filter(|v| v.len() <= MAX_ENTRIES) {
            for records in plugins.values().filter_map(Value::as_array) {
                for record in records.iter().take(MAX_ENTRIES) {
                    if budget.visit().is_err() { return paths; }
                    if !matches!(record.get("scope").and_then(Value::as_str), Some("user" | "managed")) { continue; }
                    if let Some(path) = record.get("installPath").and_then(Value::as_str).map(PathBuf::from) {
                        if path.is_absolute() && !expand_tilde("~/").starts_with(&path) && checked_root(&path).is_ok() { paths.push(path); }
                    }
                }
            }
        }
    }
    if let Ok(caches) = dirs(&expand_tilde("~/.npm/_npx"), &mut budget) {
        for cache in caches {
            let path = cache.join("node_modules/@qoder-ai/qodercli");
            if checked_root(&path).is_ok() && path.is_dir() { paths.push(path); }
        }
    }
    if let Some(command) = crate::application::agent_launch::find_command("qodercli") {
        paths.push(command.clone());
        if let Ok(executable) = fs::canonicalize(command) {
            if executable.ends_with("bundle/qodercli.js") {
                if let Some(root) = executable.parent().and_then(Path::parent) { paths.push(root.to_path_buf()); }
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    use std::io::Write;
    fn host() -> &'static Host { hosts().iter().find(|h| h.agent_id == "qoder-cli").unwrap() }
    fn write(root: &Path, input: &str, body: &str) {
        let p = root.join(input); fs::create_dir_all(p.parent().unwrap()).unwrap(); fs::write(p, body).unwrap();
    }
    #[test]
    fn cli_convention_precedence_and_explicit_overlay_do_not_export_values() {
        let home = TestHome::new("qoder-cli-overlay"); let root = home.home.join("plugin");
        write(&root, ".qoder-plugin/plugin.json", r#"{"name":"sample","mcpServers":{"same":{"command":"secret-overlay"},"explicit":{"url":"https://private.invalid"}}}"#);
        write(&root, ".mcp.json", r#"{"mcpServers":{"base":{"command":"secret-base"},"same":{"command":"secret-old"}}}"#);
        write(&root, "mcp.json", r#"{"mcpServers":{"ignored":{"command":"secret-ignored"}}}"#);
        let mut scan = PluginScan::default();
        let p = package(host(), &Package::Disk(&root), "sample@local", PluginPresence::Installed, &[], &mut scan).unwrap();
        assert!(!scan.unavailable);
        assert_eq!(p.components.iter().filter_map(|c| c.native_id.as_deref()).collect::<BTreeSet<_>>(), BTreeSet::from(["base","explicit","same"]));
        assert!(p.components.iter().find(|c| c.name=="same").unwrap().source_path.ends_with(".qoder-plugin/plugin.json"));
        let encoded = serde_json::to_string(&p).unwrap(); assert!(!encoded.contains("secret")); assert!(!encoded.contains("private.invalid"));
    }
    #[test]
    fn cli_ignores_root_ide_projection_and_observes_convention_without_bodies() {
        let home = TestHome::new("qoder-cli-projection"); let root = home.home.join("plugin");
        write(&root, "plugin.json", r#"{"name":"projection","skills":"examples","mcpServers":"other.json"}"#);
        write(&root, "examples/wrong/SKILL.md", "private body"); write(&root, "skills/actual/SKILL.md", "private body");
        write(&root, "other.json", r#"{"mcpServers":{"ignored":{"command":"secret"}}}"#);
        let mut scan = PluginScan::default();
        let p = package(host(), &Package::Disk(&root), "sample@local", PluginPresence::Installed, &[], &mut scan).unwrap();
        assert!(!scan.unavailable); assert_eq!(p.name,"sample"); assert_eq!(p.components.len(),1);
        assert_eq!(p.components[0].name,"skills/actual/SKILL.md"); assert!(!serde_json::to_string(&p).unwrap().contains("private body"));
    }
    #[test]
    fn sites_archive_rejects_traversal_even_with_matching_artifact_digest() {
        let home = TestHome::new("qoder-cli-sites-path"); let root=home.home.join("dist");
        let path=root.join("bundle/vendor/sites/sites.zip"); fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut zip=zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file("../escape.json",zip::write::SimpleFileOptions::default()).unwrap(); zip.write_all(b"{}").unwrap(); zip.finish().unwrap();
        let bytes=fs::read(&path).unwrap();
        write(&root,"bundle/vendor/sites/artifact.json",&serde_json::json!({"target":"universal","size":bytes.len(),"sha256":hex::encode(Sha256::digest(&bytes))}).to_string());
        let mut scan=PluginScan::default(); assert!(sites(host(),&root,PluginPresence::CacheOnly,&mut scan).is_err());
        assert!(scan.plugins.is_empty()); assert!(!home.home.join("escape.json").exists());
    }
}


