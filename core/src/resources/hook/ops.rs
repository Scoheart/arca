//! Central storage and lifecycle operations for Hooks.

use std::fs;
use std::path::PathBuf;
use crate::domain::hook::HookDefinition;
use crate::paths::{central_hook_file, hook_assets_dir};
use crate::resources::mcp::scanner::expand_tilde;

use super::claude::{parse_claude_hooks, project_claude_hooks, remove_claude_hook};
use super::codex::{parse_codex_hooks, project_codex_hooks, remove_codex_hook};

/// Lists all centrally saved hook definitions.
pub fn list_central_hooks() -> Result<Vec<HookDefinition>, String> {
    let dir = hook_assets_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(def) = serde_json::from_str::<HookDefinition>(&content) {
                    results.push(def);
                }
            }
        }
    }

    results.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(results)
}

/// Reads a single central hook definition by name.
pub fn get_central_hook(name: &str) -> Result<Option<HookDefinition>, String> {
    let file = central_hook_file(name);
    if !file.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let def = serde_json::from_str::<HookDefinition>(&content)
        .map_err(|e| format!("Failed to parse hook '{name}': {e}"))?;
    Ok(Some(def))
}

/// Saves or updates a central hook definition.
pub fn save_central_hook(mut definition: HookDefinition) -> Result<(), String> {
    definition
        .validate()
        .map_err(|e| format!("Invalid hook definition: {:?}", e))?;

    let dir = hook_assets_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().to_rfc3339();
    if definition.created_at.is_empty() {
        definition.created_at = now.clone();
    }
    definition.updated_at = now;

    let file = central_hook_file(&definition.name);
    let json = serde_json::to_string_pretty(&definition).map_err(|e| e.to_string())?;
    fs::write(&file, json).map_err(|e| e.to_string())?;

    Ok(())
}

/// Deletes a central hook definition.
pub fn delete_central_hook(name: &str) -> Result<(), String> {
    let file = central_hook_file(name);
    if file.exists() {
        fs::remove_file(&file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Determines the configuration target file path for an Agent's hooks.
pub fn agent_hook_target_file(agent_id: &str) -> Result<Option<PathBuf>, String> {
    let agents = crate::agents::load_agents();
    let Some(agent) = agents.get(agent_id) else {
        return Ok(None);
    };
    let Some(hooks_cap) = &agent.hooks else {
        return Ok(None);
    };

    Ok(Some(expand_tilde(&hooks_cap.config_file)))
}

/// Lists all hooks currently configured on an Agent.
pub fn list_agent_hooks(agent_id: &str) -> Result<Vec<HookDefinition>, String> {
    let agents = crate::agents::load_agents();
    let Some(agent) = agents.get(agent_id) else {
        return Ok(Vec::new());
    };
    let Some(hooks_cap) = &agent.hooks else {
        return Ok(Vec::new());
    };

    let target = expand_tilde(&hooks_cap.config_file);
    if !target.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&target).map_err(|e| e.to_string())?;
    match hooks_cap.format.as_str() {
        "codex" => parse_codex_hooks(&content),
        "claude" => parse_claude_hooks(&content),
        other => Err(format!("Unsupported hooks format: '{other}'")),
    }
}

/// Projects a central hook definition to an Agent's native configuration.
pub fn project_hook_to_agent(
    agent_id: &str,
    definition: &HookDefinition,
) -> Result<PathBuf, String> {
    let agents = crate::agents::load_agents();
    let agent = agents
        .get(agent_id)
        .ok_or_else(|| format!("Unknown agent: '{agent_id}'"))?;
    let hooks_cap = agent
        .hooks
        .as_ref()
        .ok_or_else(|| format!("Agent '{agent_id}' does not declare hooks capability"))?;

    let target = expand_tilde(&hooks_cap.config_file);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let existing_content = if target.exists() {
        Some(fs::read_to_string(&target).map_err(|e| e.to_string())?)
    } else {
        None
    };

    let new_content = match hooks_cap.format.as_str() {
        "codex" => project_codex_hooks(existing_content.as_deref(), definition)?,
        "claude" => project_claude_hooks(existing_content.as_deref(), definition)?,
        other => return Err(format!("Unsupported hooks format: '{other}'")),
    };

    fs::write(&target, new_content).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Removes a hook from an Agent's native configuration.
pub fn remove_hook_from_agent(agent_id: &str, hook_name_or_command: &str) -> Result<(), String> {
    let agents = crate::agents::load_agents();
    let agent = agents
        .get(agent_id)
        .ok_or_else(|| format!("Unknown agent: '{agent_id}'"))?;
    let hooks_cap = agent
        .hooks
        .as_ref()
        .ok_or_else(|| format!("Agent '{agent_id}' does not declare hooks capability"))?;

    let target = expand_tilde(&hooks_cap.config_file);
    if !target.exists() {
        return Ok(());
    }

    // Resolve command if this is a central hook name
    let search_key = if let Ok(Some(def)) = get_central_hook(hook_name_or_command) {
        def.command
    } else {
        hook_name_or_command.to_string()
    };

    let content = fs::read_to_string(&target).map_err(|e| e.to_string())?;
    let new_content = match hooks_cap.format.as_str() {
        "codex" => remove_codex_hook(&content, &search_key)?,
        "claude" => remove_claude_hook(&content, &search_key)?,
        other => return Err(format!("Unsupported hooks format: '{other}'")),
    };

    fs::write(&target, new_content).map_err(|e| e.to_string())?;
    Ok(())
}

/// Adopts an existing native hook from an Agent into central Arca storage.
pub fn adopt_hook(agent_id: &str, hook_name: &str) -> Result<HookDefinition, String> {
    let hooks = list_agent_hooks(agent_id)?;
    let hook = hooks
        .into_iter()
        .find(|h| h.name == hook_name || h.command == hook_name)
        .ok_or_else(|| format!("Hook '{hook_name}' not found on agent '{agent_id}'"))?;

    save_central_hook(hook.clone())?;
    Ok(hook)
}
