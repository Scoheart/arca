//! Passive Cordis patch editing. Never evaluate tags or load Harness modules.
use serde_json::{Map, Value};
use std::{collections::BTreeSet, fs, io::Read, path::{Component, Path, PathBuf}, str::FromStr};
use yaml_edit::{Document, Mapping, Sequence, YamlFile, YamlNode};

mod file_skills;
pub(crate) use file_skills::observe_file_skills;

const LIMIT: usize = 2 * 1024 * 1024;
const ACP_BUNDLE: &str = "@deepseek-ai/dsh-acp-app";
const SDK_BUNDLE: &str = "@deepseek-ai/dsh-sdk-app";
const SDK_MINIMAL_BUNDLE: &str = "@deepseek-ai/dsh-sdk-minimal";
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

pub(crate) fn home_patch_path() -> PathBuf { home_dir().join("cordis.patch.yml") }

pub(crate) fn default_profile(agent_id: &str) -> Option<&'static str> {
    match agent_id {
        "deepseek-harness" => Some("web"),
        "deepseek-harness-desktop" => Some("desktop"),
        _ => None,
    }
}

fn profiles_dir() -> PathBuf { home_dir().join("profiles") }

fn valid_profile_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && !matches!(name, "." | "..")
        && !name.eq_ignore_ascii_case("node_modules")
        && !name.contains(['/', '\\']) && !name.chars().any(char::is_control)
}

pub fn profile_path(profile: &str) -> String {
    super::mcp::scanner::collapse_home(
        &profiles_dir().join(profile).join("cordis.patch.yml").to_string_lossy(),
    )
}

pub(crate) fn profile_at(path: &Path) -> Result<&str, String> {
    let profile = path.parent().and_then(Path::file_name).and_then(|name| name.to_str())
        .filter(|name| valid_profile_name(name))
        .ok_or_else(|| "Invalid DeepSeek Harness profile path".to_string())?;
    if path != profiles_dir().join(profile).join("cordis.patch.yml") {
        return Err("DeepSeek Harness requires a cordis.patch.yml directly under $DSH_HOME/profiles/<name>".into());
    }
    Ok(profile)
}

pub(crate) fn agent_profile_at<'a>(agent_id: &str, path: &'a Path) -> Result<&'a str, String> {
    let profile = profile_at(path)?;
    let default = default_profile(agent_id).ok_or_else(|| "Unknown DeepSeek Harness Agent".to_string())?;
    if (default == "desktop" && profile != "desktop")
        || (default == "web" && profile.eq_ignore_ascii_case("desktop"))
    {
        return Err("DeepSeek Harness Desktop and CLI use separate profiles".into());
    }
    Ok(profile)
}

pub(crate) fn normalize_agent_patch_path(agent_id: &str, value: &str) -> Result<String, String> {
    let path = super::mcp::scanner::expand_tilde(value.trim());
    let profile = agent_profile_at(agent_id, &path)?;
    if Some(profile) != default_profile(agent_id) {
        profile_bundles(path.parent().ok_or_else(error)?)?;
    }
    Ok(profile_path(profile))
}

pub(crate) fn profile_options(agent_id: &str) -> Vec<crate::domain::agents::AgentNativeProfile> {
    let Some(default) = default_profile(agent_id) else { return Vec::new(); };
    let mut options = vec![crate::domain::agents::AgentNativeProfile {
        name: default.into(), path: profile_path(default),
    }];
    if default == "web" {
        let directory = profiles_dir();
        if let Ok(entries) = fs::read_dir(directory) {
            options.extend(entries.take(256).filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name().into_string().ok()?;
                if !entry.file_type().ok()?.is_dir() || !valid_profile_name(&name)
                    || name.starts_with('.') || name.eq_ignore_ascii_case("web")
                    || name.eq_ignore_ascii_case("desktop") || profile_bundles(&entry.path()).is_err()
                {
                    return None;
                }
                Some(crate::domain::agents::AgentNativeProfile { path: profile_path(&name), name })
            }));
            options[1..].sort_by(|left, right| left.name.cmp(&right.name));
        }
    }
    options
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

