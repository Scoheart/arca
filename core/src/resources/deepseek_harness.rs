//! Passive Cordis patch editing. Never evaluate tags or load Harness modules.
use serde_json::{Map, Value};
use std::{collections::BTreeSet, fs, io::Read, path::{Component, Path, PathBuf}, str::FromStr};
use yaml_edit::{Document, Mapping, Sequence, YamlFile, YamlNode};

const LIMIT: usize = 2 * 1024 * 1024;
fn error() -> String { "DeepSeek Harness patch is ambiguous or cannot be edited safely".into() }

/// Native resolveRoot: a nonblank env value is used verbatim, then tilde-expanded
/// and resolved against cwd. Whitespace-only values use the default.
fn resolve_home_env(variable: &str, fallback: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    let selected = std::env::var_os(variable).filter(|v| !v.to_string_lossy().trim().is_empty());
    let mut path = selected.map(PathBuf::from).unwrap_or_else(|| home.join(fallback));
    if let Some(s) = path.to_str() {
        if s == "~" { path = home; }
        else if s.starts_with("~/") || s.starts_with("~\\") { path = home.join(&s[2..]); }
    }
    if !path.is_absolute() { path = std::env::current_dir().unwrap_or_default().join(path); }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component { Component::CurDir => {}, Component::ParentDir => { normalized.pop(); }, other => normalized.push(other.as_os_str()) }
    }
    normalized
}

pub fn home_dir() -> PathBuf { resolve_home_env("DSH_HOME", ".dsh") }

pub fn agents_home_dir() -> PathBuf { resolve_home_env("DSH_AGENTS_HOME", ".agents") }

pub fn profile_path(profile: &str) -> String {
    let path = home_dir().join("profiles").join(profile).join("cordis.patch.yml");
    if let Some(home) = dirs::home_dir() {
        if let Ok(tail) = path.strip_prefix(home) { return format!("~/{}", tail.to_string_lossy()); }
    }
    path.to_string_lossy().into_owned()
}

/// Metadata/text only, bounded and opened one real parent at a time. Missing
/// descendants are absence; aliases, FIFOs and symlink parents are not files.
pub fn read_text(path: &Path) -> Result<Option<String>, String> {
    if !path.is_absolute() || path.as_os_str().to_string_lossy().chars().any(char::is_control)
        || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) { return Err(error()); }
    #[cfg(unix)] {
        use rustix::fs::{openat, Mode, OFlags, CWD};
        let flags = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
        let mut directory = fs::File::from(openat(CWD, Path::new("/"), flags | OFlags::DIRECTORY, Mode::empty()).map_err(|_| error())?);
        let parts = path.components().filter_map(|c| if let Component::Normal(n) = c { Some(n) } else { None }).collect::<Vec<_>>();
        if parts.is_empty() { return Err(error()); }
        for (index, part) in parts.iter().enumerate() {
            let final_part = index + 1 == parts.len();
            let opened = match openat(&directory, *part, if final_part { flags } else { flags | OFlags::DIRECTORY }, Mode::empty()) {
                Ok(fd) => fs::File::from(fd), Err(rustix::io::Errno::NOENT) => return Ok(None), Err(_) => return Err(error()),
            };
            if final_part {
                let metadata = opened.metadata().map_err(|_| error())?;
                if !metadata.is_file() || metadata.len() > LIMIT as u64 { return Err(error()); }
                let mut text = String::new();
                opened.take((LIMIT + 1) as u64).read_to_string(&mut text).map_err(|_| error())?;
                if text.len() > LIMIT { return Err(error()); }
                return Ok(Some(text));
            }
            directory = opened;
        }
        Err(error())
    }
    #[cfg(not(unix))] { let _ = path; Err("secure Cordis patch access is unavailable on this platform".into()) }
}

