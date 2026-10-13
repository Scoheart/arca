//! Disable/enable is a destructive-looking workflow, so exercise its durability
//! contract end to end against a real global Agent file.

use arca_core::disabled::{load_disabled, remember, DisabledEntry};
use arca_core::ops;
use arca_core::testenv::TestHome;
use arca_core::types::{McpConfig, StdioConfig};
use serde_json::Value;
use std::collections::HashMap;

#[test]
fn workbuddy_native_pause_keeps_live_config_and_observes_external_disabled_state() {
    let home = TestHome::new("workbuddy-native-pause");
    let path = home.home.join(".workbuddy-ai/mcp.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = serde_json::json!({
        "userPolicy": {"keep": true},
        "mcpServers": {
            "context7": {"url": "https://mcp.context7.com/mcp", "headers": {"X-Test": "fixture"}, "disabled": true, "timeout": 60, "disabledTools": ["publish"]},
            "sibling": {"command": "unchanged"}
        }
    });
    std::fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(&path).unwrap().ino()
    };
    let observed = ops::scan_installed(None).into_iter().find(|row| row.agent == "workbuddy" && row.name == "context7").unwrap();
    assert!(!observed.enabled);
    assert!(!observed.file_path.is_empty());
    assert!(!load_disabled().contains_key("workbuddy"));
    for enabled in [true, false, false, true] {
        if enabled {
            ops::enable("context7", "http", "global", &["workbuddy".into()], None).unwrap();
        } else {
            ops::disable("context7", "http", "global", &["workbuddy".into()], None).unwrap();
        }
        let actual: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut expected = original.clone();
        expected["mcpServers"]["context7"]["disabled"] = Value::Bool(!enabled);
        assert_eq!(actual, expected);
        assert!(!load_disabled().contains_key("workbuddy"));
        assert_eq!(ops::scan_installed(None).into_iter().find(|row| row.agent == "workbuddy" && row.name == "context7").unwrap().enabled, enabled);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
    }
    ops::disable("context7", "http", "global", &["workbuddy".into()], None).unwrap();
    ops::delete("context7", "http", "global", &["workbuddy".into()], None).unwrap();
    let actual: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(actual["mcpServers"].get("context7").is_none());
    assert_eq!(actual["mcpServers"]["sibling"], original["mcpServers"]["sibling"]);
}

#[test]
fn workbuddy_restores_legacy_disabled_snapshot_then_uses_native_pause() {
    let home = TestHome::new("workbuddy-legacy-pause");
    let config = McpConfig::Stdio(StdioConfig {
        command: "example".into(), args: None, env: None, cwd: None,
    });
    remember("workbuddy", DisabledEntry {
        name: "srv".into(), transport: "stdio".into(), scope: "global".into(), config,
        snapshot: Some(serde_json::json!({"command":"example", "timeout":42})),
    }).unwrap();
    ops::enable("srv", "stdio", "global", &["workbuddy".into()], None).unwrap();
    ops::disable("srv", "stdio", "global", &["workbuddy".into()], None).unwrap();
    let value: Value = serde_json::from_str(&std::fs::read_to_string(home.home.join(".workbuddy-ai/mcp.json")).unwrap()).unwrap();
    assert_eq!(value["mcpServers"]["srv"], serde_json::json!({"command":"example", "timeout":42, "disabled":true}));
    assert!(!load_disabled().contains_key("workbuddy"));
}

#[test]
fn workbuddy_native_toggle_refuses_ambiguous_flags_and_stale_snapshots() {
    let home = TestHome::new("workbuddy-native-refusal");
    let path = home.home.join(".workbuddy-ai/mcp.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    for invalid in [
        r#"{"mcpServers":{"docs":{"url":"https://example.test/mcp","disabled":"true"}}}"#,
        r#"{"mcpServers":{"docs":{"url":"https://example.test/mcp","disabled":true,"disabled":false}}}"#,
        r#"{"mcpServers":[]}"#,
    ] {
        std::fs::write(&path, invalid).unwrap();
        assert!(ops::enable("docs", "http", "global", &["workbuddy".into()], None).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    }
    let original = r#"{"mcpServers":{"docs":{"url":"https://example.test/mcp","disabled":true}}}"#;
    std::fs::write(&path, original).unwrap();
    let definition = &arca_core::agents::builtin_agents()["workbuddy"];
    let adapter = arca_core::adapter::get_agent_adapter_for(definition, "workbuddy");
    let snapshot = adapter.snapshot(&path, "docs").unwrap().unwrap();
    let changed = original.replace("example.test", "changed.test");
    std::fs::write(&path, &changed).unwrap();
    assert!(adapter.set_enabled(&path, "docs", true, &snapshot).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);
}

fn claude_config() -> &'static str {
    r#"{
  "account": {"token": "private", "theme": "dark"},
  "mcpServers": {
    "srv": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "srv"],
      "env": {"TOKEN": "secret"},
      "enabled": false,
      "timeout": 120,
      "oauth": {"clientId": "client"},
      "allowedTools": ["read"]
    },
    "sibling": {"command": "keep", "approval": "always"}
  }
}
"#
}

