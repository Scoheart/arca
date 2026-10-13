//! YAML MCP Agent adapter.

use crate::domain::types::McpConfig;
use crate::resources::mcp::adapter::Adapter;
use crate::resources::mcp::codec::{Codec, EntryPatch, ObjectPatch};
use crate::safe_write::write_if_unchanged;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::str::FromStr;
use yaml_edit::{Document, Mapping, Sequence, YamlFile, YamlNode};

/// Lossless YAML adapter for both map-shaped sections (Hermes, Goose) and
/// list-shaped sections whose entries carry their own identity (Continue).
pub struct YamlAdapter {
    key: String,
    codec: Codec,
    list: bool,
    identity_field: Option<String>,
    root_defaults: BTreeMap<String, Value>,
}

impl YamlAdapter {
    pub fn with_spec(
        key: &str,
        codec: Codec,
        list: bool,
        identity_field: Option<String>,
        root_defaults: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            key: key.into(),
            codec,
            list,
            identity_field,
            root_defaults,
        }
    }

    fn read_document(&self, path: &Path) -> Result<(YamlFile, Document, Option<String>), String> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let file = YamlFile::from_str(&text).map_err(|error| {
                    format!(
                        "refusing to modify invalid YAML at {}: {}",
                        path.display(),
                        error
                    )
                })?;
                let documents: Vec<_> = file.documents().collect();
                if documents.len() != 1 {
                    return Err(format!(
                        "refusing to modify {}: expected exactly one YAML document",
                        path.display()
                    ));
                }
                Ok((file, documents[0].clone(), Some(text)))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let file = YamlFile::new();
                file.push_document(Document::new_mapping());
                let document = file.document().expect("new YAML document");
                Ok((file, document, None))
            }
            Err(error) => Err(format!("failed to read {}: {}", path.display(), error)),
        }
    }

    fn write_document(
        &self,
        path: &Path,
        file: &YamlFile,
        original: Option<&str>,
    ) -> Result<(), String> {
        let mut content = file.to_string();
        if !content.ends_with('\n') {
            content.push('\n');
        }
        write_if_unchanged(path, original, &content)
    }

    fn json_to_node(value: &Value) -> Result<YamlNode, String> {
        let wrapper = serde_json::json!({"__arca_value": value});
        let yaml = serde_yaml::to_string(&wrapper).map_err(|error| error.to_string())?;
        Document::from_str(&yaml)
            .map_err(|error| error.to_string())?
            .get("__arca_value")
            .ok_or_else(|| "failed to construct YAML value".into())
    }

    fn json_to_flow_node(value: &Value) -> Result<YamlNode, String> {
        let wrapper = serde_json::json!({"__arca_value": value});
        Document::from_str(&wrapper.to_string())
            .map_err(|error| error.to_string())?
            .get("__arca_value")
            .ok_or_else(|| "failed to construct flow-style YAML value".into())
    }

    fn set_node(target: &Mapping, key: &str, value: &Value) -> Result<(), String> {
        match Self::json_to_node(value)? {
            YamlNode::Scalar(node) => target.set(key, node),
            YamlNode::Mapping(node) => target.set(key, node),
            YamlNode::Sequence(node) => target.set(key, node),
            YamlNode::Alias(node) => target.set(key, node),
            YamlNode::TaggedNode(node) => target.set(key, node),
        }
        Ok(())
    }

    fn mapping_keys(target: &Mapping) -> Vec<String> {
        target
            .entries()
            .filter_map(|entry| {
                entry
                    .key_node()
                    .and_then(|key| Self::node_to_json(&key).ok())
                    .and_then(|key| key.as_str().map(str::to_string))
            })
            .collect()
    }

    fn mapping_set(target: &Mapping, key: &str, value: &Value) -> Result<(), String> {
        match value {
            Value::Object(fields) => {
                if target.get_mapping(key).is_none() {
                    target.remove(key);
                    match Self::json_to_flow_node(value)? {
                        YamlNode::Mapping(node) => target.set(key, node),
                        _ => return Err(format!("failed to construct YAML mapping for '{key}'")),
                    }
                    return Ok(());
                }
                let nested = target
                    .get_mapping(key)
                    .ok_or_else(|| format!("failed to construct YAML mapping for '{key}'"))?;
                for existing in Self::mapping_keys(&nested) {
                    if !fields.contains_key(&existing) {
                        nested.remove(existing.as_str());
                    }
                }
                for (field, field_value) in fields {
                    Self::mapping_set(&nested, field, field_value)?;
                }
                Ok(())
            }
            Value::Array(_) => {
                target.remove(key);
                match Self::json_to_flow_node(value)? {
                    YamlNode::Sequence(node) => target.set(key, node),
                    _ => return Err(format!("failed to construct YAML sequence for '{key}'")),
                }
                Ok(())
            }
            _ => Self::set_node(target, key, value),
        }
    }

    fn write_new_config(&self, path: &Path, name: &str, patch: EntryPatch) -> Result<(), String> {
        let entry = self.materialize_entry(name, patch)?;
        self.write_new_entry(path, name, &entry)
    }

    fn write_new_entry(&self, path: &Path, name: &str, entry: &Value) -> Result<(), String> {
        let mut content = if self.root_defaults.is_empty() {
            String::new()
        } else {
            serde_yaml::to_string(&self.root_defaults).map_err(|error| error.to_string())?
        };
        content.push_str(&format!("{}:\n", self.key));
        let entry = serde_json::to_string(entry).map_err(|error| error.to_string())?;
        if self.list {
            content.push_str(&format!("  - {entry}\n"));
        } else {
            let name = serde_json::to_string(name).map_err(|error| error.to_string())?;
            content.push_str(&format!("  {name}: {entry}\n"));
        }
        Self::validate_generated_yaml(path, &content)?;
        write_if_unchanged(path, None, &content)
    }

    fn validate_generated_yaml(path: &Path, content: &str) -> Result<(), String> {
        serde_yaml::from_str::<Value>(content).map_err(|error| {
            format!(
                "refusing to write invalid generated YAML at {}: {}",
                path.display(),
                error
            )
        })?;
        YamlFile::from_str(content).map_err(|error| {
            format!(
                "refusing to write YAML unsupported by the lossless parser at {}: {}",
                path.display(),
                error
            )
        })?;
        Ok(())
    }

    fn insert_map_entry(
        &self,
        path: &Path,
        original: &str,
        section: &Mapping,
        name: &str,
        entry: &Value,
    ) -> Result<(), String> {
        let range = section.byte_range();
        let start = range.start as usize;
        let end = range.end as usize;
        let source = original
            .get(start..end)
            .ok_or_else(|| "invalid YAML mapping source range".to_string())?;
        let name = serde_json::to_string(name).map_err(|error| error.to_string())?;
        let entry = serde_json::to_string(entry).map_err(|error| error.to_string())?;
        let mut content = original.to_string();

        if source.trim_start().starts_with('{') {
            let close = source
                .rfind('}')
                .ok_or_else(|| "invalid flow-style YAML mapping".to_string())?;
            let before = &source[..close];
            let separator = if before.trim_end().ends_with('{') || before.trim_end().ends_with(',')
            {
                ""
            } else {
                ", "
            };
            content.insert_str(start + close, &format!("{separator}{name}: {entry}"));
        } else {
            let indent = section.start_position(original).column.saturating_sub(1);
            let mut addition = String::new();
            if !original[..end].ends_with('\n') {
                addition.push('\n');
            }
            addition.push_str(&" ".repeat(indent));
            addition.push_str(&format!("{name}: {entry}"));
            if !original[end..].starts_with('\n') {
                addition.push('\n');
            }
            content.insert_str(end, &addition);
        }
        Self::validate_generated_yaml(path, &content)?;
        write_if_unchanged(path, Some(original), &content)
    }

    fn insert_list_entry(
        &self,
        path: &Path,
        original: &str,
        section: &Sequence,
        entry: &Value,
    ) -> Result<(), String> {
        let range = section.byte_range();
        let start = range.start as usize;
        let end = range.end as usize;
        let source = original
            .get(start..end)
            .ok_or_else(|| "invalid YAML sequence source range".to_string())?;
        let entry = serde_json::to_string(entry).map_err(|error| error.to_string())?;
        let mut content = original.to_string();

        if source.trim_start().starts_with('[') {
            let close = source
                .rfind(']')
                .ok_or_else(|| "invalid flow-style YAML sequence".to_string())?;
            let before = &source[..close];
            let separator = if before.trim_end().ends_with('[') || before.trim_end().ends_with(',')
            {
                ""
            } else {
                ", "
            };
            content.insert_str(start + close, &format!("{separator}{entry}"));
        } else {
            let indent = section.start_position(original).column.saturating_sub(1);
            let mut addition = String::new();
            if !original[..end].ends_with('\n') {
                addition.push('\n');
            }
            addition.push_str(&" ".repeat(indent));
            addition.push_str(&format!("- {entry}"));
            if !original[end..].starts_with('\n') {
                addition.push('\n');
            }
            content.insert_str(end, &addition);
        }
        Self::validate_generated_yaml(path, &content)?;
        write_if_unchanged(path, Some(original), &content)
    }

    fn append_section(
        &self,
        path: &Path,
        original: &str,
        name: &str,
        entry: &Value,
    ) -> Result<(), String> {
        let mut content = original.to_string();
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&format!("{}:\n", self.key));
        let entry = serde_json::to_string(entry).map_err(|error| error.to_string())?;
        if self.list {
            content.push_str(&format!("  - {entry}\n"));
        } else {
            let name = serde_json::to_string(name).map_err(|error| error.to_string())?;
            content.push_str(&format!("  {name}: {entry}\n"));
        }
        Self::validate_generated_yaml(path, &content)?;
        write_if_unchanged(path, Some(original), &content)
    }

    fn node_to_json(node: &YamlNode) -> Result<Value, String> {
        serde_yaml::from_str(&node.to_string()).map_err(|error| error.to_string())
    }

    fn ensure_unique_mapping(mapping: &Mapping, path: &Path, context: &str) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for entry in mapping.entries() {
            let Some(key) = entry.key_node() else {
                continue;
            };
            let key_value = Self::node_to_json(&key).map_err(|error| {
                format!(
                    "refusing to modify {}: invalid YAML key in '{}': {}",
                    path.display(),
                    context,
                    error
                )
            })?;
            let canonical = serde_json::to_string(&key_value).map_err(|error| error.to_string())?;
            if !seen.insert(canonical) {
                return Err(format!(
                    "refusing to modify {}: duplicate YAML key {} in '{}' is ambiguous",
                    path.display(),
                    key_value,
                    context
                ));
            }
            if let Some(value) = entry.value_node() {
                let child = key_value
                    .as_str()
                    .map(|key| format!("{context}.{key}"))
                    .unwrap_or_else(|| context.to_string());
                Self::ensure_unique_node(&value, path, &child)?;
            }
        }
        Ok(())
    }

    fn ensure_unique_node(node: &YamlNode, path: &Path, context: &str) -> Result<(), String> {
        if let Some(mapping) = node.as_mapping() {
            Self::ensure_unique_mapping(mapping, path, context)?;
        } else if let Some(sequence) = node.as_sequence() {
            for item in sequence {
                Self::ensure_unique_node(&item, path, context)?;
            }
        }
        Ok(())
    }

    fn ensure_root(&self, document: &Document, path: &Path) -> Result<Mapping, String> {
        let root = document.as_mapping().ok_or_else(|| {
            format!(
                "refusing to modify {}: YAML root is not a mapping",
                path.display()
            )
        })?;
        Self::ensure_unique_mapping(&root, path, "$root")?;
        Ok(root)
    }

    fn materialize_entry(&self, name: &str, mut patch: EntryPatch) -> Result<Value, String> {
        patch.fields.extend(patch.defaults);
        let mut fields: Map<String, Value> = patch.fields.into_iter().collect();
        for nested in patch.object_patches {
            if !nested.fields.is_empty() {
                fields.insert(
                    nested.parent.into(),
                    Value::Object(nested.fields.into_iter().collect()),
                );
            }
        }
        if let Some(identity) = &self.identity_field {
            fields
                .entry(identity.clone())
                .or_insert_with(|| Value::String(name.into()));
        }
        Ok(Value::Object(fields))
    }

    fn patch_nested_object(
        target: &Mapping,
        patch: ObjectPatch,
        path: &Path,
        context: &str,
    ) -> Result<(), String> {
        let matches = target.find_all_entries_by_key(patch.parent).count();
        if matches > 1 {
            return Err(format!(
                "refusing to modify {}: duplicate YAML key '{}.{}' is ambiguous",
                path.display(),
                context,
                patch.parent
            ));
        }
        if matches == 0 && patch.fields.is_empty() {
            return Ok(());
        }
        if matches == 0 {
            Self::mapping_set(target, patch.parent, &serde_json::json!({}))?;
        }
        let nested = target.get_mapping(patch.parent).ok_or_else(|| {
            format!(
                "refusing to modify {}: '{}.{}' is not a mapping",
                path.display(),
                context,
                patch.parent
            )
        })?;
        Self::ensure_unique_mapping(&nested, path, &format!("{}.{}", context, patch.parent))?;
        for field in patch.controlled {
            if let Some((_, value)) = patch.fields.iter().find(|(name, _)| name == field) {
                Self::mapping_set(&nested, field, value)?;
            } else {
                nested.remove(field);
            }
        }
        Ok(())
    }

    fn patch_existing(
        &self,
        target: &Mapping,
        name: &str,
        patch: EntryPatch,
        path: &Path,
    ) -> Result<(), String> {
        let context = format!("{}.{}", self.key, name);
        Self::ensure_unique_mapping(target, path, &context)?;
        for field in patch.controlled {
            if let Some((_, value)) = patch.fields.iter().find(|(name, _)| name == field) {
                Self::mapping_set(target, field, value)?;
            } else {
                target.remove(field);
            }
        }
        for nested in patch.object_patches {
            Self::patch_nested_object(target, nested, path, &context)?;
        }
        for (field, value) in patch.defaults {
            if !target.contains_key(field.as_str()) {
                Self::mapping_set(target, &field, &value)?;
            }
        }
        if let Some(identity) = &self.identity_field {
            if !target.contains_key(identity.as_str()) {
                target.set(identity.as_str(), name);
            }
        }
        Ok(())
    }

    fn identity(&self, node: &YamlNode) -> Option<String> {
        let field = self.identity_field.as_deref()?;
        node.as_mapping()?
            .get(field)
            .and_then(|value| Self::node_to_json(&value).ok())
            .and_then(|value| value.as_str().map(str::to_string))
    }

    fn ensure_unique_identities(&self, sequence: &Sequence, path: &Path) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for item in sequence {
            Self::ensure_unique_node(&item, path, &self.key)?;
            let Some(identity) = self.identity(&item) else {
                continue;
            };
            if !seen.insert(identity.clone()) {
                return Err(format!(
                    "refusing to modify {}: duplicate YAML identity '{}.{}' is ambiguous",
                    path.display(),
                    self.key,
                    identity
                ));
            }
        }
        Ok(())
    }

    fn section_mapping(
        &self,
        root: &Mapping,
        path: &Path,
        create: bool,
    ) -> Result<Option<Mapping>, String> {
        let count = root.find_all_entries_by_key(self.key.as_str()).count();
        if count > 1 {
            return Err(format!(
                "refusing to modify {}: duplicate YAML key '$root.{}' is ambiguous",
                path.display(),
                self.key
            ));
        }
        if count == 0 && create {
            Self::mapping_set(root, &self.key, &serde_json::json!({}))?;
        }
        match root.get_mapping(self.key.as_str()) {
            Some(section) => Ok(Some(section)),
            None if count == 0 => Ok(None),
            None => Err(format!(
                "refusing to modify {}: '{}' is not a YAML mapping",
                path.display(),
                self.key
            )),
        }
    }

    fn section_sequence(
        &self,
        root: &Mapping,
        path: &Path,
        create: bool,
    ) -> Result<Option<Sequence>, String> {
        let count = root.find_all_entries_by_key(self.key.as_str()).count();
        if count > 1 {
            return Err(format!(
                "refusing to modify {}: duplicate YAML key '$root.{}' is ambiguous",
                path.display(),
                self.key
            ));
        }
        if count == 0 && create {
            Self::mapping_set(root, &self.key, &serde_json::json!([]))?;
        }
        match root.get_sequence(self.key.as_str()) {
            Some(section) => Ok(Some(section)),
            None if count == 0 => Ok(None),
            None => Err(format!(
                "refusing to modify {}: '{}' is not a YAML sequence",
                path.display(),
                self.key
            )),
        }
    }

    fn snapshot_node(&self, path: &Path, name: &str) -> Result<Option<YamlNode>, String> {
        let (_, document, _) = self.read_document(path)?;
        let root = self.ensure_root(&document, path)?;
        if self.list {
            let Some(sequence) = self.section_sequence(&root, path, false)? else {
                return Ok(None);
            };
            self.ensure_unique_identities(&sequence, path)?;
            let snapshot = (&sequence)
                .into_iter()
                .find(|item| self.identity(item).as_deref() == Some(name));
            Ok(snapshot)
        } else {
            let Some(section) = self.section_mapping(&root, path, false)? else {
                return Ok(None);
            };
            Self::ensure_unique_mapping(&section, path, &self.key)?;
            Ok(section.get(name))
        }
    }
}

