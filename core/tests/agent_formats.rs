use arca_core::adapter::{get_agent_adapter, get_agent_adapter_for};
use arca_core::agents::builtin_agents;
use arca_core::codec::{from_name, normalize_with_codec};
use arca_core::types::{AgentDefinition, HttpConfig, McpConfig, StdioConfig};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP_FILE: AtomicUsize = AtomicUsize::new(0);

fn temp_file(name: &str, extension: &str) -> PathBuf {
    let token = format!(
        "{}-{}",
        std::process::id(),
        NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
    );
    std::fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(format!("arca-agent-format-{name}-{token}.{extension}"))
}

fn fixture(name: &str) -> &'static str {
    match name {
        "mimo-code" => include_str!("fixtures/mimo-code.jsonc"),
        "jcode" => include_str!("fixtures/jcode.json"),
        "jan-desktop" | "jan-cli" => include_str!("fixtures/jan.json"),
        "anythingllm" => include_str!("fixtures/anythingllm.json"),
        "ibm-bob" => include_str!("fixtures/ibm-bob.json"),
        "kiro-cli" | "junie-cli" => include_str!("fixtures/shared-surfaces.json"),
        "goose-desktop" => include_str!("fixtures/goose-desktop.yaml"),
        "cline-cli" => include_str!("fixtures/cline.json"),
        "codex-ide" => include_str!("fixtures/codex.toml"),
        "kilo-vscode" => include_str!("fixtures/opencode.json"),
        "kilo-desktop" => include_str!("fixtures/kilo-desktop.jsonc"),
        "kimi-code-desktop" => include_str!("fixtures/kimi-code-desktop.json"),
        "workbuddy" => include_str!("fixtures/workbuddy.json"),
        "zcode" => include_str!("fixtures/zcode.json"),
        "antigravity-cli" => include_str!("fixtures/antigravity-cli.json"),
        "continue-cli" => include_str!("fixtures/continue-cli.yaml"),
        "trae-cli" => include_str!("fixtures/trae-cli.toml"),
        "qoder-desktop" => include_str!("fixtures/qoder-desktop.json"),
        "qwenwork" => include_str!("fixtures/qwenwork.json"),
        "step-code" => include_str!("fixtures/step-code.toml"),
        "opencode" => include_str!("fixtures/opencode.json"),
        "codex" => include_str!("fixtures/codex.toml"),
        "gemini" => include_str!("fixtures/gemini.json"),
        "grok-build" => include_str!("fixtures/grok-build.toml"),
        "minimax-code" => include_str!("fixtures/minimax-code.json"),
        "windsurf" => include_str!("fixtures/windsurf.json"),
        "cline" => include_str!("fixtures/cline.json"),
        _ => panic!("unknown fixture"),
    }
}

