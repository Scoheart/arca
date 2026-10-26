// End-to-end for the source-based catalog: add a local config file as a source,
// confirm its servers populate the catalog, toggle it off/on, and remove it.
// HOME/ARCA_HOME are redirected to a temp dir so ~/.arca stays isolated. No network.
use std::fs;
use tauri::async_runtime::block_on;

use desktop_lib::commands::{
    add_builtin_collection, add_local_source, list_registry, remove_source, set_source_enabled,
};

#[test]
fn local_source_flow_populates_toggles_and_removes() {
    let th = arca_core::testenv::TestHome::new("sources");
    let home = th.home.clone();

    // A standard mcpServers config file the user "adds" as a local source.
    let cfg = home.join("team-mcp.json");
    fs::write(
        &cfg,
        r#"{"account":{"token":"must-not-be-cached"},"history":{"enabled":true},"mcpServers":{
            "git":{"command":"npx","args":["-y","git-mcp"]},
            "wiki":{"url":"https://deepwiki.example/mcp","type":"http"}
        }}"#,
    )
    .unwrap();

    // Empty catalog to start (no built-in base anymore).
    assert_eq!(block_on(list_registry()).unwrap().len(), 0, "catalog should start empty");

    // Add it as a local source.
    let view = block_on(add_local_source(cfg.display().to_string(), Some("团队配置".into())))
        .expect("add_local_source should succeed");
    assert_eq!(view.kind, "local");
    assert_eq!(view.server_count, 2);
    let id = view.id.clone();

    // Its two servers now populate the catalog, tagged with the source origin.
    let cat = block_on(list_registry()).unwrap();
    assert_eq!(cat.len(), 2, "both servers should be in the catalog");
    assert!(cat.iter().all(|e| e
        .origin
        .as_ref()
        .map(|o| o.kind == "local" && o.source.as_deref() == Some(id.as_str()))
        .unwrap_or(false)));
    assert!(cat
        .iter()
        .any(|e| e.name == "git" && e.transport() == "stdio"));
    assert!(cat
        .iter()
        .any(|e| e.name == "wiki" && e.transport() == "http"));

    // The cached copy exists under ~/.arca/assets/mcps/sources/local/.
    let cached = home
        .join(".arca")
        .join("sources")
        .join("local")
        .join(format!("{id}.json"));
    assert!(
        cached.exists(),
        "cached local copy should exist at {cached:?}"
    );
    let cached_text = fs::read_to_string(&cached).unwrap();
    assert!(cached_text.contains("mcpServers"));
    assert!(!cached_text.contains("account"));
    assert!(!cached_text.contains("history"));
    assert!(!cached_text.contains("must-not-be-cached"));

    // Disabling the source removes its servers from the catalog…
    block_on(set_source_enabled(id.clone(), false)).unwrap();
    assert_eq!(
        block_on(list_registry()).unwrap().len(),
        0,
        "disabled source contributes nothing"
    );
    // …and re-enabling brings them back.
    block_on(set_source_enabled(id.clone(), true)).unwrap();
    assert_eq!(block_on(list_registry()).unwrap().len(), 2);

    // Removing the source deletes its cached file and empties the catalog.
    block_on(remove_source(id.clone())).unwrap();
    assert_eq!(block_on(list_registry()).unwrap().len(), 0);
    assert!(!cached.exists(), "cached file should be deleted on remove");

    // The opt-in curated collection is available as a local source (no network).
    let curated = block_on(add_builtin_collection()).expect("add_builtin_collection should succeed");
    let expected = arca_core::registry::builtin_registry().len();
    assert_eq!(curated.kind, "local");
    assert_eq!(curated.name, "Arca 精选");
    assert_eq!(curated.server_count as usize, expected);
    assert_eq!(block_on(list_registry()).unwrap().len(), expected);
}
