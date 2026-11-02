//! Google Antigravity subagent codec: JSON format.

use crate::domain::subagent::SubagentDefinition;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AntigravitySubagentPayload {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_true")]
    pub enable_write_tools: bool,
    #[serde(default = "default_true")]
    pub enable_mcp_tools: bool,
    #[serde(default)]
    pub enable_subagent_tools: bool,
}

fn default_model() -> String {
    "inherit".into()
}

fn default_true() -> bool {
    true
}

/// Serializes a SubagentDefinition to Google Antigravity JSON format.
pub fn project_antigravity_subagent(definition: &SubagentDefinition) -> Result<String, String> {
    let payload = AntigravitySubagentPayload {
        name: definition.name.clone(),
        description: definition.description.clone(),
        system_prompt: definition.system_prompt.clone(),
        model: definition
            .model
            .clone()
            .unwrap_or_else(|| "inherit".to_string()),
        enable_write_tools: !definition.readonly,
        enable_mcp_tools: true,
        enable_subagent_tools: definition.allow_subagents,
    };

    serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
}

/// Parses Google Antigravity JSON into a SubagentDefinition.
pub fn parse_antigravity_subagent(fallback_name: &str, content: &str) -> Result<SubagentDefinition, String> {
    let payload: AntigravitySubagentPayload =
        serde_json::from_str(content).map_err(|e| format!("Invalid Antigravity subagent JSON: {e}"))?;

    let name = if payload.name.trim().is_empty() {
        fallback_name.to_string()
    } else {
        payload.name
    };

    Ok(SubagentDefinition {
        name: name.clone(),
        display_name: name,
        description: payload.description,
        system_prompt: payload.system_prompt,
        model: if payload.model == "inherit" {
            None
        } else {
            Some(payload.model)
        },
        tools: Vec::new(),
        mcp_dependencies: Vec::new(),
        readonly: !payload.enable_write_tools,
        allow_subagents: payload.enable_subagent_tools,
        created_at: String::new(),
        updated_at: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_antigravity_subagent() {
        let def = SubagentDefinition {
            name: "image-generator".into(),
            display_name: "image-generator".into(),
            description: "Generates images from a brief".into(),
            system_prompt: "Create images according to instructions.".into(),
            model: Some("smart".into()),
            tools: Vec::new(),
            mcp_dependencies: Vec::new(),
            readonly: false,
            allow_subagents: true,
            created_at: "".into(),
            updated_at: "".into(),
        };

        let projected = project_antigravity_subagent(&def).unwrap();
        let parsed = parse_antigravity_subagent("image-generator", &projected).unwrap();

        assert_eq!(parsed.name, def.name);
        assert_eq!(parsed.description, def.description);
        assert_eq!(parsed.system_prompt, def.system_prompt);
        assert_eq!(parsed.model, def.model);
        assert_eq!(parsed.readonly, def.readonly);
        assert_eq!(parsed.allow_subagents, def.allow_subagents);
    }
}
