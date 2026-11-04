use arca_core::application::mcp::{inspect_configuration, McpInspectionRequest, McpInspectionSource};
use arca_core::testenv::TestHome;
use serde_json::json;
use std::fs;

fn configured(agent: &str, key: &str) -> McpInspectionRequest {
    McpInspectionRequest { agent_id: agent.into(), source: McpInspectionSource::Configured { key: key.into() } }
}

#[test]
fn compound_credential_names_are_hidden_and_ambiguous_command_values_are_not_returned() {
    let home = TestHome::new("mcp-inspect-compound-credentials");
    let path = home.home.join(".cursor/mcp.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let source = json!({"mcpServers": {"fixture": {
        "command": "fixture-server", "args": ["--aws-secret-access-key=fixture-sensitive-value", "public-file"],
        "secret_access_key": "fixture-other-sensitive-value", "secret_key": "fixture-third-sensitive-value",
        "echo": "fixture-sensitive-value fixture-other-sensitive-value fixture-third-sensitive-value",
        "token_budget": 4096, "secret-less": true
    }}}).to_string();
    fs::write(&path, &source).unwrap();
    let view = inspect_configuration(configured("cursor", "fixture::stdio")).unwrap();
    let config = view.documents[0].configuration.as_ref().unwrap();
    let rendered = serde_json::to_string(&view).unwrap();
    for value in ["fixture-sensitive-value", "fixture-other-sensitive-value", "fixture-third-sensitive-value"] {
        assert!(!rendered.contains(value));
    }
    assert_eq!(config["args"][1], "public-file");
    assert_eq!(config["token_budget"], 4096);
    assert_eq!(config["secret-less"], true);
    assert_eq!(fs::read_to_string(&path).unwrap(), source);

    let source = json!({"mcpServers": {"fixture": {"command": "fixture-server", "args": ["--token=\"unterminated"],
        "echo": "do not expose other fields when redaction is ambiguous"}}}).to_string();
    fs::write(&path, &source).unwrap();
    let view = inspect_configuration(configured("cursor", "fixture::stdio")).unwrap();
    assert!(view.documents[0].configuration.is_none());
    assert!(view.documents[0].unavailable_reason.as_ref().unwrap().contains("无法可靠隐藏"));
    assert!(!serde_json::to_string(&view).unwrap().contains("do not expose"));
    assert_eq!(fs::read_to_string(path).unwrap(), source);
    assert!(!home.home.join(".arca/settings.json").exists());
}

#[test]
fn external_mcp_is_readable_without_adoption_and_hides_credentials_before_serialization() {
    let home = TestHome::new("mcp-inspect-external");
    let path = home.home.join(".cursor/mcp.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let source = json!({
        "account": {"token": "unrelated-account-secret"},
        "mcpServers": {
            "docs": {"command": "npx", "args": ["-y", "fixture-server", "--api-key", "argument-secret"],
                "env": {"ACCESS_TOKEN": "environment-secret", "MODE": "fixture"},
                "headers": {"X-Custom": "header-secret"}, "timeout": 9000,
                "policy": {"allow": ["read"]}, "echo": "environment-secret argument-secret header-secret"},
            "sibling": {"command": "node", "env": {"SECRET": "unrelated-sibling-secret"}}
        }
    }).to_string();
    fs::write(&path, &source).unwrap();
    let view = inspect_configuration(configured("cursor", "docs::stdio")).unwrap();
    assert_eq!(view.documents.len(), 1);
    assert_eq!(view.documents[0].source_path, "~/.cursor/mcp.json");
    let value = view.documents[0].configuration.as_ref().unwrap();
    assert_eq!(value["command"], "npx");
    assert_eq!(value["args"][0], "-y");
    // The package name repeats a hidden environment value; that occurrence
    // must be hidden too, rather than becoming a second disclosure channel.
    assert_eq!(value["args"][1], "[已隐藏]-server");
    assert_eq!(value["env"]["ACCESS_TOKEN"], "[已隐藏]");
    assert_eq!(value["timeout"], 9000);
    assert_eq!(value["policy"]["allow"], json!(["read"]));
    let rendered = serde_json::to_string(&view).unwrap();
    for secret in ["unrelated-account-secret", "unrelated-sibling-secret", "environment-secret", "argument-secret", "header-secret"] {
        assert!(!rendered.contains(secret), "credentials must not leave Core");
    }
    assert_eq!(fs::read_to_string(path).unwrap(), source);
    assert!(!home.home.join(".arca/settings.json").exists());
    assert!(!home.home.join(".arca/assets").exists());
}

