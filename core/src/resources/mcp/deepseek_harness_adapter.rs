//! DeepSeek Harness's global configured MCP fleet, not plugin registrations.
use super::{adapter::Adapter, codec::Codec};
use crate::{domain::types::McpConfig, resources::deepseek_harness::{guard_profile, PatchFile}, safe_write::write_if_unchanged};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::Path};

const MANAGER: &str = "mcp-manager";
pub struct DeepseekHarnessAdapter;
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn check_name(name: &str) -> Result<(), String> {
    if valid_name(name) { Ok(()) } else { Err("DeepSeek Harness server name must match [A-Za-z0-9_-]{1,32}".into()) }
}
impl DeepseekHarnessAdapter {
    fn servers(file: &PatchFile) -> Result<Map<String, Value>, String> {
        match file.config(MANAGER)?.and_then(|v| v.get("servers").cloned()) {
            None => Ok(Map::new()), Some(Value::Object(map)) => Ok(map),
            _ => Err("DeepSeek Harness manager servers must be a mapping".into()),
        }
    }
    fn load(path: &Path) -> Result<PatchFile, String> { guard_profile(path, &[MANAGER])?; PatchFile::read(path) }
    fn save(path: &Path, file: &PatchFile) -> Result<(), String> {
        let text = file.render()?;
        guard_profile(path, &[MANAGER])?;
        write_if_unchanged(path, file.original(), &text)
    }
    fn managed(value: &Value) -> bool { Codec::DeepseekHarness.validate_existing_entry(value).is_ok() }
    fn require(value: &Value) -> Result<(), String> { Codec::DeepseekHarness.validate_existing_entry(value) }
}
impl Adapter for DeepseekHarnessAdapter {
    fn declaration_names(&self, text: &str) -> Result<Vec<String>, String> {
        Ok(Self::servers(&PatchFile::from_text(text)?)?.into_iter().map(|(name, _)| name).collect())
    }
    fn inspect_text(&self, text: &str, name: &str) -> Result<Option<Value>, String> {
        Ok(Self::servers(&PatchFile::from_text(text)?)?.remove(name))
    }
    fn read(&self, path: &Path) -> BTreeMap<String, McpConfig> {
        self.read_with_enabled(path).into_iter().filter_map(|(name,(config,enabled))| enabled.then_some((name,config))).collect()
    }
    fn read_with_enabled(&self, path: &Path) -> BTreeMap<String, (McpConfig, bool)> {
        let Ok(servers) = Self::load(path).and_then(|f| Self::servers(&f)) else { return BTreeMap::new(); };
        servers.into_iter().filter_map(|(name, value)| {
            if !valid_name(&name) { return None; }
            Codec::DeepseekHarness.decode_with_enabled(&value).map(|config| (name, config))
        }).collect()
    }
    fn supports_native_enabled(&self) -> bool { true }
    fn set_enabled(&self, path: &Path, name: &str, enabled: bool, snapshot: &Value) -> Result<(), String> {
        check_name(name)?;
        let mut file = Self::load(path)?;
        let current = Self::servers(&file)?.remove(name).ok_or("DeepSeek Harness MCP is missing")?;
        Self::require(&current)?;
        if &current != snapshot { return Err("DeepSeek Harness MCP changed after observation".into()); }
        if current.get("enabled").and_then(Value::as_bool).unwrap_or(true) == enabled { return Ok(()); }
        file.set_config_path(MANAGER, &["servers", name, "enabled"], &Value::Bool(enabled))?;
        Self::save(path, &file)
    }
    fn upsert(&self, path: &Path, name: &str, config: &McpConfig) -> Result<(), String> {
        check_name(name)?;
        let patch = Codec::DeepseekHarness.patch(config)?;
        let mut file = Self::load(path)?;
        let current = Self::servers(&file)?.remove(name);
        if let Some(value) = &current { Self::require(value)?; }
        for field in patch.controlled {
            if let Some((_, value)) = patch.fields.iter().find(|(key, _)| key == field) {
                file.set_config_path(MANAGER, &["servers", name, field], value)?;
            } else { file.remove_config_path(MANAGER, &["servers", name, field])?; }
        }
        for (field, value) in patch.defaults {
            if current.as_ref().and_then(|v| v.get(&field)).is_none() { file.set_config_path(MANAGER, &["servers", name, &field], &value)?; }
        }
        let final_entry = Self::servers(&file)?.remove(name).ok_or("DeepSeek Harness MCP construction failed")?;
        Self::require(&final_entry)?;
        Self::save(path, &file)
    }
    fn remove(&self, path: &Path, names: &[String]) -> Result<(), String> {
        let mut file = Self::load(path)?;
        let servers = Self::servers(&file)?;
        for name in names {
            check_name(name)?;
            if let Some(entry) = servers.get(name) { Self::require(entry)?; }
        }
        let mut changed = false;
        for name in names {
            if servers.contains_key(name) { file.remove_config_path(MANAGER, &["servers", name])?; changed = true; }
        }
        if changed { Self::save(path, &file)?; }
        Ok(())
    }
    fn clear(&self, path: &Path) -> Result<(), String> {
        let mut file = Self::load(path)?;
        let servers = Self::servers(&file)?;
        // Keep unknown declarations and the explicit empty map. Removing the
        // config override would reactivate inherited bundle entries.
        if file.config(MANAGER)?.and_then(|v| v.get("servers").cloned()).is_none() {
            file.set_config_field(MANAGER, "servers", &serde_json::json!({}))?;
        }
        for (name, value) in servers {
            if valid_name(&name) && Self::managed(&value) { file.remove_config_path(MANAGER, &["servers", &name])?; }
        }
        Self::save(path, &file)
    }
    fn has_entries(&self, path: &Path) -> Result<bool, String> {
        Ok(Self::servers(&Self::load(path)?)?.iter().any(|(name,value)| valid_name(name) && Self::managed(value)))
    }
    fn snapshot(&self, path: &Path, name: &str) -> Result<Option<Value>, String> {
        check_name(name)?;
        let value = Self::servers(&Self::load(path)?)?.remove(name);
        if let Some(v) = &value { Self::require(v)?; }
        Ok(value)
    }
    fn remove_snapshot(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String> {
        check_name(name)?;
        let mut file = Self::load(path)?;
        let current = Self::servers(&file)?.remove(name).ok_or("DeepSeek Harness MCP is missing")?;
        Self::require(&current)?;
        if &current != snapshot { return Err("DeepSeek Harness MCP changed after observation".into()); }
        file.remove_config_path(MANAGER, &["servers", name])?;
        Self::save(path, &file)
    }
    fn restore(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String> {
        check_name(name)?; Self::require(snapshot)?;
        let mut file = Self::load(path)?;
        if Self::servers(&file)?.contains_key(name) { return Err("DeepSeek Harness MCP was recreated before restore".into()); }
        file.set_config_path(MANAGER, &["servers", name], snapshot)?;
        Self::save(path, &file)
    }
}
