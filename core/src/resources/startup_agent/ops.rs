//! Central storage and lifecycle operations for Startup Agents.

use std::fs;
use std::path::PathBuf;
use crate::domain::startup_agent::{
    AgentStartupAgentItemView, AgentStartupAgentsStateView, StartupAgentDefinition,
    StartupAgentSource,
};
use crate::paths::{central_startup_agent_file, startup_agent_assets_dir};
use crate::resources::mcp::scanner::expand_tilde;

use super::codecs::{
    parse_claude_agent, parse_opencode_agent, parse_vibe_agent, project_claude_agent,
    project_opencode_agent, project_vibe_agent,
};
use super::config_editor::{read_native_startup_agent, write_native_startup_agent};

/// Lists all centrally saved startup agents.
pub fn list_central_startup_agents() -> Result<Vec<StartupAgentDefinition>, String> {
    let dir = startup_agent_assets_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(def) = serde_json::from_str::<StartupAgentDefinition>(&content) {
                    results.push(def);
                }
            }
        }
    }

    results.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(results)
}

/// Reads a single central startup agent by name.
pub fn get_central_startup_agent(name: &str) -> Result<Option<StartupAgentDefinition>, String> {
    let file = central_startup_agent_file(name);
    if !file.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let def = serde_json::from_str::<StartupAgentDefinition>(&content)
        .map_err(|e| format!("Failed to parse startup agent '{name}': {e}"))?;
    Ok(Some(def))
}

/// Saves or updates a central startup agent definition.
pub fn save_central_startup_agent(mut definition: StartupAgentDefinition) -> Result<(), String> {
    definition
        .validate()
        .map_err(|e| format!("Invalid startup agent: {:?}", e))?;

    let dir = startup_agent_assets_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().to_rfc3339();
    if definition.created_at.is_empty() {
        definition.created_at = now.clone();
    }
    definition.updated_at = now;

    let file = central_startup_agent_file(&definition.name);
    let json = serde_json::to_string_pretty(&definition).map_err(|e| e.to_string())?;
    fs::write(&file, json).map_err(|e| e.to_string())?;

    Ok(())
}

