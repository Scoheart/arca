//! Hermes' distinct memory-provider, native-plugin and global resource metadata.
//! Never imports Python, evaluates entry points, reads private component bodies,
//! or infers live enablement from a declaration. MCP has its own config reader.
use super::*;
use super::electron_package::Guard;
use std::ffi::OsStr;

mod runtime;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryProvider { name: String, path: String, version: String, guards: Vec<Guard> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id: String, command: String, source_commit: String,
    manifest_paths: Vec<String>, entrypoint_group: String,
    skill_marker: String, skill_excluded_dirs: Vec<String>, memory_providers: Vec<MemoryProvider>,
}
fn contract() -> &'static Contract {
    static CONTRACT: OnceLock<Contract> = OnceLock::new();
    CONTRACT.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-hermes.json"))
        .expect("audited Hermes native metadata contract"))
}

#[derive(Default)]
struct Context { scan: PluginScan, watches: BTreeSet<PathBuf> }
struct GlobalPaths { root: PathBuf }

fn exact(value: &str) -> bool { metadata_name(value).as_deref() == Some(value) }
fn basename(path: &Path) -> Result<&str, ()> { path.file_name().and_then(OsStr::to_str).filter(|s| exact(s)).ok_or(()) }
fn watch(path: &Path, ctx: &mut Context) {
    if ctx.watches.len() < MAX_SCAN_ENTRIES && checked_root(path).is_ok() { ctx.watches.insert(path.to_path_buf()); }
    else { ctx.scan.unavailable = true; }
}
fn stable(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    let (Ok(a), Ok(b)) = (before.modified(), after.modified()) else { return false; };
    if before.file_type() != after.file_type() || before.len() != after.len() || a != b { return false; }
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino()
            || before.ctime() != after.ctime() || before.ctime_nsec() != after.ctime_nsec() { return false; }
    }
    true
}
fn metadata(path: &Path, ctx: &mut Context) -> Result<Option<fs::Metadata>, ()> {
    ctx.scan.budget.visit()?; checked_root(path)?;
    match fs::symlink_metadata(path) {
        Ok(m) => Ok(Some(m)), Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None), Err(_) => Err(()),
    }
}
fn read_metadata(path: &Path, limit: u64, ctx: &mut Context) -> Result<Vec<u8>, ()> {
    ctx.scan.budget.references += 1;
    if ctx.scan.budget.references > MAX_REFERENCES { return Err(()); }
    let before = metadata(path, ctx)?.ok_or(())?;
    if !before.file_type().is_file() || before.len() > limit
        || before.len() > MAX_READ_BYTES.saturating_sub(ctx.scan.budget.bytes) as u64 { return Err(()); }
    let mut file = open_confined_file(path)?;
    if !stable(&before, &file.metadata().map_err(|_| ())?) { return Err(()); }
    let mut bytes = Vec::new(); let read = (&mut file).take(limit + 1).read_to_end(&mut bytes);
    ctx.scan.budget.bytes += bytes.len(); read.map_err(|_| ())?;
    if bytes.len() as u64 != before.len() || !stable(&before, &file.metadata().map_err(|_| ())?)
        || !stable(&before, &fs::symlink_metadata(path).map_err(|_| ())?) { return Err(()); }
    checked_root(path)?;
    Ok(bytes)
}
fn entries(path: &Path, ctx: &mut Context) -> Result<Vec<ScanEntry>, ()> {
    let Some(before) = metadata(path, ctx)? else { return Ok(Vec::new()); };
    if !before.file_type().is_dir() { return Err(()); }
    let entries = directory_entries(path, &mut ctx.scan.budget)?;
    if !stable(&before, &fs::symlink_metadata(path).map_err(|_| ())?) { return Err(()); }
    Ok(entries)
}
fn absolute_root(value: &OsStr) -> Result<PathBuf, ()> {
    let text = value.to_str().ok_or(())?; let path = PathBuf::from(value);
    if text.is_empty() || text.len() > 4096 || !path.is_absolute() || text.chars().any(char::is_control)
        || text.contains('~') || text.contains('$') || path.components().any(|c| !matches!(c, Component::RootDir | Component::Normal(_))) { return Err(()); }
    let clean: PathBuf = path.components().collect();
    if clean.as_os_str() != path.as_os_str() { return Err(()); }
    checked_root(&path)?; Ok(path)
}
fn native_home() -> Result<PathBuf, ()> {
    match std::env::var_os("HOME") { Some(value) => absolute_root(&value), None => absolute_root(dirs::home_dir().ok_or(())?.as_os_str()) }
}
fn profile_name(name: &str) -> bool {
    name.len() <= 64 && name.as_bytes().first().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}
