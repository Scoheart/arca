//! Source-bound CodeBuddy CLI metadata. No Node entry, lifecycle, component,
//! native plugin service, private option store or Skill body is executed/read.
use super::*;
use std::collections::VecDeque;
use std::io::{Seek, SeekFrom};
use std::sync::Mutex;

const MAX_HASH_BYTES: u64 = 128 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_VISITS: usize = 8192;
const MAX_BOUND: usize = 512;
const MAX_ITEMS: usize = 4096;
const MAX_PLUGINS: usize = 512;
const MAX_DIRECTORY: usize = 1024;
const MAX_STAT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STAT_TOTAL: u64 = 64 * 1024 * 1024;
const MAX_CACHE: usize = 32;
const METADATA_DIRS: [&str; 3] = [".codebuddy-plugin", ".workbuddy-plugin", ".claude-plugin"];
const PRODUCT_ENV: [&str; 4] = ["ACC_PRODUCT_CONFIG_PATH", "ACC_PRODUCT_CONFIG_V3", "ACC_PRODUCT_CONFIG_V2", "ACC_PRODUCT_CONFIG"];

#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Guard { path: String, bytes: u64, sha256: String }
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Skill { name: String, path: String, bytes: u64 }
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence { archive_sha256: String, research_sha256: String }
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    schema_version: u32, agent_id: String, command: String, package_name: String,
    version: String, entry: String, guards: Vec<Guard>, builtin_skills: Vec<Skill>, evidence: Evidence,
}
fn hash_text(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'a'..=b'f'))
}
fn native_text(value: &str) -> Result<String, ()> {
    if value.is_empty() || value.len() > 240 || value.chars().all(char::is_whitespace)
        || value.chars().any(char::is_control) { Err(()) } else { Ok(value.into()) }
}
fn relative_member(root: &Path, input: &str) -> Result<PathBuf, ()> {
    if input.is_empty() || input.len() > 4096 || input.contains('\\') || input.contains("://") || input.chars().any(char::is_control) { return Err(()); }
    relative(root, input)
}
fn absolute(input: &str) -> Result<PathBuf, ()> {
    let p = PathBuf::from(input);
    if !p.is_absolute() || input.len() > 4096 || input.chars().any(char::is_control)
        || p.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_))) { return Err(()); }
    Ok(p)
}
impl Contract {
    fn validate(&self) -> Result<(), ()> {
        let required = BTreeSet::from(["package.json", "bin/codebuddy", "dist/codebuddy.js",
            "dist/codebuddy-headless.js", "dist/codebuddy-lite-wb.mjs", "product.json",
            "product.cloudhosted.json", "product.internal.json", "product.ioa.json", "product.selfhosted.json"]);
        if self.schema_version != 1 || self.agent_id != "codebuddy-code" || self.command != "codebuddy"
            || self.package_name != "@tencent-ai/codebuddy-code" || self.entry != "bin/codebuddy"
            || self.version.split('.').count() != 3 || self.version.split('.').any(|p| p.parse::<u32>().is_err())
            || self.guards.len() != required.len() || !hash_text(&self.evidence.archive_sha256)
            || !hash_text(&self.evidence.research_sha256) { return Err(()); }
        let mut paths = BTreeSet::new(); let mut total = 0u64;
        for g in &self.guards {
            if !required.contains(g.path.as_str()) || !paths.insert(g.path.as_str()) || g.bytes == 0 || !hash_text(&g.sha256) { return Err(()); }
            total = total.checked_add(g.bytes).ok_or(())?;
        }
        if total > MAX_HASH_BYTES || self.builtin_skills.len() != 1 { return Err(()); }
        let skill = &self.builtin_skills[0];
        if skill.name != "mods-create" || skill.path != "builtin/skills/mods-create/SKILL.md"
            || skill.bytes == 0 || skill.bytes > 256 * 1024 { return Err(()); }
        Ok(())
    }
}
fn contract() -> Option<&'static Contract> {
    static C: OnceLock<Option<Contract>> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-codebuddy-cli.json"))
        .ok().filter(|c| c.validate().is_ok())).as_ref()
}
fn node_environment() -> Result<(), ()> {
    if ["NODE_OPTIONS", "NODE_PATH"].iter().any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty())) { return Err(()); }
    Ok(())
}
fn selected_default() -> Result<Option<PathBuf>, ()> {
    use crate::application::agent_launch::{info_unlocked, LaunchTarget};
    let info = info_unlocked("codebuddy-code").map_err(|_| ())?;
    if info.configured_target.is_some() { return Err(()); }
    match info.resolved_target {
        Some(LaunchTarget::Cli { command, args, env }) if args.is_empty() && env.is_empty() => Ok(Some(absolute(&command)?)),
        None => Ok(None), _ => Err(()),
    }
}
fn native_home() -> Result<PathBuf, ()> {
    if PRODUCT_ENV.iter().any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty())) { return Err(()); }
    let home = match std::env::var_os("CODEBUDDY_CONFIG_DIR").filter(|v| !v.to_string_lossy().trim().is_empty()) {
        Some(p) => absolute(p.to_str().ok_or(())?)?, None => expand_tilde("~/.codebuddy"),
    };
    checked_root(&home)?; Ok(home)
}
struct Selection { alias: PathBuf, physical: PathBuf, stamp: Stamp, link: Option<PathBuf> }
impl Selection {
    fn capture(alias: &Path) -> Result<Self, ()> {
        node_environment()?; absolute(alias.to_str().ok_or(())?)?; checked_root(alias.parent().ok_or(())?)?;
        let m = fs::symlink_metadata(alias).map_err(|_| ())?;
        if !m.file_type().is_file() && !m.file_type().is_symlink() { return Err(()); }
        let stamp = Stamp::capture(&m)?;
        let link = if m.file_type().is_symlink() { Some(fs::read_link(alias).map_err(|_| ())?) } else { None };
        let physical = fs::canonicalize(alias).map_err(|_| ())?; checked_root(&physical)?;
        let m = fs::symlink_metadata(&physical).map_err(|_| ())?;
        #[cfg(unix)] { use std::os::unix::fs::MetadataExt; if m.mode() & 0o111 == 0 { return Err(()); } }
        let value = Self { alias: alias.into(), physical, stamp, link }; value.revalidate()?; Ok(value)
    }
    fn revalidate(&self) -> Result<(), ()> {
        node_environment()?; checked_root(self.alias.parent().ok_or(())?)?;
        let m = fs::symlink_metadata(&self.alias).map_err(|_| ())?;
        if Stamp::capture(&m)? != self.stamp || fs::canonicalize(&self.alias).map_err(|_| ())? != self.physical { return Err(()); }
        match &self.link {
            Some(target) if m.file_type().is_symlink() && fs::read_link(&self.alias).map_err(|_| ())? == *target => {},
            None if m.file_type().is_file() => {}, _ => return Err(()),
        }
        checked_root(&self.physical)
    }
}
struct Verified { selection: Selection, root: PathBuf, directories: Vec<Bound>, members: Vec<Bound>, identity: String }
impl Verified {
    fn capture(selected: &Path, c: &Contract, cache: &Mutex<DigestCache>) -> Result<Self, ()> {
        c.validate()?; let selection = Selection::capture(selected)?;
        let root = selection.physical.parent().and_then(Path::parent).ok_or(())?.to_path_buf();
        if relative_member(&root, &c.entry)? != selection.physical { return Err(()); }
        let mut budget = Budget::default();
        let directories = [root.clone(), root.join("bin"), root.join("dist")].into_iter()
            .map(|p| Bound::capture(p, true, &mut budget)).collect::<Result<Vec<_>, _>>()?;
        let context = hex::encode(Sha256::digest(serde_json::to_vec(&(&selection.alias, &selection.stamp,
            &selection.physical, directories.iter().map(|v| (&v.path, &v.stamp)).collect::<Vec<_>>())).map_err(|_| ())?));
        let mut proof = Self { selection, root, directories, members: Vec::new(), identity: context.clone() };
        for guard in &c.guards {
            let mut file = Bound::capture(relative_member(&proof.root, &guard.path)?, false, &mut budget)?;
            if file.stamp.bytes != guard.bytes { return Err(()); }
            let key = DigestKey { context: context.clone(), path: file.path.clone(), stamp: file.stamp.clone(), expected: guard.sha256.clone() };
            if !cache.lock().map_err(|_| ())?.contains(&key) {
                if guard.bytes > MAX_HASH_BYTES.saturating_sub(budget.hashed_bytes) { return Err(()); }
                let mut hash = Sha256::new(); let mut count = 0u64; let mut buffer = [0u8; 65536];
                loop {
                    let n = (&mut file.file).take(guard.bytes.saturating_sub(count) + 1).read(&mut buffer).map_err(|_| ())?;
                    if n == 0 { break; } count += n as u64; budget.hashed_bytes += n as u64;
                    if count > guard.bytes || budget.hashed_bytes > MAX_HASH_BYTES { return Err(()); } hash.update(&buffer[..n]);
                }
                if count != guard.bytes || hex::encode(hash.finalize()) != guard.sha256 { return Err(()); }
                file.revalidate()?; proof.revalidate()?; cache.lock().map_err(|_| ())?.insert(key);
            }
            file.revalidate()?; proof.members.push(file);
        }
        let package = proof.json("package.json", &mut budget)?;
        if package.get("name").and_then(Value::as_str) != Some(c.package_name.as_str())
            || package.get("version").and_then(Value::as_str) != Some(c.version.as_str())
            || package.pointer("/bin/codebuddy").and_then(Value::as_str) != Some("./bin/codebuddy")
            || package.pointer("/publishConfig/customPackage").is_some() { return Err(()); }
        let product = proof.json("product.json", &mut budget)?;
        if product.get("platform").and_then(Value::as_str) != Some("CLI") || product.get("dataLayout").is_some() { return Err(()); }
        proof.identity = hex::encode(Sha256::digest(serde_json::to_vec(&(&context, c, proof.members.iter().map(|v| (&v.path, &v.stamp)).collect::<Vec<_>>())).map_err(|_| ())?));
        proof.revalidate()?; Ok(proof)
    }
    fn json(&mut self, name: &str, budget: &mut Budget) -> Result<Value, ()> {
        let p = self.root.join(name); let f = self.members.iter_mut().find(|v| v.path == p).ok_or(())?;
        serde_json::from_slice(&f.bytes(MAX_DOCUMENT_BYTES, budget)?).map_err(|_| ())
    }
    fn revalidate(&self) -> Result<(), ()> {
        self.selection.revalidate()?; for d in &self.directories { d.revalidate()?; }
        for f in &self.members { f.revalidate()?; } self.selection.revalidate()
    }
}
fn cache() -> &'static Mutex<DigestCache> { static C: OnceLock<Mutex<DigestCache>> = OnceLock::new(); C.get_or_init(Mutex::default) }
pub(super) fn runtime_version(selected: &Path) -> Option<String> {
    let c = contract()?; let proof = Verified::capture(selected, c, cache()).ok()?; proof.revalidate().ok()?; Some(c.version.clone())
}