#[test]
fn disable_enable_restores_complete_target_entry() {
    let th = TestHome::new("disable-restore");
    let path = th.home.join(".claude.json");
    std::fs::write(&path, claude_config()).unwrap();
    let before: Value = serde_json::from_str(claude_config()).unwrap();

    ops::disable("srv", "stdio", "global", &["claude-code".into()], None).unwrap();

    let disabled_file: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(disabled_file["mcpServers"].get("srv").is_none());
    assert_eq!(disabled_file["account"], before["account"]);
    assert_eq!(
        disabled_file["mcpServers"]["sibling"],
        before["mcpServers"]["sibling"]
    );
    let snapshot = load_disabled()["claude-code"][0]
        .snapshot
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(snapshot, before["mcpServers"]["srv"]);

    ops::enable("srv", "stdio", "global", &["claude-code".into()], None).unwrap();

    let restored: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(restored, before);
    assert!(!load_disabled().contains_key("claude-code"));
}

#[test]
fn disable_never_removes_entry_when_snapshot_cannot_be_saved() {
    let th = TestHome::new("disable-save-failure");
    let agent_path = th.home.join(".claude.json");
    std::fs::write(&agent_path, claude_config()).unwrap();
    let settings_path = th.home.join(".arca/settings.json");
    std::fs::create_dir_all(&settings_path).unwrap();

    let result = ops::disable("srv", "stdio", "global", &["claude-code".into()], None);

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(agent_path).unwrap(),
        claude_config()
    );
}

#[test]
fn unavailable_or_unknown_agents_fail_without_losing_snapshots() {
    let _home = TestHome::new("unavailable-agent");
    let config = McpConfig::Stdio(StdioConfig {
        command: "npx".into(),
        args: Some(vec!["-y".into(), "srv".into()]),
        env: None,
        cwd: None,
    });
    arca_core::registry::write_manual_entry(&arca_core::types::RegistryEntry {
        name: "srv".into(),
        description: String::new(),
        tags: Vec::new(),
        config: config.clone().into(),
        origin: None,
        repo: None,
    })
    .unwrap();

    let unavailable = ops::install(
        "srv",
        "stdio",
        "global",
        &["devin".into()],
        None,
        &HashMap::new(),
    )
    .unwrap_err();
    assert!(unavailable[0].contains("transport is not supported"));

    let unknown = ops::install(
        "srv",
        "stdio",
        "global",
        &["missing-agent".into()],
        None,
        &HashMap::new(),
    )
    .unwrap_err();
    assert!(unknown[0].contains("unknown Agent"));

    let saved = DisabledEntry {
        name: "srv".into(),
        transport: "stdio".into(),
        scope: "global".into(),
        config,
        snapshot: Some(serde_json::json!({"command": "npx"})),
    };
    remember("devin", saved.clone()).unwrap();
    let delete_error = ops::delete("srv", "stdio", "global", &["devin".into()], None).unwrap_err();
    assert!(delete_error[0].contains("snapshot retained"));
    assert_eq!(load_disabled()["devin"], vec![saved]);

    let clean = ops::clean(Some("missing-agent"));
    assert!(clean.cleaned.is_empty());
    assert!(clean.errors[0].contains("unknown Agent"));
}

#[test]
fn vt_code_enable_keeps_snapshot_when_root_mcp_switch_was_disabled() {
    let th = TestHome::new("vt-code-disabled-before-restore");
    let directory = th.home.join(".vtcode");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("vtcode.toml");
    std::fs::write(
        &path,
        r#"[mcp]
enabled = true

[[mcp.providers]]
name = "srv"
enabled = true
command = "npx"
args = ["-y", "srv"]
"#,
    )
    .unwrap();

    ops::disable("srv", "stdio", "global", &["vt-code".into()], None).unwrap();
    assert!(load_disabled().contains_key("vt-code"));

    let disabled_root = "[mcp]\nenabled = false\n";
    std::fs::write(&path, disabled_root).unwrap();
    let errors = ops::enable("srv", "stdio", "global", &["vt-code".into()], None).unwrap_err();

    assert!(errors.iter().any(|error| error.contains("mcp.enabled")));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), disabled_root);
    assert!(load_disabled().contains_key("vt-code"));
}
