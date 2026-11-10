//! Subagent value objects persisted centrally and projected to Agent runtimes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubagentDefinition {
    /// Unique identifier for the subagent (kebab-case, e.g. "code-reviewer")
    pub name: String,
    /// Display name shown in UI
    pub display_name: String,
    /// Brief description of the subagent's role and purpose
    pub description: String,
    /// Complete system prompt instructions for this subagent
    pub system_prompt: String,
    /// Optional preferred model (e.g. "inherit", "fast", "smart", or specific profile id)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Tool whitelist allowed for this subagent
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Optional list of MCP server dependencies required by this subagent
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_dependencies: Vec<String>,
    /// Whether this subagent runs in read-only sandbox mode (no write tools)
    #[serde(default)]
    pub readonly: bool,
    /// Whether this subagent is permitted to spawn descendant subagents
    #[serde(default)]
    pub allow_subagents: bool,
    /// ISO-8601 creation timestamp
    #[serde(default)]
    pub created_at: String,
    /// ISO-8601 last update timestamp
    #[serde(default)]
    pub updated_at: String,
}

impl SubagentDefinition {
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
