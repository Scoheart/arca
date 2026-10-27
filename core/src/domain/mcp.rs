//! MCP value objects that do not perform filesystem or network I/O.

use super::types::McpConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Fields that differ from the canonical MCP asset.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OverridePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

/// A full snapshot of an MCP removed from an Agent while its desired
/// relationship is disabled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisabledEntry {
    pub name: String,
    pub transport: String,
    pub scope: String,
    pub config: McpConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Value>,
}

/// Installed native provider probe metadata; not a writable MCP configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuiltinMcpObservation {
    pub agent_id: String,
    pub server_id: String,
    pub name: String,
    pub provider: String,
    pub source_path: String,
    pub app_version: String,
    pub docs_url: String,
    pub source_fingerprint: String,
    pub plugin_id: String,
    pub plugin_version: String,
    pub manifest_path: String,
}
