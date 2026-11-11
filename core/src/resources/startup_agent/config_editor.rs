//! Lossless reader and writer for native Agent config files (JSON, JSONC, TOML).

use std::fs;
use std::path::Path;
use jsonc_parser::cst::{CstInputValue, CstObject, CstRootNode};
use jsonc_parser::ParseOptions;
use serde_json::Value;
use toml_edit::{Document, Item, Table};

fn read_jsonc(path: &Path) -> Result<(CstRootNode, Option<String>), String> {
    if !path.exists() {
        let root = CstRootNode::parse("{}", &ParseOptions::default())
            .map_err(|e| format!("Failed to initialize empty JSON: {e}"))?;
        return Ok((root, None));
    }
    let original = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let root = CstRootNode::parse(&original, &ParseOptions::default())
        .map_err(|e| format!("Invalid JSON/JSONC at {}: {e}", path.display()))?;
    Ok((root, Some(original)))
}

fn input_value(value: Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s),
        Value::Array(a) => CstInputValue::Array(a.into_iter().map(input_value).collect()),
        Value::Object(o) => {
            CstInputValue::Object(o.into_iter().map(|(k, v)| (k, input_value(v))).collect())
        }
    }
}

fn set_json(object: &CstObject, key: &str, value: Option<Value>) {
    match (object.get(key), value) {
        (Some(property), Some(val)) => property.set_value(input_value(val)),
        (None, Some(val)) => {
            object.append(key, input_value(val));
        }
        (Some(property), None) => property.remove(),
        (None, None) => {}
    }
}

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp_path = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    fs::write(&temp_path, content).map_err(|e| e.to_string())?;
    fs::rename(&temp_path, path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        e.to_string()
    })?;
    Ok(())
}