#[test]
fn every_registered_mcp_target_can_inspect_its_own_wire_format_without_changing_it() {
    let _home = TestHome::new("mcp-inspect-all");
    let definitions = arca_core::agents::builtin_agents();
    let mut checked = Vec::new();
    for (id, definition) in definitions {
        let Some(target) = definition.global.as_deref() else { continue; };
        let path = arca_core::scanner::expand_tilde(target);
        let config = arca_core::types::McpConfig::Stdio(arca_core::types::StdioConfig {
            command: "fixture-inspected-command".into(), args: None, env: None, cwd: None,
        });
        let adapter = arca_core::adapter::get_agent_adapter_for(&definition, &id);
        adapter.upsert(&path, "inspect-fixture", &config).unwrap_or_else(|error| panic!("{id}: {error}"));
        let before = fs::read(&path).unwrap();
        let names = adapter.declaration_names(std::str::from_utf8(&before).unwrap())
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        assert!(names.iter().any(|name| name == "inspect-fixture"), "{id}: a declaration disappeared before connection decoding");
        let view = inspect_configuration(configured(&id, "inspect-fixture::stdio")).unwrap_or_else(|error| panic!("{id}: {error}"));
        assert_eq!(view.documents.len(), 1, "{id}");
        assert!(view.documents[0].configuration.is_some(), "{id}: {:?}", view.documents[0].unavailable_reason);
        assert!(serde_json::to_string(&view).unwrap().contains("fixture-inspected-command"), "{id}");
        assert_eq!(fs::read(&path).unwrap(), before, "{id}: inspection must not write");
        checked.push(id);
    }
    assert!(!checked.is_empty(), "the full registered writer matrix must run");
}

#[test]
fn credential_bearing_mcp_can_be_inspected_while_its_writer_remains_closed() {
    let home = TestHome::new("mcp-inspect-oauth");
    let definition = arca_core::agents::builtin_agents()["chatmcp"].clone();
    let path = arca_core::scanner::expand_tilde(definition.global.as_ref().unwrap());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = json!({"mcpServers": {"docs": {"type": "streamable", "command": "https://example.test/mcp",
        "oauth": {"clientSecret": "oauth-private-value", "accessToken": "oauth-token-value"}}}}).to_string();
    fs::write(&path, &original).unwrap();
    let declarations = arca_core::application::assets::list_native_resources("chatmcp");
    let serialized = serde_json::to_value(&declarations).unwrap();
    assert_eq!(serialized["configured_mcps"][0]["name"], "docs");
    assert_eq!(serialized["configured_unavailable"], false);
    assert!(!serialized.to_string().contains("oauth-private-value"));
    let request: McpInspectionRequest = serde_json::from_value(json!({
        "agent_id": "chatmcp", "source": {"kind": "configured_name", "name": "docs"}
    })).unwrap();
    assert!(inspect_configuration(request).unwrap().documents[0].configuration.is_some());
    let view = inspect_configuration(configured("chatmcp", "docs::http")).unwrap();
    let rendered = serde_json::to_string(&view).unwrap();
    assert!(rendered.contains("https://example.test/mcp"));
    assert!(!rendered.contains("oauth-private-value"));
    assert!(!rendered.contains("oauth-token-value"));
    let adapter = arca_core::adapter::get_agent_adapter_for(&definition, "chatmcp");
    let config = arca_core::types::McpConfig::Stdio(arca_core::types::StdioConfig {
        command: "fixture".into(), args: None, env: None, cwd: None,
    });
    assert!(adapter.upsert(&path, "docs", &config).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), original);
    assert!(!home.home.join(".arca/settings.json").exists());
}

