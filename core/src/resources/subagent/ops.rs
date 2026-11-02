//! Central storage and lifecycle operations for Subagents.

use std::fs;
use std::path::PathBuf;
use crate::domain::subagent::SubagentDefinition;
use crate::paths::{central_subagent_file, subagent_assets_dir};
use crate::resources::mcp::scanner::expand_tilde;

use super::antigravity::{parse_antigravity_subagent, project_antigravity_subagent};
use super::claude::{parse_claude_subagent, project_claude_subagent};

/// Lists all centrally saved subagents.
pub fn list_central_subagents() -> Result<Vec<SubagentDefinition>, String> {
    let dir = subagent_assets_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(def) = serde_json::from_str::<SubagentDefinition>(&content) {
                    results.push(def);
                }
            }
        }
    }

    results.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(results)
}

/// Reads a single central subagent by name.
pub fn get_central_subagent(name: &str) -> Result<Option<SubagentDefinition>, String> {
    let file = central_subagent_file(name);
    if !file.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let def = serde_json::from_str::<SubagentDefinition>(&content)
        .map_err(|e| format!("Failed to parse subagent '{name}': {e}"))?;
    Ok(Some(def))
}

/// Saves or updates a central subagent definition.
pub fn save_central_subagent(mut definition: SubagentDefinition) -> Result<(), String> {
    definition
        .validate()
        .map_err(|e| format!("Invalid subagent: {:?}", e))?;

    let dir = subagent_assets_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().to_rfc3339();
    if definition.created_at.is_empty() {
        definition.created_at = now.clone();
    }
    definition.updated_at = now;

    let file = central_subagent_file(&definition.name);
    let json = serde_json::to_string_pretty(&definition).map_err(|e| e.to_string())?;
    fs::write(&file, json).map_err(|e| e.to_string())?;

    Ok(())
}

/// Deletes a central subagent definition.
pub fn delete_central_subagent(name: &str) -> Result<(), String> {
    let file = central_subagent_file(name);
    if file.exists() {
        fs::remove_file(&file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Determines the file path of a subagent projected to a specific Agent.
pub fn agent_subagent_target_file(
    agent_id: &str,
    subagent_name: &str,
) -> Result<Option<PathBuf>, String> {
    let agents = crate::agents::load_agents();
    let Some(agent) = agents.get(agent_id) else {
        return Ok(None);
    };
    let Some(subagents_cap) = &agent.subagents else {
        return Ok(None);
    };

    let dir = expand_tilde(&subagents_cap.global_dir);
    let ext = if subagents_cap.format == "markdown" {
        "md"
    } else {
        "json"
    };

    Ok(Some(dir.join(format!("{}.{}", subagent_name, ext))))
}

/// Projects a central subagent definition to an Agent's native config file.
pub fn project_subagent_to_agent(
    agent_id: &str,
    definition: &SubagentDefinition,
) -> Result<PathBuf, String> {
    let agents = crate::agents::load_agents();
    let agent = agents
        .get(agent_id)
        .ok_or_else(|| format!("Unknown agent: '{agent_id}'"))?;
    let subagents_cap = agent
        .subagents
        .as_ref()
        .ok_or_else(|| format!("Agent '{agent_id}' does not declare subagents capability"))?;

    let dir = expand_tilde(&subagents_cap.global_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let (content, target_path) = if subagents_cap.format == "markdown" {
        let content = project_claude_subagent(definition);
        let target = dir.join(format!("{}.md", definition.name));
        (content, target)
    } else {
        let content = project_antigravity_subagent(definition)?;
        let target = dir.join(format!("{}.json", definition.name));
        (content, target)
    };

    fs::write(&target_path, content).map_err(|e| e.to_string())?;
    Ok(target_path)
}

/// Removes a subagent's projected native config from an Agent.
pub fn remove_subagent_from_agent(agent_id: &str, subagent_name: &str) -> Result<(), String> {
    if let Some(target) = agent_subagent_target_file(agent_id, subagent_name)? {
        if target.exists() {
            fs::remove_file(&target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Parses an existing physical subagent file from an Agent's directory and saves it centrally.
pub fn adopt_subagent(agent_id: &str, subagent_name: &str) -> Result<SubagentDefinition, String> {
    let target = agent_subagent_target_file(agent_id, subagent_name)?
        .ok_or_else(|| format!("Agent '{agent_id}' has no subagents capability"))?;

    if !target.exists() {
        return Err(format!(
            "Target subagent file does not exist: {}",
            target.display()
        ));
    }

    let content = fs::read_to_string(&target).map_err(|e| e.to_string())?;
    let agents = crate::agents::load_agents();
    let agent = agents.get(agent_id).unwrap();
    let format = &agent.subagents.as_ref().unwrap().format;

    let def = if format == "markdown" {
        parse_claude_subagent(subagent_name, &content)?
    } else {
        parse_antigravity_subagent(subagent_name, &content)?
    };

    save_central_subagent(def.clone())?;
    Ok(def)
}
