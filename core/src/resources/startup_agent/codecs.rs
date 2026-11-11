//! Codecs for Startup Agents across different AI coding tools.

use crate::domain::startup_agent::StartupAgentDefinition;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OpenCodeFrontmatter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ClaudeFrontmatter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VibeAgentFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
}

/// Serializes to OpenCode Markdown (`~/.config/opencode/agents/<name>.md`).
pub fn project_opencode_agent(definition: &StartupAgentDefinition) -> String {
    let mode = if definition.mode.is_empty() { "primary" } else { definition.mode.as_str() };
    let mut frontmatter = format!(
        "---\ndescription: {}\nmode: {}\n",
        definition.description, mode
    );

    if let Some(model) = &definition.model {
        frontmatter.push_str(&format!("model: {}\n", model));
    }

    if !definition.tools.is_empty() {
        frontmatter.push_str("tools:\n");
        for tool in &definition.tools {
            frontmatter.push_str(&format!("  - {}\n", tool));
        }
    }

    frontmatter.push_str("---\n\n");
    frontmatter.push_str(definition.system_prompt.trim());
    frontmatter.push('\n');
    frontmatter
}

/// Parses OpenCode Markdown.
pub fn parse_opencode_agent(fallback_name: &str, content: &str) -> Result<StartupAgentDefinition, String> {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return Ok(StartupAgentDefinition {
            name: fallback_name.to_string(),
            display_name: fallback_name.to_string(),
            description: String::new(),
            system_prompt: trimmed.to_string(),
            model: None,
            tools: Vec::new(),
            mode: "primary".to_string(),
            readonly: false,
            created_at: String::new(),
            updated_at: String::new(),
        });
    }

    let rest = &trimmed[3..];
    let Some(closing_idx) = rest.find("\n---") else {
        return Ok(StartupAgentDefinition {
            name: fallback_name.to_string(),
            display_name: fallback_name.to_string(),
            description: String::new(),
            system_prompt: trimmed.to_string(),
            model: None,
            tools: Vec::new(),
            mode: "primary".to_string(),
            readonly: false,
            created_at: String::new(),
            updated_at: String::new(),
        });
    };

    let yaml_str = &rest[..closing_idx];
    let body_str = rest[closing_idx + 4..].trim_start_matches(|c| c == '\r' || c == '\n').trim();

    let meta: OpenCodeFrontmatter = serde_yaml::from_str(yaml_str).unwrap_or_default();

    Ok(StartupAgentDefinition {
        name: fallback_name.to_string(),
        display_name: fallback_name.to_string(),
        description: meta.description.unwrap_or_default(),
        system_prompt: body_str.to_string(),
        model: meta.model,
        tools: meta.tools,
        mode: meta.mode.unwrap_or_else(|| "primary".to_string()),
        readonly: false,
        created_at: String::new(),
        updated_at: String::new(),
    })
}

/// Serializes to Claude Code / Grok Build / Kimi Code Markdown format.
pub fn project_claude_agent(definition: &StartupAgentDefinition) -> String {
    let mut frontmatter = format!(
        "---\nname: {}\ndescription: {}\n",
        definition.name, definition.description
    );

    if let Some(model) = &definition.model {
        frontmatter.push_str(&format!("model: {}\n", model));
    }

    if !definition.tools.is_empty() {
        frontmatter.push_str("tools:\n");
        for tool in &definition.tools {
            frontmatter.push_str(&format!("  - {}\n", tool));
        }
    }

    frontmatter.push_str("---\n\n");
    frontmatter.push_str(definition.system_prompt.trim());
    frontmatter.push('\n');
    frontmatter
}

/// Parses Claude / Grok / Kimi Code Markdown.
pub fn parse_claude_agent(fallback_name: &str, content: &str) -> Result<StartupAgentDefinition, String> {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return Ok(StartupAgentDefinition {
            name: fallback_name.to_string(),
            display_name: fallback_name.to_string(),
            description: String::new(),
            system_prompt: trimmed.to_string(),
            model: None,
            tools: Vec::new(),
            mode: "primary".to_string(),
            readonly: false,
            created_at: String::new(),
            updated_at: String::new(),
        });
    }

    let rest = &trimmed[3..];
    let Some(closing_idx) = rest.find("\n---") else {
        return Ok(StartupAgentDefinition {
            name: fallback_name.to_string(),
            display_name: fallback_name.to_string(),
            description: String::new(),
            system_prompt: trimmed.to_string(),
            model: None,
            tools: Vec::new(),
            mode: "primary".to_string(),
            readonly: false,
            created_at: String::new(),
            updated_at: String::new(),
        });
    };

    let yaml_str = &rest[..closing_idx];
    let body_str = rest[closing_idx + 4..].trim_start_matches(|c| c == '\r' || c == '\n').trim();

    let meta: ClaudeFrontmatter = serde_yaml::from_str(yaml_str).unwrap_or_default();

    let name = meta.name.unwrap_or_else(|| fallback_name.to_string());
    let description = meta.description.unwrap_or_default();
    let display_name = name.clone();

    Ok(StartupAgentDefinition {
        name,
        display_name,
        description,
        system_prompt: body_str.to_string(),
        model: meta.model,
        tools: meta.tools,
        mode: "primary".to_string(),
        readonly: false,
        created_at: String::new(),
        updated_at: String::new(),
    })
}

