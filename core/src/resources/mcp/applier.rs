//! MCP configuration application.

use crate::domain::types::{AgentDefinition, McpConfig};
use crate::resources::mcp::adapter::get_agent_adapter_for;
use crate::resources::mcp::differ::{DiffAction, DiffEntry};
use crate::resources::mcp::scanner::expand_tilde;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// A single failed apply target, collected so one failure does not abort the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyError {
    pub target: String,
    pub error: String,
}

fn target_path(def: &AgentDefinition, scope: &str, project_dir: Option<&Path>) -> Option<PathBuf> {
    if scope == "global" {
        def.global.as_ref().map(|g| expand_tilde(g))
    } else {
        match (&def.project, project_dir) {
            (Some(p), Some(base)) => Some(base.join(p)),
            _ => None,
        }
    }
}

fn backup_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub(crate) fn backup(
    path: &Path,
    backups_dir: &Path,
    stamp: &str,
    agent: &str,
    scope: &str,
) -> Result<(), String> {
    if crate::safe_write::has_private_transaction_snapshot(path) {
        return Ok(());
    }
    if agent == "kilo-desktop" && path.exists() {
        return Err("kilo_private_transaction_required: edit Kilo Desktop config through a reviewed asset operation".into());
    }
    if matches!(agent, "qoder-desktop" | "qoder-cli") && path.exists() {
        let content = fs::read_to_string(path).map_err(|_| "qoder_config_unreadable".to_string())?;
        let root = jsonc_parser::cst::CstRootNode::parse(&content, &jsonc_parser::ParseOptions::default())
            .map_err(|_| "qoder_config_unparseable".to_string())?;
        if root.object_value().is_some_and(|root| root.get("providers").is_some()) {
            return Err("qoder_private_transaction_required: edit shared Qoder settings through a reviewed asset operation".into());
        }
    }
    if !path.exists() {
        return Ok(());
    }
    fs::create_dir_all(backups_dir)
        .map_err(|error| format!("failed to create backup directory: {error}"))?;
    #[cfg(unix)]
    fs::set_permissions(backups_dir, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("failed to secure backup directory: {error}"))?;
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let base_name = format!(
        "{}-{}-{}-{}",
        fname,
        stamp,
        backup_component(agent),
        backup_component(scope)
    );
    #[cfg(unix)]
    let permissions = fs::Permissions::from_mode(0o600);
    #[cfg(not(unix))]
    let permissions = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();

    for suffix in 0_u32.. {
        let name = if suffix == 0 {
            base_name.clone()
        } else {
            format!("{base_name}-{suffix}")
        };
        let backup_path = backups_dir.join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut destination = match options.open(&backup_path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "failed to create backup {}: {}",
                    backup_path.display(),
                    error
                ));
            }
        };
        if let Err(error) = destination.set_permissions(permissions.clone()) {
            let _ = fs::remove_file(&backup_path);
            return Err(format!(
                "failed to preserve permissions on {}: {}",
                backup_path.display(),
                error
            ));
        }
        let copy_result = fs::File::open(path)
            .and_then(|mut source| std::io::copy(&mut source, &mut destination))
            .and_then(|_| destination.sync_all());
        return match copy_result {
            Ok(_) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(&backup_path);
                Err(format!(
                    "failed to back up {} to {}: {}",
                    path.display(),
                    backup_path.display(),
                    error
                ))
            }
        };
    }
    unreachable!("u32 backup suffix space exhausted")
}

/// Persist bytes already captured from an anchored, reviewed source. This is
/// used when reopening the source path would reintroduce a target-swap race.
pub(crate) fn backup_bytes(
    path: &Path,
    bytes: &[u8],
    backups_dir: &Path,
    stamp: &str,
    agent: &str,
    scope: &str,
) -> Result<(), String> {
    if crate::safe_write::has_private_transaction_snapshot(path) {
        return Ok(());
    }
    if agent == "kilo-desktop" {
        return Err("kilo_private_transaction_required: edit Kilo Desktop config through a reviewed asset operation".into());
    }
    fs::create_dir_all(backups_dir)
        .map_err(|error| format!("failed to create backup directory: {error}"))?;
    #[cfg(unix)]
    fs::set_permissions(backups_dir, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("failed to secure backup directory: {error}"))?;
    let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let base_name = format!(
        "{}-{}-{}-{}",
        fname,
        stamp,
        backup_component(agent),
        backup_component(scope)
    );

    for suffix in 0_u32.. {
        let name = if suffix == 0 {
            base_name.clone()
        } else {
            format!("{base_name}-{suffix}")
        };
        let backup_path = backups_dir.join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut destination = match options.open(&backup_path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "failed to create backup {}: {}",
                    backup_path.display(),
                    error
                ));
            }
        };
        let result = destination
            .write_all(bytes)
            .and_then(|_| destination.sync_all());
        return match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(&backup_path);
                Err(format!(
                    "failed to back up reviewed bytes to {}: {}",
                    backup_path.display(),
                    error
                ))
            }
        };
    }
    unreachable!("u32 backup suffix space exhausted")
}