pub const DSH_MCP_CLIENT_NAME: &str = "@deepseek-ai/dsh-mcp-client";
const DSH_MCP_MANAGER_ID: &str = "mcp-manager";

pub fn valid_mcp_server_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

pub fn node_json_inert(node: &YamlNode) -> Result<Value, String> { node_json_inert_at(node, 0) }
fn node_json_inert_at(node: &YamlNode, depth: usize) -> Result<Value, String> {
    if depth > 64 { return Err(error()); }
    match node {
        YamlNode::Mapping(mapping) => {
            let mut object = Map::new();
            for entry in mapping.entries() {
                let key = node_json_inert_at(&entry.key_node().ok_or_else(error)?, depth + 1)?;
                let key = key.as_str().ok_or_else(error)?.to_string();
                if key == "<<" || object.contains_key(&key) { return Err(error()); }
                object.insert(key, node_json_inert_at(&entry.value_node().ok_or_else(error)?, depth + 1)?);
            }
            Ok(Value::Object(object))
        }
        YamlNode::Sequence(sequence) => sequence.into_iter().map(|n| node_json_inert_at(&n, depth + 1)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        YamlNode::Scalar(scalar) => if scalar.is_quoted() || scalar.value().starts_with(['|', '>']) {
            Ok(Value::String(scalar.as_string()))
        } else { serde_yaml::from_str(&scalar.value()).map_err(|_| error()) },
        YamlNode::TaggedNode(tagged) => {
            let tag = tagged.tag().unwrap_or_default();
            let val = tagged.value().map(|s| s.as_string()).unwrap_or_else(|| tagged.to_string());
            if tag.is_empty() {
                Ok(Value::String(val))
            } else {
                Ok(Value::String(format!("{tag} {val}")))
            }
        }
        _ => Err(error()),
    }
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
                let expected = match id {
                    "mcp-manager" => "@deepseek-ai/dsh-mcp-manager",
                    "llm-pi-ai" => "@deepseek-ai/dsh-llm-pi-ai",
                    "agent-default-model" => "@deepseek-ai/dsh-agent-default-model",
                    "acp" => "@deepseek-ai/dsh-acp",
                    _ => "",
                };
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

    fn legacy_mcp_servers(&self) -> Result<Option<Mapping>, String> {
        let Some(row) = self.row(DSH_MCP_MANAGER_ID)? else { return Ok(None); };
        let Some(config) = row.get("config") else { return Ok(None); };
        let YamlNode::Mapping(config) = config else { return Err(error()); };
        match config.get("servers") {
            Some(YamlNode::Mapping(servers)) => Ok(Some(servers)),
            None => Ok(None),
            _ => Err(error()),
        }
    }

    fn mcp_entries(&self) -> Result<Vec<(String, Mapping)>, String> {
        let mut entries = Vec::new();
        let mut names = BTreeSet::new();
        let mut ids = BTreeSet::new();

        for node in &self.sequence()? {
            let Some(mapping) = node.as_mapping() else { return Err(error()); };
            if let Some(id_node) = mapping.get("id") {
                let id_str = node_json_inert(&id_node)?.as_str().ok_or_else(error)?.to_string();
                if !ids.insert(id_str) { return Err("duplicate id in patch file".into()); }
            }
            if let Some(insert) = mapping.get("insert") {
                let YamlNode::Sequence(insert_seq) = insert else { return Err(error()); };
                for item in &insert_seq {
                    let Some(plugin) = item.as_mapping().cloned() else { continue; };
                    if let Some(id_node) = plugin.get("id") {
                        let id_str = node_json_inert(&id_node)?.as_str().ok_or_else(error)?.to_string();
                        if !ids.insert(id_str) { return Err("duplicate id in patch file".into()); }
                    }
                    let is_mcp = plugin.get("name")
                        .and_then(|n| node_json_inert(&n).ok())
                        .and_then(|v| v.as_str().map(|s| s == DSH_MCP_CLIENT_NAME))
                        .unwrap_or(false);
                    if !is_mcp { continue; }
                    let config_node = plugin.get("config").ok_or_else(error)?;
                    let config_val = node_json_inert(&config_node)?;
                    let Some(server_name) = config_val.get("serverName").and_then(Value::as_str) else {
                        return Err(error());
                    };
                    if !valid_mcp_server_name(server_name) { return Err(error()); }
                    if !names.insert(server_name.to_string()) {
                        return Err("duplicate serverName in DeepSeek Harness patch".into());
                    }
                    entries.push((server_name.to_string(), plugin));
                }
            }
        }
        Ok(entries)
    }

    pub fn mcp_declaration_names(&self) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        for node in &self.sequence()? {
            let Some(mapping) = node.as_mapping() else { continue; };
            let Some(insert_seq) = mapping.get_sequence("insert") else { continue; };
            for item in &insert_seq {
                let Some(plugin) = item.as_mapping() else { continue; };
                let is_mcp = plugin.get("name")
                    .and_then(|n| node_json_inert(&n).ok())
                    .and_then(|v| v.as_str().map(|s| s == DSH_MCP_CLIENT_NAME))
                    .unwrap_or(false);
                if !is_mcp { continue; }
                if let Some(config_node) = plugin.get("config") {
                    if let Ok(config_val) = node_json_inert(&config_node) {
                        if let Some(server_name) = config_val.get("serverName").and_then(Value::as_str) {
                            if valid_mcp_server_name(server_name) && !names.contains(&server_name.to_string()) {
                                names.push(server_name.to_string());
                            }
                        }
                    }
                }
            }
        }
        if let Some(servers) = self.legacy_mcp_servers()? {
            for name in keys(&servers)? {
                if valid_mcp_server_name(&name) && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        Ok(names)
    }

    pub fn inspect_mcp_declaration(&self, name: &str) -> Result<Option<Value>, String> {
        if let Some(client) = self.inspect_mcp(name)? {
            return Ok(Some(client));
        }
        self.legacy_mcp_servers()?.and_then(|servers| servers.get(name))
            .map(|entry| node_json_inert(&entry)).transpose()
    }

    pub fn inspect_mcp(&self, name: &str) -> Result<Option<Value>, String> {
        for node in &self.sequence()? {
            let Some(mapping) = node.as_mapping() else { continue; };
            let Some(insert_seq) = mapping.get_sequence("insert") else { continue; };
            for item in &insert_seq {
                let Some(plugin) = item.as_mapping() else { continue; };
                let is_mcp = plugin.get("name")
                    .and_then(|n| node_json_inert(&n).ok())
                    .and_then(|v| v.as_str().map(|s| s == DSH_MCP_CLIENT_NAME))
                    .unwrap_or(false);
                if !is_mcp { continue; }
                let Some(config_node) = plugin.get("config") else { continue; };
                let Ok(config_val) = node_json_inert(&config_node) else { continue; };
                if config_val.get("serverName").and_then(Value::as_str) == Some(name) {
                    let Value::Object(mut map) = config_val else { return Err(error()); };
                    let disabled = plugin.get("disabled").map(|n| node_json_inert(&n))
                        .transpose()?.unwrap_or(Value::Bool(false));
                    map.insert("disabled".into(), disabled);
                    return Ok(Some(Value::Object(map)));
                }
            }
        }
        Ok(None)
    }

    pub fn mcp_servers(&self) -> Result<Map<String, Value>, String> {
        let mut map = Map::new();
        for (name, plugin) in self.mcp_entries()? {
            let config_node = plugin.get("config").ok_or_else(error)?;
            let config_val = node_json_inert(&config_node)?;
            let Value::Object(mut entry_map) = config_val else { return Err(error()); };
            let disabled = plugin.get("disabled").map(|n| node_json_inert(&n))
                .transpose()?.unwrap_or(Value::Bool(false));
            entry_map.insert("disabled".into(), disabled);
            map.insert(name, Value::Object(entry_map));
        }
        Ok(map)
    }

    pub fn set_mcp_enabled(&mut self, name: &str, enabled: bool) -> Result<(), String> {
        let entries = self.mcp_entries()?;
        for (server_name, plugin) in entries {
            if server_name == name {
                set(&plugin, "disabled", &Value::Bool(!enabled))?;
                return Ok(());
            }
        }
        Err("DeepSeek Harness MCP is missing".into())
    }

    pub fn upsert_mcp(
        &mut self,
        name: &str,
        fields: &[(String, Value)],
        controlled: &[&str],
        defaults: &[(String, Value)],
    ) -> Result<(), String> {
        if !valid_mcp_server_name(name) {
            return Err("DeepSeek Harness server name must match [A-Za-z0-9_-]{1,32}".into());
        }

        let existing_entry = self.mcp_entries()?.into_iter().find(|(n, _)| n == name);
        if let Some((_, plugin)) = existing_entry {
            let config = plugin.get_mapping("config").ok_or_else(error)?;
            return update_mcp_config(&config, name, fields, controlled, defaults);
        }

        let target_id = format!("mcp-{name}");
        for node in &self.sequence()? {
            if contains_id(&node, &target_id, 0)? {
                return Err("DeepSeek Harness id already exists in patch".into());
            }
        }

        if let Some(servers) = self.legacy_mcp_servers()? {
            if let Some(legacy) = servers.find_entry_by_key(name) {
                // Older Arca versions wrote an override for a manager absent
                // from the published carriers. Move only the selected server.
                self.config_node(DSH_MCP_MANAGER_ID, false)?;
                let entry = node_json(&legacy.value_node().ok_or_else(error)?)?;
                let enabled = match entry.get("enabled") {
                    None => true,
                    Some(Value::Bool(enabled)) => *enabled,
                    _ => return Err("DeepSeek Harness MCP enabled must be boolean".into()),
                };
                if entry.get("serverName").is_some() || entry.get("disabled").is_some()
                    || contains_credential_template(&entry)
                {
                    return Err("DeepSeek Harness MCP uses manager-only configuration; convert it in Harness before assigning".into());
                }
                // Copy the whole pair so the server key's comment travels
                // with its configuration, including in a flow-style map.
                let pair = legacy.to_string();
                let copied = if servers.is_flow_style() { format!("{{{pair}}}") } else { pair };
                let row = Document::from_str(&copied).map_err(|_| error())?;
                let plugin = row.as_mapping().ok_or_else(error)?;
                if keys(&plugin)? != [name] || !plugin.rename_key(name, "config") { return Err(error()); }
                set(&plugin, "id", &Value::String(target_id))?;
                set(&plugin, "name", &Value::String(DSH_MCP_CLIENT_NAME.into()))?;
                let config = plugin.get_mapping("config").ok_or_else(error)?;
                config.remove("enabled");
                update_mcp_config(&config, name, fields, controlled, defaults)?;
                if !enabled {
                    set(&plugin, "disabled", &Value::Bool(true))?;
                }
                self.mcp_insert_sequence()?.push(plugin);
                servers.remove(name);
                return Ok(());
            }
        }

        let mut config_obj = Map::new();
        config_obj.insert("serverName".into(), Value::String(name.into()));
        for (field, value) in fields {
            config_obj.insert(field.clone(), value.clone());
        }
        for (field, value) in defaults {
            if !config_obj.contains_key(field) {
                config_obj.insert(field.clone(), value.clone());
            }
        }

        let plugin_val = serde_json::json!({
            "id": target_id,
            "name": DSH_MCP_CLIENT_NAME,
            "config": config_obj,
        });
        let plugin_node = json_node(&plugin_val)?;
        let YamlNode::Mapping(plugin_mapping) = plugin_node else { return Err(error()); };
        self.mcp_insert_sequence()?.push(plugin_mapping);
        Ok(())
    }

    pub fn remove_mcp(&mut self, name: &str) -> Result<bool, String> {
        let top_seq = self.sequence()?;
        let mut found_pos = None;

        for (top_idx, node) in top_seq.clone().into_iter().enumerate() {
            let Some(mapping) = node.as_mapping().cloned() else { continue; };
            let Some(insert_seq) = mapping.get_sequence("insert") else { continue; };
            for (item_idx, item) in insert_seq.clone().into_iter().enumerate() {
                let Some(plugin) = item.as_mapping() else { continue; };
                let is_mcp = plugin.get("name")
                    .and_then(|n| node_json_inert(&n).ok())
                    .and_then(|v| v.as_str().map(|s| s == DSH_MCP_CLIENT_NAME))
                    .unwrap_or(false);
                if !is_mcp { continue; }
                let Some(config_node) = plugin.get("config") else { continue; };
                let Ok(config_val) = node_json_inert(&config_node) else { continue; };
                if config_val.get("serverName").and_then(Value::as_str) == Some(name) {
                    found_pos = Some((top_idx, mapping, insert_seq, item_idx));
                    break;
                }
            }
            if found_pos.is_some() { break; }
        }

        let Some((top_idx, mapping, insert_seq, item_idx)) = found_pos else { return Ok(false); };
        insert_seq.remove(item_idx);
        if insert_seq.is_empty() {
            let remaining_keys = keys(&mapping)?;
            if remaining_keys.len() == 1 && remaining_keys[0] == "insert" {
                top_seq.remove(top_idx);
            }
        }
        Ok(true)
    }

    pub fn restore_mcp(&mut self, name: &str, snapshot: &Value) -> Result<(), String> {
        if self.inspect_mcp(name)?.is_some() {
            return Err("DeepSeek Harness MCP was recreated before restore".into());
        }
        let Value::Object(snapshot_map) = snapshot else { return Err(error()); };
        let mut config_obj = snapshot_map.clone();
        let disabled = config_obj.remove("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
        config_obj.insert("serverName".into(), Value::String(name.into()));

        let target_id = format!("mcp-{}", name);
        for node in &self.sequence()? {
            if contains_id(&node, &target_id, 0)? {
                return Err("DeepSeek Harness id already exists in patch".into());
            }
        }

        let mut plugin_val = serde_json::json!({
            "id": target_id,
            "name": DSH_MCP_CLIENT_NAME,
            "config": config_obj,
        });
        if disabled {
            plugin_val.as_object_mut().unwrap().insert("disabled".into(), Value::Bool(true));
        }
        let plugin_node = json_node(&plugin_val)?;
        let YamlNode::Mapping(plugin_mapping) = plugin_node else { return Err(error()); };
        self.mcp_insert_sequence()?.push(plugin_mapping);
        Ok(())
    }

    fn mcp_insert_sequence(&self) -> Result<Sequence, String> {
        let sequence = self.sequence()?;
        for node in &sequence {
            let mapping = node.as_mapping().ok_or_else(error)?;
            if keys(mapping)? == ["insert"] {
                return mapping.get_sequence("insert").ok_or_else(error);
            }
        }
        // An insertion on another patch row is relative to that target.
        let row = json_node(&serde_json::json!({"insert": []}))?;
        let YamlNode::Mapping(row) = row else { return Err(error()); };
        sequence.push(row);
        sequence.into_iter().last()
            .and_then(|node| node.as_mapping()?.get_sequence("insert")).ok_or_else(error)
    }
}

fn update_mcp_config(
    config: &Mapping,
    name: &str,
    fields: &[(String, Value)],
    controlled: &[&str],
    defaults: &[(String, Value)],
) -> Result<(), String> {
    for key in controlled {
        if *key != "serverName" && !fields.iter().any(|(field, _)| field == key) {
            config.remove(*key);
        }
    }
    for (field, value) in fields {
        set(config, field, value)?;
    }
    set(config, "serverName", &Value::String(name.into()))?;
    for (field, value) in defaults {
        if config.get(field).is_none() {
            set(config, field, value)?;
        }
    }
    Ok(())
}

fn contains_credential_template(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("${credential:"),
        Value::Array(values) => values.iter().any(contains_credential_template),
        Value::Object(fields) => fields.values().any(contains_credential_template),
        _ => false,
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
        YamlNode::TaggedNode(_) => return Ok(false),
        YamlNode::Alias(_) => return Err(error()),
        _ => {},
    }
    Ok(false)
}

fn profile_bundles(directory: &Path) -> Result<(Value, Vec<String>), String> {
    let text = read_text(&directory.join("package.json"))?
        .ok_or("Initialize this DeepSeek Harness profile before configuring it in Arca")?;
    let manifest: Value = serde_json::from_str(&text).map_err(|_| error())?;
    let bundles = manifest.pointer("/dsh/profile/bundles").and_then(Value::as_array)
        .ok_or("DeepSeek Harness profile has no bundle declaration")?
        .iter().map(|name| name.as_str().map(str::to_owned).ok_or_else(error))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((manifest, bundles))
}

/// The ACP bridge gives each new session an explicit model, overriding the
/// base bundle's otherwise shared agent-default-model service.
pub(crate) fn model_default_entry(path: &Path) -> Result<Option<&'static str>, String> {
    profile_at(path)?;
    let (_, bundles) = profile_bundles(path.parent().ok_or_else(error)?)?;
    if bundles.iter().any(|name| matches!(name.as_str(), SDK_BUNDLE | SDK_MINIMAL_BUNDLE)) {
        // SDK initialize supplies the provider/model from its client.
        return Ok(None);
    }
    Ok(Some(if bundles.iter().any(|name| name == ACP_BUNDLE) { "acp" } else { "agent-default-model" }))
}

fn guard_bundle_inheritance(path: &Path, needs_defaults: bool) -> Result<(), String> {
    const BASE: &str = "@deepseek-ai/dsh-base";
    const WEB: &str = "@deepseek-ai/dsh-web-app";
    const HEADLESS: &str = "@deepseek-ai/dsh-headless";
    let directory = path.parent().ok_or_else(error)?;
    let (manifest, bundles) = profile_bundles(directory)?;
    let names: Vec<_> = bundles.iter().map(String::as_str).collect();
    if names.first() != Some(&BASE) {
        return Err("This DeepSeek Harness profile does not select the standard base bundle".into());
    }
    if needs_defaults && !matches!(names.as_slice(),
        [BASE] | [BASE, WEB | HEADLESS | ACP_BUNDLE | SDK_BUNDLE] | [BASE, WEB, HEADLESS])
    {
        return Err("Arca cannot verify inherited configuration from this profile's custom bundles".into());
    }
    for name in names.into_iter().filter(|name| matches!(*name, BASE | WEB | HEADLESS | ACP_BUNDLE | SDK_BUNDLE)) {
        if ["dependencies", "devDependencies", "optionalDependencies"].iter()
            .any(|key| manifest.get(key).and_then(|m| m.get(name)).is_some())
            || fs::symlink_metadata(directory.join("node_modules").join(name)).is_ok()
        {
            return Err("DeepSeek Harness has a profile-local override of an official bundle".into());
        }
    }
    Ok(())
}

/// Home patches override profile configs. Creating a complete config over a
/// bundle requires a verified inherited entry; an existing profile-owned
/// object can be edited leaf by leaf without guessing inherited values.
pub fn guard_profile(path: &Path, ids: &[&str]) -> Result<(), String> {
    profile_at(path)?;
    let own = PatchFile::read(path)?;
    let higher = PatchFile::read(&home_patch_path())?;
    let mut needs_defaults = false;
    for id in ids {
        if higher.has_row(id)? { return Err("DeepSeek Harness home patch overrides this profile target".into()); }
        own.config_node(id, false)?;
        if own.config(id)?.is_none() { needs_defaults = true; }
    }
    guard_bundle_inheritance(path, needs_defaults)
}

pub fn guard_mcp_profile(path: &Path, names: &[&str]) -> Result<(), String> {
    profile_at(path)?;
    let own = PatchFile::read(path)?;
    let higher = PatchFile::read(&home_patch_path())?;
    let entries = if names.is_empty() { Vec::new() } else { own.mcp_entries()? };
    for name in names {
        let target_id = format!("mcp-{name}");
        let own_id = entries.iter().find(|(server, _)| server == name)
            .and_then(|(_, plugin)| plugin.get("id")).map(|node| node_json(&node)).transpose()?;
        let shadowed = match own_id.as_ref().and_then(Value::as_str) {
            Some(id) => higher.has_row(id)?,
            None => false,
        };
        if higher.has_row(&target_id)? || higher.has_row(name)?
            || higher.inspect_mcp_declaration(name)?.is_some()
            || shadowed
        {
            return Err("DeepSeek Harness home patch overrides this profile target".into());
        }
    }
    guard_bundle_inheritance(path, true)
}
