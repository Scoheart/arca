//! JSON and JSONC MCP Agent adapter.

use crate::domain::types::McpConfig;
use crate::resources::mcp::adapter::Adapter;
use crate::resources::mcp::codec::{Codec, ObjectPatch};
use crate::safe_write::{write_if_unchanged, write_if_unchanged_with_settings_lock};
use jsonc_parser::cst::{CstInputValue, CstNode, CstObject, CstRootNode};
use jsonc_parser::ParseOptions;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

pub struct JsonAdapter {
    pub key: String,
    key_path: bool,
    codec: Codec,
    root_defaults: BTreeMap<String, Value>,
}

fn valid_pi_server_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

impl JsonAdapter {
    pub fn new(key: &str) -> Self {
        Self::with_codec(key, Codec::Standard)
    }

    pub fn with_codec(key: &str, codec: Codec) -> Self {
        Self::with_spec(key, codec, BTreeMap::new())
    }

    pub fn with_spec(key: &str, codec: Codec, root_defaults: BTreeMap<String, Value>) -> Self {
        Self::with_spec_and_key_path(key, codec, root_defaults, false)
    }

    pub fn with_spec_and_key_path(
        key: &str,
        codec: Codec,
        root_defaults: BTreeMap<String, Value>,
        key_path: bool,
    ) -> Self {
        Self {
            key: key.to_string(),
            key_path,
            codec,
            root_defaults,
        }
    }