impl Adapter for YamlAdapter {
    fn read(&self, path: &Path) -> BTreeMap<String, McpConfig> {
        let Ok((_, document, _)) = self.read_document(path) else {
            return BTreeMap::new();
        };
        let Some(root) = document.as_mapping() else {
            return BTreeMap::new();
        };
        if Self::ensure_unique_mapping(&root, path, "$root").is_err() {
            return BTreeMap::new();
        }
        if self.list {
            let Ok(Some(sequence)) = self.section_sequence(&root, path, false) else {
                return BTreeMap::new();
            };
            if self.ensure_unique_identities(&sequence, path).is_err() {
                return BTreeMap::new();
            }
            (&sequence)
                .into_iter()
                .filter_map(|node| {
                    let name = self.identity(&node)?;
                    let value = Self::node_to_json(&node).ok()?;
                    self.codec.decode(&value).map(|config| (name, config))
                })
                .collect()
        } else {
            let Ok(Some(section)) = self.section_mapping(&root, path, false) else {
                return BTreeMap::new();
            };
            if Self::ensure_unique_mapping(&section, path, &self.key).is_err() {
                return BTreeMap::new();
            }
            section
                .entries()
                .filter_map(|entry| {
                    let name = entry
                        .key_node()
                        .and_then(|node| Self::node_to_json(&node).ok())?
                        .as_str()?
                        .to_string();
                    let value = entry.value_node()?;
                    let value = Self::node_to_json(&value).ok()?;
                    self.codec.decode(&value).map(|config| (name, config))
                })
                .collect()
        }
    }