/// Deletes a central startup agent definition.
pub fn delete_central_startup_agent(name: &str) -> Result<(), String> {
    let file = central_startup_agent_file(name);
    if file.exists() {
        fs::remove_file(&file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Determines the file path of a startup agent projected to a specific Agent.
pub fn agent_startup_agent_target_file(
    agent_id: &str,
    startup_agent_name: &str,
) -> Result<Option<PathBuf>, String> {
    let agents = crate::agents::load_agents();
    let Some(agent) = agents.get(agent_id) else {
        return Ok(None);
    };
    let Some(startup_cap) = &agent.startup_agents else {
        return Ok(None);
    };
    let Some(global_dir) = &startup_cap.global_dir else {
        return Ok(None);
    };

    let dir = expand_tilde(global_dir);
    let ext = match startup_cap.file_format.as_deref() {
        Some("vibe-toml") => "toml",
        _ => "md",
    };

    Ok(Some(dir.join(format!("{}.{}", startup_agent_name, ext))))
}

/// Projects a central startup agent definition to an Agent's native config file.
pub fn project_startup_agent_to_agent(
    agent_id: &str,
    definition: &StartupAgentDefinition,
) -> Result<PathBuf, String> {
    let agents = crate::agents::load_agents();
    let agent = agents
        .get(agent_id)
        .ok_or_else(|| format!("Unknown agent: '{agent_id}'"))?;
    let startup_cap = agent
        .startup_agents
        .as_ref()
        .ok_or_else(|| format!("Agent '{agent_id}' does not declare startup_agents capability"))?;
    let global_dir = startup_cap
        .global_dir
        .as_ref()
        .ok_or_else(|| format!("Agent '{agent_id}' does not support custom startup agent directory"))?;

    let dir = expand_tilde(global_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let (content, target_path) = match startup_cap.file_format.as_deref() {
        Some("opencode-markdown") => {
            let content = project_opencode_agent(definition);
            let target = dir.join(format!("{}.md", definition.name));
            (content, target)
        }
        Some("vibe-toml") => {
            let content = project_vibe_agent(definition)?;
            let target = dir.join(format!("{}.toml", definition.name));
            (content, target)
        }
        _ => {
            let content = project_claude_agent(definition);
            let target = dir.join(format!("{}.md", definition.name));
            (content, target)
        }
    };

    fs::write(&target_path, content).map_err(|e| e.to_string())?;
    Ok(target_path)
}

/// Removes a startup agent's projected native file from an Agent.
pub fn remove_startup_agent_from_agent(agent_id: &str, startup_agent_name: &str) -> Result<(), String> {
    if let Some(target) = agent_startup_agent_target_file(agent_id, startup_agent_name)? {
        if target.exists() {
            fs::remove_file(&target).map_err(|e| e.to_string())?;
        }
    }

    // If removing the currently active startup agent, reset it to default
    let state = get_agent_startup_agents_state(agent_id)?;
    if state.active_agent.as_deref() == Some(startup_agent_name) {
        set_active_startup_agent(agent_id, None)?;
    }

    Ok(())
}

/// Parses an existing physical startup agent file from an Agent's directory and saves it centrally.
pub fn adopt_startup_agent(agent_id: &str, startup_agent_name: &str) -> Result<StartupAgentDefinition, String> {
    let target = agent_startup_agent_target_file(agent_id, startup_agent_name)?
        .ok_or_else(|| format!("Agent '{agent_id}' has no startup_agents directory"))?;

    if !target.exists() {
        return Err(format!(
            "Startup agent file '{}' does not exist at {}",
            startup_agent_name,
            target.display()
        ));
    }

    let agents = crate::agents::load_agents();
    let agent = agents
        .get(agent_id)
        .ok_or_else(|| format!("Unknown agent: '{agent_id}'"))?;
    let startup_cap = agent
        .startup_agents
        .as_ref()
        .ok_or_else(|| format!("Agent '{agent_id}' does not declare startup_agents capability"))?;

    let content = fs::read_to_string(&target).map_err(|e| e.to_string())?;
    let definition = match startup_cap.file_format.as_deref() {
        Some("opencode-markdown") => parse_opencode_agent(startup_agent_name, &content)?,
        Some("vibe-toml") => parse_vibe_agent(startup_agent_name, &content)?,
        _ => parse_claude_agent(startup_agent_name, &content)?,
    };

    save_central_startup_agent(definition.clone())?;
    Ok(definition)
}

/// Retrieves the complete startup agents view for an Agent.
pub fn get_agent_startup_agents_state(agent_id: &str) -> Result<AgentStartupAgentsStateView, String> {
    let agents = crate::agents::load_agents();
    let Some(agent) = agents.get(agent_id) else {
        return Ok(AgentStartupAgentsStateView {
            agent_id: agent_id.to_string(),
            active_agent: None,
            default_agent: None,
            supports_config_persistence: false,
            cli_flag: None,
            items: Vec::new(),
        });
    };

    let Some(cap) = &agent.startup_agents else {
        return Ok(AgentStartupAgentsStateView {
            agent_id: agent_id.to_string(),
            active_agent: None,
            default_agent: None,
            supports_config_persistence: false,
            cli_flag: None,
            items: Vec::new(),
        });
    };

    let default_agent = cap
        .builtins
        .iter()
        .find(|b| b.is_default)
        .map(|b| b.id.clone())
        .or_else(|| cap.builtins.first().map(|b| b.id.clone()));

    // 1. Determine active agent: Check native config file first if available
    let mut active_agent = None;
    if let (Some(config_file), Some(format), Some(key)) = (
        &cap.config_file,
        &cap.config_format,
        &cap.config_key,
    ) {
        let path = expand_tilde(config_file);
        if let Ok(Some(name)) = read_native_startup_agent(&path, format, key) {
            active_agent = Some(name);
        }
    }

    // 2. If not found in native config, check Arca launch preferences
    if active_agent.is_none() {
        let settings = crate::settings::load_settings();
        if let Some(launch_prefs) = settings.agent_launch.as_ref().and_then(|m| m.get(agent_id)) {
            if let Some(name) = &launch_prefs.startup_agent {
                if !name.trim().is_empty() {
                    active_agent = Some(name.trim().to_string());
                }
            }
        }
    }

    // 3. If still None and there is a default agent, default agent is implicitly active
    let effective_active = active_agent.clone().or_else(|| default_agent.clone());

    let mut items = Vec::new();

    // 4. Add built-in agents
    for builtin in &cap.builtins {
        let is_active = effective_active.as_deref() == Some(&builtin.id);
        items.push(AgentStartupAgentItemView {
            id: builtin.id.clone(),
            name: builtin.name.clone(),
            description: builtin.description.clone(),
            model: None,
            is_active,
            is_default: builtin.is_default,
            source: StartupAgentSource::Builtin,
            config_path: None,
        });
    }

    // 5. Scan custom agents in global_dir
    if let Some(global_dir) = &cap.global_dir {
        let dir = expand_tilde(global_dir);
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or_default();
                    if !path.is_file() || !matches!(ext, "md" | "toml") {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    // Skip if collision with builtin id
                    if cap.builtins.iter().any(|b| b.id == stem) {
                        continue;
                    }

                    let content = fs::read_to_string(&path).unwrap_or_default();
                    let (display_name, description, model) = match cap.file_format.as_deref() {
                        Some("opencode-markdown") => {
                            if let Ok(d) = parse_opencode_agent(stem, &content) {
                                (d.display_name, d.description, d.model)
                            } else {
                                (stem.to_string(), String::new(), None)
                            }
                        }
                        Some("vibe-toml") => {
                            if let Ok(d) = parse_vibe_agent(stem, &content) {
                                (d.display_name, d.description, d.model)
                            } else {
                                (stem.to_string(), String::new(), None)
                            }
                        }
                        _ => {
                            if let Ok(d) = parse_claude_agent(stem, &content) {
                                (d.display_name, d.description, d.model)
                            } else {
                                (stem.to_string(), String::new(), None)
                            }
                        }
                    };

                    let is_central = central_startup_agent_file(stem).exists();
                    let source = if is_central {
                        StartupAgentSource::Central
                    } else {
                        StartupAgentSource::External
                    };
                    let is_active = effective_active.as_deref() == Some(stem);

                    items.push(AgentStartupAgentItemView {
                        id: stem.to_string(),
                        name: display_name,
                        description,
                        model,
                        is_active,
                        is_default: false,
                        source,
                        config_path: Some(path.display().to_string()),
                    });
                }
            }
        }
    }

    Ok(AgentStartupAgentsStateView {
        agent_id: agent_id.to_string(),
        active_agent,
        default_agent,
        supports_config_persistence: cap.config_file.is_some(),
        cli_flag: cap.cli_flag.clone(),
        items,
    })
}

/// Sets the active startup agent for an Agent. If None, resets to tool default.
pub fn set_active_startup_agent(agent_id: &str, startup_agent_id: Option<String>) -> Result<(), String> {
    let agents = crate::agents::load_agents();
    let agent = agents
        .get(agent_id)
        .ok_or_else(|| format!("Unknown agent: '{agent_id}'"))?;

    let startup_cap = agent.startup_agents.as_ref();

    // 1. Write to native config if supported
    if let Some(cap) = startup_cap {
        if let (Some(config_file), Some(format), Some(key)) = (
            &cap.config_file,
            &cap.config_format,
            &cap.config_key,
        ) {
            let path = expand_tilde(config_file);
            write_native_startup_agent(&path, format, key, startup_agent_id.as_deref())?;
        }
    }

    // 2. Persist to Arca LaunchPreferences
    crate::settings::mutate_settings_checked(|settings| {
        settings
            .agent_launch
            .get_or_insert_default()
            .entry(agent_id.to_string())
            .or_default()
            .startup_agent = startup_agent_id.clone();
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    Ok(())
}
