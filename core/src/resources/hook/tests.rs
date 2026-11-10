#[cfg(test)]
mod tests {
    use crate::resources::hook::claude::{parse_claude_hooks, project_claude_hooks, remove_claude_hook};
    use crate::resources::hook::codex::{parse_codex_hooks, project_codex_hooks, remove_codex_hook};
    use crate::domain::hook::HookDefinition;

    fn sample_hook(name: &str) -> HookDefinition {
        HookDefinition {
            name: name.to_string(),
            display_name: "Test Hook".to_string(),
            description: "Test hook for verification".to_string(),
            event: "UserPromptSubmit".to_string(),
            matcher: "^test".to_string(),
            command: "echo 'hello from hook'".to_string(),
            command_windows: None,
            hook_type: "command".to_string(),
            timeout: 15,
            enabled: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn codex_project_and_parse_roundtrip() {
        let hook = sample_hook("test-codex-hook");
        let json = project_codex_hooks(None, &hook).expect("project codex");
        let parsed = parse_codex_hooks(&json).expect("parse codex");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].event, "UserPromptSubmit");
        assert_eq!(parsed[0].matcher, "^test");
        assert_eq!(parsed[0].command, "echo 'hello from hook'");
        assert_eq!(parsed[0].timeout, 15);
    }

    #[test]
    fn codex_remove_hook() {
        let hook = sample_hook("test-codex-hook");
        let json = project_codex_hooks(None, &hook).expect("project codex");
        let removed = remove_codex_hook(&json, "echo 'hello from hook'").expect("remove codex hook");
        let parsed = parse_codex_hooks(&removed).expect("parse codex after remove");
        assert_eq!(parsed.len(), 0);
    }

    #[test]
    fn claude_project_and_parse_roundtrip() {
        let initial_settings = r#"{"agentPushNotifEnabled":true,"language":"Chinese","hooks":{}}"#;
        let hook = sample_hook("test-claude-hook");
        let json = project_claude_hooks(Some(initial_settings), &hook).expect("project claude");
        
        // Ensure other settings preserved
        let root: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(root.get("language").and_then(|v| v.as_str()), Some("Chinese"));
        assert_eq!(root.get("agentPushNotifEnabled").and_then(|v| v.as_bool()), Some(true));

        let parsed = parse_claude_hooks(&json).expect("parse claude");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].event, "UserPromptSubmit");
        assert_eq!(parsed[0].command, "echo 'hello from hook'");
        assert_eq!(parsed[0].timeout, 15);
    }

    #[test]
    fn claude_remove_hook() {
        let hook = sample_hook("test-claude-hook");
        let json = project_claude_hooks(None, &hook).expect("project claude");
        let removed = remove_claude_hook(&json, "echo 'hello from hook'").expect("remove claude hook");
        let parsed = parse_claude_hooks(&removed).expect("parse claude after remove");
        assert_eq!(parsed.len(), 0);
    }
}
