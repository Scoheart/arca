//! Hook value objects persisted centrally and projected to Agent runtimes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookDefinition {
    /// Unique identifier for the hook (kebab-case, e.g. "pre-commit-lint", "session-sync")
    pub name: String,
    /// Display name shown in UI
    pub display_name: String,
    /// Brief description of the hook's purpose
    pub description: String,
    /// Event name: e.g. "SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop", "StopFailure", "SessionEnd", "Notification"
    pub event: String,
    /// Matcher pattern (e.g. tool name, regex, or empty for all)
    #[serde(default)]
    pub matcher: String,
    /// Shell command executed by the hook
    pub command: String,
    /// Optional command specifically for Windows environments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_windows: Option<String>,
    /// Hook type: "command" (default)
    #[serde(default = "default_hook_type")]
    pub hook_type: String,
    /// Timeout in seconds (default: 10)
    #[serde(default = "default_timeout")]
    pub timeout: u32,
    /// Whether the hook is currently enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// ISO-8601 creation timestamp
    #[serde(default)]
    pub created_at: String,
    /// ISO-8601 last update timestamp
    #[serde(default)]
    pub updated_at: String,
}

fn default_hook_type() -> String {
    "command".to_string()
}

fn default_timeout() -> u32 {
    10
}

fn default_enabled() -> bool {
    true
}

impl HookDefinition {
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
        if self.event.trim().is_empty() {
            return Err(crate::domain::assets::SelectionError::InvalidIdentity {
                field: "event",
                value: self.event.clone(),
            });
        }
        if self.command.trim().is_empty() {
            return Err(crate::domain::assets::SelectionError::InvalidIdentity {
                field: "command",
                value: self.command.clone(),
            });
        }
        Ok(())
    }
}
