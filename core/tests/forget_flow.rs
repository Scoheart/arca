//! `ops::forget_entry` removes a manual entry from the catalog AND uninstalls it
//! from the agents that had it.

use std::collections::HashMap;

use arca_core::ops;
use arca_core::registry::{read_registry, write_manual_entry};
use arca_core::testenv::TestHome;
use arca_core::types::{RegistryConfig, RegistryEntry, StdioConfig};

#[test]
fn forget_removes_from_catalog_and_uninstalls() {
    let _th = TestHome::new("forget");

    write_manual_entry(&RegistryEntry {
        name: "srv".into(),
        description: String::new(),
        tags: vec![],
        config: RegistryConfig {
            stdio: Some(StdioConfig {
                command: "npx".into(),
                args: None,
                env: None,
                cwd: None,
            }),
            http: None,
        },
        origin: None,
        repo: None,
    })
    .unwrap();
    ops::install(
        "srv",
        "stdio",
        "global",
        &["claude-code".into()],
        None,
        &HashMap::new(),
    )
    .unwrap();

    // Present in the catalog and installed in the agent.
    assert!(read_registry().iter().any(|e| e.name == "srv"));
    assert!(ops::scan_installed(None)
        .iter()
        .any(|i| i.agent == "claude-code" && i.name == "srv"));

    // Forget: gone from the catalog and uninstalled from the agent.
    ops::forget_entry("srv", "stdio").unwrap();
    assert!(
        !read_registry().iter().any(|e| e.name == "srv"),
        "removed from catalog"
    );
    assert!(
        !ops::scan_installed(None).iter().any(|i| i.name == "srv"),
        "uninstalled from agents"
    );
}