fn component(items: &mut Vec<PluginComponent>, kind: PluginComponentKind, name: &str, source: &Path, b: &mut Budget) -> Result<(), ()> {
    b.component()?; items.push(PluginComponent { kind, name: native_text(name)?, source_path: collapse_home(source), native_id: None }); Ok(())
}
fn observation(host: &Host, proof: &Verified, id: &str, name: &str, version: &str, presence: PluginPresence,
    mut sources: Vec<String>, mut components: Vec<PluginComponent>, context: &str) -> Result<NativePluginObservation, ()> {
    native_text(id)?; native_text(name)?; components.sort(); components.dedup(); sources.sort(); sources.dedup();
    Ok(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: id.into(), name: name.into(), version: version.into(),
        revision: format!("public-package-sha256:{}; {context}", proof.identity), presence, enabled: None,
        source_paths: sources, components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() })
}
fn bundled(host: &Host, c: &Contract, proof: &Verified, s: &mut Snapshot, b: &mut Budget) -> Result<(), ()> {
    let mut items = Vec::new(); let mut sources = vec![collapse_home(&proof.root.join("package.json"))];
    for skill in &c.builtin_skills {
        let p = relative_member(&proof.root, &skill.path)?; sources.push(collapse_home(&p));
        if s.stat(&p, b)?.is_some() { component(&mut items, PluginComponentKind::Skill, &skill.name, &p, b)?; }
        else { s.incomplete = true; }
    }
    let availability = s.availability(Some(&proof.root), &proof.root.join("package.json"))?;
    s.plugins.push(observation(host, proof, "@builtin/skills", "CodeBuddy bundled Skills", &c.version,
        PluginPresence::Bundled, sources, items, &format!("application-owned; file availability; body/frontmatter/activation=unknown; availability:{availability}"))?);
    Ok(())
}
struct Record { id: String, name: String, market: String, scope: String, path: String, version: String }
fn record(key: Option<&str>, value: &Value) -> Result<Option<Record>, ()> {
    let o = value.as_object().ok_or(())?;
    let scope = match o.get("scope").and_then(Value::as_str).unwrap_or("user") {
        "project" | "local" | "project-local" => return Ok(None), "user" => "user", "managed" => "managed", _ => return Err(()),
    };
    if o.get("projectPath").is_some_and(|v| !v.is_null()) { return Err(()); }
    let generated;
    let id = if let Some(v) = o.get("pluginId").and_then(Value::as_str) { v }
        // Native object spread lets even a non-string property override the
        // registry key; only explicit name/marketplace fields can recover it.
        else if !o.contains_key("pluginId") && key.is_some() { key.ok_or(())? }
        else { generated = format!("{}@{}", o.get("name").and_then(Value::as_str).ok_or(())?,
            o.get("marketplaceName").and_then(Value::as_str).ok_or(())?); &generated };
    native_text(id)?; let (name, market) = id.split_once('@').ok_or(())?; native_text(name)?; native_text(market)?;
    let path = o.get("installPath").and_then(Value::as_str).or_else(|| o.get("path").and_then(Value::as_str)).ok_or(())?;
    if path.is_empty() { return Ok(None); }
    let version = o.get("version").and_then(Value::as_str).filter(|v| !v.is_empty()).unwrap_or("unknown"); native_text(version)?;
    Ok(Some(Record { id: id.into(), name: name.into(), market: market.into(), scope: scope.into(), path: path.into(), version: version.into() }))
}
fn records(value: &Value) -> Result<(Vec<Record>, bool), ()> {
    let payload = if value.is_array() { value } else { value.get("plugins").ok_or(())? };
    let mut out = Vec::new(); let mut issue = false; let mut visited = 0usize;
    let mut add = |key, value| -> Result<(), ()> {
        visited += 1; if visited > MAX_PLUGINS { return Err(()); }
        match record(key, value) { Ok(Some(row)) => out.push(row), Ok(None) => {}, Err(()) => issue = true }
        Ok(())
    };
    match payload {
        Value::Array(values) if values.len() <= MAX_PLUGINS => for value in values { add(None, value)?; },
        Value::Object(values) if values.len() <= MAX_PLUGINS => for (id, values) in values {
            match values {
                Value::Array(values) if values.len() <= MAX_PLUGINS => for value in values { add(Some(id.as_str()), value)?; },
                Value::Array(_) => return Err(()), _ => add(Some(id.as_str()), values)?,
            }
        }, _ => return Err(()),
    }
    Ok((out, issue))
}
fn sanitized_name(value: &str) -> String {
    value.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '-' }).collect()
}
fn cache_path(home: &Path, row: &Record) -> Result<PathBuf, ()> {
    let p = absolute(&row.path)?;
    let root = home.join("plugins/cache").join(sanitized_name(&row.market)).join(sanitized_name(&row.name));
    let tail = p.strip_prefix(&root).map_err(|_| ())?; let parts = tail.components().collect::<Vec<_>>();
    if parts.len() != 1 || !matches!(parts[0], Component::Normal(_)) { return Err(()); }
    checked_root(&p)?; Ok(p)
}
fn declared_paths(value: Option<&Value>, s: &mut Snapshot) -> Vec<String> {
    match value {
        None => Vec::new(), Some(Value::String(v)) => vec![v.clone()],
        Some(Value::Array(values)) => values.iter().filter_map(|v| {
            if let Some(v) = v.as_str() { return Some(v.into()); }
            let p = v.as_object().and_then(|v| v.get("path").or_else(|| v.get("location")).or_else(|| v.get("file"))).and_then(Value::as_str);
            if p.is_none() { s.incomplete = true; } p.map(str::to_string)
        }).collect(),
        Some(Value::Object(o)) => match o.get("path").or_else(|| o.get("location")).or_else(|| o.get("file")).and_then(Value::as_str) {
            Some(v) => vec![v.into()], None => { s.incomplete = true; Vec::new() },
        }, _ => { s.incomplete = true; Vec::new() },
    }
}
fn markdown_tree(root: &Path, path: &Path, kind: PluginComponentKind, depth: usize,
    items: &mut Vec<PluginComponent>, s: &mut Snapshot, b: &mut Budget) -> Result<(), ()> {
    if depth > 8 || !path.starts_with(root) { return Err(()); }
    if path.extension().and_then(|v| v.to_str()) == Some("md") {
        if kind == PluginComponentKind::Skill && path.file_name().and_then(|v| v.to_str()) != Some("SKILL.md") { return Ok(()); }
        if s.stat(path, b)?.is_some() {
            let title = if kind == PluginComponentKind::Skill {
                if path == root.join("SKILL.md") { "SKILL.md" }
                else { path.parent().and_then(Path::file_name).and_then(|v| v.to_str()).ok_or(())? }
            } else { path.file_name().and_then(|v| v.to_str()).ok_or(())? };
            component(items, kind, title, path, b)?;
        }
        return Ok(());
    }
    for entry in s.list(path, b)? {
        if entry.directory { markdown_tree(root, &entry.path, kind.clone(), depth + 1, items, s, b)?; }
        else if entry.file && entry.path.extension().and_then(|v| v.to_str()) == Some("md") {
            if kind != PluginComponentKind::Skill || entry.path.file_name().and_then(|v| v.to_str()) == Some("SKILL.md") {
                markdown_tree(root, &entry.path, kind.clone(), depth + 1, items, s, b)?;
            }
        }
    }
    Ok(())
}
fn document_names(value: &Value, key: &str, kind: PluginComponentKind, source: &Path,
    items: &mut Vec<PluginComponent>, b: &mut Budget) -> Result<(), ()> {
    let root = value.as_object().ok_or(())?;
    let map = if let Some(value) = root.get(key) { value.as_object().ok_or(())? } else { root };
    if map.len() > MAX_PLUGINS { return Err(()); }
    for name in map.keys() { component(items, kind.clone(), name, source, b)?; } Ok(())
}
fn json_reference(root: &Path, value: &str, key: &str, kind: PluginComponentKind, body_paths: &BTreeSet<PathBuf>,
    items: &mut Vec<PluginComponent>, s: &mut Snapshot, b: &mut Budget) -> Result<(), ()> {
    let p = relative_member(root, value)?;
    // Explicit JSON sources only. A manifest cannot turn a Skill/hook/module
    // body into a metadata read merely by naming it as MCP configuration.
    let filename = p.file_name().and_then(|v| v.to_str()).ok_or(())?;
    if p.extension().and_then(|v| v.to_str()) != Some("json") || component_payload_is_opaque(root, &p, body_paths) {
        component(items, kind, filename, &p, b)?;
        return Err(());
    }
    if let Some(value) = s.document(&p, true, b)? { document_names(&value, key, kind, &p, items, b)?; } Ok(())
}
fn json_tree(root: &Path, path: &Path, depth: usize, key: &str, kind: PluginComponentKind, body_paths: &BTreeSet<PathBuf>,
    items: &mut Vec<PluginComponent>, s: &mut Snapshot, b: &mut Budget) -> Result<(), ()> {
    if depth > 8 { return Err(()); }
    for entry in s.list(path, b)? {
        if entry.directory {
            if json_tree(root, &entry.path, depth + 1, key, kind.clone(), body_paths, items, s, b).is_err() { s.incomplete = true; }
        }
        else if entry.file && entry.path.extension().and_then(|v| v.to_str()) == Some("json") {
            if json_reference(root, entry.path.strip_prefix(root).map_err(|_| ())?.to_str().ok_or(())?, key, kind.clone(), body_paths, items, s, b).is_err() { s.incomplete = true; }
        }
    }
    Ok(())
}
fn components(root: &Path, manifest: &Value, manifest_path: &Path, items: &mut Vec<PluginComponent>, s: &mut Snapshot, b: &mut Budget) -> Result<(), ()> {
    let mut body_paths = BTreeSet::from([root.join("hooks/hooks.json")]);
    let hooks = manifest.get("hooks");
    if hooks.is_some_and(|v| v.is_string() || v.is_array()
        || v.as_object().is_some_and(|v| ["path", "file", "location"].iter().any(|k| v.get(*k).is_some_and(Value::is_string)))) {
        for path in declared_paths(hooks, s) { if let Ok(path) = relative_member(root, &path) { body_paths.insert(path); } }
    }
    for (field, directory, kind) in [("skills", "skills", PluginComponentKind::Skill), ("commands", "commands", PluginComponentKind::Command),
        ("agents", "agents", PluginComponentKind::Agent), ("outputStyles", "output-styles", PluginComponentKind::OutputStyle)] {
        let mut paths = declared_paths(manifest.get(field), s); paths.push(directory.into());
        if field == "skills" { paths.push("SKILL.md".into()); } paths.sort(); paths.dedup();
        for p in paths {
            let result = relative_member(root, &p).and_then(|p| markdown_tree(root, &p, kind.clone(), 0, items, s, b));
            if result.is_err() { s.incomplete = true; }
        }
    }
    for (field, key, file, kind) in [("mcpServers", "mcpServers", ".mcp.json", PluginComponentKind::McpReference),
        ("lspServers", "lspServers", ".lsp.json", PluginComponentKind::Lsp)] {
        if let Some(value) = manifest.get(field) {
            let result = if let Some(path) = value.as_str() { json_reference(root, path, key, kind.clone(), &body_paths, items, s, b) }
                else { document_names(value, key, kind.clone(), manifest_path, items, b) };
            if result.is_err() { s.incomplete = true; }
        }
        if json_reference(root, file, key, kind.clone(), &body_paths, items, s, b).is_err() { s.incomplete = true; }
        if field == "mcpServers" && json_tree(root, &root.join("mcp"), 0, key, kind, &body_paths, items, s, b).is_err() { s.incomplete = true; }
    }
    let hooks = root.join("hooks/hooks.json");
    if manifest.get("hooks").is_some() || s.stat(&hooks, b)?.is_some() {
        component(items, PluginComponentKind::Hook, "Hooks declaration", manifest_path, b)?;
    }
    for (field, kind, title) in [("client", PluginComponentKind::Renderer, "Client declaration"),
        ("settings", PluginComponentKind::Policy, "Plugin settings"), ("options", PluginComponentKind::TypeDeclaration, "Plugin options")] {
        if manifest.get(field).is_some() { component(items, kind, title, manifest_path, b)?; }
    }
    if let Some(bundles) = manifest.get("mcpBundles").and_then(Value::as_array) {
        if bundles.len() > MAX_PLUGINS { return Err(()); }
        for bundle in bundles {
            if let Some(name) = bundle.get("name").and_then(Value::as_str) { component(items, PluginComponentKind::McpBundle, name, manifest_path, b)?; }
            else { s.incomplete = true; }
        }
    }
    Ok(())
}
fn registered(host: &Host, proof: &Verified, home: &Path, s: &mut Snapshot, b: &mut Budget) -> Result<(), ()> {
    let registry = home.join("plugins/installed_plugins.json");
    let Some(value) = s.document(&registry, false, b)? else { return Ok(()); };
    let (rows, issue) = records(&value)?; s.incomplete |= issue;
    for row in rows {
        b.visit()?; let mut items = Vec::new(); let mut sources = vec![collapse_home(&registry)]; let mut observed_root = None;
        let parsed = (|| {
            let root = cache_path(home, &row)?; s.bind(&root, true, b)?.ok_or(())?;
            for directory in METADATA_DIRS {
                let file = root.join(directory).join("plugin.json");
                if let Some(manifest) = s.document(&file, false, b)? {
                    if manifest.get("name").and_then(Value::as_str) != Some(row.name.as_str()) { return Err(()); }
                    return Ok((root, file, manifest));
                }
            }
            Err(())
        })();
        let presence = match parsed {
            Ok((root, file, manifest)) => {
                observed_root = Some(root.clone()); sources.push(collapse_home(&root)); sources.push(collapse_home(&file));
                if components(&root, &manifest, &file, &mut items, s, b).is_err() { s.incomplete = true; }
                PluginPresence::Installed
            },
            Err(()) => { s.incomplete = true; PluginPresence::Declared },
        };
        let availability = s.availability(observed_root.as_deref(), &registry)?;
        s.plugins.push(observation(host, proof, &row.id, &row.name, &row.version, presence, sources, items,
            &format!("registered {} scope; selected version/policy/session/activation=unknown; file availability; body/frontmatter=unknown; availability:{availability}", row.scope))?);
    }
    Ok(())
}
fn valid_host(host: &Host) -> Result<(), ()> {
    if host.agent_id != "codebuddy-code" || host.adapter != "codebuddy_cli" || host.roots.len() != 1 { return Err(()); }
    let root = &host.roots[0];
    if root.path != "~/.codebuddy/plugins/installed_plugins.json" || root.layout != "codebuddy_installed_registry"
        || root.presence != PluginPresence::Installed { return Err(()); } Ok(())
}
struct Audit { proof: Verified, snapshot: Snapshot, home: Option<PathBuf> }
impl Audit {
    fn revalidate(&self) -> Result<(), ()> {
        if selected_default()?.as_ref() != Some(&self.proof.selection.alias) { return Err(()); }
        if let Some(home) = &self.home { if &native_home()? != home { return Err(()); } }
        self.snapshot.revalidate()?; self.proof.revalidate()?; self.snapshot.revalidate()
    }
}
fn capture(host: &Host, c: &Contract, cache: &Mutex<DigestCache>) -> Result<Option<Audit>, ()> {
    valid_host(host)?; let Some(selected) = selected_default()? else { return Ok(None); };
    let proof = Verified::capture(&selected, c, cache)?; let mut snapshot = Snapshot::default(); let mut budget = Budget::default();
    bundled(host, c, &proof, &mut snapshot, &mut budget)?;
    let home = match native_home() {
        Ok(home) => { if registered(host, &proof, &home, &mut snapshot, &mut budget).is_err() { snapshot.incomplete = true; } Some(home) },
        Err(()) => { snapshot.incomplete = true; None },
    };
    let audit = Audit { proof, snapshot, home }; audit.revalidate()?; Ok(Some(audit))
}
fn observe_contract(host: &Host, c: &Contract, cache: &Mutex<DigestCache>, result: &mut PluginScan) -> Result<(), ()> {
    let Some(audit) = capture(host, c, cache)? else { return Ok(()); }; audit.revalidate()?;
    result.unavailable |= audit.snapshot.incomplete; result.plugins.extend(audit.snapshot.plugins); Ok(())
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let Some(c) = contract() else { result.unavailable = true; return; };
    if observe_contract(host, c, cache(), result).is_err() { result.unavailable = true; }
}
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let Some(c) = contract() else { return Vec::new(); }; if valid_host(host).is_err() { return Vec::new(); }
    let mut paths = crate::application::agent_launch::command_candidates(&c.command).into_iter().collect::<BTreeSet<_>>();
    if let Ok(home) = native_home() { paths.insert(home.join("plugins")); paths.insert(home.join("plugins/installed_plugins.json")); }
    if let Ok(Some(audit)) = capture(host, c, cache()) { if audit.revalidate().is_ok() {
        paths.insert(audit.proof.selection.alias.clone()); paths.insert(audit.proof.root.clone());
        paths.extend(audit.proof.members.iter().map(|v| v.path.clone())); paths.extend(audit.snapshot.watches);
    } }
    paths.into_iter().filter(|p| checked_root(p).is_ok()).collect()
}

