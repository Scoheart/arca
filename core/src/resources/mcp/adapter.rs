//! MCP Agent adapter contract.

use crate::domain::types::{AgentDefinition, McpConfig};
use crate::resources::mcp::codec::{for_agent, from_name, Codec};
use crate::resources::mcp::json_adapter::JsonAdapter;
use crate::resources::mcp::toml_adapter::TomlAdapter;
use crate::resources::mcp::toml_list_adapter::TomlListAdapter;
use crate::resources::mcp::yaml_adapter::YamlAdapter;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub trait Adapter {
    /// Read the `key` section as `{name: config}`, parsing each entry
    /// independently — an entry that doesn't fit the stdio/http shape is skipped
    /// (it stays on disk, it just isn't surfaced). A missing file, unparseable
    /// content, or absent key degrade gracefully to an empty map.
    fn read(&self, path: &Path) -> BTreeMap<String, McpConfig>;
    /// Observe installed entries including an Agent's native paused state.
    fn read_with_enabled(&self, path: &Path) -> BTreeMap<String, (McpConfig, bool)> {
        self.read(path).into_iter().map(|(name, config)| (name, (config, true))).collect()
    }
    fn supports_native_enabled(&self) -> bool { false }
    /// Change only the native state flag, bound to the complete observed entry.
    fn set_enabled(&self, _path: &Path, _name: &str, _enabled: bool, _snapshot: &Value) -> Result<(), String> {
        Err("Agent does not support a native MCP enabled flag".into())
    }
    /// Insert or update a SINGLE server by name under `key`, leaving every other
    /// entry's raw on-disk representation untouched. Within an existing target,
    /// only codec-owned connection fields change; user policy fields survive.
    fn upsert(&self, path: &Path, name: &str, cfg: &McpConfig) -> Result<(), String>;
    /// Remove `names` from the `key` section, leaving all other entries untouched.
    /// No-op if the file does not exist.
    fn remove(&self, path: &Path, names: &[String]) -> Result<(), String>;
    /// Remove every entry from the MCP section while preserving all sibling
    /// configuration. Unlike `read` + `remove`, this also clears entries whose
    /// payload cannot be decoded as a supported MCP transport.
    fn clear(&self, path: &Path) -> Result<(), String>;
    /// Return whether the raw MCP section contains any entries, including
    /// entries that are not decodable as supported MCP transports.
    fn has_entries(&self, path: &Path) -> Result<bool, String>;
    /// Capture one complete server entry before a destructive operation. Unlike
    /// `read`, this includes Agent-owned policy fields that Arca does not model.
    fn snapshot(&self, path: &Path, name: &str) -> Result<Option<Value>, String>;
    /// Remove one entry only if its complete semantic value still matches the
    /// snapshot persisted by the disable flow.
    fn remove_snapshot(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String>;
    /// Restore a previously captured complete entry. Refuses to overwrite an
    /// entry recreated by the Agent or user while it was disabled.
    fn restore(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String>;
}

/// Pick the generic map adapter used by imported catalog sources.
pub fn get_adapter(format: &str, key: &str) -> Box<dyn Adapter> {
    get_map_adapter(format, key, Codec::Standard, BTreeMap::new())
}

/// Backward-compatible helper for tests and legacy callers. Built-in production
/// targets use [`get_agent_adapter_for`] so format/layout/codec come from the
/// audited Agent definition instead of an ever-growing ID switch.
pub fn get_agent_adapter(format: &str, key: &str, agent_id: &str) -> Box<dyn Adapter> {
    get_map_adapter(format, key, for_agent(agent_id), BTreeMap::new())
}

pub fn get_agent_adapter_for(definition: &AgentDefinition, agent_id: &str) -> Box<dyn Adapter> {
    let codec = from_name(definition.codec.as_deref(), agent_id);
    let root_defaults = definition.root_defaults.clone().unwrap_or_default();
    let list = definition.layout.as_deref() == Some("list");
    match (definition.format.as_str(), list) {
        ("json", false) => Box::new(JsonAdapter::with_spec_and_key_path(
            &definition.key,
            codec,
            root_defaults,
            definition.key_path,
        )),
        ("toml", false) => Box::new(TomlAdapter::with_spec(
            &definition.key,
            codec,
            root_defaults,
        )),
        ("toml", true) => match definition.identity_field.as_deref() {
            Some(identity) => Box::new(TomlListAdapter::with_spec(
                &definition.key,
                identity,
                codec,
                root_defaults,
            )),
            None => Box::new(UnsupportedAdapter::new(
                "list-shaped TOML Agent is missing identity_field",
            )),
        },
        ("yaml", _) => Box::new(YamlAdapter::with_spec(
            &definition.key,
            codec,
            list,
            definition.identity_field.clone(),
            root_defaults,
        )),
        ("json", true) => Box::new(UnsupportedAdapter::new(
            "list-shaped JSON Agent configs are not supported",
        )),
        (format, _) => Box::new(UnsupportedAdapter::new(&format!(
            "unsupported Agent config format: {format}"
        ))),
    }
}

fn get_map_adapter(
    format: &str,
    key: &str,
    codec: Codec,
    root_defaults: BTreeMap<String, Value>,
) -> Box<dyn Adapter> {
    match format {
        "json" => Box::new(JsonAdapter::with_spec(key, codec, root_defaults)),
        "toml" => Box::new(TomlAdapter::with_spec(key, codec, root_defaults)),
        "yaml" => Box::new(YamlAdapter::with_spec(
            key,
            codec,
            false,
            None,
            root_defaults,
        )),
        _ => Box::new(UnsupportedAdapter::new(&format!(
            "unsupported config format: {format}"
        ))),
    }
}

struct UnsupportedAdapter {
    error: String,
}

impl UnsupportedAdapter {
    fn new(error: &str) -> Self {
        Self {
            error: error.into(),
        }
    }

    fn error(&self) -> Result<(), String> {
        Err(self.error.clone())
    }
}

impl Adapter for UnsupportedAdapter {
    fn read(&self, _path: &Path) -> BTreeMap<String, McpConfig> {
        BTreeMap::new()
    }

    fn upsert(&self, _path: &Path, _name: &str, _cfg: &McpConfig) -> Result<(), String> {
        self.error()
    }

    fn remove(&self, _path: &Path, _names: &[String]) -> Result<(), String> {
        self.error()
    }

    fn clear(&self, _path: &Path) -> Result<(), String> {
        self.error()
    }

    fn has_entries(&self, _path: &Path) -> Result<bool, String> {
        Err(self.error.clone())
    }

    fn snapshot(&self, _path: &Path, _name: &str) -> Result<Option<Value>, String> {
        Err(self.error.clone())
    }

    fn remove_snapshot(&self, _path: &Path, _name: &str, _snapshot: &Value) -> Result<(), String> {
        self.error()
    }

    fn restore(&self, _path: &Path, _name: &str, _snapshot: &Value) -> Result<(), String> {
        self.error()
    }
}