    fn read_document(&self, path: &Path) -> Result<(CstRootNode, Option<String>), String> {
        let text = match fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            Err(e) => return Err(format!("failed to read {}: {}", path.display(), e)),
        };
        let root = CstRootNode::parse(
            text.as_deref().unwrap_or_default(),
            &ParseOptions::default(),
        )
        .map_err(|e| {
            format!(
                "refusing to modify invalid JSON/JSONC at {}: {}",
                path.display(),
                e
            )
        })?;
        if self.codec == Codec::Jcode && self.key == "servers"
            && root.object_value().is_some_and(|object| object.get("mcpServers").is_some()) {
            return Err("jcode MCP uses a different map key in this file; set the actual key in Arca before writing".into());
        }
        Ok((root, text))
    }

    fn write_document(
        &self,
        path: &Path,
        root: &CstRootNode,
        original: Option<&str>,
    ) -> Result<(), String> {
        let content = root.to_string();
        if self.codec == Codec::ZCode {
            let has_native = |value: Option<Value>| value
                .and_then(|root| root.get("mcp")?.get("servers")?.as_object().cloned())
                .is_some_and(|servers| !servers.is_empty());
            let before = original.and_then(|text| CstRootNode::parse(text, &ParseOptions::default()).ok())
                .and_then(|root| root.to_serde_value());
            if has_native(before) != has_native(root.to_serde_value()) {
                let fallback = crate::resources::mcp::scanner::expand_tilde("~/.agents/mcp.json");
                let (fallback_root, _) = self.read_document(&fallback)?;
                let has_fallback = fallback_root.to_serde_value()
                    .and_then(|root| root.get("mcpServers")?.as_object().cloned())
                    .is_some_and(|servers| !servers.is_empty());
                if has_fallback {
                    return Err("zcode_mcp_fallback_conflict: ZCode 的原生 MCP 配置会切换 ~/.agents/mcp.json 的生效状态。请先在 ZCode 中将共享服务导入原生配置，再应用此操作。".into());
                }
            }
        }
        if self.codec == Codec::Cline {
            write_if_unchanged_with_settings_lock(path, original, &content)
        } else {
            write_if_unchanged(path, original, &content)
        }
    }

    fn input_value(value: Value) -> CstInputValue {
        match value {
            Value::Null => CstInputValue::Null,
            Value::Bool(value) => CstInputValue::Bool(value),
            Value::Number(value) => CstInputValue::Number(value.to_string()),
            Value::String(value) => CstInputValue::String(value),
            Value::Array(values) => {
                CstInputValue::Array(values.into_iter().map(Self::input_value).collect())
            }
            Value::Object(values) => CstInputValue::Object(
                values
                    .into_iter()
                    .map(|(name, value)| (name, Self::input_value(value)))
                    .collect(),
            ),
        }
    }

    fn property_count(object: &CstObject, name: &str) -> usize {
        object
            .properties()
            .into_iter()
            .filter(|property| {
                property
                    .name()
                    .and_then(|name| name.decoded_value().ok())
                    .is_some_and(|decoded| decoded == name)
            })
            .count()
    }

    fn ensure_unique_property(
        object: &CstObject,
        name: &str,
        path: &Path,
        context: &str,
    ) -> Result<(), String> {
        if Self::property_count(object, name) > 1 {
            return Err(format!(
                "refusing to modify {}: duplicate JSON key '{}.{}' is ambiguous",
                path.display(),
                context,
                name
            ));
        }
        Ok(())
    }

    fn ensure_unique_keys(object: &CstObject, path: &Path, context: &str) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for property in object.properties() {
            let Some(name) = property.name().and_then(|name| name.decoded_value().ok()) else {
                continue;
            };
            if !seen.insert(name.clone()) {
                return Err(format!(
                    "refusing to modify {}: duplicate JSON key '{}.{}' is ambiguous",
                    path.display(),
                    context,
                    name
                ));
            }
        }
        Ok(())
    }

    fn ensure_unique_nested_keys(node: &CstNode, path: &Path, context: &str) -> Result<(), String> {
        if let Some(object) = node.as_object() {
            Self::ensure_unique_keys(&object, path, context)?;
            for property in object.properties() {
                let name = property
                    .name()
                    .and_then(|name| name.decoded_value().ok())
                    .unwrap_or_else(|| "<unknown>".into());
                if let Some(value) = property.value() {
                    Self::ensure_unique_nested_keys(&value, path, &format!("{context}.{name}"))?;
                }
            }
        } else if let Some(array) = node.as_array() {
            for (index, element) in array.elements().into_iter().enumerate() {
                Self::ensure_unique_nested_keys(&element, path, &format!("{context}[{index}]"))?;
            }
        }
        Ok(())
    }

    fn section_object(
        &self,
        root: CstObject,
        path: &Path,
        create: bool,
    ) -> Result<Option<CstObject>, String> {
        // Junie's current parser selects mcpServers when present, otherwise
        // the root map. Keep an existing direct map in place: introducing a
        // wrapper beside its entries would hide them from the native loader.
        if self.codec == Codec::JunieCli && !self.key_path && self.key == "mcpServers"
            && root.get("mcpServers").is_none()
            && (!create || !root.properties().is_empty()) {
            return Ok(Some(root));
        }
        let mut current = root;
        let mut context = "$root".to_string();
        let components = if self.key_path {
            self.key.split('.').collect::<Vec<_>>()
        } else {
            vec![self.key.as_str()]
        };
        for name in components {
            if name.is_empty() {
                return Err(format!(
                    "refusing to modify {}: JSON key path '{}' contains an empty component",
                    path.display(),
                    self.key
                ));
            }
            Self::ensure_unique_property(&current, name, path, &context)?;
            let next = if create {
                current.object_value_or_create(name).ok_or_else(|| {
                    format!(
                        "refusing to modify {}: '{}.{}' is not an object",
                        path.display(),
                        context,
                        name
                    )
                })?
            } else {
                let Some(property) = current.get(name) else {
                    return Ok(None);
                };
                property.object_value().ok_or_else(|| {
                    format!(
                        "refusing to read {}: '{}.{}' is not an object",
                        path.display(),
                        context,
                        name
                    )
                })?
            };
            context.push('.');
            context.push_str(name);
            current = next;
        }
        Ok(Some(current))
    }

    fn patch_nested_object(
        target: &CstObject,
        patch: ObjectPatch,
        path: &Path,
        context: &str,
    ) -> Result<(), String> {
        Self::ensure_unique_property(target, patch.parent, path, context)?;
        if target.get(patch.parent).is_none() && patch.fields.is_empty() {
            return Ok(());
        }
        let nested = target.object_value_or_create(patch.parent).ok_or_else(|| {
            format!(
                "refusing to modify {}: '{}.{}' is not an object",
                path.display(),
                context,
                patch.parent
            )
        })?;
        Self::ensure_unique_keys(&nested, path, &format!("{}.{}", context, patch.parent))?;
        for field in patch.controlled {
            if let Some((_, value)) = patch.fields.iter().find(|(name, _)| name == field) {
                if let Some(property) = nested.get(field) {
                    property.set_value(Self::input_value(value.clone()));
                } else {
                    nested.append(field, Self::input_value(value.clone()));
                }
            } else if let Some(property) = nested.get(field) {
                property.remove();
            }
        }
        Ok(())
    }

    fn new_entry_value(mut patch: crate::resources::mcp::codec::EntryPatch) -> Value {
        patch.fields.extend(patch.defaults);
        let mut fields: serde_json::Map<String, Value> = patch.fields.into_iter().collect();
        for nested in patch.object_patches {
            if !nested.fields.is_empty() {
                fields.insert(
                    nested.parent.into(),
                    Value::Object(nested.fields.into_iter().collect()),
                );
            }
        }
        Value::Object(fields)
    }

    fn validate_junie_section_schema(&self, section: &CstObject) -> Result<(), String> {
        for property in section.properties() {
            let value = property.to_serde_value().ok_or("Junie CLI MCP entry has no valid JSON value")?;
            self.codec.validate_existing_entry(&value)?;
        }
        Ok(())
    }

    /// Reject incompatible shared formats before any mutation. ChatMCP stores
    /// OAuth material beside MCP connection metadata. A normal
    /// whole-file backup would copy those external credentials into Arca's
    /// backup area, so any credential-bearing entry makes the entire file
    /// read-only. The error deliberately reports only policy, never values.
    fn validate_section_mutation_policy(
        &self,
        section: &CstObject,
        path: &Path,
    ) -> Result<(), String> {
        if !matches!(self.codec, Codec::ChatMcp | Codec::IbmBob | Codec::JunieCli) {
            return Ok(());
        }
        if self.codec == Codec::JunieCli { self.validate_junie_section_schema(section)?; }
        for property in section.properties() {
            let value = property.to_serde_value().ok_or_else(|| {
                format!(
                    "refusing to modify {}: MCP entry has no valid JSON value",
                    path.display()
                )
            })?;
            if self.codec == Codec::IbmBob && value.get("httpURL").is_none() {
                continue;
            }
            // Native McpOAuthConfig includes a literal clientSecret. Do not
            // duplicate such external credentials into whole-file backups.
            if self.codec == Codec::JunieCli && value.pointer("/oauth/clientSecret")
                .is_some_and(|secret| !secret.is_null() && secret.as_str().is_none_or(|value| !value.is_empty())) {
                return Err("Junie CLI credential-bearing OAuth configuration must be managed in Junie".into());
            }
            self.codec
                .validate_existing_entry(&value)
                .map_err(|reason| format!("refusing to modify {}: {reason}", path.display()))?;
        }
        Ok(())
    }
}