/// Restore one complete MCP entry captured before disable. The write receives
/// the same backup and fail-closed treatment as normal installs, but restores
/// Agent-owned fields in addition to Arca's modeled connection fields.
pub fn restore_snapshot(
    agent_id: &str,
    def: &AgentDefinition,
    server_name: &str,
    snapshot: &Value,
    scope: &str,
    project_dir: Option<&Path>,
    timestamp: &str,
) -> Result<(), Vec<ApplyError>> {
    let Some(path) = target_path(def, scope, project_dir) else {
        return Err(vec![ApplyError {
            target: agent_id.to_string(),
            error: "target config path is unavailable".into(),
        }]);
    };
    let adapter = get_agent_adapter_for(def, agent_id);
    if path.exists() {
        if let Err(error) = adapter.snapshot(&path, server_name) {
            return Err(vec![ApplyError {
                target: path.display().to_string(),
                error,
            }]);
        }
    }
    if path.exists() {
        backup(
            &path,
            &crate::paths::backups_dir(),
            timestamp,
            agent_id,
            scope,
        )
        .map_err(|error| {
            vec![ApplyError {
                target: path.display().to_string(),
                error,
            }]
        })?;
    }
    adapter
        .restore(&path, server_name, snapshot)
        .map_err(|error| {
            vec![ApplyError {
                target: path.display().to_string(),
                error,
            }]
        })
}

/// Remove one live entry only if it still matches the snapshot already saved by
/// the disable flow. A policy change in the gap is preserved and reported.
pub fn remove_snapshot(
    agent_id: &str,
    def: &AgentDefinition,
    server_name: &str,
    snapshot: &Value,
    scope: &str,
    project_dir: Option<&Path>,
    timestamp: &str,
) -> Result<(), Vec<ApplyError>> {
    let Some(path) = target_path(def, scope, project_dir) else {
        return Err(vec![ApplyError {
            target: agent_id.to_string(),
            error: "target config path is unavailable".into(),
        }]);
    };
    let adapter = get_agent_adapter_for(def, agent_id);
    if path.exists() {
        if let Err(error) = adapter.snapshot(&path, server_name) {
            return Err(vec![ApplyError {
                target: path.display().to_string(),
                error,
            }]);
        }
    }
    if path.exists() {
        backup(
            &path,
            &crate::paths::backups_dir(),
            timestamp,
            agent_id,
            scope,
        )
        .map_err(|error| {
            vec![ApplyError {
                target: path.display().to_string(),
                error,
            }]
        })?;
    }
    adapter
        .remove_snapshot(&path, server_name, snapshot)
        .map_err(|error| {
            vec![ApplyError {
                target: path.display().to_string(),
                error,
            }]
        })
}