pub fn node_json(node: &YamlNode) -> Result<Value, String> { node_json_at(node, 0) }
fn node_json_at(node: &YamlNode, depth: usize) -> Result<Value, String> {
    if depth > 64 { return Err(error()); }
    match node {
        YamlNode::Mapping(mapping) => {
            let mut object = Map::new();
            for entry in mapping.entries() {
                let key = node_json_at(&entry.key_node().ok_or_else(error)?, depth + 1)?;
                let key = key.as_str().ok_or_else(error)?.to_string();
                if key == "<<" || object.contains_key(&key) { return Err(error()); }
                object.insert(key, node_json_at(&entry.value_node().ok_or_else(error)?, depth + 1)?);
            }
            Ok(Value::Object(object))
        }
        YamlNode::Sequence(sequence) => sequence.into_iter().map(|n| node_json_at(&n, depth + 1)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        YamlNode::Scalar(scalar) => if scalar.is_quoted() || scalar.value().starts_with(['|', '>']) {
            Ok(Value::String(scalar.as_string()))
        } else { serde_yaml::from_str(&scalar.value()).map_err(|_| error()) },
        _ => Err(error()),
    }
}

fn keys(mapping: &Mapping) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    for entry in mapping.entries() {
        let key = node_json(&entry.key_node().ok_or_else(error)?)?.as_str().ok_or_else(error)?.to_string();
        if key == "<<" || !seen.insert(key) { return Err(error()); }
    }
    Ok(seen.into_iter().collect())
}
fn structural(node: &YamlNode, depth: usize) -> Result<(), String> {
    if depth > 64 { return Err(error()); }
    match node {
        YamlNode::Mapping(m) => { keys(m)?; for e in m.entries() { structural(&e.value_node().ok_or_else(error)?, depth + 1)?; } },
        YamlNode::Sequence(s) => for n in s { structural(&n, depth + 1)?; },
        YamlNode::Alias(_) => return Err(error()),
        // Unrelated !!js is retained as inert CST; never deserialize/evaluate it.
        _ => {},
    }
    Ok(())
}
fn json_node(value: &Value) -> Result<YamlNode, String> {
    Document::from_str(&serde_json::json!({"value": value}).to_string()).map_err(|_| error())?.get("value").ok_or_else(error)
}
fn set(mapping: &Mapping, key: &str, value: &Value) -> Result<(), String> {
    if let (Some(existing), Value::Object(fields)) = (mapping.get_mapping(key), value) {
        for old in keys(&existing)? { if !fields.contains_key(&old) { existing.remove(old.as_str()); } }
        for (key, value) in fields { set(&existing, key, value)?; }
        return Ok(());
    }
    match json_node(value)? {
        YamlNode::Mapping(n) => mapping.set(key, n), YamlNode::Sequence(n) => mapping.set(key, n),
        YamlNode::Scalar(n) => mapping.set(key, n), _ => return Err(error()),
    }
    Ok(())
}

