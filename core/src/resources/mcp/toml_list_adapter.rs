//! TOML list-shaped MCP Agent adapter.

use crate::domain::types::McpConfig;
use crate::resources::mcp::adapter::Adapter;
use crate::resources::mcp::codec::{Codec, EntryPatch, ObjectPatch};
use crate::safe_write::write_if_unchanged;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use toml::Value as Toml;
use toml_edit::{ArrayOfTables, Document, Item, Table};

/// Lossless TOML adapter for `[[section]]` arrays whose entries are identified
/// by a field such as `name` (Mistral Vibe).
pub struct TomlListAdapter {
    key: String,
    identity_field: String,
    codec: Codec,
    root_defaults: BTreeMap<String, Value>,
}

impl TomlListAdapter {
    pub fn with_spec(
        key: &str,
        identity_field: &str,
        codec: Codec,
        root_defaults: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            key: key.into(),
            identity_field: identity_field.into(),
            codec,
            root_defaults,
        }
    }

    fn read_document(&self, path: &Path) -> Result<(Document, Option<String>), String> {
        match fs::read_to_string(path) {
            Ok(text) => text
                .parse::<Document>()
                .map(|document| (document, Some(text)))
                .map_err(|error| {
                    format!(
                        "refusing to modify invalid TOML at {}: {}",
                        path.display(),
                        error
                    )
                }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok((Document::new(), None)),
            Err(error) => Err(format!("failed to read {}: {}", path.display(), error)),
        }
    }

    fn write_document(
        &self,
        path: &Path,
        document: &Document,
        original: Option<&str>,
    ) -> Result<(), String> {
        write_if_unchanged(path, original, &document.to_string())
    }

    fn fields_table(fields: Vec<(String, Value)>) -> Result<Table, String> {
        let value = Value::Object(fields.into_iter().collect());
        toml_edit::ser::to_document(&value)
            .map(|document| document.as_table().clone())
            .map_err(|error| error.to_string())
    }

    fn identity(table: &Table, field: &str) -> Option<String> {
        table
            .get(field)
            .and_then(Item::as_value)
            .and_then(toml_edit::Value::as_str)
            .map(str::to_string)
    }

    fn ensure_unique_identities(&self, section: &ArrayOfTables, path: &Path) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for table in section.iter() {
            let Some(name) = Self::identity(table, &self.identity_field) else {
                continue;
            };
            if !seen.insert(name.clone()) {
                return Err(format!(
                    "refusing to modify {}: duplicate TOML identity '{}.{}' is ambiguous",
                    path.display(),
                    self.key,
                    name
                ));
            }
        }
        Ok(())
    }

    fn key_parts(&self) -> Vec<&str> {
        self.key.split('.').collect()
    }

    fn section<'a>(&self, document: &'a Document) -> Option<&'a ArrayOfTables> {
        let parts = self.key_parts();
        let (leaf, parents) = parts.split_last()?;
        let mut table = document.as_table();
        for parent in parents {
            table = table.get(parent)?.as_table()?;
        }
        table.get(leaf)?.as_array_of_tables()
    }

    fn section_mut<'a>(
        &self,
        document: &'a mut Document,
        path: &Path,
        create: bool,
    ) -> Result<Option<&'a mut ArrayOfTables>, String> {
        let parts = self.key_parts();
        let (leaf, parents) = parts
            .split_last()
            .ok_or_else(|| "TOML array key must not be empty".to_string())?;
        if parts.iter().any(|part| part.is_empty()) {
            return Err(format!("invalid dotted TOML array key '{}'", self.key));
        }
        let mut table = document.as_table_mut();
        for parent in parents {
            if !table.contains_key(parent) && create {
                table.insert(parent, Item::Table(Table::new()));
            }
            let Some(item) = table.get_mut(parent) else {
                return Ok(None);
            };
            table = item.as_table_mut().ok_or_else(|| {
                format!(
                    "refusing to modify {}: '{}' is not a TOML table",
                    path.display(),
                    parent
                )
            })?;
        }
        if !table.contains_key(leaf) && create {
            table.insert(leaf, Item::ArrayOfTables(ArrayOfTables::new()));
        }
        match table.get_mut(leaf) {
            Some(Item::ArrayOfTables(section)) => Ok(Some(section)),
            None => Ok(None),
            Some(_) => Err(format!(
                "refusing to modify {}: '{}' is not a TOML array of tables",
                path.display(),
                self.key
            )),
        }
    }

    fn patch_nested_object(target: &mut Table, patch: ObjectPatch) -> Result<(), String> {
        if !target.contains_key(patch.parent) && patch.fields.is_empty() {
            return Ok(());
        }
        if !target.contains_key(patch.parent) {
            target.insert(patch.parent, Item::Table(Table::new()));
        }
        let nested = target
            .get_mut(patch.parent)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| format!("'{}' is not a TOML table", patch.parent))?;
        let fields = Self::fields_table(patch.fields)?;
        for field in patch.controlled {
            if let Some(value) = fields.get(field).cloned() {
                nested.insert(field, value);
            } else {
                nested.remove(field);
            }
        }
        Ok(())
    }

    fn patch_existing(
        &self,
        target: &mut Table,
        name: &str,
        patch: EntryPatch,
    ) -> Result<(), String> {
        let fields = Self::fields_table(patch.fields)?;
        for field in patch.controlled {
            if let Some(value) = fields.get(field).cloned() {
                target.insert(field, value);
            } else {
                target.remove(field);
            }
        }
        let defaults = Self::fields_table(patch.defaults)?;
        for (field, value) in defaults.iter() {
            if !target.contains_key(field) {
                target.insert(field, value.clone());
            }
        }
        for nested in patch.object_patches {
            Self::patch_nested_object(target, nested)?;
        }
        if !target.contains_key(&self.identity_field) {
            target.insert(&self.identity_field, toml_edit::value(name));
        }
        Ok(())
    }

    fn new_table(&self, name: &str, mut patch: EntryPatch) -> Result<Table, String> {
        patch.fields.extend(patch.defaults);
        for nested in patch.object_patches {
            if !nested.fields.is_empty() {
                patch.fields.push((
                    nested.parent.into(),
                    Value::Object(nested.fields.into_iter().collect()),
                ));
            }
        }
        patch
            .fields
            .push((self.identity_field.clone(), Value::String(name.into())));
        Self::fields_table(patch.fields)
    }

    fn apply_root_defaults(&self, document: &mut Document) -> Result<(), String> {
        let mut defaults = Table::new();
        for (field, value) in &self.root_defaults {
            defaults.insert(field, Self::default_item(value)?);
        }
        Self::merge_missing_defaults(document.as_table_mut(), &defaults);
        Ok(())
    }

    fn default_item(value: &Value) -> Result<Item, String> {
        if let Value::Object(fields) = value {
            let mut table = Table::new();
            for (field, value) in fields {
                table.insert(field, Self::default_item(value)?);
            }
            return Ok(Item::Table(table));
        }
        let fields = Self::fields_table(vec![("value".into(), value.clone())])?;
        fields
            .get("value")
            .cloned()
            .ok_or_else(|| "failed to materialize TOML root default".to_string())
    }

    /// Merge only absent default fields. Nested tables are traversed so a
    /// capability-level switch can be materialized in an existing config, but
    /// every explicit user value (including `false`) remains authoritative.
    fn merge_missing_defaults(target: &mut Table, defaults: &Table) {
        for (field, default) in defaults.iter() {
            let Some(existing) = target.get_mut(field) else {
                target.insert(field, default.clone());
                continue;
            };
            if let (Some(existing), Some(defaults)) = (existing.as_table_mut(), default.as_table())
            {
                Self::merge_missing_defaults(existing, defaults);
            }
        }
    }

    fn semantic_entries(&self, document: &Document) -> Result<Vec<Value>, String> {
        let semantic =
            toml::from_str::<Toml>(&document.to_string()).map_err(|error| error.to_string())?;
        let mut section = &semantic;
        for part in self.key.split('.') {
            let Some(value) = section.get(part) else {
                return Ok(Vec::new());
            };
            section = value;
        }
        let Some(entries) = section.as_array() else {
            return Ok(Vec::new());
        };
        entries
            .iter()
            .map(|entry| serde_json::to_value(entry).map_err(|error| error.to_string()))
            .collect()
    }

    fn validate_root_enablement(&self, document: &Document) -> Result<(), String> {
        if self.codec != Codec::VtCode {
            return Ok(());
        }
        let semantic =
            toml::from_str::<Toml>(&document.to_string()).map_err(|error| error.to_string())?;
        let Some(mcp) = semantic.get("mcp") else {
            return Ok(());
        };
        let table = mcp
            .as_table()
            .ok_or_else(|| "VT Code 'mcp' root is not a TOML table".to_string())?;
        match table.get("enabled") {
            None | Some(Toml::Boolean(true)) => Ok(()),
            Some(Toml::Boolean(false)) => Err(
                "VT Code MCP is disabled by 'mcp.enabled'; enable it in VT Code before Arca updates it"
                    .into(),
            ),
            Some(_) => Err(
                "VT Code MCP switch 'mcp.enabled' is not a boolean; refusing to infer state"
                    .into(),
            ),
        }
    }
}