#[test]
fn newly_audited_surfaces_preserve_non_connection_state_and_reject_unsafe_inputs() {
    let home = arca_core::testenv::TestHome::new("agent-gap-formats");
    let agents = builtin_agents();
    for id in ["cline-cli", "kiro-cli", "junie-cli", "goose-desktop", "kilo-vscode", "kilo-desktop",
        "codex-ide", "mimo-code", "jcode", "jan-cli", "jan-desktop", "anythingllm", "ibm-bob"] {
        let definition = &agents[id];
        let path = home.home.join(format!("{id}.{}", definition.format));
        let original = fixture(id);
        std::fs::write(&path, original).unwrap();
        let adapter = get_agent_adapter_for(definition, id);
        let codec = from_name(definition.codec.as_deref(), id);
        let initial = adapter.read(&path);
        let (local_name, remote_name) = match id {
            "cline-cli" => ("shared-local", "shared-http"),
            "kilo-vscode" | "kilo-desktop" => ("local-tools", "remote-tools"),
            "codex-ide" => ("local", "figma"),
            _ => ("local", "remote"),
        };
        assert!(initial.contains_key(local_name), "{id}");
        let local = McpConfig::Stdio(StdioConfig {
            command: "bunx".into(), args: Some(vec!["updated-server".into()]),
            env: Some(HashMap::from([("MODE".into(), "updated".into())])), cwd: None,
        });
        adapter.upsert(&path, local_name, &local).unwrap();
        assert_eq!(adapter.read(&path)[local_name], normalize_with_codec(codec, &local), "{id}");
        if id != "jcode" {
            let remote = http("https://updated.example.test/mcp");
            adapter.upsert(&path, remote_name, &remote).unwrap();
            assert_eq!(adapter.read(&path)[remote_name], normalize_with_codec(codec, &remote), "{id}");
        } else {
            let before = std::fs::read(&path).unwrap();
            assert!(adapter.upsert(&path, "remote", &http("https://unsupported.test/mcp")).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        let after = std::fs::read_to_string(&path).unwrap();
        for preserved in match id {
            "mimo-code" => vec!["// Model and permissions belong to MiMoCode.", "fixture-provider/fixture-model", "\"sampling\": \"ask\"", "\"oauth\": false"],
            "jcode" => vec!["\"shared\": false", "\"timeout_secs\": 90"],
            "jan-cli" | "jan-desktop" => vec!["\"active\": false", "\"enableSmartToolRouting\": true"],
            "anythingllm" => vec!["\"autoStart\": true", "\"suppressedTools\"", "\"delete\""],
            "ibm-bob" => vec!["\"alwaysAllow\"", "\"timeout\": 90"],
            "goose-desktop" => vec!["# Desktop and CLI share this file on macOS.", "active_provider: fixture-provider", "Keep this description"],
            "kilo-desktop" => vec!["// Desktop-owned providers and permissions", "\"desktop-local\"", "\"desktop-webmcp\"", "\"suggest\": \"deny\"", "\"oauth\": false"],
            _ => Vec::new(),
        } {
            assert!(after.contains(preserved), "{id}: missing {preserved}");
        }
        adapter.remove(&path, &[local_name.into()]).unwrap();
        assert!(!adapter.read(&path).contains_key(local_name), "{id}");
        let malformed = match definition.format.as_str() {
            "yaml" => "extensions: [\n",
            "toml" => "[mcp_servers\n",
            _ => "{",
        };
        std::fs::write(&path, malformed).unwrap();
        assert!(adapter.upsert(&path, "local", &local).is_err(), "{id}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), malformed, "{id}");
    }
}

#[test]
fn new_native_switch_and_shared_format_guards_fail_closed() {
    let home = arca_core::testenv::TestHome::new("agent-gap-policy");
    let agents = builtin_agents();
    let jan_path = home.home.join("jan.json");
    std::fs::write(&jan_path, fixture("jan-cli")).unwrap();
    let jan = get_agent_adapter_for(&agents["jan-cli"], "jan-cli");
    assert!(jan.supports_native_enabled());
    assert!(!jan.read_with_enabled(&jan_path)["paused"].1);
    let snapshot = jan.snapshot(&jan_path, "paused").unwrap().unwrap();
    jan.set_enabled(&jan_path, "paused", true, &snapshot).unwrap();
    assert!(jan.read_with_enabled(&jan_path)["paused"].1);
    assert!(jan.set_enabled(&jan_path, "paused", false, &snapshot).is_err());

    let local = McpConfig::Stdio(StdioConfig { command: "node".into(), args: None, env: None, cwd: None });
    for (id, invalid) in [
        ("jcode", r#"{"mcpServers":{"local":{"command":"original"}}}"#),
        ("jcode", r#"{"servers":{"local":{"command":"original","enabled":false}}}"#),
        ("mimo-code", r#"{"mcp":{"local":{"type":"local","command":["original"],"enabled":false}}}"#),
        ("jan-cli", r#"{"mcpServers":{"local":{"command":"original","active":"true"}}}"#),
        ("anythingllm", r#"{"mcpServers":{"local":{"command":"original","anythingllm":{"autoStart":false}}}}"#),
        ("ibm-bob", r#"{"mcpServers":{"shell":{"httpURL":"https://shell.test/mcp"}}}"#),
    ] {
        let path = home.home.join(format!("{id}-policy.json"));
        std::fs::write(&path, invalid).unwrap();
        let adapter = get_agent_adapter_for(&agents[id], id);
        assert!(adapter.upsert(&path, "local", &local).is_err(), "{id}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid, "{id}");
    }
    for id in ["mimo-code", "jcode"] {
        let adapter = get_agent_adapter_for(&agents[id], id);
        let path = home.home.join(format!("{id}-cwd.json"));
        let mut unsupported = local.clone();
        if let McpConfig::Stdio(config) = &mut unsupported { config.cwd = Some("/unsupported".into()); }
        assert!(adapter.upsert(&path, "local", &unsupported).is_err());
        assert!(!path.exists());
    }
    for (id, sibling) in [("kiro-cli", "kiro"), ("junie-cli", "junie"),
        ("cline-cli", "cline"), ("goose-desktop", "goose"), ("kilo-vscode", "kilo-code"),
        ("codex-ide", "codex"), ("jan-cli", "jan-desktop")] {
        assert_eq!(agents[id].global, agents[sibling].global);
        assert_eq!(agents[id].skills.as_ref().unwrap().target_id, agents[sibling].skills.as_ref().unwrap().target_id);
    }
    assert!(agents["deepseek-harness"].global.is_none());
    assert_eq!(agents["deepseek-harness"].skills.as_ref().unwrap().global_dir, "~/.dsh/skills");
}

#[test]
fn workbuddy_editions_keep_independent_paths_launchers_and_native_pause_state() {
    let home = arca_core::testenv::TestHome::new("workbuddy-editions");
    let agents = builtin_agents();
    let launchers: Value = serde_json::from_str(include_str!("../../data/agent-launchers.json")).unwrap();
    let mut paths = Vec::new();
    for (id, directory, launcher, domain, target_id) in [
        ("workbuddy", ".workbuddy-ai", "WorkBuddy AI", "www.workbuddy.ai", "workbuddy-ai-user"),
        ("workbuddy-cn", ".workbuddy", "WorkBuddy", "www.workbuddy.cn", "workbuddy-cn-user"),
    ] {
        let agent = &agents[id];
        assert_eq!(agent.global.as_deref(), Some(format!("~/{directory}/mcp.json").as_str()));
        assert!(agent.project.is_none());
        assert!(agent.docs.as_ref().unwrap().contains(domain));
        assert_eq!(agent.skills.as_ref().unwrap().target_id, target_id);
        assert_eq!(agent.skills.as_ref().unwrap().global_dir, format!("~/{directory}/skills"));
        assert!(agent.skills.as_ref().unwrap().aliases.is_empty());
        assert_eq!(launchers[id]["candidates"], serde_json::json!([launcher]));
        let path = home.home.join(directory).join("mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, fixture("workbuddy")).unwrap();
        paths.push(path);
    }
    let overseas_before = std::fs::read(&paths[0]).unwrap();
    let domestic = get_agent_adapter_for(&agents["workbuddy-cn"], "workbuddy-cn");
    let config = http("https://cn.example.test/mcp");
    domestic.upsert(&paths[1], "docs", &config).unwrap();
    assert_eq!(domestic.read(&paths[1])["docs"], normalize_with_codec(from_name(Some("workbuddy"), "workbuddy-cn"), &config));
    let snapshot = domestic.snapshot(&paths[1], "docs").unwrap().unwrap();
    domestic.set_enabled(&paths[1], "docs", false, &snapshot).unwrap();
    assert!(!domestic.read_with_enabled(&paths[1])["docs"].1);
    assert_eq!(std::fs::read(&paths[0]).unwrap(), overseas_before);
    let domestic_before = std::fs::read(&paths[1]).unwrap();
    let overseas = get_agent_adapter_for(&agents["workbuddy"], "workbuddy");
    overseas.remove(&paths[0], &["docs".into()]).unwrap();
    assert_eq!(std::fs::read(&paths[1]).unwrap(), domestic_before);
}

#[test]
fn workbuddy_roundtrip_preserves_policy_and_paused_state_and_rejects_invalid_entries() {
    let home = arca_core::testenv::TestHome::new("workbuddy-formats");
    let path = home.home.join("mcp.json");
    std::fs::write(&path, fixture("workbuddy")).unwrap();
    let agents = builtin_agents();
    let adapter = get_agent_adapter_for(&agents["workbuddy"], "workbuddy");
    let original: Value = serde_json::from_str(fixture("workbuddy")).unwrap();
    assert!(!adapter.read(&path).contains_key("paused"));
    assert!(matches!(&adapter.read(&path)["legacy"], McpConfig::Http(config) if config.kind == "sse"));
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(&path).unwrap().ino()
    };
    for kind in ["http", "streamable-http", "sse"] {
        let config = McpConfig::Http(HttpConfig {
            kind: kind.into(), url: "https://updated.example.test/mcp".into(),
            headers: Some(HashMap::from([("X-Test".into(), "value".into())])),
        });
        adapter.upsert(&path, "docs", &config).unwrap();
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["mcpServers"]["docs"]["type"].as_str(), (kind == "sse").then_some("sse"));
        assert_eq!(adapter.read(&path)["docs"], normalize_with_codec(from_name(Some("workbuddy"), "workbuddy"), &config));
        for field in ["timeout", "disabled", "disabledTools", "defer_loading", "description"] {
            assert_eq!(value["mcpServers"]["docs"][field], original["mcpServers"]["docs"][field]);
        }
    }
    let local = McpConfig::Stdio(StdioConfig {
        command: "node".into(), args: Some(vec!["updated.mjs".into()]),
        env: Some(HashMap::from([("MODE".into(), "safe".into())])), cwd: Some("/tmp/example".into()),
    });
    adapter.upsert(&path, "docs", &local).unwrap();
    assert_eq!(adapter.read(&path)["docs"], local);
    adapter.upsert(&path, "paused", &McpConfig::Http(HttpConfig {
        kind: "http".into(), url: "https://paused.example.test/mcp".into(), headers: None,
    })).unwrap();
    let paused: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(paused["mcpServers"]["paused"]["disabled"], true);
    assert!(!adapter.read(&path).contains_key("paused"));
    adapter.remove(&path, &["docs".into()]).unwrap();
    let remaining: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(remaining["x-workbuddy"], original["x-workbuddy"]);
    for name in ["local", "legacy", "paused"] {
        assert_eq!(remaining["mcpServers"][name], original["mcpServers"][name]);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
    }
    for invalid in [r#"{"mcpServers": []}"#, r#"{"mcpServers":{"docs":{"disabled":"false","url":"https://example.test"}}}"#, r#"{"mcpServers":{"docs":{},"docs":{}}}"#, "{"] {
        std::fs::write(&path, invalid).unwrap();
        assert!(adapter.upsert(&path, "docs", &local).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    }
}

#[test]
fn pi_native_mcp_preserves_policy_and_respects_enabled_state() {
    let home = arca_core::testenv::TestHome::new("pi-native-mcp");
    let path = home.home.join(".pi/agent/mcp.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{
  "mcpServers": {
    "invalid.name": {"command": "echo"},
    "docs": {
      "url": "https://docs.example.test/mcp",
      "enabled": false,
      "exposure": "direct",
      "timeout": 90,
      "toolExposure": {"delete_*": "hidden"}
    }
  },
  "other": {"keep": true}
}"#).unwrap();
    let agents = builtin_agents();
    let adapter = get_agent_adapter_for(&agents["pi"], "pi");
    assert!(adapter.supports_native_enabled());
    assert!(!adapter.read_with_enabled(&path).contains_key("invalid.name"));
    assert_eq!(adapter.read_with_enabled(&path)["docs"].1, false);
    assert!(!adapter.read(&path).contains_key("docs"));
    let snapshot = adapter.snapshot(&path, "docs").unwrap().unwrap();
    adapter.set_enabled(&path, "docs", true, &snapshot).unwrap();
    assert_eq!(adapter.read_with_enabled(&path)["docs"].1, true);
    assert!(adapter.set_enabled(&path, "docs", false, &snapshot).is_err());

    adapter.upsert(&path, "docs", &http("https://updated.example.test/mcp")).unwrap();
    let updated: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(updated["mcpServers"]["docs"]["url"], "https://updated.example.test/mcp");
    assert_eq!(updated["mcpServers"]["docs"]["enabled"], true);
    assert_eq!(updated["mcpServers"]["docs"]["exposure"], "direct");
    assert_eq!(updated["mcpServers"]["docs"]["timeout"], 90);
    assert_eq!(updated["mcpServers"]["docs"]["toolExposure"]["delete_*"], "hidden");
    assert_eq!(updated["other"]["keep"], true);
    assert_eq!(updated["mcpServers"]["invalid.name"]["command"], "echo");

    assert!(adapter.upsert(&path, "invalid.name", &http("https://example.test/mcp")).is_err());
    assert!(adapter.upsert(&path, "legacy", &McpConfig::Http(HttpConfig {
        kind: "sse".into(), url: "https://example.test/sse".into(), headers: None,
    })).is_err());
}

#[test]
fn kimi_desktop_and_cli_share_lossless_mcp_and_skill_contracts() {
    let home = arca_core::testenv::TestHome::new("kimi-desktop-formats");
    let agents = builtin_agents();
    let desktop = &agents["kimi-code-desktop"];
    let cli = &agents["kimi-code"];
    assert_eq!(desktop.global, cli.global);
    assert_eq!(desktop.skills.as_ref().unwrap().target_id, cli.skills.as_ref().unwrap().target_id);
    assert_eq!(desktop.skills.as_ref().unwrap().global_dir, cli.skills.as_ref().unwrap().global_dir);
    assert!(desktop.project.is_none());

    for id in ["kimi-code", "kimi-code-desktop"] {
        let path = home.home.join(format!("{id}.json"));
        std::fs::write(&path, fixture("kimi-code-desktop")).unwrap();
        let adapter = get_agent_adapter_for(&agents[id], id);
        let configs = adapter.read(&path);
        assert!(!configs.contains_key("paused"));
        assert!(matches!(&configs["legacy"], McpConfig::Http(config) if config.kind == "sse"));
        assert!(matches!(&configs["local"], McpConfig::Stdio(_)));

        let config = http("https://updated.example.test/mcp");
        adapter.upsert(&path, "docs", &config).unwrap();
        assert_eq!(adapter.read(&path)["docs"], normalize_with_codec(from_name(Some("kimi"), id), &config));
        let saved = std::fs::read_to_string(&path).unwrap();
        let value: Value = serde_json::from_str(&saved).unwrap();
        let original: Value = serde_json::from_str(fixture("kimi-code-desktop")).unwrap();
        for field in ["enabled", "deferred", "bearerTokenEnvVar", "startupTimeoutMs", "toolTimeoutMs", "enabledTools", "disabledTools"] {
            assert_eq!(value["mcpServers"]["docs"][field], original["mcpServers"]["docs"][field]);
        }
        assert!(value["mcpServers"]["docs"].get("transport").is_none());
        assert!(value["mcpServers"]["docs"].get("type").is_none());
        assert!(adapter.upsert(&path, "paused", &config).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
        adapter.remove(&path, &["docs".into()]).unwrap();
        let remaining: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(remaining["userPolicy"], original["userPolicy"]);
        for name in ["local", "legacy", "paused"] {
            assert_eq!(remaining["mcpServers"][name], original["mcpServers"][name]);
        }
        for invalid in [r#"{"mcpServers": []}"#, r#"{"mcpServers":{"docs":{"enabled":"false","url":"https://example.test"}}}"#, r#"{"mcpServers":{"docs":{},"docs":{}}}"#] {
            std::fs::write(&path, invalid).unwrap();
            assert!(adapter.upsert(&path, "docs", &config).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
        }
    }
}

#[test]
fn zcode_native_mcp_keeps_policy_and_refuses_disabled_entries() {
    let home = arca_core::testenv::TestHome::new("zcode-native-mcp");
    let path = home.home.join("config.json");
    std::fs::write(&path, fixture("zcode")).unwrap();
    let agents = builtin_agents();
    let adapter = get_agent_adapter_for(&agents["zcode"], "zcode");
    let config = http("https://updated.example.test/mcp");
    adapter.upsert(&path, "docs", &config).unwrap();
    assert_eq!(adapter.read(&path)["docs"], config);
    let saved = std::fs::read_to_string(&path).unwrap();
    let value: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(value["theme"], "system");
    assert_eq!(value["mcp"]["servers"]["docs"]["timeout"], 9000);
    assert_eq!(value["mcp"]["servers"]["docs"]["enable"], true);
    assert!(adapter.upsert(&path, "paused", &config).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
}

#[test]
fn zcode_does_not_hide_shared_fallback_when_creating_native_config() {
    let home = arca_core::testenv::TestHome::new("zcode-fallback");
    std::fs::create_dir_all(home.home.join(".agents")).unwrap();
    std::fs::write(home.home.join(".agents/mcp.json"), r#"{"mcpServers":{"shared":{"command":"example"}}}"#).unwrap();
    let path = home.home.join(".zcode/cli/config.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let agents = builtin_agents();
    let adapter = get_agent_adapter_for(&agents["zcode"], "zcode");
    let error = adapter.upsert(&path, "docs", &http("https://example.test/mcp")).unwrap_err();
    assert!(error.contains("zcode_mcp_fallback_conflict"));
    assert!(!path.exists());
}

#[test]
fn new_cli_writers_roundtrip_official_shapes_and_keep_unowned_policy() {
    let home = arca_core::testenv::TestHome::new("new-cli-formats");
    let agents = builtin_agents();
    for (id, extension, marker) in [
        ("antigravity-cli", "json", "\"disabledTools\": [\"publish\"]"),
        ("continue-cli", "yaml", "connectionTimeout: 9000"),
    ] {
        let path = home.home.join(format!("{id}.{extension}"));
        std::fs::write(&path, fixture(id)).unwrap();
        let adapter = get_agent_adapter_for(&agents[id], id);
        assert!(matches!(adapter.read(&path)["docs"], McpConfig::Http(_)));
        let cfg = http("https://updated.example.test/mcp");
        adapter.upsert(&path, "docs", &cfg).unwrap();
        assert_eq!(adapter.read(&path)["docs"], cfg);
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains(marker), "{id}: policy changed");
        if id == "antigravity-cli" {
            assert!(written.contains("// CLI and IDE"));
            assert!(written.contains("\"serverUrl\""));
            assert!(!written.contains("\"url\""));
            assert!(written.contains("\"userPolicy\""));
        } else {
            assert!(written.contains("# Continue CLI and IDE"));
            assert!(written.contains("Keep my review rules"));
        }
        adapter.remove(&path, &["docs".into()]).unwrap();
        assert!(adapter.read(&path).is_empty());
    }

    let path = home.home.join("traecli.toml");
    std::fs::write(&path, fixture("trae-cli")).unwrap();
    let adapter = get_agent_adapter_for(&agents["trae-cli"], "trae-cli");
    let cfg = McpConfig::Stdio(StdioConfig {
        command: "node".into(), args: Some(vec!["new-server.mjs".into()]), env: None, cwd: None,
    });
    adapter.upsert(&path, "docs", &cfg).unwrap();
    assert_eq!(adapter.read(&path)["docs"], cfg);
    let before = std::fs::read(&path).unwrap();
    assert!(String::from_utf8_lossy(&before).contains("trust_level = \"trusted\""));
    assert!(String::from_utf8_lossy(&before).contains("approval_policy = \"on-request\""));
    assert!(adapter.upsert(&path, "remote", &http("https://remote.example.test/mcp")).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before, "unsupported transport must not alter TOML");
    adapter.remove(&path, &["docs".into()]).unwrap();
    assert!(adapter.read(&path).is_empty());
    assert!(std::fs::read_to_string(path).unwrap().contains("trust_level = \"trusted\""));
}

#[test]
fn new_agent_capabilities_do_not_infer_unsupported_skill_or_model_layouts() {
    let agents = builtin_agents();
    for id in ["antigravity-cli", "continue-cli", "trae-cli"] {
        assert!(agents[id].skills.is_none(), "{id}");
    }
    assert_eq!(agents["antigravity-cli"].global, agents["antigravity"].global);
    assert_eq!(agents["continue-cli"].global, agents["continue"].global);
    let openclaw = &agents["openclaw"];
    assert!(openclaw.global.is_none());
    let skills = openclaw.skills.as_ref().unwrap();
    assert_eq!(skills.global_dir, "~/.openclaw/skills");
    assert!(skills.aliases.is_empty());
    assert_eq!(skills.probes.len(), 2);
}

fn is_iso_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn write_fixture(name: &str, extension: &str) -> PathBuf {
    let path = temp_file(name, extension);
    std::fs::write(&path, fixture(name)).unwrap();
    path
}

fn http(url: &str) -> McpConfig {
    McpConfig::Http(HttpConfig {
        kind: "streamable-http".into(),
        url: url.into(),
        headers: Some(HashMap::from([("X-New".into(), "value".into())])),
    })
}

#[test]
fn scans_official_opencode_gemini_and_windsurf_shapes() {
    let open_path = write_fixture("opencode", "json");
    let open = get_agent_adapter("json", "mcp", "opencode").read(&open_path);
    assert!(matches!(open["local-tools"], McpConfig::Stdio(_)));
    assert!(matches!(open["remote-tools"], McpConfig::Http(_)));

    let gemini_path = write_fixture("gemini", "json");
    let gemini = get_agent_adapter("json", "mcpServers", "gemini").read(&gemini_path);
    assert_eq!(
        match &gemini["docs"] {
            McpConfig::Http(config) => config.kind.as_str(),
            _ => panic!("expected HTTP"),
        },
        "streamable-http"
    );
    assert_eq!(
        match &gemini["legacy"] {
            McpConfig::Http(config) => config.kind.as_str(),
            _ => panic!("expected SSE"),
        },
        "sse"
    );

    let windsurf_path = write_fixture("windsurf", "json");
    let windsurf = get_agent_adapter("json", "mcpServers", "windsurf").read(&windsurf_path);
    assert!(matches!(windsurf["docs"], McpConfig::Http(_)));

    for path in [open_path, gemini_path, windsurf_path] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn opencode_update_preserves_private_settings_and_entry_policy() {
    let path = write_fixture("opencode", "json");
    let adapter = get_agent_adapter("json", "mcp", "opencode");
    adapter
        .upsert(
            &path,
            "local-tools",
            &McpConfig::Stdio(StdioConfig {
                command: "bun".into(),
                args: Some(vec!["x".into(), "new-server".into()]),
                env: Some(HashMap::from([("NEW_TOKEN".into(), "new-value".into())])),
                cwd: Some("/new/worktree".into()),
            }),
        )
        .unwrap();

    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(root["model"], "provider/private-model");
    let target = &root["mcp"]["local-tools"];
    assert_eq!(target["type"], "local");
    assert_eq!(
        target["command"],
        serde_json::json!(["bun", "x", "new-server"])
    );
    assert_eq!(target["environment"]["NEW_TOKEN"], "new-value");
    assert_eq!(target["enabled"], false);
    assert_eq!(target["timeout"], 9000);
    assert_eq!(root["mcp"]["remote-tools"]["oauth"], false);
    let _ = std::fs::remove_file(path);
}

#[test]
fn step_code_preserves_settings_and_refuses_disabled_or_sse_entries() {
    let agents = builtin_agents();
    let definition = &agents["step-code"];
    assert_eq!(definition.global.as_deref(), Some("~/.stepcode/config.toml"));
    assert!(definition.project.is_none());
    let skills = definition.skills.as_ref().unwrap();
    assert_eq!(skills.global_dir, "~/.stepcode/agent/skills");
    assert_eq!(skills.aliases[0].global_dir, "~/.agents/skills");
    assert!(skills.probes.iter().all(|probe| !matches!(
        probe,
        arca_core::types::AgentInstallProbe::Command { name } if name == "step"
    )));
    let launchers: Value =
        serde_json::from_str(include_str!("../../data/agent-launchers.json")).unwrap();
    assert_eq!(
        launchers["step-code"]["candidates"],
        serde_json::json!(["~/.stepcode/bin/step"])
    );

    let path = write_fixture("step-code", "toml");
    let adapter = get_agent_adapter_for(definition, "step-code");
    let scanned = adapter.read(&path);
    assert!(matches!(scanned["local"], McpConfig::Stdio(_)));
    assert!(matches!(scanned["docs"], McpConfig::Http(_)));
    assert!(!scanned.contains_key("paused"));

    let original = std::fs::read_to_string(&path).unwrap();
    assert!(adapter
        .upsert(&path, "paused", &http("https://new.example.com/mcp"))
        .is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let sse = McpConfig::Http(HttpConfig {
        kind: "sse".into(),
        url: "https://sse.example.com/mcp".into(),
        headers: None,
    });
    assert!(adapter.upsert(&path, "docs", &sse).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

    adapter
        .upsert(&path, "docs", &http("https://new.example.com/mcp"))
        .unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    let root: toml::Value = toml::from_str(&written).unwrap();
    let target = &root["mcp_servers"]["docs"];
    assert_eq!(root["theme"].as_str(), Some("step-blue"));
    assert_eq!(root["defaultProvider"].as_str(), Some("step"));
    assert_eq!(root["defaultModel"].as_str(), Some("step-3.7-flash"));
    assert_eq!(target["url"].as_str(), Some("https://new.example.com/mcp"));
    assert_eq!(target["http_headers"]["X-New"].as_str(), Some("value"));
    assert_eq!(target["bearer_token_env_var"].as_str(), Some("DOCS_TOKEN"));
    assert_eq!(target["startup_timeout_sec"].as_integer(), Some(30));
    assert_eq!(target["oauth"]["callback_port"].as_integer(), Some(8976));
    assert_eq!(root["mcp_servers"]["paused"]["enabled"].as_bool(), Some(false));
    assert!(written.contains("enabled_tools = [\"search\"] # keep policy and comment"));
    assert!(written.contains("# keep Step Code settings and comments"));
    assert!(!target.as_table().unwrap().contains_key("type"));
    assert!(!target.as_table().unwrap().contains_key("headers"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn codex_update_preserves_tool_policy_and_uses_http_headers() {
    let path = write_fixture("codex", "toml");
    let adapter = get_agent_adapter("toml", "mcp_servers", "codex");
    adapter
        .upsert(&path, "figma", &http("https://new.example.com/mcp"))
        .unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    let root: toml::Value = toml::from_str(&written).unwrap();
    let target = &root["mcp_servers"]["figma"];
    assert_eq!(root["model"].as_str(), Some("gpt-private"));
    assert_eq!(root["history"]["persistence"].as_str(), Some("save-all"));
    assert_eq!(target["url"].as_str(), Some("https://new.example.com/mcp"));
    assert_eq!(target["http_headers"]["X-New"].as_str(), Some("value"));
    assert_eq!(target["bearer_token_env_var"].as_str(), Some("FIGMA_TOKEN"));
    assert_eq!(
        target["tools"]["open"]["approval_mode"].as_str(),
        Some("approve")
    );
    assert!(
        written.contains("enabled_tools = [\"open\", \"screenshot\"] # keep policy and comment")
    );
    assert!(!target.as_table().unwrap().contains_key("type"));
    assert!(!target.as_table().unwrap().contains_key("headers"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn grok_build_update_preserves_models_auth_and_mcp_policy() {
    let path = write_fixture("grok-build", "toml");
    let adapter = get_agent_adapter("toml", "mcp_servers", "grok-build");
    let scanned = adapter.read(&path);
    assert!(matches!(scanned["local"], McpConfig::Stdio(_)));
    assert!(matches!(scanned["docs"], McpConfig::Http(_)));

    adapter
        .upsert(&path, "docs", &http("https://new.example.com/mcp"))
        .unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    let root: toml::Value = toml::from_str(&written).unwrap();
    let target = &root["mcp_servers"]["docs"];
    assert_eq!(root["models"]["default"].as_str(), Some("private"));
    assert_eq!(
        root["model"]["private"]["api_backend"].as_str(),
        Some("responses")
    );
    assert_eq!(
        root["auth_provider_command"].as_str(),
        Some("security find-generic-password -w -s grok")
    );
    assert_eq!(target["url"].as_str(), Some("https://new.example.com/mcp"));
    assert_eq!(target["headers"]["X-New"].as_str(), Some("value"));
    assert_eq!(target["enabled"].as_bool(), Some(false));
    assert_eq!(target["startup_timeout_sec"].as_integer(), Some(45));
    assert_eq!(target["tool_timeout_sec"].as_integer(), Some(6000));
    assert_eq!(target["tool_timeouts"]["slow"].as_integer(), Some(120));
    assert_eq!(target["bearer_token_env_var"].as_str(), Some("DOCS_TOKEN"));
    assert!(written.contains("# keep Grok Build settings and comments"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn minimax_code_update_preserves_runtime_settings_and_server_policy() {
    let path = write_fixture("minimax-code", "json");
    let definition = &builtin_agents()["minimax-code"];
    let adapter = get_agent_adapter_for(definition, "minimax-code");
    let scanned = adapter.read(&path);
    assert!(matches!(scanned["local-tools"], McpConfig::Stdio(_)));
    assert!(matches!(scanned["docs"], McpConfig::Http(_)));
    assert_eq!(
        match &scanned["legacy"] {
            McpConfig::Http(config) => config.kind.as_str(),
            _ => panic!("expected SSE"),
        },
        "sse"
    );

    adapter
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .unwrap();

    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let target = &root["mcpServers"]["docs"];
    assert_eq!(root["profile"]["owner"], "user");
    assert_eq!(root["profile"]["theme"], "dark");
    assert_eq!(target["type"], "http");
    assert_eq!(target["url"], "https://new.example/mcp");
    assert_eq!(target["headers"]["X-New"], "value");
    assert_eq!(target["enabled"], false);
    assert_eq!(target["timeout"], 90);
    assert_eq!(target["customPolicy"]["allow"][0], "search");
    assert_eq!(root["mcpServers"]["legacy"]["type"], "sse");
    let _ = std::fs::remove_file(path);
}

#[test]
fn copilot_defaults_tools_only_for_new_entries() {
    let path = temp_file("copilot", "json");
    std::fs::write(
        &path,
        r#"{"mcpServers":{"existing":{"type":"http","url":"https://old","tools":["read"]}}}"#,
    )
    .unwrap();
    let adapter = get_agent_adapter("json", "mcpServers", "copilot-cli");
    adapter
        .upsert(&path, "existing", &http("https://new"))
        .unwrap();
    adapter.upsert(&path, "new", &http("https://new")).unwrap();

    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        root["mcpServers"]["existing"]["tools"],
        serde_json::json!(["read"])
    );
    assert_eq!(root["mcpServers"]["new"]["tools"], serde_json::json!(["*"]));
    let _ = std::fs::remove_file(path);
}

#[test]
fn cline_update_uses_nested_transport_and_preserves_registration_state() {
    let path = write_fixture("cline", "json");
    let adapter = get_agent_adapter("json", "mcpServers", "cline");
    let scanned = adapter.read(&path);
    assert!(matches!(scanned["shared-local"], McpConfig::Stdio(_)));
    assert!(matches!(scanned["shared-http"], McpConfig::Http(_)));

    adapter
        .upsert(
            &path,
            "shared-local",
            &McpConfig::Stdio(StdioConfig {
                command: "bunx".into(),
                args: Some(vec!["new-server".into()]),
                env: None,
                cwd: Some("/new/worktree".into()),
            }),
        )
        .unwrap();

    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let target = &root["mcpServers"]["shared-local"];
    assert_eq!(root["account"]["profile"], "private");
    assert_eq!(target["transport"]["type"], "stdio");
    assert_eq!(target["transport"]["command"], "bunx");
    assert_eq!(target["transport"]["cwd"], "/new/worktree");
    assert_eq!(target["disabled"], true);
    assert_eq!(target["metadata"]["owner"], "user");
    assert_eq!(target["oauth"]["tokens"]["access_token"], "fixture-token");
    assert!(target.get("command").is_none());
    let _ = std::fs::remove_file(path);
}

#[test]
fn target_entry_with_non_object_value_is_never_replaced() {
    let path = temp_file("bad-target", "json");
    let original = r#"{"private":{"token":"secret"},"mcp":{"docs":"managed elsewhere"}}"#;
    std::fs::write(&path, original).unwrap();
    let result =
        get_agent_adapter("json", "mcp", "opencode").upsert(&path, "docs", &http("https://new"));
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let _ = std::fs::remove_file(path);
}

#[test]
fn every_writable_builtin_roundtrips_through_its_wire_format() {
    let agents = builtin_agents();
    let writable = agents
        .values()
        .filter(|agent| agent.global.is_some())
        .count();
    assert_eq!(writable, 74);

    for (agent_id, definition) in agents {
        if definition.global.is_none() {
            continue;
        }
        let path = temp_file(
            &format!("matrix-{agent_id}"),
            match definition.format.as_str() {
                "toml" => "toml",
                "yaml" => "yaml",
                _ => "json",
            },
        );
        let adapter = get_agent_adapter_for(&definition, &agent_id);
        let codec = from_name(definition.codec.as_deref(), &agent_id);
        let supports_http = definition
            .transports
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|transport| transport == "http");
        let strict_stdio = matches!(
            codec,
            arca_core::codec::Codec::AgentKube | arca_core::codec::Codec::ChatMcp | arca_core::codec::Codec::MiMo | arca_core::codec::Codec::Jcode
        );
        let local = McpConfig::Stdio(StdioConfig {
            command: "npx".into(),
            args: Some(vec!["-y".into(), "matrix-server".into()]),
            env: Some(HashMap::from([(
                if codec == arca_core::codec::Codec::ChatMcp {
                    "MATRIX_MODE"
                } else {
                    "TOKEN"
                }
                .into(),
                "fixture".into(),
            )])),
            cwd: (!strict_stdio).then(|| "/tmp/arca-matrix".into()),
        });
        let remote = McpConfig::Http(HttpConfig {
            kind: if codec == arca_core::codec::Codec::AgentKube {
                "sse".into()
            } else {
                "streamable-http".into()
            },
            url: "https://example.com/mcp".into(),
            headers: (!matches!(
                codec,
                arca_core::codec::Codec::AgentKube | arca_core::codec::Codec::ChatMcp
            ))
            .then(|| HashMap::from([("X-New".into(), "value".into())])),
        });

        adapter
            .upsert(&path, "local", &local)
            .unwrap_or_else(|error| panic!("{agent_id} failed to write stdio config: {error}"));
        let remote_result = adapter.upsert(&path, "remote", &remote);
        if !supports_http {
            assert!(remote_result.is_err(), "{agent_id} must reject remote MCPs");
        } else {
            remote_result
                .unwrap_or_else(|error| panic!("{agent_id} failed to write HTTP config: {error}"));
        }
        let scanned = adapter.read(&path);
        let scanned_local = scanned
            .get("local")
            .unwrap_or_else(|| panic!("{agent_id} did not scan its stdio entry"));
        assert_eq!(
            scanned_local,
            &normalize_with_codec(codec, &local),
            "{agent_id}"
        );
        if !supports_http {
            assert!(!scanned.contains_key("remote"));
        } else {
            let scanned_remote = scanned
                .get("remote")
                .unwrap_or_else(|| panic!("{agent_id} did not scan its HTTP entry"));
            assert_eq!(
                scanned_remote,
                &normalize_with_codec(codec, &remote),
                "{agent_id}"
            );
        }
        let snapshot = adapter
            .snapshot(&path, "local")
            .unwrap_or_else(|error| panic!("{agent_id} failed to snapshot: {error}"))
            .unwrap_or_else(|| panic!("{agent_id} returned no snapshot"));
        adapter
            .remove_snapshot(&path, "local", &snapshot)
            .unwrap_or_else(|error| panic!("{agent_id} failed to remove snapshot: {error}"));
        assert!(!adapter.read(&path).contains_key("local"), "{agent_id}");
        adapter
            .restore(&path, "local", &snapshot)
            .unwrap_or_else(|error| panic!("{agent_id} failed to restore snapshot: {error}"));
        assert_eq!(
            adapter.read(&path).get("local"),
            Some(&normalize_with_codec(codec, &local)),
            "{agent_id}"
        );
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn standard_agent_update_preserves_target_policy_fields() {
    let path = temp_file("kiro-policy", "json");
    std::fs::write(
        &path,
        r#"{"mcpServers":{"docs":{"url":"https://old","disabled":true,"autoApprove":["read"],"oauth":{"clientId":"private"}}}}"#,
    )
    .unwrap();
    get_agent_adapter("json", "mcpServers", "kiro")
        .upsert(&path, "docs", &http("https://new"))
        .unwrap();

    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let target = &root["mcpServers"]["docs"];
    assert_eq!(target["url"], "https://new");
    assert_eq!(target["disabled"], true);
    assert_eq!(target["autoApprove"], serde_json::json!(["read"]));
    assert_eq!(target["oauth"]["clientId"], "private");
    let _ = std::fs::remove_file(path);
}

#[test]
fn qoder_desktop_round_trip_preserves_shared_cli_settings() {
    let path = write_fixture("qoder-desktop", "json");
    let adapter = get_agent_adapter("json", "mcpServers", "qoder-desktop");
    let before: Value = serde_json::from_str(fixture("qoder-desktop")).unwrap();
    assert!(matches!(adapter.read(&path)["local-tools"], McpConfig::Stdio(_)));
    adapter.upsert(&path, "docs", &http("https://new.example/mcp")).unwrap();
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for key in ["model", "permissions", "unknownPreference"] {
        assert_eq!(after[key], before[key]);
    }
    assert_eq!(after["mcpServers"]["local-tools"], before["mcpServers"]["local-tools"]);
    assert_eq!(after["mcpServers"]["docs"]["type"], "http");
    assert_eq!(after["mcpServers"]["docs"]["disabled"], true);
    assert_eq!(after["mcpServers"]["docs"]["customPolicy"], before["mcpServers"]["docs"]["customPolicy"]);
    let cli = get_agent_adapter("json", "mcpServers", "qoder-cli").read(&path);
    assert_eq!(adapter.read(&path), cli);
    adapter.remove(&path, &["docs".into()]).unwrap();
    assert!(adapter.read(&path).contains_key("local-tools"));
    let remaining: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(remaining["model"], before["model"]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn qwenwork_editions_keep_separate_paths_and_round_trip_custom_mcp() {
    let home = arca_core::testenv::TestHome::new("qwenwork-formats");
    let agents = builtin_agents();
    for (id, directory, app) in [
        ("qwenwork", ".qwenwork", "QwenWork"),
        ("qwenwork-cn", ".qwenworkcn", "QwenWorkCN"),
    ] {
        let definition = &agents[id];
        assert_eq!(definition.global, Some(format!("~/{directory}/mcp.json")));
        let skills = definition.skills.as_ref().unwrap();
        assert_eq!(skills.global_dir, format!("~/{directory}/skills"));
        assert_eq!(skills.target_id, format!("{id}-user"));
        assert!(skills.aliases.is_empty());
        let launchers: Value = serde_json::from_str(include_str!("../../data/agent-launchers.json")).unwrap();
        assert_eq!(launchers[id]["candidates"], serde_json::json!([app]));

        let path = home.home.join(directory).join("mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, fixture("qwenwork")).unwrap();
        let adapter = get_agent_adapter_for(definition, id);
        let before = adapter.read(&path);
        assert!(matches!(&before["local"], McpConfig::Stdio(_)));
        assert!(matches!(&before["events"], McpConfig::Http(config) if config.kind == "sse"));
        assert!(!before.contains_key("paused"));
        assert!(!before.contains_key("organization"));
        let original = std::fs::read(&path).unwrap();
        assert!(adapter.upsert(&path, "paused", &http("https://example.invalid/new")).is_err());
        assert!(adapter.upsert(&path, "organization", &http("https://example.invalid/new")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);

        let updated = http("https://example.invalid/new");
        adapter.upsert(&path, "docs", &updated).unwrap();
        adapter.upsert(&path, "local-copy", &before["local"]).unwrap();
        assert_eq!(adapter.read(&path)["docs"], updated);
        assert_eq!(adapter.read(&path)["local-copy"], before["local"]);
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["privateMetadata"]["preserve"], true);
        assert_eq!(saved["mcpServers"]["docs"]["timeout"], 45);
        assert_eq!(saved["mcpServers"]["docs"]["type"], "streamable-http");
        assert_eq!(saved["mcpServers"]["local-copy"]["type"], "stdio");
        adapter.remove(&path, &["docs".into(), "local-copy".into()]).unwrap();
        let remaining: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(remaining["mcpServers"]["organization"], saved["mcpServers"]["organization"]);
        assert_eq!(remaining["mcpServers"]["paused"], saved["mcpServers"]["paused"]);
    }
}

#[test]
fn qoderwork_update_preserves_file_metadata_and_uses_documented_http_type() {
    let path = temp_file("qoderwork-mcp", "json");
    std::fs::write(
        &path,
        r#"{
  "schemaVersion": 1,
  "privateMetadata": {"owner": "user"},
  "mcpServers": {"docs": {"type": "sse", "url": "https://old", "disabled": true}}
}"#,
    )
    .unwrap();

    get_agent_adapter("json", "mcpServers", "qoderwork")
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .unwrap();

    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(root["schemaVersion"], 1);
    assert_eq!(root["privateMetadata"]["owner"], "user");
    let target = &root["mcpServers"]["docs"];
    assert_eq!(target["type"], "streamable-http");
    assert_eq!(target["url"], "https://new.example/mcp");
    assert_eq!(target["headers"]["X-New"], "value");
    assert_eq!(target["disabled"], true);
    let _ = std::fs::remove_file(path);
}

#[test]
fn warp_remote_uses_url_inference_and_preserves_environment() {
    let path = temp_file("warp-remote", "json");
    std::fs::write(
        &path,
        r#"{"mcpServers":{"docs":{"type":"streamable-http","url":"https://old","env":{"API_KEY":"${API_KEY}"}}}}"#,
    )
    .unwrap();

    get_agent_adapter("json", "mcpServers", "warp")
        .upsert(&path, "docs", &http("https://new"))
        .unwrap();

    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let target = &root["mcpServers"]["docs"];
    assert_eq!(target["url"], "https://new");
    assert_eq!(target["env"]["API_KEY"], "${API_KEY}");
    assert!(target.get("type").is_none());
    let _ = std::fs::remove_file(path);
}

#[test]
fn builtin_global_paths_match_current_product_docs() {
    let agents = builtin_agents();
    let expected = [
        ("agentkube", "~/.agentkube/mcp.json"),
        ("amp", "~/.config/amp/settings.json"),
        ("amazon-q", "~/.aws/amazonq/default.json"),
        ("antigravity", "~/.gemini/config/mcp_config.json"),
        ("augment", "~/.augment/settings.json"),
        ("boltai", "~/.boltai/mcp.json"),
        ("claude-code", "~/.claude.json"),
        (
            "claude-desktop",
            "~/Library/Application Support/Claude/claude_desktop_config.json",
        ),
        (
            "chatmcp",
            "~/Library/Application Support/ChatMcp/mcp_server.json",
        ),
        ("cline", "~/.cline/data/settings/cline_mcp_settings.json"),
        ("codebuddy-code", "~/.codebuddy/.mcp.json"),
        ("codewhale", "~/.codewhale/mcp.json"),
        ("codex", "~/.codex/config.toml"),
        ("codex-desktop", "~/.codex/config.toml"),
        ("continue", "~/.continue/config.yaml"),
        ("copilot-cli", "~/.copilot/mcp-config.json"),
        ("crush", "~/.config/crush/crush.json"),
        ("cursor", "~/.cursor/mcp.json"),
        ("cursor-cli", "~/.cursor/mcp.json"),
        ("factory-desktop", "~/.factory/mcp.json"),
        ("factory-droid", "~/.factory/mcp.json"),
        ("firebender", "~/.firebender/firebender.json"),
        ("gemini", "~/.gemini/settings.json"),
        (
            "goose",
            "~/Library/Application Support/Block/goose/config/config.yaml",
        ),
        ("grok-build", "~/.grok/config.toml"),
        ("hermes", "~/.hermes/config.yaml"),
        ("junie", "~/.junie/mcp/mcp.json"),
        ("kilo-code", "~/.config/kilo/kilo.jsonc"),
        ("kilo-desktop", "~/Library/Application Support/Kilo Desktop/plugins/kilo-server/runtime/config/kilo/kilo.jsonc"),
        ("kimi-code", "~/.kimi-code/mcp.json"),
        ("kimi-code-desktop", "~/.kimi-code/mcp.json"),
        ("kiro", "~/.kiro/settings/mcp.json"),
        ("lmstudio", "~/.lmstudio/mcp.json"),
        ("minimax-code", "~/.mavis/mcp.json"),
        ("mistral-vibe", "~/.vibe/config.toml"),
        ("opencode", "~/.config/opencode/opencode.json"),
        ("opencode-desktop", "~/.config/opencode/opencode.json"),
        ("openhands", "~/.openhands/mcp.json"),
        ("pi", "~/.pi/agent/mcp.json"),
        ("qoder", "~/.qoder/mcp.json"),
        ("qoder-cli", "~/.qoder/settings.json"),
        ("qoder-desktop", "~/.qoder/settings.json"),
        ("qoderwork", "~/.qoderwork/mcp.json"),
        ("qwen-code", "~/.qwen/settings.json"),
        ("qwenwork", "~/.qwenwork/mcp.json"),
        ("qwenwork-cn", "~/.qwenworkcn/mcp.json"),
        (
            "roo-code",
            "~/Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json",
        ),
        ("rovo-dev", "~/.rovodev/mcp.json"),
        ("stakpak", "~/.stakpak/mcp.toml"),
        ("step-code", "~/.stepcode/config.toml"),
        ("tabnine", "~/.tabnine/mcp_servers.json"),
        (
            "vscode",
            "~/Library/Application Support/Code/User/mcp.json",
        ),
        ("vt-code", "~/.vtcode/vtcode.toml"),
        ("workbuddy", "~/.workbuddy-ai/mcp.json"),
        ("workbuddy-cn", "~/.workbuddy/mcp.json"),
        ("warp", "~/.warp/.mcp.json"),
        ("windsurf", "~/.codeium/windsurf/mcp_config.json"),
        ("zed", "~/.config/zed/settings.json"),
    ];

    for (agent_id, path) in expected {
        assert_eq!(agents[agent_id].global.as_deref(), Some(path), "{agent_id}");
    }
    assert!(agents["devin"].global.is_none());
}

#[test]
fn verified_and_catalog_definitions_have_auditable_boundaries() {
    let verified: std::collections::BTreeMap<String, arca_core::types::AgentDefinition> =
        serde_json::from_str(include_str!("../../data/agents.json")).unwrap();
    let catalog: std::collections::BTreeMap<String, arca_core::types::AgentDefinition> =
        serde_json::from_str(include_str!("../../data/agent-catalog.json")).unwrap();
    let verified_ids: std::collections::BTreeSet<_> = verified.keys().cloned().collect();
    let catalog_ids: std::collections::BTreeSet<_> = catalog.keys().cloned().collect();
    let all_ids: std::collections::BTreeSet<_> =
        verified_ids.union(&catalog_ids).cloned().collect();

    assert_eq!(verified.len(), 89);
    assert_eq!(catalog.len(), 278);
    assert_eq!(verified_ids.intersection(&catalog_ids).count(), 64);
    assert_eq!(all_ids.len(), 303);
    assert_eq!(
        verified
            .values()
            .filter(|item| item.global.is_some())
        .count(),
        74
    );
    assert!(catalog.len() >= 170);
    for (id, definition) in verified {
        assert_eq!(definition.builtin, Some(true), "{id}");
        assert!(
            matches!(
                definition.codec.as_deref(),
                Some(
                    "standard"
                        | "workbuddy"
                        | "zcode"
                        | "claude_desktop"
                        | "explicit_type"
                        | "url_inferred"
                        | "pi"
                        | "vscode"
                        | "codex"
                        | "opencode"
                        | "mimo"
                        | "jcode"
                        | "jan"
                        | "anythingllm"
                        | "ibm_bob"
                        | "gemini"
                        | "windsurf"
                        | "qoder"
                        | "qoderwork"
                        | "qwenwork"
                        | "copilot"
                        | "cline"
                        | "roo"
                        | "warp"
                        | "kimi"
                        | "transport"
                        | "tabnine"
                        | "continue"
                        | "goose"
                        | "vibe"
                        | "vtcode"
                        | "agentkube"
                        | "chatmcp"
                        | "server_url"
                        | "url_transport"
                        | "stdio_only"
                        | "stepcode"
                )
            ),
            "{id}: unknown codec {:?}",
            definition.codec
        );
        assert!(
            matches!(definition.layout.as_deref(), Some("map" | "list")),
            "{id}: invalid layout {:?}",
            definition.layout
        );
        if definition.layout.as_deref() == Some("list") {
            assert!(definition.identity_field.is_some(), "{id}");
            assert_ne!(definition.format, "json", "{id}");
        }
        let transports = definition.transports.as_deref().unwrap_or_default();
        assert!(
            transports
                .iter()
                .all(|transport| matches!(transport.as_str(), "stdio" | "http")),
            "{id}: invalid transport {transports:?}"
        );
        let unique_transports: std::collections::BTreeSet<_> = transports.iter().collect();
        assert_eq!(unique_transports.len(), transports.len(), "{id}");
        assert!(
            definition
                .name
                .as_deref()
                .is_some_and(|name| !name.is_empty()),
            "{id}"
        );
        assert!(
            definition
                .docs
                .as_deref()
                .is_some_and(|url| url.starts_with("https://")),
            "{id}"
        );
        assert!(
            definition.verified_at.as_deref().is_some_and(is_iso_date),
            "{id}: invalid verification date {:?}",
            definition.verified_at
        );
        if definition.global.is_some() {
            let evidence = definition.evidence.as_deref().unwrap_or_default();
            assert!(
                matches!(
                    evidence,
                    "official" | "official-source" | "community-extension"
                ),
                "{id}: {evidence}"
            );
            if evidence == "community-extension" {
                assert!(
                    definition
                        .note
                        .as_deref()
                        .is_some_and(|note| note.contains("pi-mcp-adapter")),
                    "{id}"
                );
            }
            assert!(
                matches!(definition.format.as_str(), "json" | "toml" | "yaml"),
                "{id}"
            );
            assert!(!definition.key.is_empty(), "{id}");
            assert!(
                !definition
                    .transports
                    .as_deref()
                    .unwrap_or_default()
                    .is_empty(),
                "{id}"
            );
        }
    }
    for (id, definition) in catalog {
        if verified_ids.contains(&id) {
            continue; // Audited definition takes precedence over its catalog copy.
        }
        assert_eq!(definition.builtin, Some(true), "{id}");
        assert!(definition.global.is_none(), "{id}");
        assert_eq!(definition.format, "unknown", "{id}");
        assert!(
            matches!(
                definition.evidence.as_deref(),
                Some("catalog" | "official" | "official-source" | "acp-registry")
            ),
            "{id}: invalid evidence {:?}",
            definition.evidence
        );
        assert!(
            definition
                .transports
                .as_deref()
                .unwrap_or_default()
                .is_empty(),
            "{id}"
        );
    }
}

#[test]
fn continue_yaml_list_is_lossless_and_adds_required_root_metadata() {
    let definition = &builtin_agents()["continue"];
    let path = temp_file("continue-policy", "yaml");
    let original = r#"# keep this comment
telemetry:
  apiKey: private
mcpServers:
  - name: docs
    type: streamable-http
    url: https://old.example/mcp
    requestOptions:
      timeout: 9000
      headers:
        Old: value
    toolPolicy:
      allow: [search]
"#;
    std::fs::write(&path, original).unwrap();

    get_agent_adapter_for(definition, "continue")
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    let root: Value = serde_yaml::from_str(&written).unwrap();
    let target = &root["mcpServers"][0];
    assert!(written.contains("# keep this comment"));
    assert_eq!(root["telemetry"]["apiKey"], "private");
    assert!(root.get("name").is_none());
    assert_eq!(target["name"], "docs");
    assert_eq!(target["type"], "streamable-http");
    assert_eq!(target["url"], "https://new.example/mcp");
    assert_eq!(target["requestOptions"]["timeout"], 9000);
    assert_eq!(target["requestOptions"]["headers"]["X-New"], "value");
    assert_eq!(target["toolPolicy"]["allow"][0], "search");

    let new_path = temp_file("continue-new", "yaml");
    get_agent_adapter_for(definition, "continue")
        .upsert(&new_path, "docs", &http("https://new.example/mcp"))
        .unwrap();
    let root: Value = serde_yaml::from_str(&std::fs::read_to_string(&new_path).unwrap()).unwrap();
    assert_eq!(root["name"], "Local Config");
    assert_eq!(root["version"], "0.0.1");
    assert_eq!(root["schema"], "v1");

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(new_path);
}

#[test]
fn goose_yaml_map_uses_current_fields_and_preserves_extension_policy() {
    let definition = &builtin_agents()["goose"];
    let path = temp_file("goose-policy", "yaml");
    let original = r#"# private Goose settings
provider: private-model
extensions:
  docs:
    name: Custom display name
    type: streamable_http
    uri: https://old.example/mcp
    description: keep me
    enabled: false
    timeout: 42
"#;
    std::fs::write(&path, original).unwrap();

    get_agent_adapter_for(definition, "goose")
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    let root: Value = serde_yaml::from_str(&written).unwrap();
    let target = &root["extensions"]["docs"];
    assert!(written.contains("# private Goose settings"));
    assert_eq!(root["provider"], "private-model");
    assert_eq!(target["name"], "Custom display name");
    assert_eq!(target["type"], "streamable_http");
    assert_eq!(target["uri"], "https://new.example/mcp");
    assert_eq!(target["description"], "keep me");
    assert_eq!(target["enabled"], false);
    assert_eq!(target["timeout"], 42);
    assert!(target.get("url").is_none());
    let _ = std::fs::remove_file(path);
}

#[test]
fn vibe_toml_list_is_lossless_and_uses_transport_field() {
    let definition = &builtin_agents()["mistral-vibe"];
    let path = temp_file("vibe-policy", "toml");
    let original = r#"model = "private-model" # keep root

[[mcp_servers]]
name = "docs"
transport = "http"
url = "https://old.example/mcp"
enabled = false
timeout = 42 # keep policy
"#;
    std::fs::write(&path, original).unwrap();

    get_agent_adapter_for(definition, "mistral-vibe")
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    let root: toml::Value = toml::from_str(&written).unwrap();
    let target = &root["mcp_servers"][0];
    assert!(written.contains("# keep root"));
    assert!(written.contains("# keep policy"));
    assert_eq!(root["model"].as_str(), Some("private-model"));
    assert_eq!(target["name"].as_str(), Some("docs"));
    assert_eq!(target["transport"].as_str(), Some("streamable-http"));
    assert_eq!(target["url"].as_str(), Some("https://new.example/mcp"));
    assert_eq!(target["enabled"].as_bool(), Some(false));
    assert_eq!(target["timeout"].as_integer(), Some(42));
    let _ = std::fs::remove_file(path);
}

#[test]
fn new_agent_mcp_formats_preserve_user_policy_and_nested_toml() {
    let agents = builtin_agents();

    let codewhale = temp_file("codewhale", "json");
    let original = r#"{
  "default_text_model": "private-model",
  "servers": {
    "docs": {
      "url": "https://old.example/mcp",
      "disabled": true,
      "timeout": 90
    }
  }
}"#;
    std::fs::write(&codewhale, original).unwrap();
    get_agent_adapter_for(&agents["codewhale"], "codewhale")
        .upsert(&codewhale, "docs", &http("https://new.example/mcp"))
        .unwrap();
    let root: Value = serde_json::from_str(&std::fs::read_to_string(&codewhale).unwrap()).unwrap();
    assert_eq!(root["default_text_model"], "private-model");
    assert_eq!(root["servers"]["docs"]["url"], "https://new.example/mcp");
    assert_eq!(root["servers"]["docs"]["disabled"], true);
    assert_eq!(root["servers"]["docs"]["timeout"], 90);

    let stakpak = temp_file("stakpak", "toml");
    let original = r#"# keep Stakpak profile
[profiles.default]
model = "private/model"

[mcpServers.docs]
url = "https://old.example/mcp"
disabled = true
timeout = 90 # keep policy
"#;
    std::fs::write(&stakpak, original).unwrap();
    get_agent_adapter_for(&agents["stakpak"], "stakpak")
        .upsert(&stakpak, "docs", &http("https://new.example/mcp"))
        .unwrap();
    let written = std::fs::read_to_string(&stakpak).unwrap();
    let root: toml::Value = toml::from_str(&written).unwrap();
    assert!(written.contains("# keep Stakpak profile"));
    assert!(written.contains("# keep policy"));
    assert_eq!(
        root["profiles"]["default"]["model"].as_str(),
        Some("private/model")
    );
    assert_eq!(
        root["mcpServers"]["docs"]["url"].as_str(),
        Some("https://new.example/mcp")
    );
    assert_eq!(root["mcpServers"]["docs"]["disabled"].as_bool(), Some(true));
    assert_eq!(root["mcpServers"]["docs"]["timeout"].as_integer(), Some(90));

    let vtcode = temp_file("vt-code", "toml");
    let original = r#"# keep VT Code settings
[agent]
provider = "private"
default_model = "private-model"

[mcp]
enabled = true

[[mcp.providers]]
name = "docs"
enabled = true
endpoint = "https://old.example/mcp"
api_key_env = "DOCS_TOKEN"
protocol_version = "2025-06-18" # keep policy
"#;
    std::fs::write(&vtcode, original).unwrap();
    let adapter = get_agent_adapter_for(&agents["vt-code"], "vt-code");
    assert!(matches!(adapter.read(&vtcode)["docs"], McpConfig::Http(_)));
    adapter
        .upsert(&vtcode, "docs", &http("https://new.example/mcp"))
        .unwrap();
    let written = std::fs::read_to_string(&vtcode).unwrap();
    let root: toml::Value = toml::from_str(&written).unwrap();
    let target = &root["mcp"]["providers"][0];
    assert!(written.contains("# keep VT Code settings"));
    assert!(written.contains("# keep policy"));
    assert_eq!(root["agent"]["provider"].as_str(), Some("private"));
    assert_eq!(root["mcp"]["enabled"].as_bool(), Some(true));
    assert_eq!(target["endpoint"].as_str(), Some("https://new.example/mcp"));
    assert_eq!(target["enabled"].as_bool(), Some(true));
    assert_eq!(target["api_key_env"].as_str(), Some("DOCS_TOKEN"));
    assert_eq!(target["protocol_version"].as_str(), Some("2025-06-18"));
    assert!(target.get("url").is_none());

    let vtcode_missing_switch = temp_file("vt-code-missing-switch", "toml");
    std::fs::write(
        &vtcode_missing_switch,
        r#"# keep existing VT Code model settings
[agent]
provider = "private"
default_model = "private-model"
"#,
    )
    .unwrap();
    adapter
        .upsert(
            &vtcode_missing_switch,
            "docs",
            &http("https://new.example/mcp"),
        )
        .unwrap();
    let written = std::fs::read_to_string(&vtcode_missing_switch).unwrap();
    let root: toml::Value = toml::from_str(&written).unwrap();
    assert!(written.contains("# keep existing VT Code model settings"));
    assert_eq!(root["mcp"]["enabled"].as_bool(), Some(true));
    assert_eq!(
        root["mcp"]["providers"][0]["endpoint"].as_str(),
        Some("https://new.example/mcp")
    );

    let vtcode_explicitly_disabled = temp_file("vt-code-disabled", "toml");
    std::fs::write(
        &vtcode_explicitly_disabled,
        r#"[mcp]
enabled = false
"#,
    )
    .unwrap();
    let disabled_root = std::fs::read_to_string(&vtcode_explicitly_disabled).unwrap();
    assert!(adapter.read(&vtcode_explicitly_disabled).is_empty());
    assert!(adapter
        .upsert(
            &vtcode_explicitly_disabled,
            "docs",
            &http("https://new.example/mcp"),
        )
        .unwrap_err()
        .contains("mcp.enabled"));
    assert_eq!(
        std::fs::read_to_string(&vtcode_explicitly_disabled).unwrap(),
        disabled_root
    );

    let vtcode_disabled_entry = temp_file("vt-code-disabled-entry", "toml");
    let disabled_entry = r#"[mcp]
enabled = true

[[mcp.providers]]
name = "docs"
enabled = false
endpoint = "https://old.example/mcp"
"#;
    std::fs::write(&vtcode_disabled_entry, disabled_entry).unwrap();
    assert!(adapter.read(&vtcode_disabled_entry).is_empty());
    assert!(adapter
        .upsert(
            &vtcode_disabled_entry,
            "docs",
            &http("https://new.example/mcp"),
        )
        .unwrap_err()
        .contains("disabled by 'enabled'"));
    assert_eq!(
        std::fs::read_to_string(&vtcode_disabled_entry).unwrap(),
        disabled_entry
    );

    for path in [
        codewhale,
        stakpak,
        vtcode,
        vtcode_missing_switch,
        vtcode_explicitly_disabled,
        vtcode_disabled_entry,
    ] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn new_json_codecs_use_documented_nested_and_transport_fields() {
    let agents = builtin_agents();
    let path = temp_file("new-json-codecs", "json");
    std::fs::write(
        &path,
        r#"{"mcpServers":{"docs":{"url":"https://old","requestInit":{"cache":"private","headers":{"Old":"value"}},"policy":{"allow":true}}}}"#,
    )
    .unwrap();
    get_agent_adapter_for(&agents["tabnine"], "tabnine")
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .unwrap();
    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let target = &root["mcpServers"]["docs"];
    assert_eq!(target["requestInit"]["cache"], "private");
    assert_eq!(target["requestInit"]["headers"]["X-New"], "value");
    assert_eq!(target["policy"]["allow"], true);

    for (agent_id, expected_transport) in [("rovo-dev", "http"), ("kimi-code", "")] {
        let path = temp_file(agent_id, "json");
        get_agent_adapter_for(&agents[agent_id], agent_id)
            .upsert(&path, "docs", &http("https://new.example/mcp"))
            .unwrap();
        let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let target = &root["mcpServers"]["docs"];
        if expected_transport.is_empty() {
            assert!(target.get("transport").is_none(), "{agent_id}");
        } else {
            assert_eq!(target["transport"], expected_transport, "{agent_id}");
        }
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn amp_explicitly_uses_a_nested_json_key_path() {
    let agents = builtin_agents();
    let definition = &agents["amp"];
    assert!(definition.key_path);

    let path = temp_file("amp-nested-key-path", "json");
    let adapter = get_agent_adapter_for(definition, "amp");
    adapter
        .upsert(
            &path,
            "docs",
            &McpConfig::Stdio(StdioConfig {
                command: "npx".into(),
                args: Some(vec!["-y".into(), "server".into()]),
                env: None,
                cwd: None,
            }),
        )
        .unwrap();

    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(root["amp"]["mcpServers"]["docs"]["command"], "npx");
    assert!(root.get("amp.mcpServers").is_none());
    assert!(adapter.read(&path).contains_key("docs"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn custom_dotted_json_key_remains_a_literal_top_level_property() {
    let definition = AgentDefinition {
        global: Some("~/.custom/config.json".into()),
        format: "json".into(),
        key: "custom.mcpServers".into(),
        enabled: true,
        ..Default::default()
    };
    assert!(!definition.key_path);

    let path = temp_file("custom-literal-dotted-key", "json");
    let adapter = get_agent_adapter_for(&definition, "custom-agent");
    adapter
        .upsert(
            &path,
            "docs",
            &McpConfig::Stdio(StdioConfig {
                command: "npx".into(),
                args: None,
                env: None,
                cwd: None,
            }),
        )
        .unwrap();

    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(root["custom.mcpServers"]["docs"]["command"], "npx");
    assert!(root.get("custom").is_none());
    assert!(adapter.read(&path).contains_key("docs"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn audited_remote_schemas_use_exact_documented_fields() {
    let agents = builtin_agents();

    let path = temp_file("antigravity-server-url", "json");
    get_agent_adapter_for(&agents["antigravity"], "antigravity")
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .unwrap();
    let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let target = &root["mcpServers"]["docs"];
    assert_eq!(target["serverUrl"], "https://new.example/mcp");
    assert!(target.get("url").is_none());
    let _ = std::fs::remove_file(path);

    for agent_id in ["augment", "openhands"] {
        let path = temp_file(agent_id, "json");
        get_agent_adapter_for(&agents[agent_id], agent_id)
            .upsert(&path, "docs", &http("https://new.example/mcp"))
            .unwrap();
        let root: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let target = &root["mcpServers"]["docs"];
        assert_eq!(target["type"], "http", "{agent_id}");
        assert_eq!(target["url"], "https://new.example/mcp", "{agent_id}");
        let _ = std::fs::remove_file(path);
    }

    let path = temp_file("boltai-stdio-only", "json");
    let error = get_agent_adapter_for(&agents["boltai"], "boltai")
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .unwrap_err();
    assert!(error.contains("stdio servers only"));
    assert!(!path.exists());
}

#[test]
fn hermes_uses_transport_only_for_legacy_sse() {
    let agents = builtin_agents();
    let path = temp_file("hermes-transports", "yaml");
    let adapter = get_agent_adapter_for(&agents["hermes"], "hermes");
    adapter
        .upsert(&path, "http", &http("https://new.example/mcp"))
        .unwrap();
    adapter
        .upsert(
            &path,
            "sse",
            &McpConfig::Http(HttpConfig {
                kind: "sse".into(),
                url: "https://new.example/sse".into(),
                headers: None,
            }),
        )
        .unwrap();
    let root: Value = serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(root["mcp_servers"]["http"].get("transport").is_none());
    assert_eq!(root["mcp_servers"]["sse"]["transport"], "sse");
    let _ = std::fs::remove_file(path);
}

#[test]
fn duplicate_yaml_server_keys_fail_closed() {
    let agents = builtin_agents();
    let path = temp_file("duplicate-yaml", "yaml");
    let original =
        "private: keep\nmcp_servers:\n  docs: {url: https://one}\n  docs: {url: https://two}\n";
    std::fs::write(&path, original).unwrap();
    let result = get_agent_adapter_for(&agents["hermes"], "hermes").upsert(
        &path,
        "docs",
        &http("https://new.example/mcp"),
    );
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let _ = std::fs::remove_file(path);
}

#[test]
fn duplicate_toml_list_identities_fail_closed() {
    let agents = builtin_agents();
    let path = temp_file("duplicate-toml-list", "toml");
    let original = r#"private = "keep"

[[mcp_servers]]
name = "docs"
transport = "http"
url = "https://one"

[[mcp_servers]]
name = "docs"
transport = "http"
url = "https://two"
"#;
    std::fs::write(&path, original).unwrap();
    let adapter = get_agent_adapter_for(&agents["mistral-vibe"], "mistral-vibe");
    assert!(adapter.read(&path).is_empty());
    assert!(adapter
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let _ = std::fs::remove_file(path);
}

#[test]
fn catalog_only_and_unknown_formats_fail_closed() {
    let definition = &builtin_agents()["devin"];
    let path = temp_file("unsupported", "json");
    let result = get_agent_adapter_for(definition, "devin").upsert(
        &path,
        "docs",
        &http("https://new.example/mcp"),
    );
    assert!(result.is_err());
    assert!(!path.exists());
}

#[test]
fn agentkube_codec_preserves_agent_owned_policy() {
    let agents = builtin_agents();
    let agentkube = temp_file("agentkube-policy", "json");
    std::fs::write(
        &agentkube,
        r#"{"mcpServers":{"docs":{"transport":"stdio","command":"old","enabled":true,"policy":"keep"}}}"#,
    )
    .unwrap();
    get_agent_adapter_for(&agents["agentkube"], "agentkube")
        .upsert(
            &agentkube,
            "docs",
            &McpConfig::Http(HttpConfig {
                kind: "sse".into(),
                url: "https://new.example/sse".into(),
                headers: None,
            }),
        )
        .unwrap();
    let root: Value = serde_json::from_str(&std::fs::read_to_string(&agentkube).unwrap()).unwrap();
    let target = &root["mcpServers"]["docs"];
    assert_eq!(target["transport"], "sse");
    assert_eq!(target["url"], "https://new.example/sse");
    assert_eq!(target["enabled"], true);
    assert_eq!(target["policy"], "keep");
    let _ = std::fs::remove_file(agentkube);
}

#[test]
fn new_codecs_reject_unrepresentable_fields() {
    let agents = builtin_agents();
    let path = temp_file("strict-new-codecs", "json");
    let with_cwd = McpConfig::Stdio(StdioConfig {
        command: "npx".into(),
        args: None,
        env: None,
        cwd: Some("/tmp/not-supported".into()),
    });
    assert!(get_agent_adapter_for(&agents["chatmcp"], "chatmcp")
        .upsert(&path, "docs", &with_cwd)
        .unwrap_err()
        .contains("does not support cwd"));
    assert!(!path.exists());

    let sse_with_headers = McpConfig::Http(HttpConfig {
        kind: "sse".into(),
        url: "https://new.example/mcp".into(),
        headers: Some(HashMap::from([("X-New".into(), "value".into())])),
    });
    assert!(get_agent_adapter_for(&agents["agentkube"], "agentkube")
        .upsert(&path, "docs", &sse_with_headers)
        .unwrap_err()
        .contains("does not support custom headers"));
    assert!(!path.exists());

    let streamable = McpConfig::Http(HttpConfig {
        kind: "streamable-http".into(),
        url: "https://not-supported.example".into(),
        headers: None,
    });
    assert!(get_agent_adapter_for(&agents["agentkube"], "agentkube")
        .upsert(&path, "docs", &streamable)
        .unwrap_err()
        .contains("over SSE only"));
    assert!(!path.exists());
}

#[test]
fn agentkube_disabled_entries_are_skipped_and_rejected() {
    let agents = builtin_agents();
    let path = temp_file("agentkube-disabled-final", "json");
    let original = r#"{"mcpServers":{"docs":{"command":"old","enabled":false}}}"#;
    std::fs::write(&path, original).unwrap();
    let adapter = get_agent_adapter_for(&agents["agentkube"], "agentkube");
    assert!(!adapter.read(&path).contains_key("docs"));
    assert!(adapter
        .upsert(&path, "docs", &http("https://new.example/mcp"))
        .is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let _ = std::fs::remove_file(path);
}

#[test]
fn chatmcp_credential_bearing_files_are_external_managed_and_read_only() {
    let agents = builtin_agents();
    let path = temp_file("chatmcp-oauth-final", "json");
    let original = r#"{"mcpServers":{"docs":{"type":"streamable","command":"https://same.example/mcp","oauth":{"clientSecret":"fixture"},"policy":"keep"}}}"#;
    std::fs::write(&path, original).unwrap();
    let adapter = get_agent_adapter_for(&agents["chatmcp"], "chatmcp");
    let remote = McpConfig::Http(HttpConfig {
        kind: "streamable-http".into(),
        url: "https://same.example/mcp".into(),
        headers: None,
    });
    assert!(adapter.read(&path).is_empty());
    assert!(adapter.upsert(&path, "docs", &remote).is_err());
    assert!(adapter.snapshot(&path, "docs").is_err());
    assert!(adapter.remove(&path, &["docs".into()]).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    let _ = std::fs::remove_file(path);
}

#[test]
fn opencode_desktop_shares_cli_wire_format_without_owning_other_settings() {
    let home = arca_core::testenv::TestHome::new("opencode-desktop-formats");
    let agents = builtin_agents();
    let desktop = &agents["opencode-desktop"];
    let cli = &agents["opencode"];
    assert_eq!(desktop.category.as_deref(), Some("desktop"));
    assert_eq!(desktop.global, cli.global);
    assert_eq!(desktop.skills.as_ref().unwrap().target_id, cli.skills.as_ref().unwrap().target_id);
    assert_eq!(desktop.skills.as_ref().unwrap().global_dir, cli.skills.as_ref().unwrap().global_dir);
    assert!(desktop.project.is_none());
    let path = home.home.join("opencode.json");
    std::fs::write(&path, fixture("opencode")).unwrap();
    let adapter = get_agent_adapter_for(desktop, "opencode-desktop");
    let config = http("https://updated.example.test/mcp");
    adapter.upsert(&path, "remote-tools", &config).unwrap();
    assert_eq!(adapter.read(&path)["remote-tools"], config);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let original: Value = serde_json::from_str(fixture("opencode")).unwrap();
    assert_eq!(saved["model"], original["model"]);
    assert_eq!(saved["mcp"]["local-tools"], original["mcp"]["local-tools"]);
    assert_eq!(saved["mcp"]["remote-tools"]["oauth"], false);
    adapter.remove(&path, &["remote-tools".into()]).unwrap();
    assert!(!adapter.read(&path).contains_key("remote-tools"));
    for invalid in [r#"{"mcp": []}"#, r#"{"mcp":{"remote-tools":{},"remote-tools":{}}}"#] {
        std::fs::write(&path, invalid).unwrap();
        assert!(adapter.upsert(&path, "remote-tools", &config).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    }
}