fn paths_from(home: &Path, env_home: Option<&OsStr>, ctx: &mut Context) -> Result<GlobalPaths, ()> {
    let home = absolute_root(home.as_os_str())?;
    let default = home.join(".hermes"); let sticky = default.join("active_profile"); watch(&sticky, ctx);
    if metadata(&sticky, ctx)?.is_some() {
        let bytes = read_metadata(&sticky, 256, ctx)?;
        let name = std::str::from_utf8(&bytes).map_err(|_| ())?.trim();
        if !name.is_empty() && name != "default" {
            if !profile_name(name) { return Err(()); }
            let root = default.join("profiles").join(name);
            if !metadata(&root, ctx)?.is_some_and(|m| m.file_type().is_dir()) { return Err(()); }
            return Ok(GlobalPaths { root });
        }
    }
    Ok(GlobalPaths { root: match env_home { Some(value) => absolute_root(value)?, None => absolute_root(default.as_os_str())? } })
}

fn component(kind: PluginComponentKind, name: impl Into<String>, path: &Path) -> PluginComponent {
    PluginComponent { kind, name: name.into(), source_path: collapse_home(path), native_id: None }
}
fn reference(name: &str, path: &Path) -> PluginComponent { component(PluginComponentKind::PluginReference, name, path) }
fn observation(host: &Host, id: String, name: String, version: String, presence: PluginPresence,
    paths: Vec<String>, components: Vec<PluginComponent>, revision: String, ctx: &mut Context) {
    if ctx.scan.plugins.len() >= MAX_ENTRIES || components.len() > MAX_COMPONENTS { ctx.scan.unavailable = true; return; }
    ctx.scan.plugins.push(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: id, name, version, revision,
        presence, enabled: None, source_paths: paths, components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() });
}
fn physical_id(scope: &str, path: &Path) -> String { format!("@{scope}/{}", hex::encode(Sha256::digest(collapse_home(path).as_bytes()))) }
fn failure(host: &Host, scope: &str, path: &Path, message: &str, ctx: &mut Context) {
    ctx.scan.unavailable = true;
    observation(host, physical_id(scope, path), basename(path).unwrap_or("Unverified metadata").into(), "unknown".into(),
        PluginPresence::Declared, vec![collapse_home(path)], vec![reference(message, path)], String::new(), ctx);
}

fn bundled(host: &Host, c: &Contract, root: &Path, ctx: &mut Context) {
    for provider in &c.memory_providers {
        let expected = format!("plugins/memory/{}", provider.name);
        let names: BTreeSet<_> = provider.guards.iter().map(|g| g.path.as_str()).collect();
        let expected_init = format!("{expected}/__init__.py"); let expected_yaml = format!("{expected}/plugin.yaml");
        let mut verified = exact(&provider.name) && provider.path == expected && names.len() == 2
            && names.contains(expected_init.as_str()) && names.contains(expected_yaml.as_str());
        let mut before = Vec::new(); let mut paths = Vec::new();
        for guard in &provider.guards {
            let Ok(path) = relative(root, &guard.path) else { verified = false; continue; };
            watch(&path, ctx); paths.push(collapse_home(&path));
            match fs::symlink_metadata(&path) {
                Ok(m) => { before.push((path.clone(), m)); if verify_artifact(&path, guard.bytes, &guard.sha256, &mut ctx.scan.budget).is_err() { verified = false; } },
                Err(_) => verified = false,
            }
        }
        for (path, before) in before { if !fs::symlink_metadata(path).is_ok_and(|after| stable(&before, &after)) { verified = false; } }
        let path = root.join(&expected_init);
        if !verified { ctx.scan.unavailable = true; }
        observation(host, format!("@memory/{}", provider.name), provider.name.clone(), provider.version.clone(),
            if verified { PluginPresence::Bundled } else { PluginPresence::Declared }, paths,
            vec![component(if verified { PluginComponentKind::Code } else { PluginComponentKind::PluginReference },
                if verified { "Bundled memory provider; availability and activation unobserved" } else { "Memory provider source identity unverified" }, &path)],
            provider.guards.iter().map(|g| g.sha256.as_str()).collect::<String>(), ctx);
    }
}