    fn upsert(&self, path: &Path, name: &str, config: &McpConfig) -> Result<(), String> {
        let (file, document, original) = self.read_document(path)?;
        let patch = self.codec.patch(config)?;
        if original.is_none() {
            return self.write_new_config(path, name, patch);
        }
        let root = self.ensure_root(&document, path)?;
        if self.list {
            let Some(sequence) = self.section_sequence(&root, path, false)? else {
                let value = self.materialize_entry(name, patch)?;
                return self.append_section(path, original.as_deref().unwrap(), name, &value);
            };
            self.ensure_unique_identities(&sequence, path)?;
            let existing = (&sequence)
                .into_iter()
                .find(|item| self.identity(item).as_deref() == Some(name));
            if let Some(node) = existing {
                let target = node.as_mapping().ok_or_else(|| {
                    format!(
                        "refusing to modify {}: list entry is not a mapping",
                        path.display()
                    )
                })?;
                self.patch_existing(target, name, patch, path)?;
            } else {
                let value = self.materialize_entry(name, patch)?;
                return self.insert_list_entry(
                    path,
                    original.as_deref().unwrap(),
                    &sequence,
                    &value,
                );
            }
        } else {
            let Some(section) = self.section_mapping(&root, path, false)? else {
                let value = self.materialize_entry(name, patch)?;
                return self.append_section(path, original.as_deref().unwrap(), name, &value);
            };
            Self::ensure_unique_mapping(&section, path, &self.key)?;
            if let Some(node) = section.get(name) {
                let target = node.as_mapping().ok_or_else(|| {
                    format!(
                        "refusing to modify {}: '{}.{}' is not a mapping",
                        path.display(),
                        self.key,
                        name
                    )
                })?;
                self.patch_existing(target, name, patch, path)?;
            } else {
                let value = self.materialize_entry(name, patch)?;
                return self.insert_map_entry(
                    path,
                    original.as_deref().unwrap(),
                    &section,
                    name,
                    &value,
                );
            }
        }
        self.write_document(path, &file, original.as_deref())
    }