pub struct PatchFile { file: YamlFile, original: Option<String> }
impl PatchFile {
    pub fn read(path: &Path) -> Result<Self, String> {
        let original = read_text(path)?;
        let mut result = Self::from_text(original.as_deref().unwrap_or("[]\n"))?;
        result.original = original;
        Ok(result)
    }
    pub fn from_text(text: &str) -> Result<Self, String> {
        if text.len() > LIMIT || text.trim().is_empty() { return Err(error()); }
        let file = YamlFile::from_str(text).map_err(|_| error())?;
        let documents = file.documents().collect::<Vec<_>>();
        if documents.len() != 1 { return Err(error()); }
        let sequence = documents[0].as_sequence().ok_or_else(error)?;
        structural(&YamlNode::Sequence(sequence), 0)?;
        Ok(Self { file, original: Some(text.into()) })
    }
    pub fn original(&self) -> Option<&str> { self.original.as_deref() }
    fn sequence(&self) -> Result<Sequence, String> { self.file.document().and_then(|d| d.as_sequence()).ok_or_else(error) }
    fn row(&self, id: &str) -> Result<Option<Mapping>, String> {
        let mut result = None;
        for node in &self.sequence()? {
            let mapping = node.as_mapping().ok_or_else(error)?;
            if let Some(value) = mapping.get("id") {
                let value = node_json(&value)?;
                if value.as_str().ok_or_else(error)? == id {
                    if result.is_some() || mapping.contains_key("insert") { return Err(error()); }
                    result = Some(mapping.clone());
                }
            } else if mapping.get("config").is_some() && mapping.get("insert").is_none() {
                return Err(error());
            }
            // Insertions may introduce a second entry with this id. They are
            // not a simple patch to the known row and never grant a writer.
            if let Some(insert) = mapping.get("insert") {
                if contains_id(&insert, id, 0)? { return Err(error()); }
            }
        }
        Ok(result)
    }
    pub fn has_row(&self, id: &str) -> Result<bool, String> { Ok(self.row(id)?.is_some()) }
    pub fn config(&self, id: &str) -> Result<Option<Value>, String> {
        self.row(id)?.and_then(|m| m.get("config")).map(|n| node_json(&n)).transpose()
            .and_then(|v| if v.as_ref().is_some_and(|v| !v.is_object()) { Err(error()) } else { Ok(v) })
    }
    pub fn set_config_field(&mut self, id: &str, field: &str, value: &Value) -> Result<(), String> { self.set_config_path(id, &[field], value) }
    pub fn remove_config_field(&mut self, id: &str, field: &str) -> Result<(), String> { self.remove_config_path(id, &[field]) }
    fn config_node(&self, id: &str, create: bool) -> Result<Option<YamlNode>, String> {
        let row = if let Some(row) = self.row(id)? { row } else if create {
            let node = json_node(&serde_json::json!({"id": id, "config": {}}))?;
            let YamlNode::Mapping(mapping) = node else { return Err(error()); };
            self.sequence()?.push(mapping);
            self.row(id)?.ok_or_else(error)?
        } else { return Ok(None); };
        for key in keys(&row)? {
            if key == "disabled" && row.get("disabled").map(|n| node_json(&n)).transpose()? == Some(Value::Bool(false)) { continue; }
            if key == "name" {
                let expected = match id { "mcp-manager" => "@deepseek-ai/dsh-mcp-manager", "llm-pi-ai" => "@deepseek-ai/dsh-llm-pi-ai", "agent-default-model" => "@deepseek-ai/dsh-agent-default-model", _ => "" };
                let name = row.get("name").map(|n| node_json(&n)).transpose()?;
                if name.as_ref().and_then(Value::as_str).is_some_and(|n| n.is_empty() || (!expected.is_empty() && n == expected)) { continue; }
            }
            if !matches!(key.as_str(), "id" | "config") { return Err("Cordis target has disabled or unsupported structural policy; edit it in DeepSeek Harness".into()); }
        }
        if row.get("config").is_none() && create { set(&row, "config", &serde_json::json!({}))?; }
        let node = row.get("config");
        if node.as_ref().is_some_and(|n| n.as_mapping().is_none()) { return Err(error()); }
        Ok(node)
    }
    pub fn set_config_path(&mut self, id: &str, path: &[&str], value: &Value) -> Result<(), String> { self.change(id, path, Some(value)) }
    pub fn remove_config_path(&mut self, id: &str, path: &[&str]) -> Result<(), String> {
        if path.is_empty() { if let Some(row) = self.row(id)? { if let Some(node) = row.get("config") { node_json(&node)?; } row.remove("config"); } return Ok(()); }
        self.change(id, path, None)
    }
    fn change(&self, id: &str, path: &[&str], value: Option<&Value>) -> Result<(), String> {
        if path.is_empty() || path.iter().any(|p| p.is_empty() || *p == "<<") { return Err(error()); }
        let Some(mut node) = self.config_node(id, value.is_some())? else { return Ok(()); };
        for (index, key) in path.iter().enumerate() {
            let last = index + 1 == path.len();
            match node {
                YamlNode::Mapping(ref mapping) => {
                    keys(mapping)?;
                    if last {
                        if let Some(existing) = mapping.get(*key) { node_json(&existing)?; }
                        if let Some(value) = value { set(mapping, key, value)?; } else { mapping.remove(*key); }
                        return Ok(());
                    }
                    if mapping.get(*key).is_none() {
                        if value.is_none() { return Ok(()); }
                        set(mapping, key, &serde_json::json!({}))?;
                    }
                    node = mapping.get(*key).ok_or_else(error)?;
                }
                YamlNode::Sequence(ref sequence) => {
                    let position = key.parse::<usize>().map_err(|_| error())?;
                    if position.to_string() != *key { return Err(error()); }
                    if last {
                        if position >= sequence.len() { return Err(error()); }
                        node_json(&sequence.into_iter().nth(position).ok_or_else(error)?)?;
                        if let Some(value) = value { sequence.set(position, json_node(value)?); } else { sequence.remove(position); }
                        return Ok(());
                    }
                    let next = { sequence.into_iter().nth(position).ok_or_else(error)? };
                    node = next;
                }
                _ => return Err(error()),
            }
        }
        Err(error())
    }
    pub fn render(&self) -> Result<String, String> {
        let mut text = self.file.to_string();
        if !text.ends_with('\n') { text.push('\n'); }
        Self::from_text(&text)?;
        Ok(text)
    }
}