/// Reads the startup agent from a native config file.
pub fn read_native_startup_agent(path: &Path, format: &str, key: &str) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }

    match format {
        "json" | "jsonc" => {
            let (root, _) = read_jsonc(path)?;
            let Some(object) = root.object_value() else {
                return Ok(None);
            };
            let Some(prop) = object.get(key) else {
                return Ok(None);
            };
            let Some(val) = prop.value() else {
                return Ok(None);
            };
            if let Some(Value::String(s)) = val.to_serde_value() {
                if !s.trim().is_empty() {
                    return Ok(Some(s));
                }
            }
            Ok(None)
        }
        "toml" => {
            let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
            let doc: Document = content.parse().map_err(|e| format!("Invalid TOML at {}: {e}", path.display()))?;
            if key == "agent.name" {
                if let Some(name) = doc.get("agent").and_then(|a| a.get("name")).and_then(|n| n.as_str()) {
                    if !name.trim().is_empty() {
                        return Ok(Some(name.to_string()));
                    }
                }
            } else if let Some(val) = doc.get(key).and_then(|n| n.as_str()) {
                if !val.trim().is_empty() {
                    return Ok(Some(val.to_string()));
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// Updates or removes the startup agent in a native config file, preserving comments and unknown fields.
pub fn write_native_startup_agent(
    path: &Path,
    format: &str,
    key: &str,
    startup_agent: Option<&str>,
) -> Result<(), String> {
    match format {
        "json" | "jsonc" => {
            let (root, _) = read_jsonc(path)?;
            let object = root.object_value_or_create().ok_or_else(|| {
                format!("JSON root at {} is not an object", path.display())
            })?;
            match startup_agent {
                Some(name) if !name.trim().is_empty() => {
                    set_json(&object, key, Some(Value::String(name.trim().to_string())));
                }
                _ => {
                    set_json(&object, key, None);
                }
            }
            let new_content = root.to_string();
            atomic_write(path, &new_content)?;
            Ok(())
        }
        "toml" => {
            let content = if path.exists() {
                fs::read_to_string(path).map_err(|e| e.to_string())?
            } else {
                String::new()
            };
            let mut doc: Document = content.parse().map_err(|e| format!("Invalid TOML at {}: {e}", path.display()))?;

            if key == "agent.name" {
                match startup_agent {
                    Some(name) if !name.trim().is_empty() => {
                        if !doc.contains_key("agent") || !doc["agent"].is_table() {
                            doc["agent"] = Item::Table(Table::new());
                        }
                        doc["agent"]["name"] = toml_edit::value(name.trim());
                    }
                    _ => {
                        if let Some(agent_item) = doc.get_mut("agent") {
                            if let Some(agent_table) = agent_item.as_table_like_mut() {
                                agent_table.remove("name");
                            }
                        }
                    }
                }
            } else {
                match startup_agent {
                    Some(name) if !name.trim().is_empty() => {
                        doc[key] = toml_edit::value(name.trim());
                    }
                    _ => {
                        doc.as_table_mut().remove(key);
                    }
                }
            }

            let new_content = doc.to_string();
            atomic_write(path, &new_content)?;
            Ok(())
        }
        other => Err(format!("Unsupported config format for startup agent: '{other}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonc_lossless_default_agent() {
        let temp_dir = std::env::temp_dir().join(format!("arca-test-jsonc-{}", uuid::Uuid::new_v4()));
        let config_file = temp_dir.join("opencode.jsonc");
        fs::create_dir_all(&temp_dir).unwrap();

        let initial = r#"{
  // Main settings comment
  "mcp": {
    "git": { "command": "git" }
  },
  "default_agent": "build"
}
"#;
        fs::write(&config_file, initial).unwrap();

        // 1. Read
        let read = read_native_startup_agent(&config_file, "jsonc", "default_agent").unwrap();
        assert_eq!(read, Some("build".to_string()));

        // 2. Update to plan
        write_native_startup_agent(&config_file, "jsonc", "default_agent", Some("plan")).unwrap();
        let updated = read_native_startup_agent(&config_file, "jsonc", "default_agent").unwrap();
        assert_eq!(updated, Some("plan".to_string()));

        let text = fs::read_to_string(&config_file).unwrap();
        assert!(text.contains("// Main settings comment"), "Preserves comment");
        assert!(text.contains("\"git\""), "Preserves MCP settings");

        // 3. Reset (remove default_agent)
        write_native_startup_agent(&config_file, "jsonc", "default_agent", None).unwrap();
        let reset = read_native_startup_agent(&config_file, "jsonc", "default_agent").unwrap();
        assert_eq!(reset, None);

        let final_text = fs::read_to_string(&config_file).unwrap();
        assert!(final_text.contains("// Main settings comment"));
        assert!(!final_text.contains("default_agent"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_toml_lossless_grok_agent() {
        let temp_dir = std::env::temp_dir().join(format!("arca-test-toml-{}", uuid::Uuid::new_v4()));
        let config_file = temp_dir.join("config.toml");
        fs::create_dir_all(&temp_dir).unwrap();

        let initial = r#"# Grok config
[model]
name = "grok-beta"

[agent]
name = "grok-build"
"#;
        fs::write(&config_file, initial).unwrap();

        // 1. Read
        let read = read_native_startup_agent(&config_file, "toml", "agent.name").unwrap();
        assert_eq!(read, Some("grok-build".to_string()));

        // 2. Update to architect
        write_native_startup_agent(&config_file, "toml", "agent.name", Some("architect")).unwrap();
        let updated = read_native_startup_agent(&config_file, "toml", "agent.name").unwrap();
        assert_eq!(updated, Some("architect".to_string()));

        let text = fs::read_to_string(&config_file).unwrap();
        assert!(text.contains("# Grok config"));
        assert!(text.contains("grok-beta"));
        assert!(text.contains("architect"));

        // 3. Reset
        write_native_startup_agent(&config_file, "toml", "agent.name", None).unwrap();
        let reset = read_native_startup_agent(&config_file, "toml", "agent.name").unwrap();
        assert_eq!(reset, None);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
