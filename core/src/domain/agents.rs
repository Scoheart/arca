//! Agent identity, capability, and configuration contracts.

use crate::domain::types::ModelProtocol;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModelStorageAuthority {
    NativeRegistry,
    #[serde(alias = "mux-mapping")]
    ArcaMapping,
    Guided,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct McpConfigurationPatch {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelConfigurationPatch {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillConfigurationPatch {
    pub global_dir: String,
    #[serde(default)]
    pub alias_dirs: Vec<String>,
}

/// A partial Agent configuration update. An Agent may expose any combination
/// of MCP, Model, and Skill writers; configuring one never requires another.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct AgentConfigurationPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpConfigurationPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelConfigurationPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<SkillConfigurationPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentIdentityView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub builtin: bool,
    pub category: String,
    pub evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpAgentCapabilityView {
    pub writable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    pub format: String,
    pub key: String,
    #[serde(default)]
    pub supported_transports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelAgentCapabilityView {
    pub mode: String,
    pub storage_authority: ModelStorageAuthority,
    pub installed: bool,
    #[serde(default)]
    pub config_paths: Vec<String>,
    #[serde(default)]
    pub assigned_profiles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    pub supports_multiple: bool,
    pub supports_global_selection: bool,
    pub credential_mode: String,
    #[serde(default)]
    pub supported_protocols: Vec<ModelProtocol>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillAgentCapabilityView {
    pub installed: bool,
    pub target_id: String,
    pub global_dir: String,
    #[serde(default)]
    pub alias_dirs: Vec<String>,
    #[serde(default)]
    pub affected_agent_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AgentCapabilitySet {
    /// Native file inspection/editing; this is not a central asset domain.
    #[serde(default)]
    pub instructions: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpAgentCapabilityView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelAgentCapabilityView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<SkillAgentCapabilityView>,
}

/// Unified Agent projection used by every frontend. Domain-specific details
/// stay typed instead of occupying MCP-shaped fields at the Agent root.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentCapabilityView {
    pub identity: AgentIdentityView,
    /// Runtime evidence takes precedence; unknown runtimes use observed config.
    pub installed: bool,
    #[serde(default)]
    pub runtime_detected: Option<bool>,
    #[serde(default)]
    pub config_detected: bool,
    pub capabilities: AgentCapabilitySet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LaunchTarget {
    App {
        path: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        new_instance: bool,
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
    },
    Cli { command: String, #[serde(default)] args: Vec<String>, #[serde(default)] env: std::collections::BTreeMap<String, String> },
    Web { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LaunchProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub target: Option<LaunchTarget>,
    #[serde(default)]
    pub default_directory: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LaunchPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<LaunchTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<LaunchProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_id: Option<String>,
}

impl LaunchPreferences {
    pub fn has_valid_profile_selection(&self) -> bool {
        self.profiles.is_empty() || (self.profiles.len() <= 32
            && self.active_profile_id.as_ref().is_some_and(|id| self.profiles.iter().any(|profile| &profile.id == id))
            && self.profiles.iter().map(|profile| &profile.id).collect::<std::collections::BTreeSet<_>>().len() == self.profiles.len())
    }

    pub fn active_profile(&self) -> Option<&LaunchProfile> {
        self.profiles.iter().find(|profile| Some(&profile.id) == self.active_profile_id.as_ref())
    }

    pub fn configured_target(&self) -> Option<&LaunchTarget> {
        match self.active_profile() {
            Some(profile) => profile.target.as_ref(),
            None => self.target.as_ref(),
        }
    }

    pub fn configured_directory(&self) -> Option<&String> {
        match self.active_profile() {
            Some(profile) => profile.default_directory.as_ref(),
            None => self.default_directory.as_ref(),
        }
    }
}