struct Manifest { name: String, version: String, tools: Vec<String>, hooks: Vec<String> }
fn manifest(bytes: &[u8], fallback: &str) -> Result<Manifest, ()> {
    let value: serde_yaml::Value = serde_yaml::from_slice(bytes).map_err(|_| ())?;
    let empty = serde_yaml::Mapping::new();
    let map = match &value { serde_yaml::Value::Mapping(map) => map, serde_yaml::Value::Null => &empty, _ => return Err(()) };
    let get = |key: &str| map.get(&serde_yaml::Value::String(key.into()));
    let name = match get("name") { None => fallback, Some(serde_yaml::Value::String(s)) if exact(s) => s, _ => return Err(()) };
    if !exact(name) { return Err(()); }
    let version = match get("version") { None => "unknown", Some(serde_yaml::Value::String(s)) if s.is_empty() => "unknown",
        Some(serde_yaml::Value::String(s)) if exact(s) => s, _ => return Err(()) };
    let list = |key| -> Result<Vec<String>, ()> {
        match get(key) {
            None => Ok(Vec::new()), Some(serde_yaml::Value::Sequence(values)) if values.len() <= 128 => values.iter()
                .map(|v| v.as_str().filter(|s| exact(s)).map(str::to_owned).ok_or(())).collect(), _ => Err(()),
        }
    };
    Ok(Manifest { name: name.into(), version: version.into(), tools: list("provides_tools")?, hooks: list("provides_hooks")? })
}
fn user_plugins(host: &Host, c: &Contract, home: &Path, ctx: &mut Context) -> Result<(), ()> {
    let root = home.join("plugins"); watch(&root, ctx);
    for entry in entries(&root, ctx)? {
        if !entry.directory {
            if !entry.file { failure(host, "user", &entry.path, "Plugin directory link or type unverified", ctx); }
            continue;
        }
        let mut selected = None; let mut bad = false;
        for name in &c.manifest_paths {
            let path = entry.path.join(name); watch(&path, ctx);
            match metadata(&path, ctx) {
                Ok(Some(_)) => { selected = Some(path); break; },
                Ok(None) => {}, Err(()) => { bad = true; break; },
            }
        }
        let init = entry.path.join("__init__.py"); watch(&init, ctx);
        if bad { failure(host, "user", &entry.path, "Plugin manifest path unverified", ctx); continue; }
        let Some(path) = selected else { continue; };
        let parsed = read_metadata(&path, 65_536, ctx).and_then(|bytes| {
            let parsed = manifest(&bytes, basename(&entry.path)?)?; Ok((parsed, hex::encode(Sha256::digest(bytes))))
        });
        let (parsed, digest) = match parsed { Ok(value) => value, Err(()) => { failure(host, "user", &entry.path, "Plugin manifest shape or identity unverified", ctx); continue; } };
        let mut components = Vec::new();
        for tool in parsed.tools { components.push(component(PluginComponentKind::Tool, format!("Declared tool: {tool}; availability unobserved"), &path)); }
        for hook in parsed.hooks { components.push(component(PluginComponentKind::Hook, format!("Declared hook: {hook}; registration unobserved"), &path)); }
        let has_init = metadata(&init, ctx).is_ok_and(|m| m.is_some_and(|m| m.file_type().is_file()));
        if !has_init { ctx.scan.unavailable = true; }
        components.push(component(if has_init { PluginComponentKind::Code } else { PluginComponentKind::PluginReference },
            if has_init { "Plugin entry file; import/register unobserved" } else { "Missing or unverified __init__.py; registration unobserved" }, &init));
        observation(host, physical_id("user", &entry.path), parsed.name, parsed.version,
            if has_init { PluginPresence::Installed } else { PluginPresence::Declared }, vec![collapse_home(&path), collapse_home(&init)], components, digest, ctx);
    }
    Ok(())
}