/// Apply diffs to their target files. A single failing target is recorded and
/// does not abort the remaining work; `Ok(())` is returned only if all targets
/// succeed, otherwise `Err` carries every failure (spec §5).
///
/// `configs`: (mcp_name -> already-computed effective config) for Add diffs.
pub fn apply_diffs(
    diffs: &[DiffEntry],
    agents: &BTreeMap<String, AgentDefinition>,
    configs: &BTreeMap<String, McpConfig>,
    backups_dir: &Path,
    project_dir: Option<&Path>,
    timestamp: &str,
) -> Result<(), Vec<ApplyError>> {
    let mut backed_up = std::collections::HashSet::new();
    let mut errors: Vec<ApplyError> = Vec::new();
    for diff in diffs {
        let Some(def) = agents.get(&diff.agent) else {
            continue;
        };
        let Some(path) = target_path(def, &diff.scope, project_dir) else {
            continue;
        };
        let adapter = get_agent_adapter_for(def, &diff.agent);
        if path.exists() {
            if let Err(error) = adapter.snapshot(&path, &diff.mcp_name) {
                errors.push(ApplyError {
                    target: path.display().to_string(),
                    error,
                });
                continue;
            }
        }
        if !backed_up.contains(&path) && path.exists() {
            if let Err(error) = backup(&path, backups_dir, timestamp, &diff.agent, &diff.scope) {
                errors.push(ApplyError {
                    target: path.display().to_string(),
                    error,
                });
                continue;
            }
            backed_up.insert(path.clone());
        }
        let result = match diff.action {
            DiffAction::Add => {
                let Some(cfg) = configs.get(&diff.mcp_name) else {
                    continue;
                };
                // Single-server upsert: never re-serialize the user's other servers.
                adapter.upsert(&path, &diff.mcp_name, cfg)
            }
            DiffAction::Remove => {
                let names = vec![diff.mcp_name.clone()];
                adapter.remove(&path, &names)
            }
        };
        if let Err(e) = result {
            errors.push(ApplyError {
                target: path.display().to_string(),
                error: e,
            });
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{AgentDefinition, StdioConfig};

    #[test]
    fn applies_add_and_creates_backup() {
        let mut base = std::env::temp_dir();
        base.push(format!("arca-apply-add-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let cfg_path = base.join("mcp.json");
        std::fs::write(&cfg_path, r#"{"mcpServers":{"old":{"command":"x"}}}"#).unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(
            "test".to_string(),
            AgentDefinition {
                global: None,
                project: Some("mcp.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                builtin: None,
                ..Default::default()
            },
        );

        let mut configs = BTreeMap::new();
        configs.insert(
            "git".to_string(),
            McpConfig::Stdio(StdioConfig {
                command: "npx".into(),
                args: None,
                env: None,
                cwd: None,
            }),
        );

        let diffs = vec![DiffEntry {
            action: DiffAction::Add,
            mcp_name: "git".into(),
            agent: "test".into(),
            scope: "project".into(),
        }];
        let backups = base.join("backups");
        let res = apply_diffs(&diffs, &agents, &configs, &backups, Some(&base), "STAMP");

        assert!(res.is_ok());
        let written = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(written.contains("git"));
        assert!(backups.join("mcp.json-STAMP-test-project").exists());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn apply_only_changes_target_mcp_entry() {
        let mut base = std::env::temp_dir();
        base.push(format!("arca-apply-private-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let cfg_path = base.join("agent.json");
        let original = r#"{
  // Private agent data is outside Arca ownership.
  "account": { "token" : "secret", "history": true },
  "mcpServers": {
    "existing": { "command" : "keep", "cwd": "/tmp" },
  },
  "theme": "dark"
}
"#;
        std::fs::write(&cfg_path, original).unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(
            "test".to_string(),
            AgentDefinition {
                global: None,
                project: Some("agent.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                builtin: None,
                ..Default::default()
            },
        );
        let mut configs = BTreeMap::new();
        configs.insert(
            "git".to_string(),
            McpConfig::Stdio(StdioConfig {
                command: "npx".into(),
                args: None,
                env: None,
                cwd: None,
            }),
        );
        let diffs = vec![DiffEntry {
            action: DiffAction::Add,
            mcp_name: "git".into(),
            agent: "test".into(),
            scope: "project".into(),
        }];
        let backups = base.join("backups");

        apply_diffs(&diffs, &agents, &configs, &backups, Some(&base), "STAMP").unwrap();

        let written = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(written.contains(
            "// Private agent data is outside Arca ownership.\n  \"account\": { \"token\" : \"secret\", \"history\": true },"
        ));
        assert!(written.contains("\"existing\": { \"command\" : \"keep\", \"cwd\": \"/tmp\" },"));
        assert!(written.ends_with("  \"theme\": \"dark\"\n}\n"));
        assert_eq!(
            std::fs::read_to_string(backups.join("agent.json-STAMP-test-project")).unwrap(),
            original
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn apply_refuses_to_replace_invalid_config() {
        let mut base = std::env::temp_dir();
        base.push(format!("arca-apply-invalid-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let cfg_path = base.join("agent.json");
        let original = r#"{"account":{"token":"secret"},"mcpServers": "#;
        std::fs::write(&cfg_path, original).unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(
            "test".to_string(),
            AgentDefinition {
                global: None,
                project: Some("agent.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                builtin: None,
                ..Default::default()
            },
        );
        let mut configs = BTreeMap::new();
        configs.insert(
            "git".to_string(),
            McpConfig::Stdio(StdioConfig {
                command: "npx".into(),
                args: None,
                env: None,
                cwd: None,
            }),
        );
        let diffs = vec![DiffEntry {
            action: DiffAction::Add,
            mcp_name: "git".into(),
            agent: "test".into(),
            scope: "project".into(),
        }];

        let result = apply_diffs(
            &diffs,
            &agents,
            &configs,
            &base.join("backups"),
            Some(&base),
            "STAMP",
        );

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&cfg_path).unwrap(), original);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn apply_refuses_to_write_when_backup_fails() {
        let mut base = std::env::temp_dir();
        base.push(format!("arca-apply-backup-failure-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let cfg_path = base.join("agent.json");
        let original = r#"{"mcpServers":{"existing":{"command":"keep"}}}"#;
        std::fs::write(&cfg_path, original).unwrap();
        let backup_path = base.join("not-a-directory");
        std::fs::write(&backup_path, "occupied").unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(
            "test".to_string(),
            AgentDefinition {
                global: None,
                project: Some("agent.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                builtin: None,
                ..Default::default()
            },
        );
        let mut configs = BTreeMap::new();
        configs.insert(
            "git".to_string(),
            McpConfig::Stdio(StdioConfig {
                command: "npx".into(),
                args: None,
                env: None,
                cwd: None,
            }),
        );
        let diffs = vec![DiffEntry {
            action: DiffAction::Add,
            mcp_name: "git".into(),
            agent: "test".into(),
            scope: "project".into(),
        }];

        let result = apply_diffs(
            &diffs,
            &agents,
            &configs,
            &backup_path,
            Some(&base),
            "STAMP",
        );

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&cfg_path).unwrap(), original);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn chatmcp_credentials_are_rejected_before_whole_file_backup() {
        let mut base = std::env::temp_dir();
        base.push(format!(
            "arca-chatmcp-credential-preflight-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let cfg_path = base.join("mcp_server.json");
        let original = r#"{"mcpServers":{"private":{"type":"streamable","command":"https://private.example/mcp","oauth":{"accessToken":"fixture"}}}}"#;
        std::fs::write(&cfg_path, original).unwrap();

        let mut agents = BTreeMap::new();
        agents.insert(
            "chatmcp".to_string(),
            AgentDefinition {
                global: None,
                project: Some("mcp_server.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                codec: Some("chatmcp".into()),
                enabled: true,
                builtin: Some(true),
                ..Default::default()
            },
        );
        let mut configs = BTreeMap::new();
        configs.insert(
            "docs".to_string(),
            McpConfig::Stdio(StdioConfig {
                command: "npx".into(),
                args: None,
                env: None,
                cwd: None,
            }),
        );
        let diffs = vec![DiffEntry {
            action: DiffAction::Add,
            mcp_name: "docs".into(),
            agent: "chatmcp".into(),
            scope: "project".into(),
        }];
        let backups = base.join("backups");

        let errors =
            apply_diffs(&diffs, &agents, &configs, &backups, Some(&base), "STAMP").unwrap_err();

        assert!(errors[0].error.contains("external-managed"));
        assert_eq!(std::fs::read_to_string(&cfg_path).unwrap(), original);
        assert!(!backups.exists());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn backups_never_overwrite_and_agent_ids_are_filename_safe() {
        let mut base = std::env::temp_dir();
        base.push(format!("arca-backup-collision-{}", std::process::id()));
        let backups = base.join("backups");
        std::fs::create_dir_all(&base).unwrap();
        let source = base.join("settings.json");
        std::fs::write(&source, "first").unwrap();
        backup(&source, &backups, "STAMP", "../../custom agent", "global").unwrap();
        std::fs::write(&source, "second").unwrap();
        backup(&source, &backups, "STAMP", "../../custom agent", "global").unwrap();

        let prefix = "settings.json-STAMP-%2E%2E%2F%2E%2E%2Fcustom%20agent-global";
        assert_eq!(
            std::fs::read_to_string(backups.join(prefix)).unwrap(),
            "first"
        );
        assert_eq!(
            std::fs::read_to_string(backups.join(format!("{prefix}-1"))).unwrap(),
            "second"
        );
        assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 2);
        #[cfg(unix)]
        {
            assert_eq!(
                std::fs::metadata(&backups).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(backups.join(prefix))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(&base).ok();
    }
}
