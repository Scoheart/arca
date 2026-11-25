//! Effective MCP asset resolution.

use crate::domain::types::{McpConfig, RegistryEntry};
use crate::resources::mcp::r#override::{apply_override, OverridePatch};

/// 选定 registry 条目的基础配置：优先 stdio，其次 http
pub fn base_config(entry: &RegistryEntry) -> Option<McpConfig> {
    if let Some(s) = &entry.config.stdio {
        return Some(McpConfig::Stdio(s.clone()));
    }
    if let Some(h) = &entry.config.http {
        return Some(McpConfig::Http(h.clone()));
    }
    None
}

/// 最终配置 = base ⊕ patch
pub fn effective_config(entry: &RegistryEntry, patch: Option<&OverridePatch>) -> Option<McpConfig> {
    let base = base_config(entry)?;
    Some(match patch {
        Some(p) => apply_override(&base, p),
        None => base,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{RegistryConfig, StdioConfig};
    fn entry() -> RegistryEntry {
        RegistryEntry {
            name: "git".into(),
            description: "d".into(),
            tags: vec![],
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "npx".into(),
                    args: Some(vec!["-y".into()]),
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
            source_url: None,
        }
    }
    #[test]
    fn no_patch_returns_base() {
        let c = effective_config(&entry(), None).unwrap();
        match c {
            McpConfig::Stdio(s) => assert_eq!(s.command, "npx"),
            _ => panic!(),
        }
    }
    #[test]
    fn patch_applies() {
        let patch = OverridePatch {
            args: Some(vec!["-x".into()]),
            ..Default::default()
        };
        let c = effective_config(&entry(), Some(&patch)).unwrap();
        match c {
            McpConfig::Stdio(s) => assert_eq!(s.args.unwrap(), vec!["-x"]),
            _ => panic!(),
        }
    }
}