fn entrypoint_names(bytes: &[u8], group: &str) -> Result<Vec<(usize, String)>, ()> {
    static TARGET: OnceLock<regex::Regex> = OnceLock::new();
    let target = TARGET.get_or_init(|| regex::Regex::new(r"^[A-Za-z_][A-Za-z_0-9]*(?:\.[A-Za-z_][A-Za-z_0-9]*)*(?::[A-Za-z_][A-Za-z_0-9]*(?:\.[A-Za-z_][A-Za-z_0-9]*)*)?(?:\s*\[[A-Za-z0-9_, .-]+\])?$").expect("static Python entrypoint metadata grammar"));
    let text = std::str::from_utf8(bytes).map_err(|_| ())?; let mut active = false; let mut seen = BTreeSet::new(); let mut result = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim(); if line.is_empty() || line.starts_with('#') || line.starts_with(';') { continue; }
        if line.starts_with('[') {
            let section = line.strip_prefix('[').and_then(|v| v.strip_suffix(']')).ok_or(())?; active = section == group; continue;
        }
        if !active { continue; }
        let (name, value) = line.split_once('=').ok_or(())?; let name = name.trim(); let value = value.trim();
        if !exact(name) || !seen.insert(name.to_owned()) || value.len() > 4096 || !target.is_match(value) || result.len() >= 512 { return Err(()); }
        result.push((line_number + 1, name.to_owned()));
    }
    Ok(result)
}
fn entrypoints(host: &Host, c: &Contract, site: &Path, ctx: &mut Context) -> Result<(), ()> {
    for entry in entries(site, ctx)? {
        if !entry.path.file_name().and_then(OsStr::to_str).is_some_and(|s| s.ends_with(".dist-info")) { continue; }
        if !entry.directory { if !entry.file { failure(host, "entrypoint", &entry.path, "Distribution metadata directory unverified", ctx); } continue; }
        let path = entry.path.join("entry_points.txt"); watch(&path, ctx);
        match metadata(&path, ctx) { Ok(None) => continue, Err(()) => { failure(host, "entrypoint", &path, "Entry-point metadata path unverified", ctx); continue; }, _ => {} }
        match read_metadata(&path, 65_536, ctx).and_then(|bytes| Ok((entrypoint_names(&bytes, &c.entrypoint_group)?, hex::encode(Sha256::digest(bytes))))) {
            Ok((names, digest)) => for (line, name) in names {
                observation(host, format!("{}:{line}", physical_id("entrypoint", &path)), name, "unknown".into(), PluginPresence::Installed,
                    vec![collapse_home(&path)], vec![reference("Python entry-point declaration; import/register unobserved", &path)], digest.clone(), ctx);
            },
            Err(()) => failure(host, "entrypoint", &path, "Entry-point registration shape unverified", ctx),
        }
    }
    Ok(())
}