#[cfg(test)]
mod tests;

#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
struct Stamp {dev:u64,ino:u64,bytes:u64,mode:u32,links:u64,mtime:i64,mtime_ns:i64,ctime:i64,ctime_ns:i64}
impl Stamp {
    fn capture(metadata:&fs::Metadata)->Result<Self,()> {
        #[cfg(unix)] {use std::os::unix::fs::MetadataExt;
            Ok(Self{dev:metadata.dev(),ino:metadata.ino(),bytes:metadata.len(),mode:metadata.mode(),links:metadata.nlink(),
                mtime:metadata.mtime(),mtime_ns:metadata.mtime_nsec(),ctime:metadata.ctime(),ctime_ns:metadata.ctime_nsec()})}
        #[cfg(not(unix))] {let _=metadata;Err(())}
    }
}
#[derive(Default)]
struct Budget {visited:usize,read_bytes:usize,hashed_bytes:u64,components:usize,stat_bytes:u64}
impl Budget {
    fn visit(&mut self)->Result<(),()> {self.visited=self.visited.checked_add(1).ok_or(())?;if self.visited>MAX_VISITS{Err(())}else{Ok(())}}
    fn component(&mut self)->Result<(),()> {self.visit()?;self.components+=1;if self.components>MAX_ITEMS{Err(())}else{Ok(())}}
    fn read(&mut self,n:usize)->Result<(),()> {self.read_bytes=self.read_bytes.checked_add(n).ok_or(())?;if self.read_bytes>MAX_METADATA_BYTES{Err(())}else{Ok(())}}
}
struct Bound {path:PathBuf,file:fs::File,stamp:Stamp,directory:bool}
impl Bound {
    fn capture(path:PathBuf,directory:bool,budget:&mut Budget)->Result<Self,()> {
        budget.visit()?;checked_root(&path)?;let metadata=fs::symlink_metadata(&path).map_err(|_|())?;
        if if directory{!metadata.is_dir()}else{!metadata.file_type().is_file()}{return Err(());}
        let stamp=Stamp::capture(&metadata)?;if !directory&&stamp.links!=1{return Err(());}
        let file=open_confined(&path,directory)?;let value=Self{path,file,stamp,directory};value.revalidate()?;Ok(value)
    }
    fn revalidate(&self)->Result<(),()> {
        checked_root(&self.path)?;let m=fs::symlink_metadata(&self.path).map_err(|_|())?;
        if if self.directory{!m.is_dir()}else{!m.file_type().is_file()}{return Err(());}
        if Stamp::capture(&m)?!=self.stamp||Stamp::capture(&self.file.metadata().map_err(|_|())?)?!=self.stamp{Err(())}else{Ok(())}
    }
    fn bytes(&mut self,limit:usize,budget:&mut Budget)->Result<Vec<u8>,()> {
        if self.directory||self.stamp.bytes>limit as u64{return Err(());}
        self.revalidate()?;budget.read(self.stamp.bytes as usize)?;self.file.seek(SeekFrom::Start(0)).map_err(|_|())?;
        let mut bytes=Vec::with_capacity(self.stamp.bytes as usize);(&mut self.file).take(limit as u64+1).read_to_end(&mut bytes).map_err(|_|())?;
        self.revalidate()?;if bytes.len()as u64!=self.stamp.bytes||bytes.len()>limit{return Err(());}Ok(bytes)
    }
}
#[derive(Clone,PartialEq,Eq)]
struct DigestKey {context:String,path:PathBuf,stamp:Stamp,expected:String}
#[derive(Default)]
struct DigestCache {entries:VecDeque<DigestKey>}
impl DigestCache {
    fn contains(&mut self,key:&DigestKey)->bool {let Some(i)=self.entries.iter().position(|v|v==key)else{return false;};let v=self.entries.remove(i).expect("located cache entry");self.entries.push_back(v);true}
    fn insert(&mut self,key:DigestKey) {if self.contains(&key){return;}if self.entries.len()>=MAX_CACHE{self.entries.pop_front();}self.entries.push_back(key);}
}


