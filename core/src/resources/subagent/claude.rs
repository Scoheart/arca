//! Claude Code subagent codec: Markdown with YAML frontmatter.

use crate::domain::subagent::SubagentDefinition;
use serde::{Deserialize, Serialize};

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

/// Serializes a SubagentDefinition to Claude Code format (Markdown with YAML frontmatter).
pub fn project_claude_subagent(definition: &SubagentDefinition) -> String {
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

/// Parses Claude Code Markdown content into a SubagentDefinition.
pub fn parse_claude_subagent(fallback_name: &str, content: &str) -> Result<SubagentDefinition, String> {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return Ok(SubagentDefinition {
            name: fallback_name.to_string(),
            display_name: fallback_name.to_string(),
            description: String::new(),
            system_prompt: trimmed.to_string(),
            model: None,
            tools: Vec::new(),
            mcp_dependencies: Vec::new(),
            readonly: false,
            allow_subagents: false,
            created_at: String::new(),
            updated_at: String::new(),
        });
    }

    let rest = &trimmed[3..];
    let Some(closing_idx) = rest.find("\n---") else {
        return Ok(SubagentDefinition {
            name: fallback_name.to_string(),
            display_name: fallback_name.to_string(),
            description: String::new(),
            system_prompt: trimmed.to_string(),
            model: None,
            tools: Vec::new(),
            mcp_dependencies: Vec::new(),
            readonly: false,
            allow_subagents: false,
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

    Ok(SubagentDefinition {
        name,
        display_name,
        description,
        system_prompt: body_str.to_string(),
        model: meta.model,
        tools: meta.tools,
        mcp_dependencies: Vec::new(),
        readonly: false,
        allow_subagents: false,
        created_at: String::new(),
        updated_at: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_claude_subagent() {
        let def = SubagentDefinition {
            name: "code-reviewer".into(),
            display_name: "code-reviewer".into(),
            description: "Expert code reviewer".into(),
            system_prompt: "You are a code reviewer.\nCheck for bugs.".into(),
            model: Some("sonnet".into()),
            tools: vec!["read_file".into(), "search_web".into()],
            mcp_dependencies: Vec::new(),
            readonly: false,
            allow_subagents: false,
            created_at: "".into(),
            updated_at: "".into(),
        };

        let projected = project_claude_subagent(&def);
        let parsed = parse_claude_subagent("code-reviewer", &projected).unwrap();

        assert_eq!(parsed.name, def.name);
        assert_eq!(parsed.description, def.description);
        assert_eq!(parsed.model, def.model);
        assert_eq!(parsed.tools, def.tools);
        assert_eq!(parsed.system_prompt, def.system_prompt);
    }
}