    fn remove(&self, path: &Path, names: &[String]) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let (file, document, original) = self.read_document(path)?;
        let root = self.ensure_root(&document, path)?;
        let mut changed = false;
        if self.list {
            let Some(sequence) = self.section_sequence(&root, path, false)? else {
                return Ok(());
            };
            self.ensure_unique_identities(&sequence, path)?;
            let mut indexes: Vec<_> = (&sequence)
                .into_iter()
                .enumerate()
                .filter_map(|(index, item)| names.contains(&self.identity(&item)?).then_some(index))
                .collect();
            indexes.sort_unstable_by(|left, right| right.cmp(left));
            for index in indexes {
                sequence.remove(index);
                changed = true;
            }
        } else {
            let Some(section) = self.section_mapping(&root, path, false)? else {
                return Ok(());
            };
            Self::ensure_unique_mapping(&section, path, &self.key)?;
            for name in names {
                changed |= section.remove(name).is_some();
            }
        }
        if changed {
            self.write_document(path, &file, original.as_deref())?;
        }
        Ok(())
    }

    fn clear(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let (file, document, original) = self.read_document(path)?;
        let root = self.ensure_root(&document, path)?;
        let changed = if self.list {
            let Some(sequence) = self.section_sequence(&root, path, false)? else {
                return Ok(());
            };
            let changed = !sequence.is_empty();
            for index in (0..sequence.len()).rev() {
                sequence.remove(index);
            }
            changed
        } else {
            let Some(section) = self.section_mapping(&root, path, false)? else {
                return Ok(());
            };
            let keys = Self::mapping_keys(&section);
            for key in &keys {
                section.remove(key.as_str());
            }
            !keys.is_empty()
        };
        if changed {
            self.write_document(path, &file, original.as_deref())?;
        }
        Ok(())
    }

    fn has_entries(&self, path: &Path) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }
        let (_, document, _) = self.read_document(path)?;
        let root = self.ensure_root(&document, path)?;
        if self.list {
            Ok(self
                .section_sequence(&root, path, false)?
                .is_some_and(|section| !section.is_empty()))
        } else {
            Ok(self
                .section_mapping(&root, path, false)?
                .is_some_and(|section| !Self::mapping_keys(&section).is_empty()))
        }
    }

    fn snapshot(&self, path: &Path, name: &str) -> Result<Option<Value>, String> {
        self.snapshot_node(path, name)?
            .map(|node| {
                Self::ensure_unique_node(&node, path, &format!("{}.{}", self.key, name))?;
                let value = Self::node_to_json(&node)?;
                if !value.is_object() {
                    return Err("refusing to snapshot a non-mapping YAML entry".into());
                }
                Ok(value)
            })
            .transpose()
    }

    fn remove_snapshot(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String> {
        let current = self.snapshot(path, name)?.ok_or_else(|| {
            format!(
                "refusing to remove {}: '{}.{}' no longer exists",
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
        self.remove(path, &[name.to_string()])
    }

    fn restore(&self, path: &Path, name: &str, snapshot: &Value) -> Result<(), String> {
        let Some(mut fields) = snapshot.as_object().cloned() else {
            return Err("refusing to restore a non-mapping YAML snapshot".into());
        };
        if self.snapshot(path, name)?.is_some() {
            return Err(format!(
                "refusing to restore {}: '{}.{}' already exists",
                path.display(),
                self.key,
                name
            ));
        }
        if let Some(identity) = &self.identity_field {
            fields
                .entry(identity.clone())
                .or_insert_with(|| Value::String(name.into()));
        }
        let (_, document, original) = self.read_document(path)?;
        let value = Value::Object(fields);
        if original.is_none() {
            return self.write_new_entry(path, name, &value);
        }
        let original = original.as_deref().unwrap();
        let root = self.ensure_root(&document, path)?;
        if self.list {
            let Some(sequence) = self.section_sequence(&root, path, false)? else {
                return self.append_section(path, original, name, &value);
            };
            self.ensure_unique_identities(&sequence, path)?;
            self.insert_list_entry(path, original, &sequence, &value)
        } else {
            let Some(section) = self.section_mapping(&root, path, false)? else {
                return self.append_section(path, original, name, &value);
            };
            Self::ensure_unique_mapping(&section, path, &self.key)?;
            self.insert_map_entry(path, original, &section, name, &value)
        }
    }
}
