//! TOML map-shaped MCP Agent adapter.

use crate::domain::types::McpConfig;
use crate::resources::mcp::adapter::Adapter;
use crate::resources::mcp::codec::{Codec, EntryPatch, ObjectPatch};
use crate::safe_write::write_if_unchanged;
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use toml::Value as Toml;
use toml_edit::{Document, Item, Table};

pub struct TomlAdapter {
    pub key: String,
    codec: Codec,
    root_defaults: BTreeMap<String, serde_json::Value>,
}

impl TomlAdapter {
    // Note (intentional divergence from TS): the TS TomlAdapter hard-codes the
    // section name `mcp_servers`, whereas this Rust adapter honors the `key`
    // passed by the caller. The Rust behavior is the more correct/general one.
    pub fn new(key: &str) -> Self {
        Self::with_codec(key, Codec::Standard)
    }

    pub fn with_codec(key: &str, codec: Codec) -> Self {
        Self::with_spec(key, codec, BTreeMap::new())
    }

    pub fn with_spec(
        key: &str,
        codec: Codec,
        root_defaults: BTreeMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            key: key.to_string(),
            codec,
            root_defaults,
        }
    }

    fn read_root(&self, path: &Path) -> Toml {
        fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str::<Toml>(&content).ok())
            .filter(Toml::is_table)
            .unwrap_or_else(|| Toml::Table(toml::map::Map::new()))
    }

    fn read_document(&self, path: &Path) -> Result<(Document, Option<String>), String> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let document = text.parse::<Document>().map_err(|e| {
                    format!(
                        "refusing to modify invalid TOML at {}: {}",
                        path.display(),
                        e
                    )
                })?;
                Ok((document, Some(text)))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok((Document::new(), None)),
            Err(e) => Err(format!("failed to read {}: {}", path.display(), e)),
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

    fn fields_table(fields: Vec<(String, serde_json::Value)>) -> Result<Table, String> {
        let value = serde_json::Value::Object(fields.into_iter().collect());
        toml_edit::ser::to_document(&value)
            .map(|document| document.as_table().clone())
            .map_err(|e| e.to_string())
    }

    fn materialize_fields(mut patch: EntryPatch) -> Vec<(String, serde_json::Value)> {
        patch.fields.extend(patch.defaults);
        for nested in patch.object_patches {
            if !nested.fields.is_empty() {
                patch.fields.push((
                    nested.parent.into(),
                    serde_json::Value::Object(nested.fields.into_iter().collect()),
                ));
            }
        }
        patch.fields
    }

    fn insert_new(section: &mut Table, name: &str, patch: EntryPatch) -> Result<(), String> {
        section.insert(
            name,
            Item::Table(Self::fields_table(Self::materialize_fields(patch))?),
        );
        Ok(())
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
}

impl Adapter for TomlAdapter {
    fn read(&self, path: &Path) -> BTreeMap<String, McpConfig> {
        let root = self.read_root(path);
        let Some(section) = root.get(&self.key).and_then(Toml::as_table) else {
            return BTreeMap::new();
        };
        // Per-entry parse (toml -> json -> McpConfig); skip non-conforming entries
        // rather than discarding the whole section.
        section
            .iter()
            .filter_map(|(name, val)| {
                serde_json::to_value(val)
                    .ok()
                    .and_then(|j| self.codec.decode(&j))
                    .map(|cfg| (name.clone(), cfg))
            })
            .collect()
    }

