//! Bounded parity smoke: isolated data, fake curl, no real launches or network.
//! cargo run -p arca-core --example verify_desktop_cli -- /absolute/path/to/arca
use arca_core::{application, domain::types::ModelProtocol, resources::model::curl::{self, CurlAuth}, settings, testenv::TestHome};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path, process::Command};
#[cfg(unix)] use std::os::unix::fs::PermissionsExt;

fn run(binary: &Path, args: &[&str]) -> Value {
    let result = Command::new(binary).arg("--json").args(args).output().unwrap();
    assert!(result.status.success(), "CLI {args:?} failed: {}", String::from_utf8_lossy(&result.stderr));
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["ok"], true);
    result["data"].clone()
}

fn main() {
    let binary = std::env::args().nth(1).expect("absolute arca binary path");
    let binary = Path::new(&binary);
    assert!(binary.is_absolute());
    let fixture = TestHome::new("parity");
    let bin = fixture.home.join("bin");
    fs::write(bin.join("curl"), "#!/bin/sh\nprintf '%s\\0' \"$@\"\n").unwrap();
    fs::write(bin.join("jq"), "#!/bin/sh\n[ \"$#\" -eq 1 ] && [ \"$1\" = '-r' ] || exit 1\ncat\n").unwrap();
    fs::write(bin.join("fixture-agent"), "#!/bin/sh\nexit 0\n").unwrap();
    for name in ["curl", "jq", "fixture-agent"] {
        #[cfg(unix)] fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let terminal = fixture.home.join("System/Applications/Utilities/Terminal.app/Contents");
    fs::create_dir_all(&terminal).unwrap();
    fs::write(terminal.join("Info.plist"), "fixture").unwrap();
    let provider = serde_json::from_value(json!({
        "id": "fixture-provider", "name": "Fixture", "provider": "openai", "base_url": "https://example.invalid/v1",
        "protocols": {"openai-completions": {"endpoint_path": "/chat/completions"}}, "auth_requirement": "none"
    })).unwrap();
    let profile = serde_json::from_value(json!({
        "id": "fixture-model", "name": "Fixture Model", "provider_id": "fixture-provider", "provider": "openai",
        "protocol": "openai-completions", "model": "fixture/model"
    })).unwrap();
    settings::save_settings(&settings::Settings {
        model_providers: Some(BTreeMap::from([("fixture-provider".into(), provider)])),
        model_profiles: Some(BTreeMap::from([("fixture-model".into(), profile)])),
        ..Default::default()
    }).unwrap();
    let core = application::models::export_curl("fixture-model", true).unwrap();
    let exported = run(binary, &["model", "curl", "fixture-model", "--include-api-key"]);
    assert_eq!(exported["curl"], core);
    assert!(!core.contains("Authorization"));
    let docs = run(binary, &["model", "provider", "docs", "fixture-provider"]);
    assert_eq!(docs["url"], application::models::provider_documentation("fixture-provider").unwrap().unwrap());
    run(binary, &["settings", "locale", "en-US", "--yes"]);
    assert_eq!(application::ui::get_ui_locale().unwrap().as_deref(), Some("en-US"));
    run(binary, &["settings", "pins", "opencode", "codex", "--yes"]);
    assert_eq!(application::ui::get_pinned_agents().unwrap(), ["opencode", "codex"]);
    run(binary, &["settings", "terminal", "terminal", "--yes"]);
    assert_eq!(application::terminals::settings().unwrap().selected, "terminal");
    let target_file = fixture.home.join("launch.json");
    fs::write(&target_file, serde_json::to_vec(&json!({
        "kind": "cli", "command": bin.join("fixture-agent"), "args": ["--flag", " value with spaces "],
        "env": {"NODE_USE_SYSTEM_CA": "1", "PRIVATE_FIXTURE": "not-a-real-secret"}
    })).unwrap()).unwrap();
    run(binary, &["agent", "launch", "configure", "opencode", "--file", target_file.to_str().unwrap(),
        "--default-directory", fixture.home.to_str().unwrap(), "--yes"]);
    let info = application::agent_launch::info("opencode").unwrap();
    assert_eq!(info.default_directory.as_deref(), fixture.home.to_str());
    assert!(info.available);
    let shown = run(binary, &["agent", "launch", "show", "opencode"]);
    assert!(!shown.to_string().contains("not-a-real-secret"));
    assert_eq!(shown["configured_target"]["argument_count"], 2);
    assert_eq!(run(binary, &["agent", "run", "opencode", "--dry-run"])["dry_run"], true);
    run(binary, &["agent", "launch", "reset", "opencode", "--yes"]);
    let info = application::agent_launch::info("opencode").unwrap();
    assert!(info.configured_target.is_none() && info.default_directory.is_none());

    run(binary, &["mcp", "add", "fixture-mcp::stdio", "--command", "fixture-agent", "--arg", " value ",
        "--cwd", fixture.home.to_str().unwrap(), "--yes"]);
    run(binary, &["mcp", "assign", "fixture-mcp::stdio", "--agent", "opencode", "--yes"]);
    for (command, enabled) in [("disable-all", false), ("enable-all", true)] {
        run(binary, &["mcp", command, "--agent", "opencode", "--yes"]);
        let inventory = serde_json::to_value(application::assets::list_inventory().unwrap()).unwrap();
        assert!(inventory["consumptions"].as_array().unwrap().iter()
            .any(|row| row["agent_id"] == "opencode" && row["enabled"] == enabled));
    }
    run(binary, &["mcp", "icon", "set", "fixture-mcp::stdio", "terminal", "--yes"]);
    assert_eq!(application::ui::list_mcp_icon_preferences().unwrap()["fixture-mcp::stdio"].value, "terminal");
    run(binary, &["mcp", "icon", "reset", "fixture-mcp::stdio", "--yes"]);
    assert!(!application::ui::list_mcp_icon_preferences().unwrap().contains_key("fixture-mcp::stdio"));

    let key = "fixture-'$(printf should-not-run)`printf no`";
    for protocol in [ModelProtocol::OpenaiCompletions, ModelProtocol::OpenaiResponses,
        ModelProtocol::AnthropicMessages, ModelProtocol::GeminiGenerateContent] {
        let command = curl::render(&protocol, "https://example.invalid/models/{model}:generate", "a/b model'", CurlAuth::Key(key)).unwrap();
        let output = Command::new("/bin/sh").args(["-c", &command]).output().unwrap();
        assert!(output.status.success());
        let arguments = output.stdout.split(|byte| *byte == 0).filter(|part| !part.is_empty())
            .map(|part| std::str::from_utf8(part).unwrap()).collect::<Vec<_>>();
        let header = match protocol { ModelProtocol::AnthropicMessages => "x-api-key: ",
            ModelProtocol::GeminiGenerateContent => "x-goog-api-key: ", _ => "Authorization: Bearer " };
        assert!(arguments.contains(&format!("{header}{key}").as_str()));
        let body = arguments[arguments.iter().position(|arg| *arg == "--data-raw").unwrap() + 1];
        assert!(serde_json::from_str::<Value>(body).unwrap().is_object());
        assert!(arguments[2].contains("a%2Fb%20model'"));
    }
    assert!(curl::render(&ModelProtocol::OpenaiCompletions, "https://example.invalid", "fixture", CurlAuth::Key("bad\nheader")).is_err());
    println!("PASS: shared settings, launch configuration/reset/dry-run, MCP bulk switches/icons, documentation, no-auth cURL, four protocol payloads and shell quoting");
}