#[derive(Clone)]
struct Entry {path:PathBuf,directory:bool,file:bool}
struct Leaf {path:PathBuf,parent:usize,stamp:Stamp}
#[derive(Default)]
struct Snapshot {plugins:Vec<NativePluginObservation>,bound:Vec<Bound>,leaves:Vec<Leaf>,missing:BTreeSet<PathBuf>,
    entries:BTreeMap<PathBuf,Vec<Entry>>,watches:BTreeSet<PathBuf>,incomplete:bool}
impl Snapshot {
    fn bind(&mut self,path:&Path,directory:bool,b:&mut Budget)->Result<Option<usize>,()> {
        b.visit()?;checked_root(path)?;self.watches.insert(path.into());
        if let Some(i)=self.bound.iter().position(|v|v.path==path&&v.directory==directory){self.bound[i].revalidate()?;return Ok(Some(i));}
        match fs::symlink_metadata(path) {Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{self.missing.insert(path.into());return Ok(None);},Err(_)=>return Err(()),_=>{}}
        if self.bound.len()>=MAX_BOUND{return Err(());}self.bound.push(Bound::capture(path.into(),directory,b)?);Ok(Some(self.bound.len()-1))
    }
    fn document(&mut self,path:&Path,jsonc:bool,b:&mut Budget)->Result<Option<Value>,()> {
        if path.extension().and_then(|v|v.to_str())!=Some("json"){return Err(());}
        let Some(i)=self.bind(path,false,b)?else{return Ok(None);};
        let limit=MAX_DOCUMENT_BYTES.min(MAX_METADATA_BYTES.saturating_sub(b.read_bytes));let bytes=self.bound[i].bytes(limit,b)?;
        let text=std::str::from_utf8(&bytes).map_err(|_|())?;
        let value=if jsonc {check_jsonc_depth(text)?;let options=jsonc_parser::ParseOptions{allow_comments:true,allow_trailing_commas:true,
            allow_loose_object_property_names:false,allow_missing_commas:false,allow_single_quoted_strings:false,allow_hexadecimal_numbers:false,allow_unary_plus_numbers:false};
            jsonc_parser::parse_to_ast(text,&Default::default(),&options).map_err(|_|())?.value.ok_or(())?.into()
        }else{serde_json::from_str(text).map_err(|_|())?};Ok(Some(value))
    }
    fn list(&mut self,path:&Path,b:&mut Budget)->Result<Vec<Entry>,()> {
        let Some(index)=self.bind(path,true,b)?else{return Ok(Vec::new());};
        if let Some(v)=self.entries.get(path){return Ok(v.clone());}
        let mut rows=Vec::new();
        #[cfg(unix)] {
            use std::os::unix::ffi::OsStrExt;use rustix::fs::{AtFlags,Dir,FileType,statat};
            let file=open_confined(path,true)?;if Stamp::capture(&file.metadata().map_err(|_|())?)?!=self.bound[index].stamp{return Err(());}
            for entry in Dir::read_from(&file).map_err(|_|())? {
                let entry=entry.map_err(|_|())?;let name=entry.file_name();if matches!(name.to_bytes(),b"."|b".."){continue;}
                b.visit()?;if rows.len()>=MAX_DIRECTORY{return Err(());}let label=std::str::from_utf8(name.to_bytes()).map_err(|_|())?;native_text(label)?;
                let kind=match entry.file_type(){FileType::Unknown=>FileType::from_raw_mode(statat(&file,name,AtFlags::SYMLINK_NOFOLLOW).map_err(|_|())?.st_mode),v=>v};
                if kind==FileType::Symlink{self.incomplete=true;continue;}
                rows.push(Entry{path:path.join(label),directory:kind==FileType::Directory,file:kind==FileType::RegularFile});
            }
        }
        #[cfg(not(unix))] {let _=index;return Err(());}
        self.bound[index].revalidate()?;rows.sort_by(|a,b|a.path.cmp(&b.path));self.entries.insert(path.into(),rows.clone());Ok(rows)
    }
    fn stat(&mut self,path:&Path,b:&mut Budget)->Result<Option<Stamp>,()> {
        b.visit()?;checked_root(path)?;self.watches.insert(path.into());
        if let Some(v)=self.leaves.iter().find(|v|v.path==path){return Ok(Some(v.stamp.clone()));}
        let Some(parent)=self.bind(path.parent().ok_or(())?,true,b)?else{self.missing.insert(path.into());return Ok(None);};
        let Some(stamp)=stat_at(&self.bound[parent],path.file_name().ok_or(())?)?else{self.missing.insert(path.into());return Ok(None);};
        b.stat_bytes=b.stat_bytes.checked_add(stamp.bytes).ok_or(())?;
        if stamp.bytes>MAX_STAT_BYTES||b.stat_bytes>MAX_STAT_TOTAL||self.leaves.len()>=MAX_ITEMS{return Err(());}
        self.leaves.push(Leaf{path:path.into(),parent,stamp:stamp.clone()});Ok(Some(stamp))
    }
    fn revalidate(&self)->Result<(),()> {
        for v in &self.bound{v.revalidate()?;}
        for v in &self.leaves{if stat_at(self.bound.get(v.parent).ok_or(())?,v.path.file_name().ok_or(())?)?.as_ref()!=Some(&v.stamp){return Err(());}}
        for path in &self.missing{checked_root(path)?;match fs::symlink_metadata(path){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},_=>return Err(())}}
        for v in &self.bound{v.revalidate()?;}Ok(())
    }
    fn availability(&self,root:Option<&Path>,ledger:&Path)->Result<String,()> {
        let files=self.bound.iter().filter(|v|v.path==ledger||root.is_some_and(|p|v.path.starts_with(p))).map(|v|(&v.path,&v.stamp)).collect::<Vec<_>>();
        let leaves=self.leaves.iter().filter(|v|root.is_some_and(|p|v.path.starts_with(p))).map(|v|(&v.path,&v.stamp)).collect::<Vec<_>>();
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(files,leaves)).map_err(|_|())?)))
    }
}
fn stat_at(parent:&Bound,name:&std::ffi::OsStr)->Result<Option<Stamp>,()> {
    #[cfg(any(target_os="macos",target_os="linux"))] {use rustix::fs::{AtFlags,FileType,statat};
        let v=match statat(&parent.file,name,AtFlags::SYMLINK_NOFOLLOW){Ok(v)=>v,Err(e)if e==rustix::io::Errno::NOENT=>return Ok(None),_=>return Err(())};
        if FileType::from_raw_mode(v.st_mode)!=FileType::RegularFile{return Err(());}
        Ok(Some(Stamp{dev:v.st_dev as u64,ino:v.st_ino as u64,bytes:u64::try_from(v.st_size).map_err(|_|())?,mode:v.st_mode as u32,links:v.st_nlink as u64,
            mtime:v.st_mtime as i64,mtime_ns:v.st_mtime_nsec as i64,ctime:v.st_ctime as i64,ctime_ns:v.st_ctime_nsec as i64}))}
    #[cfg(not(any(target_os="macos",target_os="linux")))] {let _=(parent,name);Err(())}
}
