use arca_core::{
    adapter::Adapter,
    resources::{
        deepseek_harness::{guard_mcp_profile, home_dir, profile_path, PatchFile},
        mcp::deepseek_harness_adapter::DeepseekHarnessAdapter,
    },
    testenv::TestHome,
    types::{HttpConfig, McpConfig, StdioConfig},
};
use serde_json::{json, Value};
use std::{ffi::OsString, fs, path::PathBuf};

struct Fixture {
    home: TestHome,
    previous: Option<OsString>,
    path: PathBuf,
}

impl Fixture {
    fn new(profile: &str) -> Self {
        let home = TestHome::new("deepseek-harness-mcp");
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", home.home.join(".dsh"));
        let path = home_dir()
            .join("profiles")
            .join(profile)
            .join("cordis.patch.yml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path.parent().unwrap().join("package.json"),
            json!({"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app"]}}})
                .to_string(),
        )
        .unwrap();
        Self {
            home,
            previous,
            path,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var("DSH_HOME", value);
        } else {
            std::env::remove_var("DSH_HOME");
        }
    }
}

fn local() -> McpConfig {
    McpConfig::Stdio(StdioConfig {
        command: "fixture-command".into(),
        args: Some(vec!["changed".into()]),
        env: None,
        cwd: None,
    })
}

const NATIVE: &str = include_str!("fixtures/deepseek-harness-profile.yaml");

#[test]
fn native_profile_creates_mcp_client_in_insert_block() {
    let f = Fixture::new("web");
    let original = "# profile metadata\n[]\n";
    fs::write(&f.path, original).unwrap();
    DeepseekHarnessAdapter.upsert(&f.path, "safe", &local()).unwrap();
    assert!(DeepseekHarnessAdapter.read(&f.path).contains_key("safe"));
    let rendered = fs::read_to_string(&f.path).unwrap();
    assert!(rendered.contains("mcp-safe"));
    assert!(rendered.contains("@deepseek-ai/dsh-mcp-client"));
    assert!(rendered.contains("safe"));
}

