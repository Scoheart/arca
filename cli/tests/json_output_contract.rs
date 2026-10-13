use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_arca"))
        .args(args)
        .env("ARCA_NO_UPDATE_CHECK", "1")
        .output()
        .expect("run arca")
}

fn success_envelope(args: &[&str], command: &str) -> serde_json::Value {
    let output = run(args);
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse JSON success envelope");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], command);
    assert_eq!(value["changed"], false);
    value
}

#[test]
fn json_help_and_version_use_the_stable_success_envelope() {
    let help = success_envelope(&["--json", "--help"], "help");
    assert!(help["data"]["text"]
        .as_str()
        .is_some_and(|text| text.contains("Usage:")));

    let version = success_envelope(&["--json", "--version"], "version");
    assert_eq!(version["data"]["name"], "arca");
    assert!(version["data"]["version"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
}

#[test]
fn server_argument_named_json_does_not_select_json_for_parse_errors() {
    let human = run(&["mcp", "add", "invalid", "--arg", "--json"]);
    assert_eq!(human.status.code(), Some(2));
    assert!(human.stdout.is_empty());
    assert!(serde_json::from_slice::<serde_json::Value>(&human.stderr).is_err());

    let json = run(&["--json", "mcp", "add", "invalid", "--arg", "--json"]);
    assert_eq!(json.status.code(), Some(2));
    assert!(json.stdout.is_empty());
    let envelope: serde_json::Value =
        serde_json::from_slice(&json.stderr).expect("parse JSON error envelope");
    assert_eq!(envelope["schema_version"], 1);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "invalid_arguments");
}

#[test]
fn mcp_converge_usage_requires_one_agent_and_action() {
    let help = run(&["mcp", "converge", "--help"]);
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    let help = String::from_utf8(help.stdout).expect("UTF-8 help");
    assert!(help.contains("--agent <AGENT>"));
    assert!(help.contains("<ACTION>"));
    assert!(help.contains("adopt"));
    assert!(help.contains("restore"));
    assert!(help.contains("detach"));

    let missing = run(&["mcp", "converge", "demo::stdio", "restore"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
    let error = String::from_utf8(missing.stderr).expect("UTF-8 parse error");
    assert!(error.contains("the following required arguments were not provided"));
    assert!(error.contains("--agent <AGENT>"));
}