#[test]
fn configuration_declarations_include_unsupported_entries_and_report_invalid_sources() {
    let home = TestHome::new("mcp-inspect-declarations");
    let path = home.home.join(".cursor/mcp.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    for (source, expected, unavailable) in [
        (r#"{"mcpServers":{"future":{"type":"future-transport","token":"private-value"},"paused":{"command":"fixture","disabled":true}}}"#, vec!["future", "paused"], false),
        (r#"{"mcpServers":{"duplicate":{},"duplicate":{}}}"#, vec![], true),
        ("{", vec![], true),
    ] {
        fs::write(&path, source).unwrap();
        let inventory = arca_core::application::assets::list_native_resources("cursor");
        let value = serde_json::to_value(inventory).unwrap();
        let names = value["configured_mcps"].as_array().unwrap().iter()
            .map(|item| item["name"].as_str().unwrap()).collect::<Vec<_>>();
        assert_eq!(names, expected);
        assert_eq!(value["configured_unavailable"], unavailable);
        assert!(!value.to_string().contains("private-value"));
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
    }
    fs::remove_file(path).unwrap();
    let value = serde_json::to_value(arca_core::application::assets::list_native_resources("cursor")).unwrap();
    assert_eq!(value["configured_mcps"], json!([]));
    assert_eq!(value["configured_unavailable"], false);
}

#[test]
fn invalid_or_missing_mcp_never_returns_a_sibling_or_an_empty_configuration() {
    let home = TestHome::new("mcp-inspect-missing");
    let path = home.home.join(".cursor/mcp.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    for source in [r#"{"mcpServers":{"other":{"command":"private-sibling"}}}"#,
        r#"{"mcpServers":{"docs":{"command":"first","command":"second"}}}"#, "{"] {
        fs::write(&path, source).unwrap();
        let view = inspect_configuration(configured("cursor", "docs::stdio")).unwrap();
        assert!(view.documents[0].configuration.is_none());
        assert!(view.documents[0].unavailable_reason.is_some());
        assert!(!serde_json::to_string(&view).unwrap().contains("private-sibling"));
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
    }
    assert!(inspect_configuration(configured("../../private", "docs::stdio")).is_err());
}

#[test]
fn plugin_mcp_is_inspected_by_observed_identity_and_cannot_select_an_arbitrary_path() {
    let home = TestHome::new("mcp-inspect-plugin");
    let root = home.home.join(".cursor/plugins/local/fixture");
    fs::create_dir_all(root.join(".cursor-plugin")).unwrap();
    fs::write(root.join(".cursor-plugin/plugin.json"), r#"{"name":"fixture","version":"1.0.0"}"#).unwrap();
    fs::write(root.join(".mcp.json"), r#"{"mcpServers":{"docs":{"command":"npx","args":["fixture-server"],"env":{"TOKEN":"plugin-private-value"}}}}"#).unwrap();
    let observed = arca_core::application::assets::list_native_resources("cursor");
    let mcp = observed.host_mcps.iter().find(|mcp| mcp.name == "docs").unwrap();
    let request = McpInspectionRequest { agent_id: "cursor".into(), source: McpInspectionSource::Host { source_fingerprint: mcp.source_fingerprint.clone() } };
    let view = inspect_configuration(request).unwrap();
    assert_eq!(view.documents.len(), 1);
    assert_eq!(view.documents[0].configuration.as_ref().unwrap()["command"], "npx");
    assert!(!serde_json::to_string(&view).unwrap().contains("plugin-private-value"));
    let forged = McpInspectionRequest { agent_id: "cursor".into(), source: McpInspectionSource::Host { source_fingerprint: "../private.json".into() } };
    assert!(inspect_configuration(forged).is_err());
    assert!(!home.home.join(".arca/settings.json").exists());
}