#[test]
fn legacy_manager_entry_is_visible_and_assignment_moves_only_that_server() {
    let f = Fixture::new("web");
    fs::write(&f.path, r#"# keep profile comment
- id: mcp-manager
  config:
    servers:
      old: # keep server name comment
        transport: stdio
        command: fixture-command
        enabled: false
        toolCallTimeoutMs: 21000 # keep selected policy
      sibling: {transport: stdio, command: sibling-command} # keep sibling
"#).unwrap();
    let adapter = DeepseekHarnessAdapter;
    assert!(!adapter.read_with_enabled(&f.path).contains_key("old"));
    assert_eq!(adapter.declaration_names(&fs::read_to_string(&f.path).unwrap()).unwrap(), ["old", "sibling"]);

    adapter.upsert(&f.path, "old", &local()).unwrap();
    let rendered = fs::read_to_string(&f.path).unwrap();
    let file = PatchFile::from_text(&rendered).unwrap();
    let installed = file.inspect_mcp("old").unwrap().unwrap();
    assert_eq!(installed["toolCallTimeoutMs"], 21000);
    assert_eq!(installed["disabled"], true);
    let manager = file.config("mcp-manager").unwrap().unwrap();
    assert!(manager["servers"].get("old").is_none());
    assert_eq!(manager["servers"]["sibling"]["command"], "sibling-command");
    for comment in ["# keep profile comment", "# keep server name comment", "# keep selected policy", "# keep sibling"] {
        assert!(rendered.contains(comment));
    }
}

#[test]
fn legacy_flow_entry_migrates_without_copying_its_separator() {
    let f = Fixture::new("web");
    fs::write(&f.path, r#"- {"id":"mcp-manager","config":{"servers":{"old":{"transport":"stdio","command":"fixture-command"},"sibling":{"transport":"stdio","command":"sibling"}}}}
"#).unwrap();

    DeepseekHarnessAdapter.upsert(&f.path, "old", &local()).unwrap();
    let rendered = fs::read_to_string(&f.path).unwrap();
    serde_yaml::from_str::<serde_yaml::Value>(&rendered).unwrap();
    let file = PatchFile::from_text(&rendered).unwrap();
    assert!(file.inspect_mcp("old").unwrap().is_some());
    let manager = file.config("mcp-manager").unwrap().unwrap();
    assert!(manager["servers"].get("old").is_none());
    assert_eq!(manager["servers"]["sibling"]["command"], "sibling");
}

#[test]
fn legacy_credential_template_is_observable_and_stays_in_place() {
    let f = Fixture::new("desktop");
    let text = r#"- id: mcp-manager
  config:
    servers:
      old:
        transport: streamable-http
        url: https://example.test/mcp
        headers: {Authorization: 'Bearer ${credential:LEGACY_MCP}'}
"#;
    fs::write(&f.path, text).unwrap();
    let adapter = DeepseekHarnessAdapter;
    assert_eq!(adapter.declaration_names(text).unwrap(), ["old"]);
    assert!(adapter.upsert(&f.path, "old", &local()).is_err());
    assert_eq!(fs::read_to_string(&f.path).unwrap(), text);
}

#[test]
fn native_web_and_desktop_patch_keeps_paused_state_policy_comments_and_unrelated_js() {
    for profile in ["web", "desktop"] {
        let f = Fixture::new(profile);
        fs::write(&f.path, NATIVE).unwrap();
        let adapter = DeepseekHarnessAdapter;
        let observed = adapter.read_with_enabled(&f.path);
        assert!(!observed["paused"].1);
        assert!(!adapter.read(&f.path).contains_key("paused"));
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&f.path).unwrap().ino()
        };
        adapter.upsert(&f.path, "paused", &local()).unwrap();
        let file = PatchFile::read(&f.path).unwrap();
        let paused = file.inspect_mcp("paused").unwrap().unwrap();
        assert_eq!(paused["disabled"], true);
        assert_eq!(paused["startupTimeoutMs"], 42000);
        assert_eq!(paused["futureServerPolicy"], json!({"keep":true}));
        let rendered = fs::read_to_string(&f.path).unwrap();
        for text in [
            "!!js dshHomePath('unrelated')",
            "# keep command comment",
            "# native pause",
            "# keep model comment",
        ] {
            assert!(rendered.contains(text), "{text}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&f.path).unwrap().ino(), inode);
        }
    }
}

#[test]
fn native_toggle_and_snapshot_removal_are_bound_to_the_complete_entry() {
    let f = Fixture::new("web");
    fs::write(&f.path, NATIVE).unwrap();
    let adapter = DeepseekHarnessAdapter;
    let before = adapter.snapshot(&f.path, "paused").unwrap().unwrap();
    adapter.set_enabled(&f.path, "paused", true, &before).unwrap();
    assert!(adapter.read(&f.path).contains_key("paused"));
    let bytes = fs::read(&f.path).unwrap();
    assert!(adapter.remove_snapshot(&f.path, "paused", &before).is_err());
    assert_eq!(fs::read(&f.path).unwrap(), bytes);
    let current = adapter.snapshot(&f.path, "paused").unwrap().unwrap();
    adapter.remove_snapshot(&f.path, "paused", &current).unwrap();
    adapter.restore(&f.path, "paused", &current).unwrap();
    assert!(adapter.restore(&f.path, "paused", &current).is_err());
    assert_eq!(adapter.snapshot(&f.path, "paused").unwrap(), Some(current));
}

#[test]
fn clear_removes_all_mcps_and_preserves_sibling_plugins_and_models() {
    let f = Fixture::new("desktop");
    fs::write(&f.path, NATIVE).unwrap();
    let adapter = DeepseekHarnessAdapter;
    let models = PatchFile::read(&f.path).unwrap().config("llm-pi-ai").unwrap();
    adapter.clear(&f.path).unwrap();
    let file = PatchFile::read(&f.path).unwrap();
    assert_eq!(file.mcp_servers().unwrap(), serde_json::Map::new());
    assert_eq!(file.config("llm-pi-ai").unwrap(), models);
    assert!(!adapter.has_entries(&f.path).unwrap());
    assert!(fs::read_to_string(&f.path).unwrap().contains("!!js dshHomePath('unrelated')"));
}

#[test]
fn home_override_custom_inheritance_disabled_or_duplicate_target_never_grants_a_writer() {
    let f = Fixture::new("web");
    let adapter = DeepseekHarnessAdapter;
    fs::write(
        home_dir().join("cordis.patch.yml"),
        "- id: mcp-safe\n  config: {}\n",
    )
    .unwrap();
    assert!(adapter.upsert(&f.path, "safe", &local()).is_err());
    assert!(!f.path.exists());
    fs::remove_file(home_dir().join("cordis.patch.yml")).unwrap();
    fs::write(
        f.path.parent().unwrap().join("package.json"),
        r#"{"dsh":{"profile":{"bundles":["@fixture/custom"]}}}"#,
    )
    .unwrap();
    assert!(adapter.upsert(&f.path, "safe", &local()).is_err());
    for text in [
        "- insert:\n    - id: mcp-safe\n      name: '@deepseek-ai/dsh-mcp-client'\n      config: {serverName: safe}\n    - id: mcp-safe\n      name: '@deepseek-ai/dsh-mcp-client'\n      config: {serverName: safe}\n",
        "- insert:\n    - id: mcp-1\n      name: '@deepseek-ai/dsh-mcp-client'\n      config: {serverName: safe}\n    - id: mcp-2\n      name: '@deepseek-ai/dsh-mcp-client'\n      config: {serverName: safe}\n",
        "- insert: !!js compute()\n",
    ] {
        fs::write(&f.path, text).unwrap();
        assert!(adapter.upsert(&f.path, "safe", &local()).is_err());
        assert_eq!(fs::read_to_string(&f.path).unwrap(), text);
    }
}

#[test]
fn existing_empty_or_whitespace_patch_is_not_repaired_or_overwritten() {
    let f = Fixture::new("web");
    let adapter = DeepseekHarnessAdapter;
    for text in ["", "  \n\t", "# no patch sequence\n"] {
        fs::write(&f.path, text).unwrap();
        assert!(PatchFile::read(&f.path).is_err());
        assert!(adapter.upsert(&f.path, "safe", &local()).is_err());
        assert_eq!(fs::read_to_string(&f.path).unwrap(), text);
    }
    fs::remove_file(&f.path).unwrap();
    assert_eq!(PatchFile::read(&f.path).unwrap().original(), None);
}

#[test]
fn server_identity_transport_cross_fields_and_credential_templates_are_native() {
    let f = Fixture::new("web");
    let adapter = DeepseekHarnessAdapter;
    for name in [
        "",
        "has.dot",
        "has space",
        "../escape",
        "012345678901234567890123456789012",
    ] {
        assert!(adapter.upsert(&f.path, name, &local()).is_err());
    }
    let sse = McpConfig::Http(HttpConfig {
        kind: "sse".into(),
        url: "https://example.test/mcp".into(),
        headers: None,
    });
    assert!(adapter.upsert(&f.path, "safe", &sse).is_err());
    adapter.upsert(&f.path, "Upper_OK-1", &local()).unwrap();
    let text = "- insert:\n    - id: mcp-unsafe\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: unsafe\n        transport: streamable-http\n        url: https://example.test/mcp\n        env: {KEEP: value}\n        cwd: /fixture\n";
    fs::write(&f.path, text).unwrap();
    assert!(adapter.upsert(&f.path, "unsafe", &local()).is_err());
    assert_eq!(fs::read_to_string(&f.path).unwrap(), text);
}

#[test]
fn shared_leaf_array_edit_and_whole_config_removal_preserve_sibling_comments() {
    let f = Fixture::new("web");
    fs::write(&f.path, NATIVE).unwrap();
    let mut file = PatchFile::read(&f.path).unwrap();
    file.set_config_path(
        "llm-pi-ai",
        &["providers", "external", "models", "0", "contextWindow"],
        &json!(24000),
    )
    .unwrap();
    let text = file.render().unwrap();
    assert!(text.contains("# keep model comment"));
    assert_eq!(
        file.config("llm-pi-ai").unwrap().unwrap()["providers"]["external"]["models"][0]
            ["policy"],
        json!({"keep":true})
    );
    file.set_config_field(
        "agent-default-model",
        "provider",
        &json!("fixture-provider"),
    )
    .unwrap();
    file.set_config_field("agent-default-model", "model", &json!("fixture-model"))
        .unwrap();
    file.remove_config_path("agent-default-model", &[]).unwrap();
    assert!(file.config("agent-default-model").unwrap().is_none());
    assert!(file.has_row("agent-default-model").unwrap());
    assert!(file.render().unwrap().contains("!!js dshHomePath('unrelated')"));
}

#[test]
fn readonly_declared_inspection_does_not_inherit_writer_policy_or_evaluate_js() {
    let _f = Fixture::new("web");
    let adapter = DeepseekHarnessAdapter;
    let text = "- insert:\n    - id: unrelated\n      name: '@fixture/unrelated'\n      config: !!js doNotRun()\n    - id: mcp-docs\n      name: '@deepseek-ai/dsh-mcp-client'\n      disabled: true\n      config: {serverName: docs, transport: future, token: fixture}\n";
    assert_eq!(adapter.declaration_names(text).unwrap(), ["docs"]);
    assert_eq!(
        adapter.inspect_text(text, "docs").unwrap().unwrap()["transport"],
        "future"
    );
}

#[cfg(unix)]
#[test]
fn shared_reader_and_writer_reject_symlink_parents_and_alias_targets() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("web");
    let outside = f.home.home.join("outside.yaml");
    fs::write(&outside, NATIVE).unwrap();
    symlink(&outside, &f.path).unwrap();
    assert!(PatchFile::read(&f.path).is_err());
    assert!(DeepseekHarnessAdapter.upsert(&f.path, "safe", &local()).is_err());
    assert_eq!(fs::read_to_string(&outside).unwrap(), NATIVE);
    assert!(PatchFile::from_text(
        "- id: mcp-manager\n  config: &config {servers: {}}\n- id: other\n  config: *config\n"
    )
    .is_err());
    assert!(profile_path("web").ends_with("/profiles/web/cordis.patch.yml"));
    assert!(guard_mcp_profile(&f.path, &["safe"]).is_err());
    let _ = Value::Null;
}