impl Adapter for JsonAdapter {
    fn read(&self, path: &Path) -> BTreeMap<String, McpConfig> {
        self.read_with_enabled(path).into_iter()
            .filter_map(|(name, (config, enabled))| enabled.then_some((name, config)))
            .collect()
    }

    fn read_with_enabled(&self, path: &Path) -> BTreeMap<String, (McpConfig, bool)> {
        let Ok((root, _)) = self.read_document(path) else {
            return BTreeMap::new();
        };
        let Some(object) = root.object_value() else {
            return BTreeMap::new();
        };
        let Ok(Some(section)) = self.section_object(object, path, false) else {
            return BTreeMap::new();
        };
        // McpJsonParser decodes the whole map before publishing any entries.
        if self.codec == Codec::JunieCli && self.validate_junie_section_schema(&section).is_err() {
            return BTreeMap::new();
        }
        section
            .properties()
            .into_iter()
            .filter_map(|property| {
                let name = property.name()?.decoded_value().ok()?;
                if self.codec == Codec::Pi && !valid_pi_server_name(&name) {
                    return None;
                }
                property
                    .to_serde_value()
                    .and_then(|value| self.codec.decode_with_enabled(&value))
                    .map(|cfg| (name, cfg))
            })
            .collect()
    }

    fn supports_native_enabled(&self) -> bool {
        matches!(self.codec, Codec::WorkBuddy | Codec::Pi | Codec::Jan | Codec::JunieCli)
    }

