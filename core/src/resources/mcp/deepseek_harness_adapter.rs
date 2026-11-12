//! User-level MCP client entries in the DeepSeek Harness profile patch.
use super::{adapter::Adapter, codec::Codec};
use crate::{
    domain::types::McpConfig,
    resources::deepseek_harness::{guard_mcp_profile, valid_mcp_server_name, PatchFile},
    safe_write::write_if_unchanged,
};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

pub struct DeepseekHarnessAdapter;

fn check_name(name: &str) -> Result<(), String> {
    if valid_mcp_server_name(name) {
        Ok(())
    } else {
        Err("DeepSeek Harness server name must match [A-Za-z0-9_-]{1,32}".into())
    }
}

impl DeepseekHarnessAdapter {
    fn load(path: &Path, names: &[&str]) -> Result<PatchFile, String> {
        guard_mcp_profile(path, names)?;
        PatchFile::read(path)
    }

    fn save(path: &Path, file: &PatchFile, names: &[&str]) -> Result<(), String> {
        let text = file.render()?;
        guard_mcp_profile(path, names)?;
        write_if_unchanged(path, file.original(), &text)
    }

    fn managed(value: &Value) -> bool {
        Codec::DeepseekHarness.validate_existing_entry(value).is_ok()
    }

    fn require(value: &Value) -> Result<(), String> {
        Codec::DeepseekHarness.validate_existing_entry(value)
    }
}

impl Adapter for DeepseekHarnessAdapter {
    fn declaration_names(&self, text: &str) -> Result<Vec<String>, String> {
        PatchFile::from_text(text)?.mcp_declaration_names()
    }

    fn inspect_text(&self, text: &str, name: &str) -> Result<Option<Value>, String> {
        PatchFile::from_text(text)?.inspect_mcp_declaration(name)
    }

    fn read(&self, path: &Path) -> BTreeMap<String, McpConfig> {
        self.read_with_enabled(path)
            .into_iter()
            .filter_map(|(name, (config, enabled))| enabled.then_some((name, config)))
            .collect()
    }

    fn read_with_enabled(&self, path: &Path) -> BTreeMap<String, (McpConfig, bool)> {
        let Ok(file) = Self::load(path, &[]) else {
            return BTreeMap::new();
        };
        let Ok(servers) = file.mcp_servers() else {
            return BTreeMap::new();
        };
        servers
            .into_iter()
            .filter_map(|(name, value)| {
                if !valid_mcp_server_name(&name) {
                    return None;
                }
                Codec::DeepseekHarness
                    .decode_with_enabled(&value)
                    .map(|config| (name, config))
            })
            .collect()
    }

    fn supports_native_enabled(&self) -> bool {
        true
    }

    fn set_enabled(
        &self,
        path: &Path,
        name: &str,
        enabled: bool,
        snapshot: &Value,
    ) -> Result<(), String> {
        check_name(name)?;
        let mut file = Self::load(path, &[name])?;
        let current = file
            .inspect_mcp(name)?
            .ok_or("DeepSeek Harness MCP is missing")?;
        Self::require(&current)?;
        if &current != snapshot {
            return Err("DeepSeek Harness MCP changed after observation".into());
        }
        let current_enabled = !current
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if current_enabled == enabled {
            return Ok(());
        }
        file.set_mcp_enabled(name, enabled)?;
        Self::save(path, &file, &[name])
    }

    fn upsert(&self, path: &Path, name: &str, config: &McpConfig) -> Result<(), String> {
        check_name(name)?;
        let patch = Codec::DeepseekHarness.patch(config)?;
        let mut file = Self::load(path, &[name])?;
        let current = file.inspect_mcp_declaration(name)?;
        if let Some(value) = &current {
            Self::require(value)?;
        }
        file.upsert_mcp(name, &patch.fields, patch.controlled, &patch.defaults)?;
        let final_entry = file
            .inspect_mcp(name)?
            .ok_or("DeepSeek Harness MCP construction failed")?;
        Self::require(&final_entry)?;
        Self::save(path, &file, &[name])
    }

    fn remove(&self, path: &Path, names: &[String]) -> Result<(), String> {
        let name_strs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut file = Self::load(path, &name_strs)?;
        for name in names {
            check_name(name)?;
            if let Some(entry) = file.inspect_mcp(name)? {
                Self::require(&entry)?;
            }
        }
        let mut changed = false;
        for name in names {
            if file.remove_mcp(name)? {
                changed = true;
            }
        }
        if changed {
            Self::save(path, &file, &name_strs)?;
        }
        Ok(())
    }

    fn clear(&self, path: &Path) -> Result<(), String> {
        let mut file = Self::load(path, &[])?;
        let servers = file.mcp_servers()?;
        let names = servers.iter().filter_map(|(name, value)| {
            (valid_mcp_server_name(name) && Self::managed(value)).then_some(name.as_str())
        }).collect::<Vec<_>>();
        if names.is_empty() {
            return Ok(());
        }
        guard_mcp_profile(path, &names)?;
        for name in &names {
            file.remove_mcp(name)?;
        }
        Self::save(path, &file, &names)
    }

    fn has_entries(&self, path: &Path) -> Result<bool, String> {
        let Ok(file) = Self::load(path, &[]) else {
            return Ok(false);
        };
        let Ok(servers) = file.mcp_servers() else {
            return Ok(false);
        };
        Ok(servers
            .iter()
            .any(|(name, value)| valid_mcp_server_name(name) && Self::managed(value)))
    }

    fn snapshot(&self, path: &Path, name: &str) -> Result<Option<Value>, String> {
        check_name(name)?;
        let value = Self::load(path, &[name])?.inspect_mcp(name)?;
        if let Some(v) = &value {
            Self::require(v)?;
        }
        Ok(value)
    }

    fn remove_snapshot(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String> {
        check_name(name)?;
        let mut file = Self::load(path, &[name])?;
        let current = file
            .inspect_mcp(name)?
            .ok_or("DeepSeek Harness MCP is missing")?;
        Self::require(&current)?;
        if &current != snapshot {
            return Err("DeepSeek Harness MCP changed after observation".into());
        }
        file.remove_mcp(name)?;
        Self::save(path, &file, &[name])
    }

    fn restore(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String> {
        check_name(name)?;
        Self::require(snapshot)?;
        let mut file = Self::load(path, &[name])?;
        if file.inspect_mcp(name)?.is_some() {
            return Err("DeepSeek Harness MCP was recreated before restore".into());
        }
        file.restore_mcp(name, snapshot)?;
        Self::save(path, &file, &[name])
    }
}