fn contains_id(node: &YamlNode, id: &str, depth: usize) -> Result<bool, String> {
    if depth > 64 { return Err(error()); }
    match node {
        YamlNode::Mapping(m) => {
            if let Some(n) = m.get("id") { if node_json(&n)?.as_str().ok_or_else(error)? == id { return Ok(true); } }
            if let Some(n) = m.get("insert") { if contains_id(&n, id, depth + 1)? { return Ok(true); } }
            if let Some(group) = m.get("group") {
                if node_json(&group)? == Value::Bool(true) {
                    if let Some(n) = m.get("config") { if contains_id(&n, id, depth + 1)? { return Ok(true); } }
                }
            }
        }
        YamlNode::Sequence(s) => for n in s { if contains_id(&n, id, depth + 1)? { return Ok(true); } },
        YamlNode::TaggedNode(_) | YamlNode::Alias(_) => return Err(error()),
        _ => {},
    }
    Ok(false)
}

/// Home patches override profile configs. Creating a whole config can also
/// erase inherited bundle fields: only the verified default tuple is admitted
/// when the profile does not already own a complete config object.
pub fn guard_profile(path: &Path, ids: &[&str]) -> Result<(), String> {
    let home = home_dir();
    let profile = path.parent().and_then(Path::file_name).and_then(|n| n.to_str()).ok_or_else(error)?;
    if !matches!(profile, "web" | "desktop") || path != home.join("profiles").join(profile).join("cordis.patch.yml") { return Err("DeepSeek Harness writer requires its verified web/desktop profile path".into()); }
    let own = PatchFile::read(path)?;
    let higher = PatchFile::read(&home.join("cordis.patch.yml"))?;
    let mut needs_defaults = false;
    for id in ids {
        if higher.has_row(id)? { return Err("DeepSeek Harness home patch overrides this profile target".into()); }
        own.config_node(id, false)?;
        if own.config(id)?.is_none() { needs_defaults = true; }
    }
    if needs_defaults {
        let directory = path.parent().ok_or_else(error)?;
        let text = read_text(&directory.join("package.json"))?.ok_or("DeepSeek Harness inherited bundle configuration is unknown")?;
        let manifest: Value = serde_json::from_str(&text).map_err(|_| error())?;
        let defaults = serde_json::json!(["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"]);
        if manifest.pointer("/dsh/profile/bundles") != Some(&defaults) { return Err("custom DeepSeek Harness bundles require an explicit complete profile config".into()); }
        for name in ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] {
            if ["dependencies", "devDependencies", "optionalDependencies"].iter().any(|key| manifest.get(key).and_then(|m| m.get(name)).is_some())
                || fs::symlink_metadata(directory.join("node_modules").join(name)).is_ok() { return Err("profile-local bundle overrides cannot authorize inherited configuration replacement".into()); }
        }
        if ids.contains(&"mcp-manager") && own.config("mcp-manager")?.is_none() {
            super::deepseek_harness_installation::require_manager(profile)?;
        }
    }
    Ok(())
}