    fn upsert(&self, path: &Path, name: &str, cfg: &McpConfig) -> Result<(), String> {
        let (mut document, original) = self.read_document(path)?;
        if original.is_none() && !self.root_defaults.is_empty() {
            let defaults = Self::fields_table(
                self.root_defaults
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            )?;
            for (field, value) in defaults.iter() {
                if !document.as_table().contains_key(field) {
                    document.as_table_mut().insert(field, value.clone());
                }
            }
        }
        if !document.as_table().contains_key(&self.key) {
            document
                .as_table_mut()
                .insert(&self.key, Item::Table(Table::new()));
        }
        let section = document
            .as_table_mut()
            .get_mut(&self.key)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| {
                format!(
                    "refusing to modify {}: '{}' is not a TOML table",
                    path.display(),
                    self.key
                )
            })?;
        let patch = self.codec.patch(cfg)?;
        if let Some(item) = section.get(name) {
            let current = serde_json::to_value(
                toml::from_str::<Toml>(&item.to_string()).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            self.codec.validate_update(&current, cfg)?;
        }
        if let Some(item) = section.get_mut(name) {
            let target = item.as_table_mut().ok_or_else(|| {
                format!(
                    "refusing to modify {}: '{}.{}' is not a TOML table",
                    path.display(),
                    self.key,
                    name
                )
            })?;
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
        } else {
            Self::insert_new(section, name, patch)?;
        }
        self.write_document(path, &document, original.as_deref())
    }