#[test]
fn e2e_plan_and_commit_deepseek_harness_mcps() {
    use arca_core::consumption::{
        commit_asset_operation, plan_set_agent_consumption, AgentConsumptionSelection,
        AssetCommitRequest, PlanSetAgentConsumptionRequest,
    };
    use arca_core::registry::write_manual_entry;
    use arca_core::types::{HttpConfig, RegistryConfig, RegistryEntry, StdioConfig};

    let f = Fixture::new("web");

    // Write initial clean cordis.patch.yml
    fs::write(&f.path, "- id: ui-settings-general\n  name: '@deepseek-ai/dsh-client-ui-settings-general'\n  config:\n    welcomeNoticeVersion: 2026-09-28.1\n").unwrap();

    // Register Exa (HTTP) and Jenkins (STDIO) in central registry
    write_manual_entry(&RegistryEntry {
        name: "exa".into(),
        description: "Exa search".into(),
        tags: vec![],
        config: RegistryConfig {
            http: Some(HttpConfig {
                kind: "http".into(),
                url: "https://mcp.exa.ai/mcp".into(),
                headers: None,
            }),
            stdio: None,
        },
        origin: None,
        repo: None,
    })
    .unwrap();

    write_manual_entry(&RegistryEntry {
        name: "jenkins-qx".into(),
        description: "Jenkins QX".into(),
        tags: vec![],
        config: RegistryConfig {
            stdio: Some(StdioConfig {
                command: "jenkins-cli".into(),
                args: Some(vec!["--server".into(), "http://jenkins.local".into()]),
                env: None,
                cwd: None,
            }),
            http: None,
        },
        origin: None,
        repo: None,
    })
    .unwrap();

    // Plan setting consumption for deepseek-harness
    let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "deepseek-harness".into(),
        selection: AgentConsumptionSelection::Mcp {
            asset_keys: vec![
                "exa::http".into(),
                "jenkins-qx::stdio".into(),
            ],
        },
    })
    .unwrap();

    // Commit operation
    let inventory = commit_asset_operation(AssetCommitRequest {
        operation_id: plan.operation_id.clone(),
        candidate_hash: plan.candidate_hash.clone(),
    })
    .unwrap();

    // Verify convergence!
    let converged = !inventory
        .target_incidents
        .iter()
        .any(|incident| incident.operation_id == plan.operation_id);
    assert!(converged, "Expected plan to converge, but had target incidents: {:?}", inventory.target_incidents);

    // Verify file content on disk
    let content = fs::read_to_string(&f.path).unwrap();
    assert!(content.contains("@deepseek-ai/dsh-mcp-client"));
    assert!(content.contains("\"serverName\":\"exa\"") || content.contains("serverName: exa"));
    assert!(content.contains("\"serverName\":\"jenkins-qx\"") || content.contains("serverName: jenkins-qx"));

    // Toggle jenkins-qx to disabled
    use arca_core::consumption::{plan_set_mcp_enabled, PlanSetMcpEnabledRequest};
    let toggle_plan = plan_set_mcp_enabled(PlanSetMcpEnabledRequest {
        agent_id: "deepseek-harness".into(),
        asset_key: "jenkins-qx::stdio".into(),
        enabled: false,
    })
    .unwrap();
    let inventory = commit_asset_operation(AssetCommitRequest {
        operation_id: toggle_plan.operation_id.clone(),
        candidate_hash: toggle_plan.candidate_hash.clone(),
    })
    .unwrap();
    let converged = !inventory
        .target_incidents
        .iter()
        .any(|incident| incident.operation_id == toggle_plan.operation_id);
    assert!(converged);
    let content_after_toggle = fs::read_to_string(&f.path).unwrap();
    assert!(content_after_toggle.contains("\"disabled\": true") || content_after_toggle.contains("\"disabled\":true") || content_after_toggle.contains("disabled: true"));

    // Remove exa
    let remove_plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
        agent_id: "deepseek-harness".into(),
        selection: AgentConsumptionSelection::Mcp {
            asset_keys: vec!["jenkins-qx::stdio".into()],
        },
    })
    .unwrap();
    let inventory = commit_asset_operation(AssetCommitRequest {
        operation_id: remove_plan.operation_id.clone(),
        candidate_hash: remove_plan.candidate_hash.clone(),
    })
    .unwrap();
    let converged = !inventory
        .target_incidents
        .iter()
        .any(|incident| incident.operation_id == remove_plan.operation_id);
    assert!(converged);
    let content_after_remove = fs::read_to_string(&f.path).unwrap();
    assert!(!content_after_remove.contains("exa"));
    assert!(content_after_remove.contains("jenkins-qx"));
}