/// Serializes to Mistral Vibe TOML (`~/.vibe/agents/<name>.toml`).
pub fn project_vibe_agent(definition: &StartupAgentDefinition) -> Result<String, String> {
    let vibe_file = VibeAgentFile {
        name: Some(definition.name.clone()),
        description: Some(definition.description.clone()),
        active_model: definition.model.clone(),
        system_prompt: Some(definition.system_prompt.clone()),
        tools: definition.tools.clone(),
    };
    toml::to_string_pretty(&vibe_file).map_err(|e| format!("Failed to serialize Vibe TOML: {e}"))
}

/// Parses Mistral Vibe TOML.
pub fn parse_vibe_agent(fallback_name: &str, content: &str) -> Result<StartupAgentDefinition, String> {
    let vibe_file: VibeAgentFile = toml::from_str(content).map_err(|e| format!("Failed to parse Vibe TOML: {e}"))?;
    let name = vibe_file.name.unwrap_or_else(|| fallback_name.to_string());
    let display_name = name.clone();

    Ok(StartupAgentDefinition {
        name,
        display_name,
        description: vibe_file.description.unwrap_or_default(),
        system_prompt: vibe_file.system_prompt.unwrap_or_default(),
        model: vibe_file.active_model,
        tools: vibe_file.tools,
        mode: "primary".to_string(),
        readonly: false,
        created_at: String::new(),
        updated_at: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_opencode_agent() {
        let def = StartupAgentDefinition {
            name: "planner".into(),
            display_name: "planner".into(),
            description: "High level planner".into(),
            system_prompt: "You plan software tasks.".into(),
            model: Some("claude-sonnet".into()),
            tools: vec!["read_file".into()],
            mode: "primary".into(),
            readonly: false,
            created_at: "".into(),
            updated_at: "".into(),
        };

        let projected = project_opencode_agent(&def);
        let parsed = parse_opencode_agent("planner", &projected).unwrap();
        assert_eq!(parsed.name, "planner");
        assert_eq!(parsed.description, def.description);
        assert_eq!(parsed.model, def.model);
        assert_eq!(parsed.tools, def.tools);
        assert_eq!(parsed.system_prompt, def.system_prompt);
        assert_eq!(parsed.mode, "primary");
    }

    #[test]
    fn roundtrip_claude_agent() {
        let def = StartupAgentDefinition {
            name: "debugger".into(),
            display_name: "debugger".into(),
            description: "Bug diagnostic expert".into(),
            system_prompt: "Diagnose bugs thoroughly.".into(),
            model: Some("gpt-4o".into()),
            tools: vec!["run_command".into()],
            mode: "primary".into(),
            readonly: false,
            created_at: "".into(),
            updated_at: "".into(),
        };

        let projected = project_claude_agent(&def);
        let parsed = parse_claude_agent("debugger", &projected).unwrap();
        assert_eq!(parsed.name, "debugger");
        assert_eq!(parsed.description, def.description);
        assert_eq!(parsed.model, def.model);
        assert_eq!(parsed.tools, def.tools);
        assert_eq!(parsed.system_prompt, def.system_prompt);
    }

    #[test]
    fn roundtrip_vibe_agent() {
        let def = StartupAgentDefinition {
            name: "qa".into(),
            display_name: "qa".into(),
            description: "Quality assurance".into(),
            system_prompt: "Verify every detail.".into(),
            model: Some("mistral-large".into()),
            tools: vec!["test_runner".into()],
            mode: "primary".into(),
            readonly: false,
            created_at: "".into(),
            updated_at: "".into(),
        };

        let projected = project_vibe_agent(&def).unwrap();
        let parsed = parse_vibe_agent("qa", &projected).unwrap();
        assert_eq!(parsed.name, "qa");
        assert_eq!(parsed.description, def.description);
        assert_eq!(parsed.model, def.model);
        assert_eq!(parsed.tools, def.tools);
        assert_eq!(parsed.system_prompt, def.system_prompt);
    }
}