    fn remove(&self, path: &Path, names: &[String]) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let (mut document, original) = self.read_document(path)?;
        let Some(section_item) = document.as_table_mut().get_mut(&self.key) else {
            return Ok(());
        };
        let section = section_item.as_table_mut().ok_or_else(|| {
            format!(
                "refusing to modify {}: '{}' is not a TOML table",
                path.display(),
                self.key
            )
        })?;
        let mut changed = false;
        for name in names {
            changed |= section.remove(name).is_some();
        }
        if changed {
            self.write_document(path, &document, original.as_deref())?;
        }
        Ok(())
    }

    fn clear(&self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let (mut document, original) = self.read_document(path)?;
        let Some(section_item) = document.as_table_mut().get_mut(&self.key) else {
            return Ok(());
        };
        let section = section_item.as_table_mut().ok_or_else(|| {
            format!(
                "refusing to modify {}: '{}' is not a TOML table",
                path.display(),
                self.key
            )
        })?;
        let names = section
            .iter()
            .map(|(name, _)| name.to_string())
            .collect::<Vec<_>>();
        if names.is_empty() {
            return Ok(());
        }
        for name in names {
            section.remove(&name);
        }
        self.write_document(path, &document, original.as_deref())
    }

    fn has_entries(&self, path: &Path) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }
        let (document, _) = self.read_document(path)?;
        let Some(section_item) = document.as_table().get(&self.key) else {
            return Ok(false);
        };
        let section = section_item.as_table().ok_or_else(|| {
            format!(
                "refusing to read {}: '{}' is not a TOML table",
                path.display(),
                self.key
            )
        })?;
        Ok(!section.is_empty())
    }

    fn snapshot(&self, path: &Path, name: &str) -> Result<Option<serde_json::Value>, String> {
        let (document, _) = self.read_document(path)?;
        let Some(section_item) = document.as_table().get(&self.key) else {
            return Ok(None);
        };
        let section = section_item.as_table().ok_or_else(|| {
            format!(
                "refusing to read {}: '{}' is not a TOML table",
                path.display(),
                self.key
            )
        })?;
        let Some(item) = section.get(name) else {
            return Ok(None);
        };
        if !item.is_table() {
            return Err(format!(
                "refusing to read {}: '{}.{}' is not a TOML table",
                path.display(),
                self.key,
                name
            ));
        }
        let semantic =
            toml::from_str::<Toml>(&document.to_string()).map_err(|error| error.to_string())?;
        let value = semantic
            .get(&self.key)
            .and_then(Toml::as_table)
            .and_then(|section| section.get(name))
            .ok_or_else(|| "TOML snapshot disappeared during conversion".to_string())?;
        serde_json::to_value(value)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    fn remove_snapshot(
        &self,
        path: &Path,
        name: &str,
        snapshot: &serde_json::Value,
    ) -> Result<(), String> {
        let (mut document, original) = self.read_document(path)?;
        let semantic =
            toml::from_str::<Toml>(&document.to_string()).map_err(|error| error.to_string())?;
        let current = semantic
            .get(&self.key)
            .and_then(Toml::as_table)
            .and_then(|section| section.get(name))
            .ok_or_else(|| {
                format!(
                    "refusing to remove {}: '{}.{}' no longer exists",
                    path.display(),
                    self.key,
                    name
                )
            })?;
        let current = serde_json::to_value(current).map_err(|error| error.to_string())?;
        if &current != snapshot {
            return Err(format!(
                "refusing to remove {}: '{}.{}' changed after its snapshot was saved",
                path.display(),
                self.key,
                name
            ));
        }
        let section = document
            .as_table_mut()
            .get_mut(&self.key)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| {
                format!(
                    "refusing to modify {}: '{}' is not a TOML table",
                    path.display(),
                    self.key
                )
            })?;
        section.remove(name).ok_or_else(|| {
            format!(
                "refusing to remove {}: '{}.{}' no longer exists",
                path.display(),
                self.key,
                name
            )
        })?;
        self.write_document(path, &document, original.as_deref())
    }

    fn restore(&self, path: &Path, name: &str, snapshot: &serde_json::Value) -> Result<(), String> {
        if !snapshot.is_object() {
            return Err("refusing to restore a non-table MCP snapshot".into());
        }
        let (mut document, original) = self.read_document(path)?;
        if !document.as_table().contains_key(&self.key) {
            document
                .as_table_mut()
                .insert(&self.key, Item::Table(Table::new()));
        }
        let section = document
            .as_table_mut()
            .get_mut(&self.key)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| {
                format!(
                    "refusing to modify {}: '{}' is not a TOML table",
                    path.display(),
                    self.key
                )
            })?;
        if section.contains_key(name) {
            return Err(format!(
                "refusing to restore {}: '{}.{}' already exists",
                path.display(),
                self.key,
                name
            ));
        }
        let fields = snapshot
            .as_object()
            .expect("snapshot object checked above")
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        section.insert(name, Item::Table(Self::fields_table(fields)?));
        self.write_document(path, &document, original.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{McpConfig, StdioConfig};

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("arca-toml-{}-{}.toml", name, std::process::id()));
        d
    }

    fn stdio(cmd: &str) -> McpConfig {
        McpConfig::Stdio(StdioConfig {
            command: cmd.into(),
            args: None,
            env: None,
            cwd: None,
        })
    }

    #[test]
    fn upsert_then_read_roundtrips() {
        let p = tmp("rt");
        let adapter = TomlAdapter::new("mcp_servers");
        adapter.upsert(&p, "github", &stdio("npx")).unwrap();
        assert!(adapter.read(&p).contains_key("github"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn upsert_and_remove_preserve_other_servers() {
        let p = tmp("preserve");
        let adapter = TomlAdapter::new("mcp_servers");
        adapter.upsert(&p, "a", &stdio("x")).unwrap();
        adapter.upsert(&p, "b", &stdio("y")).unwrap();
        adapter.remove(&p, &["a".to_string()]).unwrap();
        let back = adapter.read(&p);
        assert!(!back.contains_key("a"));
        assert!(back.contains_key("b"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn upsert_keeps_unmodelled_fields_on_other_servers() {
        let p = tmp("extra");
        std::fs::write(
            &p,
            "[mcp_servers.mine]\ncommand = \"my-tool\"\ncwd = \"/tmp\"\n",
        )
        .unwrap();
        TomlAdapter::new("mcp_servers")
            .upsert(&p, "github", &stdio("npx"))
            .unwrap();
        let root: Toml = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let servers = root.get("mcp_servers").unwrap().as_table().unwrap();
        assert!(servers.contains_key("github"));
        assert_eq!(servers["mine"]["cwd"].as_str(), Some("/tmp")); // preserved
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn upsert_only_edits_mcp_table_and_preserves_private_toml() {
        let p = tmp("private");
        let original = r#"# Private agent settings must remain byte-for-byte intact.
model = "gpt-private" # keep inline comment

[history]
persistence = "save-all"

# Existing user-managed MCP.
[mcp_servers.mine]
command = "my-tool"
cwd = "/tmp" # unmodelled field
"#;
        std::fs::write(&p, original).unwrap();

        TomlAdapter::new("mcp_servers")
            .upsert(&p, "github", &stdio("npx"))
            .unwrap();

        let written = std::fs::read_to_string(&p).unwrap();
        assert!(written.starts_with(original));
        assert!(written.contains("[mcp_servers.github]"));
        assert!(written.contains("command = \"npx\""));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn invalid_toml_is_never_overwritten() {
        let p = tmp("invalid");
        let original = "token = \"secret\"\n[mcp_servers.git\ncommand = \"broken\"\n";
        std::fs::write(&p, original).unwrap();

        let result = TomlAdapter::new("mcp_servers").upsert(&p, "github", &stdio("npx"));

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn non_table_mcp_section_is_never_replaced() {
        let p = tmp("wrong-section");
        let original = "token = \"secret\"\nmcp_servers = \"managed elsewhere\"\n";
        std::fs::write(&p, original).unwrap();

        let result = TomlAdapter::new("mcp_servers").upsert(&p, "github", &stdio("npx"));

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn remove_missing_file_is_noop() {
        let p = tmp("rm-noop");
        let _ = std::fs::remove_file(&p);
        let adapter = TomlAdapter::new("mcp_servers");
        adapter.remove(&p, &["a".to_string()]).unwrap();
        assert!(!p.exists());
    }

    #[test]
    fn snapshot_restore_preserves_agent_owned_fields() {
        let p = tmp("snapshot-restore");
        std::fs::write(
            &p,
            r#"model = "gpt-private"

[mcp_servers.git]
command = "npx"
args = ["-y", "git"]
enabled = false
timeout_sec = 120
allowed_tools = ["read"]

[mcp_servers.git.oauth]
client_id = "client"

[mcp_servers.sibling]
command = "keep"
"#,
        )
        .unwrap();
        let adapter = TomlAdapter::new("mcp_servers");

        let snapshot = adapter.snapshot(&p, "git").unwrap().unwrap();
        adapter.remove(&p, &["git".to_string()]).unwrap();
        adapter.restore(&p, "git", &snapshot).unwrap();

        let written: Toml = toml::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(written["model"].as_str(), Some("gpt-private"));
        assert_eq!(
            written["mcp_servers"]["sibling"]["command"].as_str(),
            Some("keep")
        );
        let restored = serde_json::to_value(&written["mcp_servers"]["git"]).unwrap();
        assert_eq!(restored, snapshot);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn snapshot_removal_refuses_a_changed_target() {
        let p = tmp("snapshot-changed");
        let adapter = TomlAdapter::new("mcp_servers");
        std::fs::write(&p, "[mcp_servers.git]\ncommand = \"npx\"\ntimeout = 10\n").unwrap();
        let snapshot = adapter.snapshot(&p, "git").unwrap().unwrap();
        let newer = "[mcp_servers.git]\ncommand = \"npx\"\ntimeout = 20\n";
        std::fs::write(&p, newer).unwrap();

        let result = adapter.remove_snapshot(&p, "git", &snapshot);

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), newer);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn removing_absent_server_does_not_rewrite_file() {
        let p = tmp("rm-absent");
        let original =
            "# keep formatting\nmodel = 'private'\n\n[mcp_servers.mine]\ncommand = 'x'\n";
        std::fs::write(&p, original).unwrap();

        TomlAdapter::new("mcp_servers")
            .remove(&p, &["missing".to_string()])
            .unwrap();

        assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
        let _ = std::fs::remove_file(&p);
    }
}
