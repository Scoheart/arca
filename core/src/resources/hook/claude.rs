//! Claude Code hooks codec for ~/.claude/settings.json.

use crate::domain::hook::HookDefinition;
use serde_json::{json, Value};

/// Projects a HookDefinition into a Claude Code settings.json document.
/// Preserves all other top-level keys in settings.json.
pub fn project_claude_hooks(
    existing_content: Option<&str>,
    definition: &HookDefinition,
) -> Result<String, String> {
    let mut root: Value = match existing_content {
        Some(content) if !content.trim().is_empty() => {
            serde_json::from_str(content).map_err(|e| format!("Invalid Claude settings JSON: {e}"))?
        }
        _ => json!({ "hooks": {} }),
    };

    if !root.is_object() {
        root = json!({ "hooks": {} });
    }

    let hooks_map = root
        .as_object_mut()
        .ok_or_else(|| "Root must be a JSON object".to_string())?
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "'hooks' field must be an object".to_string())?;

    let event_list = hooks_map
        .entry(&definition.event)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| format!("'hooks.{}' must be an array", definition.event))?;

    // Check if an existing hook has the same command
    let mut found = false;
    for group in event_list.iter_mut() {
        if let Some(inner_hooks) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
            for h in inner_hooks.iter_mut() {
                if h.get("command").and_then(|c| c.as_str()) == Some(&definition.command) {
                    if let Some(obj) = h.as_object_mut() {
                        obj.insert("timeout".into(), json!(definition.timeout));
                    }
                    if let Some(group_obj) = group.as_object_mut() {
                        group_obj.insert("matcher".into(), json!(definition.matcher));
                    }
                    found = true;
                    break;
                }
            }
        }
        if found {
            break;
        }
    }

    if !found {
        let inner_hook = json!({
            "type": definition.hook_type,
            "command": definition.command,
            "timeout": definition.timeout,
        });

        let new_group = json!({
            "matcher": definition.matcher,
            "hooks": [inner_hook]
        });
        event_list.push(new_group);
    }

    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

/// Removes a hook matching command or identifier from Claude settings.json.
pub fn remove_claude_hook(existing_content: &str, command_or_identifier: &str) -> Result<String, String> {
    let mut root: Value = serde_json::from_str(existing_content)
        .map_err(|e| format!("Invalid Claude settings JSON: {e}"))?;

    if let Some(hooks_map) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (_event, event_list) in hooks_map.iter_mut() {
            if let Some(arr) = event_list.as_array_mut() {
                arr.retain_mut(|group| {
                    if let Some(inner_hooks) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                        inner_hooks.retain(|h| {
                            let cmd = h.get("command").and_then(|c| c.as_str()).unwrap_or("");
                            !cmd.contains(command_or_identifier) && cmd != command_or_identifier
                        });
                        !inner_hooks.is_empty()
                    } else {
                        true
                    }
                });
            }
        }
    }

    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

/// Parses all hooks in Claude settings.json into HookDefinition models.
pub fn parse_claude_hooks(content: &str) -> Result<Vec<HookDefinition>, String> {
    let root: Value = serde_json::from_str(content)
        .map_err(|e| format!("Invalid Claude settings JSON: {e}"))?;

    let mut result = Vec::new();
    let Some(hooks_map) = root.get("hooks").and_then(|h| h.as_object()) else {
        return Ok(result);
    };

    let mut counter = 1;
    for (event, event_val) in hooks_map {
        let Some(groups) = event_val.as_array() else {
            continue;
        };

        for group in groups {
            let matcher = group.get("matcher").and_then(|m| m.as_str()).unwrap_or("").to_string();
            let Some(inner_hooks) = group.get("hooks").and_then(|h| h.as_array()) else {
                continue;
            };

            for h in inner_hooks {
                let hook_type = h.get("type").and_then(|t| t.as_str()).unwrap_or("command").to_string();
                let command = match h.get("command").and_then(|c| c.as_str()) {
                    Some(c) if !c.trim().is_empty() => c.to_string(),
                    _ => continue,
                };
                let timeout = h.get("timeout").and_then(|t| t.as_u64()).unwrap_or(10) as u32;

                let slug_base = if command.contains("paseo") {
                    format!("paseo-claude-{}", event.to_lowercase())
                } else {
                    format!("claude-{}-{}", event.to_lowercase(), counter)
                };
                counter += 1;

                let display_name = format!("Claude {} 钩子", event);
                let description = format!("原生 Claude Code {} 事件钩子", event);

                result.push(HookDefinition {
                    name: slug_base,
                    display_name,
                    description,
                    event: event.clone(),
                    matcher: matcher.clone(),
                    command,
                    command_windows: None,
                    hook_type,
                    timeout,
                    enabled: true,
                    created_at: String::new(),
                    updated_at: String::new(),
                });
            }
        }
    }

    Ok(result)
}
