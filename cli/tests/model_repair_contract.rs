#![cfg(unix)]

use std::fs;
use std::process::{Command, Output};

use arca_core::domain::types::{ModelProfile, ModelProtocol};
use arca_core::resources::model::save_profile;
use arca_core::testenv::TestHome;

fn profile() -> ModelProfile {
    ModelProfile {
        id: "repair-contract".into(),
        provider_id: Some("repair-contract-provider".into()),
        name: "Repair Contract".into(),
        provider: "custom".into(),
        model_vendor: None,
        native_ids: Default::default(),
        protocol: ModelProtocol::OpenaiResponses,
        base_url: "https://example.invalid/v1".into(),
        endpoint_path: String::new(),
        model: "repair-contract-model".into(),
        env_key: None,
        context_window: Some(128_000),
        max_output_tokens: Some(8_192),
        reasoning: Some(true),
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_arca"))
        .args(args)
        .env("ARCA_NO_UPDATE_CHECK", "1")
        .output()
        .expect("run isolated arca command")
}

fn success_json(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "JSON success wrote to stderr");
    serde_json::from_slice(&output.stdout).expect("one JSON success envelope")
}

#[test]
fn relationship_verbs_are_idempotent_and_only_explicit_convergence_repairs_model_drift() {
    let home = TestHome::new("cli-model-repair");
    save_profile(profile(), None).unwrap();

    let assigned = success_json(run(&[
        "--json",
        "model",
        "assign",
        "repair-contract",
        "--agent",
        "grok-build",
        "--yes",
    ]));
    assert_eq!(assigned["command"], "model.assign");
    assert_eq!(assigned["changed"], true);

    let target = home.home.join(".grok/config.toml");
    let managed = fs::read_to_string(&target).unwrap();
    let drifted = managed.replace("repair-contract-model", "external-model");
    assert_ne!(managed, drifted);
    fs::write(&target, &drifted).unwrap();

    let repeated_assign = success_json(run(&[
        "--json",
        "model",
        "assign",
        "repair-contract",
        "--agent",
        "grok-build",
        "--yes",
    ]));
    assert_eq!(repeated_assign["command"], "model.assign");
    assert_eq!(repeated_assign["changed"], false);
    assert_eq!(fs::read_to_string(&target).unwrap(), drifted);

    let repeated_enable = success_json(run(&[
        "--json",
        "model",
        "enable",
        "repair-contract",
        "--agent",
        "grok-build",
        "--yes",
    ]));
    assert_eq!(repeated_enable["command"], "model.enable");
    assert_eq!(repeated_enable["changed"], false);
    assert_eq!(fs::read_to_string(&target).unwrap(), drifted);

    let repeated_use = success_json(run(&[
        "--json",
        "model",
        "use",
        "repair-contract",
        "--agent",
        "grok-build",
        "--yes",
    ]));
    assert_eq!(repeated_use["command"], "model.use");
    assert_eq!(repeated_use["changed"], false);
    assert_eq!(fs::read_to_string(&target).unwrap(), drifted);

    let repaired = success_json(run(&[
        "--json",
        "model",
        "converge",
        "repair-contract",
        "--agent",
        "grok-build",
        "restore",
        "--yes",
    ]));
    assert_eq!(repaired["command"], "converge");
    assert_eq!(repaired["changed"], true);
    assert!(fs::read_to_string(&target)
        .unwrap()
        .contains("repair-contract-model"));

    let clean = run(&[
        "--json",
        "model",
        "converge",
        "repair-contract",
        "--agent",
        "grok-build",
        "restore",
        "--yes",
    ]);
    assert!(!clean.status.success());
    let clean: serde_json::Value = serde_json::from_slice(&clean.stderr).unwrap();
    assert_eq!(clean["error"]["code"], "convergence_action_unavailable");
}
