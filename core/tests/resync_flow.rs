//! `ops::resync_entry` end-to-end: it re-stamps a clean install, skips a
//! hand-customized one unless forced, and force overwrites it.

use std::collections::HashMap;

use arca_core::ops;
use arca_core::registry::write_manual_entry;
use arca_core::testenv::TestHome;
use arca_core::types::{RegistryConfig, RegistryEntry, StdioConfig};

fn stdio_entry(args: &[&str]) -> RegistryEntry {
    RegistryEntry {
        name: "srv".into(),
        description: String::new(),
        tags: vec![],
        config: RegistryConfig {
            stdio: Some(StdioConfig {
                command: "npx".into(),
                args: Some(args.iter().map(|s| s.to_string()).collect()),
                env: None,
                cwd: None,
            }),
            http: None,
        },
        origin: None,
        repo: None,
    }
}

fn is_customized(agent: &str) -> bool {
    ops::scan_installed(None)
        .iter()
        .find(|i| i.agent == agent && i.name == "srv")
        .map(|i| i.customized)
        .expect("srv should be installed for the agent")
}

#[test]
fn resync_pushes_clean_skips_then_forces_customized() {
    let _th = TestHome::new("resync");

    // Seed a manual entry (v1) and install it to a builtin global agent.
    write_manual_entry(&stdio_entry(&["-y", "srv-a"])).unwrap();
    ops::install(
        "srv",
        "stdio",
        "global",
        &["claude-code".into()],
        None,
        &HashMap::new(),
    )
    .unwrap();
    assert!(!is_customized("claude-code"), "fresh install is clean");

    // Edit the entry directly (v2) — this bypasses upsert_entry's auto-propagation,
    // so the on-disk config stays v1 while the registry says v2 → "customized".
    write_manual_entry(&stdio_entry(&["-y", "srv-b"])).unwrap();
    assert!(
        is_customized("claude-code"),
        "on-disk now drifts from registry"
    );

    // Safe resync skips the customized install and reports it.
    let out = ops::resync_entry("srv", "stdio", false).unwrap();
    assert!(
        out.synced.is_empty(),
        "nothing synced when customized + !force"
    );
    assert_eq!(out.skipped_customized, vec!["claude-code".to_string()]);
    assert!(
        is_customized("claude-code"),
        "still stale after a safe resync"
    );

    // Forced resync overwrites it → on-disk matches registry again (clean).
    let out = ops::resync_entry("srv", "stdio", true).unwrap();
    assert_eq!(out.synced, vec!["claude-code".to_string()]);
    assert!(out.skipped_customized.is_empty());
    assert!(!is_customized("claude-code"), "forced resync re-stamped v2");
}

#[test]
fn edit_reports_agent_sync_failure_without_touching_its_config() {
    let th = TestHome::new("resync-error");

    write_manual_entry(&stdio_entry(&["-y", "srv-a"])).unwrap();
    ops::install(
        "srv",
        "stdio",
        "global",
        &["claude-code".into()],
        None,
        &HashMap::new(),
    )
    .unwrap();
    let agent_path = th.home.join(".claude.json");
    let before = std::fs::read_to_string(&agent_path).unwrap();

    let backups = th.home.join(".arca/backups");
    let _ = std::fs::remove_dir_all(&backups);
    std::fs::write(&backups, "block backup directory").unwrap();

    let error = ops::upsert_entry(stdio_entry(&["-y", "srv-b"])).unwrap_err();
    assert!(error.contains("Agent sync failed"));
    assert_eq!(std::fs::read_to_string(agent_path).unwrap(), before);
}
