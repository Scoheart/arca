//! MCP observations from Agent configuration files.

use crate::domain::types::{AgentDefinition, McpConfig};
use crate::resources::mcp::adapter::get_agent_adapter_for;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ScannedMcp {
    pub name: String,
    pub config: McpConfig,
    pub agent: String,
    pub scope: String, // "global" | "project"
    pub file_path: String,
}

pub fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return crate::brand::translate_legacy_path(Path::new(&home).join(rest));
        }
    }
    crate::brand::translate_legacy_path(PathBuf::from(p))
}

/// Collapse an absolute path under the user's home directory back to `~/…` so
/// stored agent paths stay portable (we never hardcode `/Users/<name>`). Paths
/// that already start with `~`, or that live outside home, are returned as-is.
/// The read-side inverse of [`expand_tilde`].
pub fn collapse_home(path: &str) -> String {
    let path = path.trim();
    if path.starts_with('~') {
        return path.to_string();
    }
    if let Some(home) = dirs::home_dir() {
        if let Some(home_str) = home.to_str() {
            if path == home_str {
                return "~".to_string();
            }
            if let Some(rest) = path.strip_prefix(&format!("{home_str}/")) {
                return format!("~/{rest}");
            }
        }
    }
    path.to_string()
}

pub fn scan_agents(
    agents: &BTreeMap<String, AgentDefinition>,
    project_dir: Option<&Path>,
    scan_all: bool,
) -> Vec<ScannedMcp> {
    scan_agents_with_enabled(agents, project_dir, scan_all).into_iter()
        .filter_map(|(entry, enabled)| enabled.then_some(entry)).collect()
}

pub fn scan_agents_with_enabled(
    agents: &BTreeMap<String, AgentDefinition>,
    project_dir: Option<&Path>,
    scan_all: bool,
) -> Vec<(ScannedMcp, bool)> {
    let mut out = Vec::new();
    for (name, def) in agents {
        if !scan_all && !def.enabled {
            continue;
        }
        if let Some(g) = &def.global {
            let path = expand_tilde(g);
            for (mcp_name, (cfg, enabled)) in get_agent_adapter_for(def, name).read_with_enabled(&path) {
                out.push((ScannedMcp {
                    name: mcp_name,
                    config: cfg,
                    agent: name.clone(),
                    scope: "global".into(),
                    file_path: path.display().to_string(),
                }, enabled));
            }
        }
        if let (Some(proj), Some(base)) = (&def.project, project_dir) {
            let path = base.join(proj);
            for (mcp_name, (cfg, enabled)) in get_agent_adapter_for(def, name).read_with_enabled(&path) {
                out.push((ScannedMcp {
                    name: mcp_name,
                    config: cfg,
                    agent: name.clone(),
                    scope: "project".into(),
                    file_path: path.display().to_string(),
                }, enabled));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::AgentDefinition;
    use crate::testenv::TestHome;

    #[test]
    fn scans_project_json_config() {
        let mut base = std::env::temp_dir();
        base.push(format!("arca-scan-project-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("mcp.json"),
            r#"{"mcpServers":{"git":{"command":"npx"}}}"#,
        )
        .unwrap();
        let mut agents = BTreeMap::new();
        agents.insert(
            "test".to_string(),
            AgentDefinition {
                global: None,
                project: Some("mcp.json".into()),
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: true,
                builtin: Some(true),
                ..Default::default()
            },
        );
        let found = scan_agents(&agents, Some(&base), false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "git");
        assert_eq!(found[0].scope, "project");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn skips_disabled_unless_scan_all() {
        let mut agents = BTreeMap::new();
        agents.insert(
            "off".to_string(),
            AgentDefinition {
                global: Some("~/nope.json".into()),
                project: None,
                format: "json".into(),
                key: "mcpServers".into(),
                enabled: false,
                builtin: None,
                ..Default::default()
            },
        );
        assert_eq!(scan_agents(&agents, None, false).len(), 0);
    }

    #[test]
    fn collapse_home_tilde_and_outside_paths_unchanged() {
        assert_eq!(
            collapse_home("~/Library/X/mcp.json"),
            "~/Library/X/mcp.json"
        );
        assert_eq!(collapse_home("/etc/elsewhere.json"), "/etc/elsewhere.json");
    }

    #[test]
    fn collapse_home_absolute_home_path_collapses_to_tilde() {
        let home = TestHome::new("collapse-home");
        let abs = format!("{}/Library/App Support/mcp.json", home.home.display());
        assert_eq!(collapse_home(&abs), "~/Library/App Support/mcp.json");
    }
}