fn skills(c: &Contract, root: &Path, depth: usize, components: &mut Vec<PluginComponent>, ctx: &mut Context) -> Result<(), ()> {
    if depth > 16 || components.len() >= MAX_COMPONENTS { return Err(()); }
    watch(root, ctx);
    for entry in entries(root, ctx)? {
        let name = basename(&entry.path)?;
        if entry.directory {
            if !c.skill_excluded_dirs.iter().any(|v| v == name) {
                if skills(c, &entry.path, depth + 1, components, ctx).is_err() {
                    ctx.scan.unavailable = true;
                    if components.len() < MAX_COMPONENTS { components.push(reference("Skill directory unverified", &entry.path)); }
                }
            }
        } else if name == c.skill_marker {
            watch(&entry.path, ctx);
            if entry.file && metadata(&entry.path, ctx)?.is_some_and(|m| m.file_type().is_file()) {
                let label = basename(root)?;
                components.push(component(PluginComponentKind::Skill, format!("Skill file candidate: {label}; activation unobserved"), &entry.path));
            } else { ctx.scan.unavailable = true; components.push(reference("Skill file link or type unverified", &entry.path)); }
        } else if !entry.file { ctx.scan.unavailable = true; components.push(reference("Skill path link or type unverified", &entry.path)); }
        if components.len() >= MAX_COMPONENTS { return Err(()); }
    }
    Ok(())
}
fn global_resources(host: &Host, c: &Contract, root: &Path, ctx: &mut Context) {
    let config = root.join("config.yaml"); let local = root.join("skills"); watch(&config, ctx);
    let mut components = vec![reference("Global settings path; external Skills and CLI/dotenv selection unobserved", &config)];
    if skills(c, &local, 0, &mut components, ctx).is_err() {
        ctx.scan.unavailable = true;
        if components.len() < MAX_COMPONENTS { components.push(reference("Global Skills path unverified", &local)); }
    }
    observation(host, "@global/resources".into(), "Hermes global resource candidates".into(), "unknown".into(), PluginPresence::Local,
        vec![collapse_home(&config), collapse_home(&local)], components, String::new(), ctx);
}

fn observe_bound(host: &Host, c: &Contract, bound: &runtime::BoundSource, home: &Path, env_home: Option<&OsStr>, result: &mut PluginScan) -> BTreeSet<PathBuf> {
    // Each independently bound observer gets the same allowance in all-Agent and
    // single-Agent queries. Previous Agents' budgets remain untouched.
    let mut ctx = Context::default(); bundled(host, c, &bound.root, &mut ctx);
    match paths_from(home, env_home, &mut ctx) {
        Ok(paths) => {
            if user_plugins(host, c, &paths.root, &mut ctx).is_err() { ctx.scan.unavailable = true; }
            global_resources(host, c, &paths.root, &mut ctx);
        }, Err(()) => ctx.scan.unavailable = true,
    }
    if entrypoints(host, c, &bound.site_packages, &mut ctx).is_err() { ctx.scan.unavailable = true; }
    result.plugins.extend(ctx.scan.plugins); result.unavailable |= ctx.scan.unavailable; ctx.watches
}
fn valid_contract(host: &Host, c: &Contract) -> bool {
    host.agent_id == c.agent_id && c.agent_id == "hermes" && c.command == "hermes"
        && c.source_commit == "ff6a86cb529a372198b4b80d5e022e32a4a3f2cc"
        && c.manifest_paths == ["plugin.yaml", "plugin.yml"] && c.entrypoint_group == "hermes_agent.plugins"
        && c.skill_marker == "SKILL.md" && c.memory_providers.len() <= 32
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let c = contract(); if !valid_contract(host, c) { result.unavailable = true; return; }
    let Some(selected) = crate::application::agent_launch::find_command(&c.command) else { return; };
    let Ok(bound) = runtime::bind(&selected) else { result.unavailable = true; return; };
    let Ok(home) = native_home() else {
        let mut ctx = Context::default(); bundled(host, c, &bound.root, &mut ctx);
        let _ = entrypoints(host, c, &bound.site_packages, &mut ctx);
        result.plugins.extend(ctx.scan.plugins); result.unavailable = true; return;
    };
    let env_home = std::env::var_os("HERMES_HOME"); observe_bound(host, c, &bound, &home, env_home.as_deref(), result);
}
pub(super) fn runtime_version(selected: &Path) -> Option<String> { runtime::runtime_version(selected) }
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let c = contract(); if !valid_contract(host, c) { return Vec::new(); }
    let mut paths: BTreeSet<_> = crate::application::agent_launch::command_candidates(&c.command).into_iter().collect();
    if let Some(selected) = crate::application::agent_launch::find_command(&c.command) {
        paths.extend(runtime::watch_paths(&selected));
        if let (Ok(bound), Ok(home)) = (runtime::bind(&selected), native_home()) {
            let env_home = std::env::var_os("HERMES_HOME");
            paths.extend(observe_bound(host, c, &bound, &home, env_home.as_deref(), &mut PluginScan::default()));
        }
    }
    paths.into_iter().collect()
}

#[cfg(test)]
mod tests;