    fn set_enabled(&self, path: &Path, name: &str, enabled: bool, snapshot: &Value) -> Result<(), String> {
        if !self.supports_native_enabled() {
            return Err("Agent does not support a native MCP enabled flag".into());
        }
        let (root, original) = self.read_document(path)?;
        let object = root.object_value().ok_or("MCP root is not an object")?;
        let section = self.section_object(object, path, false)?.ok_or("MCP section is missing")?;
        Self::ensure_unique_keys(&section, path, &self.key)?;
        self.validate_section_mutation_policy(&section, path)?;
        let property = section.get(name).ok_or("MCP entry is missing")?;
        let value = property.value().ok_or("MCP entry has no value")?;
        Self::ensure_unique_nested_keys(&value, path, &format!("{}.{}", self.key, name))?;
        let current = property.to_serde_value().ok_or("MCP entry is invalid")?;
        if &current != snapshot {
            return Err("MCP entry changed while Arca was preparing its enabled state".into());
        }
        let (_, current_enabled) = self.codec.decode_with_enabled(&current).ok_or("invalid native MCP entry")?;
        if current_enabled == enabled { return Ok(()); }
        let target = property.object_value().ok_or("MCP entry is not an object")?;
        let (field, value) = match self.codec {
            Codec::Pi | Codec::JunieCli => ("enabled", enabled),
            Codec::Jan => ("active", enabled),
            _ => ("disabled", !enabled),
        };
        if let Some(flag) = target.get(field) {
            flag.set_value(CstInputValue::Bool(value));
        } else {
            target.append(field, CstInputValue::Bool(value));
        }
        self.write_document(path, &root, original.as_deref())
    }

    fn upsert(&self, path: &Path, name: &str, cfg: &McpConfig) -> Result<(), String> {
        if self.codec == Codec::Pi && !valid_pi_server_name(name) {
            return Err("Pi MCP server names may contain only letters, digits, underscores, and hyphens".into());
        }
        let (root, original) = self.read_document(path)?;
        let object = root.object_value_or_create().ok_or_else(|| {
            format!(
                "refusing to modify {}: JSON root is not an object",
                path.display()
            )
        })?;
        if self.codec == Codec::JunieCli && !self.key_path && self.key == "mcpServers"
            && name == "mcpServers" && object.get("mcpServers").is_none()
            && !object.properties().is_empty() {
            return Err("Junie CLI direct MCP map cannot use the reserved wrapper name 'mcpServers'".into());
        }
        if original.is_none() {
            for (field, value) in &self.root_defaults {
                if object.get(field).is_none() {
                    object.append(field, Self::input_value(value.clone()));
                }
            }
        }
        let section = self
            .section_object(object, path, true)?
            .expect("create=true always returns a section");
        Self::ensure_unique_keys(&section, path, &self.key)?;
        self.validate_section_mutation_policy(&section, path)?;
        let patch = self.codec.patch(cfg)?;
        if let Some(property) = section.get(name) {
            let current = property.to_serde_value().ok_or_else(|| {
                format!(
                    "refusing to modify {}: '{}.{}' has no valid JSON value",
                    path.display(),
                    self.key,
                    name
                )
            })?;
            self.codec.validate_update(&current, cfg)?;
            if let Some(value) = property.value() {
                Self::ensure_unique_nested_keys(&value, path, &format!("{}.{}", self.key, name))?;
            }
            let target = property.object_value().ok_or_else(|| {
                format!(
                    "refusing to modify {}: '{}.{}' is not an object",
                    path.display(),
                    self.key,
                    name
                )
            })?;
            for field in patch.controlled {
                Self::ensure_unique_property(
                    &target,
                    field,
                    path,
                    &format!("{}.{}", self.key, name),
                )?;
            }
            for field in patch.controlled {
                if let Some((_, value)) = patch.fields.iter().find(|(name, _)| name == field) {
                    if let Some(property) = target.get(field) {
                        property.set_value(Self::input_value(value.clone()));
                    } else {
                        target.append(field, Self::input_value(value.clone()));
                    }
                } else if let Some(property) = target.get(field) {
                    property.remove();
                }
            }
            for nested in patch.object_patches {
                Self::patch_nested_object(
                    &target,
                    nested,
                    path,
                    &format!("{}.{}", self.key, name),
                )?;
            }
            for (field, value) in patch.defaults {
                if target.get(&field).is_none() {
                    target.append(&field, Self::input_value(value));
                }
            }
        } else {
            section.append(name, Self::input_value(Self::new_entry_value(patch)));
        }
        self.write_document(path, &root, original.as_deref())
    }