impl Adapter for TomlListAdapter {
    fn read(&self, path: &Path) -> BTreeMap<String, McpConfig> {
        let Ok((document, _)) = self.read_document(path) else {
            return BTreeMap::new();
        };
        if self.validate_root_enablement(&document).is_err() {
            return BTreeMap::new();
        }
        let Some(section) = self.section(&document) else {
            return BTreeMap::new();
        };
        if self.ensure_unique_identities(section, path).is_err() {
            return BTreeMap::new();
        }
        self.semantic_entries(&document)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| {
                let name = entry
                    .as_object()?
                    .get(&self.identity_field)?
                    .as_str()?
                    .to_string();
                self.codec.decode(&entry).map(|config| (name, config))
            })
            .collect()
    }

    fn upsert(&self, path: &Path, name: &str, config: &McpConfig) -> Result<(), String> {
        let (mut document, original) = self.read_document(path)?;
        self.validate_root_enablement(&document)?;
        let existing_entries = self.semantic_entries(&document)?;
        let matching_entries = existing_entries
            .iter()
            .filter(|entry| {
                entry
                    .as_object()
                    .and_then(|object| object.get(&self.identity_field))
                    .and_then(Value::as_str)
                    == Some(name)
            })
            .collect::<Vec<_>>();
        if matching_entries.len() > 1 {
            return Err(format!("duplicate TOML identity '{}.{}'", self.key, name));
        }
        if let Some(entry) = matching_entries.first() {
            self.codec.validate_update(entry, config)?;
        }
        self.apply_root_defaults(&mut document)?;
        let patch = self.codec.patch(config)?;
        let section = self
            .section_mut(&mut document, path, true)?
            .expect("created TOML array of tables");
        self.ensure_unique_identities(section, path)?;
        let indexes: Vec<_> = section
            .iter()
            .enumerate()
            .filter_map(|(index, table)| {
                (Self::identity(table, &self.identity_field).as_deref() == Some(name))
                    .then_some(index)
            })
            .collect();
        if indexes.len() > 1 {
            return Err(format!("duplicate TOML identity '{}.{}'", self.key, name));
        }
        if let Some(index) = indexes.first().copied() {
            self.patch_existing(section.get_mut(index).expect("existing table"), name, patch)?;
        } else {
            section.push(self.new_table(name, patch)?);
        }
        self.write_document(path, &document, original.as_deref())
    }

    fn remove(&self, path: &Path, names: &[String]) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let (mut document, original) = self.read_document(path)?;
        let Some(section) = self.section_mut(&mut document, path, false)? else {
            return Ok(());
        };
        self.ensure_unique_identities(section, path)?;
        let mut indexes: Vec<_> = section
            .iter()
            .enumerate()
            .filter_map(|(index, table)| {
                let identity = Self::identity(table, &self.identity_field)?;
                names.contains(&identity).then_some(index)
            })
            .collect();
        indexes.sort_unstable_by(|left, right| right.cmp(left));
        if indexes.is_empty() {
            return Ok(());
        }
        for index in indexes {
            section.remove(index);
        }
        self.write_document(path, &document, original.as_deref())
    }

    fn clear(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let (mut document, original) = self.read_document(path)?;
        let Some(section) = self.section_mut(&mut document, path, false)? else {
            return Ok(());
        };
        if section.is_empty() {
            return Ok(());
        }
        for index in (0..section.len()).rev() {
            section.remove(index);
        }
        self.write_document(path, &document, original.as_deref())
    }

    fn has_entries(&self, path: &Path) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }
        let (mut document, _) = self.read_document(path)?;
        Ok(self
            .section_mut(&mut document, path, false)?
            .is_some_and(|section| !section.is_empty()))
    }

    fn snapshot(&self, path: &Path, name: &str) -> Result<Option<Value>, String> {
        let (document, _) = self.read_document(path)?;
        self.validate_root_enablement(&document)?;
        let mut matches = self
            .semantic_entries(&document)?
            .into_iter()
            .filter(|entry| {
                entry
                    .as_object()
                    .and_then(|object| object.get(&self.identity_field))
                    .and_then(Value::as_str)
                    == Some(name)
            });
        let first = matches.next();
        if matches.next().is_some() {
            return Err(format!("duplicate TOML identity '{}.{}'", self.key, name));
        }
        if let Some(entry) = first.as_ref() {
            self.codec.validate_existing_entry(entry)?;
        }
        Ok(first)
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
        self.codec.validate_existing_entry(snapshot)?;
        let Some(mut fields) = snapshot.as_object().cloned() else {
            return Err("refusing to restore a non-table TOML snapshot".into());
        };
        if self.snapshot(path, name)?.is_some() {
            return Err(format!(
                "refusing to restore {}: '{}.{}' already exists",
                path.display(),
                self.key,
                name
            ));
        }
        fields
            .entry(self.identity_field.clone())
            .or_insert_with(|| Value::String(name.into()));
        let (mut document, original) = self.read_document(path)?;
        self.apply_root_defaults(&mut document)?;
        let section = self
            .section_mut(&mut document, path, true)?
            .expect("created TOML array of tables");
        self.ensure_unique_identities(section, path)?;
        section.push(Self::fields_table(fields.into_iter().collect())?);
        self.write_document(path, &document, original.as_deref())
    }
}
