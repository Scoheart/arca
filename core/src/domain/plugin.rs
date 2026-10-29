//! Metadata-only observations. These types are never accepted by asset writers.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PluginPresence { Bundled, Installed, Local, CacheOnly, Declared }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum HostResourceOrigin { Builtin, Plugin }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PluginComponentKind { Mcp, McpReference, Skill, Rule, Agent, Command, Hook, App, Lsp, OutputStyle, Workflow, Theme, Monitor, Eval, Executable, TypeDeclaration, Channel, McpBundle, Context, Policy, Plan, Code, AuthProvider, Extension, PluginReference, Tool, Renderer }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginComponent {
    pub kind: PluginComponentKind,
    pub name: String,
    pub source_path: String,
    pub native_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativePluginObservation {
    pub agent_id: String,
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub revision: String,
    pub presence: PluginPresence,
    /// None means the host's effective enablement has not been observed.
    pub enabled: Option<bool>,
    pub source_paths: Vec<String>,
    pub components: Vec<PluginComponent>,
    pub docs_url: String,
    pub source_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostMcpObservation {
    pub agent_id: String,
    pub server_id: String,
    pub name: String,
    pub provider: String,
    pub origin: HostResourceOrigin,
    pub presence: PluginPresence,
    pub versions: Vec<String>,
    pub source_paths: Vec<String>,
    pub docs_url: String,
    pub source_fingerprint: String,
}

/// Host metadata has its own read path; it never waits for model credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeResourceInventory {
    pub native_plugin_agents: Vec<String>,
    pub host_mcps: Vec<HostMcpObservation>,
    pub native_plugins: Vec<NativePluginObservation>,
    pub unavailable: bool,
}