    fn remove(&self, path: &Path, names: &[String]) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let (root, original) = self.read_document(path)?;
        let object = root.object_value().ok_or_else(|| {
            format!(
                "refusing to modify {}: JSON root is not an object",
                path.display()
            )
        })?;
        let Some(section) = self.section_object(object, path, false)? else {
            return Ok(());
        };
        Self::ensure_unique_keys(&section, path, &self.key)?;
        self.validate_section_mutation_policy(&section, path)?;
        let mut changed = false;
        for name in names {
            if let Some(property) = section.get(name) {
                property.remove();
                changed = true;
            }
        }
        if changed {
            self.write_document(path, &root, original.as_deref())?;
        }
        Ok(())
    }

    fn clear(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let (root, original) = self.read_document(path)?;
        let object = root.object_value().ok_or_else(|| {
            format!(
                "refusing to modify {}: JSON root is not an object",
                path.display()
            )
        })?;
        let Some(section) = self.section_object(object, path, false)? else {
            return Ok(());
        };
        self.validate_section_mutation_policy(&section, path)?;
        let properties = section.properties();
        if properties.is_empty() {
            return Ok(());
        }
        for property in properties {
            property.remove();
        }
        self.write_document(path, &root, original.as_deref())
    }

    fn has_entries(&self, path: &Path) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }
        let (root, _) = self.read_document(path)?;
        let object = root.object_value().ok_or_else(|| {
            format!(
                "refusing to read {}: JSON root is not an object",
                path.display()
            )
        })?;
        Ok(self
            .section_object(object, path, false)?
            .is_some_and(|section| !section.properties().is_empty()))
    }

    fn snapshot(&self, path: &Path, name: &str) -> Result<Option<Value>, String> {
        let (root, original) = self.read_document(path)?;
        if original
            .as_deref()
            .is_none_or(|content| content.trim().is_empty())
        {
            return Ok(None);
        }
        let object = root.object_value().ok_or_else(|| {
            format!(
                "refusing to read {}: JSON root is not an object",
                path.display()
            )
        })?;
        let Some(section) = self.section_object(object, path, false)? else {
            return Ok(None);
        };
        Self::ensure_unique_keys(&section, path, &self.key)?;
        self.validate_section_mutation_policy(&section, path)?;
        let Some(property) = section.get(name) else {
            return Ok(None);
        };
        let node = property.value().ok_or_else(|| {
            format!(
                "refusing to read {}: '{}.{}' has no JSON value",
                path.display(),
                self.key,
                name
            )
        })?;
        Self::ensure_unique_nested_keys(&node, path, &format!("{}.{}", self.key, name))?;
        let value = property.to_serde_value().ok_or_else(|| {
            format!(
                "refusing to read {}: '{}.{}' has no valid JSON value",
                path.display(),
                self.key,
                name
            )
        })?;
        if !value.is_object() {
            return Err(format!(
                "refusing to read {}: '{}.{}' is not an object",
                path.display(),
                self.key,
                name
            ));
        }
        self.codec.validate_existing_entry(&value)?;
        Ok(Some(value))
    }

    fn remove_snapshot(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String> {
        let (root, original) = self.read_document(path)?;
        let object = root.object_value().ok_or_else(|| {
            format!(
                "refusing to modify {}: JSON root is not an object",
                path.display()
            )
        })?;
        let section = self.section_object(object, path, false)?.ok_or_else(|| {
            format!(
                "refusing to modify {}: '{}' is missing",
                path.display(),
                self.key
            )
        })?;
        Self::ensure_unique_keys(&section, path, &self.key)?;
        self.validate_section_mutation_policy(&section, path)?;
        let property = section.get(name).ok_or_else(|| {
            format!(
                "refusing to remove {}: '{}.{}' no longer exists",
                path.display(),
                self.key,
                name
            )
        })?;
        let node = property.value().ok_or_else(|| {
            format!(
                "refusing to remove {}: '{}.{}' has no JSON value",
                path.display(),
                self.key,
                name
            )
        })?;
        Self::ensure_unique_nested_keys(&node, path, &format!("{}.{}", self.key, name))?;
        let current = property.to_serde_value().ok_or_else(|| {
            format!(
                "refusing to remove {}: '{}.{}' has no valid JSON value",
                path.display(),
                self.key,
                name
            )
        })?;
        if &current != snapshot {
            return Err(format!(
                "refusing to remove {}: '{}.{}' changed after its snapshot was saved",
                path.display(),
                self.key,
                name
            ));
        }
        property.remove();
        self.write_document(path, &root, original.as_deref())
    }

    fn restore(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String> {
        if !snapshot.is_object() {
            return Err("refusing to restore a non-object MCP snapshot".into());
        }
        self.codec.validate_existing_entry(snapshot)?;
        let (root, original) = self.read_document(path)?;
        let object = root.object_value_or_create().ok_or_else(|| {
            format!(
                "refusing to modify {}: JSON root is not an object",
                path.display()
            )
        })?;
        let section = self
            .section_object(object, path, true)?
            .expect("create=true always returns a section");
        Self::ensure_unique_keys(&section, path, &self.key)?;
        self.validate_section_mutation_policy(&section, path)?;
        if section.get(name).is_some() {
            return Err(format!(
                "refusing to restore {}: '{}.{}' already exists",
                path.display(),
                self.key,
                name
            ));
        }
        section.append(name, Self::input_value(snapshot.clone()));
        self.write_document(path, &root, original.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{McpConfig, StdioConfig};

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("arca-json-{}-{}.json", name, std::process::id()));
        d
    }

    fn git() -> McpConfig {
        McpConfig::Stdio(StdioConfig {
            command: "npx".into(),
            args: Some(vec!["-y".into()]),
            env: None,
            cwd: None,
        })
    }

    #[test]
    fn upsert_then_read_roundtrips() {
        let p = tmp("rt");
        let adapter = JsonAdapter::new("mcpServers");
        adapter.upsert(&p, "git", &git()).unwrap();
        assert!(adapter.read(&p).contains_key("git"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn upsert_preserves_other_keys_and_servers() {
        // The data-loss regression test: a user file with a sibling server AND a
        // non-conforming server (no command/url) must keep BOTH after we add one.
        let p = tmp("preserve");
        std::fs::write(
            &p,
            r#"{"otherKey":42,"mcpServers":{
                "mine":{"command":"my-tool","args":["x"],"cwd":"/tmp","disabled":false},
                "weird":{"url":"https://x","transport":"streamable-http"}
            }}"#,
        )
        .unwrap();
        let adapter = JsonAdapter::new("mcpServers");
        adapter.upsert(&p, "git", &git()).unwrap();

        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        // Sibling top-level key preserved.
        assert_eq!(v["otherKey"], 42);
        let servers = v["mcpServers"].as_object().unwrap();
        // All three present; the user's servers kept their raw, un-modelled fields.
        assert!(servers.contains_key("git"));
        assert_eq!(servers["mine"]["cwd"], "/tmp");
        assert_eq!(servers["mine"]["disabled"], false);
        assert_eq!(servers["weird"]["transport"], "streamable-http");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn upsert_only_edits_mcp_section_and_preserves_jsonc() {
        let p = tmp("jsonc-private");
        let original = r#"{
  // Private agent settings must never be rewritten or exposed.
  "account": { "token" : "secret", "flags": [1,  2] },
  "mcpServers": {
    // Existing user-managed server.
    "mine": { "command" : "my-tool", "cwd": "/tmp" },
  },
  "theme": "dark"
}
"#;
        std::fs::write(&p, original).unwrap();

        JsonAdapter::new("mcpServers")
            .upsert(&p, "git", &git())
            .unwrap();

        let written = std::fs::read_to_string(&p).unwrap();
        assert!(written.contains(
            "// Private agent settings must never be rewritten or exposed.\n  \"account\": { \"token\" : \"secret\", \"flags\": [1,  2] },"
        ));
        assert!(written.contains(
            "// Existing user-managed server.\n    \"mine\": { \"command\" : \"my-tool\", \"cwd\": \"/tmp\" },"
        ));
        assert!(written.ends_with("  \"theme\": \"dark\"\n}\n"));
        assert!(JsonAdapter::new("mcpServers").read(&p).contains_key("git"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn updating_one_server_keeps_sibling_bytes() {
        let p = tmp("target-only");
        let sibling = r#""private": { "command" : "keep", "env": {"TOKEN":"secret"} }"#;
        let original = format!(
            "{{\n  \"mcpServers\": {{\n    \"git\": {{\"command\":\"old\"}},\n    {sibling}\n  }},\n  \"account\": {{\"id\":7}}\n}}\n"
        );
        std::fs::write(&p, &original).unwrap();

        JsonAdapter::new("mcpServers")
            .upsert(&p, "git", &git())
            .unwrap();

        let written = std::fs::read_to_string(&p).unwrap();
        assert!(written.contains(sibling));
        assert!(written.contains("\"account\": {\"id\":7}"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn invalid_json_is_never_overwritten() {
        let p = tmp("invalid");
        let original = r#"{"private":{"token":"secret"},"mcpServers": "#;
        std::fs::write(&p, original).unwrap();

        let result = JsonAdapter::new("mcpServers").upsert(&p, "git", &git());

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn non_object_mcp_section_is_never_replaced() {
        let p = tmp("wrong-section");
        let original = r#"{"private":{"token":"secret"},"mcpServers":"managed elsewhere"}"#;
        std::fs::write(&p, original).unwrap();

        let result = JsonAdapter::new("mcpServers").upsert(&p, "git", &git());

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn duplicate_mcp_section_is_never_modified() {
        let p = tmp("duplicate-section");
        let original =
            r#"{"mcpServers":{"one":{"command":"one"}},"mcpServers":{"two":{"command":"two"}}}"#;
        std::fs::write(&p, original).unwrap();

        let result = JsonAdapter::new("mcpServers").upsert(&p, "git", &git());

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn duplicate_server_name_is_never_modified_or_removed() {
        let p = tmp("duplicate-server");
        let original = r#"{"mcpServers":{"git":{"command":"one"},"git":{"command":"two"}}}"#;
        std::fs::write(&p, original).unwrap();
        let adapter = JsonAdapter::new("mcpServers");

        assert!(adapter.upsert(&p, "git", &git()).is_err());
        assert!(adapter.remove(&p, &["git".to_string()]).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn duplicate_controlled_field_is_never_modified() {
        let p = tmp("duplicate-field");
        let original =
            r#"{"mcpServers":{"git":{"command":"one","command":"two","disabled":false}}}"#;
        std::fs::write(&p, original).unwrap();

        let result = JsonAdapter::new("mcpServers").upsert(&p, "git", &git());

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn remove_keeps_other_servers_including_non_conforming() {
        let p = tmp("rm");
        std::fs::write(
            &p,
            r#"{"mcpServers":{"git":{"command":"npx"},"weird":{"url":"https://x"}}}"#,
        )
        .unwrap();
        let adapter = JsonAdapter::new("mcpServers");
        adapter.remove(&p, &["git".to_string()]).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let servers = v["mcpServers"].as_object().unwrap();
        assert!(!servers.contains_key("git"));
        assert!(servers.contains_key("weird")); // non-conforming sibling survives
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn snapshot_restore_preserves_agent_owned_fields() {
        let p = tmp("snapshot-restore");
        let original = r#"{
  "account": {"token": "private"},
  "mcpServers": {
    "git": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "git"],
      "enabled": false,
      "timeout": 120,
      "oauth": {"clientId": "client"},
      "allowedTools": ["read"]
    },
    "sibling": {"command": "keep"}
  }
}
"#;
        std::fs::write(&p, original).unwrap();
        let adapter = JsonAdapter::new("mcpServers");

        let snapshot = adapter.snapshot(&p, "git").unwrap().unwrap();
        adapter.remove(&p, &["git".to_string()]).unwrap();
        adapter.restore(&p, "git", &snapshot).unwrap();

        let written: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(written["account"]["token"], "private");
        assert_eq!(written["mcpServers"]["sibling"]["command"], "keep");
        assert_eq!(written["mcpServers"]["git"], snapshot);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn restore_refuses_to_overwrite_recreated_entry() {
        let p = tmp("snapshot-conflict");
        let original = r#"{"mcpServers":{"git":{"command":"newer"}}}"#;
        std::fs::write(&p, original).unwrap();
        let snapshot = serde_json::json!({"command": "older", "enabled": false});

        let result = JsonAdapter::new("mcpServers").restore(&p, "git", &snapshot);

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn snapshot_refuses_nested_duplicate_policy_fields() {
        let p = tmp("snapshot-duplicate-policy");
        let original = r#"{"mcpServers":{"git":{"command":"npx","oauth":{"clientId":"one","clientId":"two"}}}}"#;
        std::fs::write(&p, original).unwrap();

        let result = JsonAdapter::new("mcpServers").snapshot(&p, "git");

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn snapshot_removal_refuses_a_changed_target() {
        let p = tmp("snapshot-changed");
        let adapter = JsonAdapter::new("mcpServers");
        std::fs::write(
            &p,
            r#"{"mcpServers":{"git":{"command":"npx","timeout":10}}}"#,
        )
        .unwrap();
        let snapshot = adapter.snapshot(&p, "git").unwrap().unwrap();
        let newer = r#"{"mcpServers":{"git":{"command":"npx","timeout":20}}}"#;
        std::fs::write(&p, newer).unwrap();

        let result = adapter.remove_snapshot(&p, "git", &snapshot);

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), newer);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn read_missing_file_is_empty() {
        let adapter = JsonAdapter::new("mcpServers");
        assert!(adapter.read(Path::new("/nonexistent/xyz.json")).is_empty());
    }

    #[test]
    fn snapshot_missing_or_blank_file_has_no_entry() {
        let adapter = JsonAdapter::new("mcpServers");
        let p = tmp("snapshot-empty");
        let _ = std::fs::remove_file(&p);
        assert_eq!(adapter.snapshot(&p, "git").unwrap(), None);

        std::fs::write(&p, "  \n").unwrap();
        assert_eq!(adapter.snapshot(&p, "git").unwrap(), None);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn read_skips_unparseable_entries_but_keeps_good_ones() {
        let p = tmp("mixed");
        std::fs::write(
            &p,
            r#"{"mcpServers":{"good":{"command":"npx"},"weird":{"foo":"bar"}}}"#,
        )
        .unwrap();
        let back = JsonAdapter::new("mcpServers").read(&p);
        assert!(back.contains_key("good"));
        assert!(!back.contains_key("weird"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn remove_missing_file_is_noop() {
        let p = tmp("rm-noop");
        let _ = std::fs::remove_file(&p);
        let adapter = JsonAdapter::new("mcpServers");
        adapter.remove(&p, &["git".to_string()]).unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn removing_absent_server_does_not_rewrite_file() {
        let p = tmp("rm-absent");
        let original = "{\n  // keep formatting\n  \"mcpServers\": {},\n  \"private\": { \"token\" : \"secret\" }\n}\n";
        std::fs::write(&p, original).unwrap();

        JsonAdapter::new("mcpServers")
            .remove(&p, &["missing".to_string()])
            .unwrap();

        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }
}
