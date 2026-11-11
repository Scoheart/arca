//! Startup Agent (Primary / Main Agent) domain contracts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StartupAgentDefinition {
    /// Unique identifier for the agent (kebab-case, e.g. "code-architect", "plan")
    pub name: String,
    /// Display name shown in UI
    pub display_name: String,
    /// Brief description of the agent's role
    pub description: String,
    /// Complete system prompt instructions
    pub system_prompt: String,
    /// Optional preferred model (e.g. "sonnet", "fast", "smart", or specific profile id)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Tool whitelist allowed for this agent
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Mode: "primary" | "all" | "subagent"
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Whether this agent runs in read-only sandbox mode
    #[serde(default)]
    pub readonly: bool,
    /// ISO-8601 creation timestamp
    #[serde(default)]
    pub created_at: String,
    /// ISO-8601 last update timestamp
    #[serde(default)]
    pub updated_at: String,
}

fn default_mode() -> String {
    "primary".to_string()
}

impl StartupAgentDefinition {
    pub fn validate(&self) -> Result<(), crate::domain::assets::SelectionError> {
        let trimmed = self.name.trim();
        if trimmed.is_empty() {
            return Err(crate::domain::assets::SelectionError::InvalidIdentity {
                field: "name",
                value: self.name.clone(),
            });
        }
        if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(crate::domain::assets::SelectionError::InvalidIdentity {
                field: "name",
                value: self.name.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StartupAgentSource {
    Builtin,
    Central,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentStartupAgentItemView {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub is_active: bool,
    pub is_default: bool,
    pub source: StartupAgentSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentStartupAgentsStateView {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,
    pub supports_config_persistence: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_flag: Option<String>,
    pub items: Vec<AgentStartupAgentItemView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StartupAgentQuickOption {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_active: bool,
    pub is_default: bool,
}
