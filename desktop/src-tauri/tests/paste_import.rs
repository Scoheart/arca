// Pasting a config blob (e.g. a `mcpServers` JSON/YAML object) adds its servers to the
// managed "manual" source.
use desktop_lib::commands::list_registry;
use tauri::async_runtime::block_on;

fn import_pasted_config(text: String) -> arca_core::domain::error::CoreResult<Vec<String>> {
    tauri::async_runtime::block_on(desktop_lib::commands::import_pasted_config(text))
}

#[test]
fn paste_import_recognizes_and_adds_manual_entries() {
    let th = arca_core::testenv::TestHome::new("paste");
    let h = th.home.clone();

    // 1) A standard mcpServers block (the yunxiao example).
    let text = r#"{
      "mcpServers": {
        "yunxiao": {
          "command": "npx",
          "args": ["-y", "alibabacloud-devops-mcp-server"],
          "env": { "YUNXIAO_ACCESS_TOKEN": "<YOUR_TOKEN>" }
        }
      }
    }"#;
    let added = import_pasted_config(text.into()).expect("paste import should succeed");
    assert_eq!(added, vec!["yunxiao".to_string()]);

    let e = block_on(list_registry()).unwrap()
        .into_iter()
        .find(|e| e.name == "yunxiao")
        .expect("yunxiao present");
    assert_eq!(e.origin.as_ref().unwrap().kind, "manual");
    let stdio = e.config.stdio.as_ref().unwrap();
    assert_eq!(stdio.command, "npx");
    assert_eq!(
        stdio
            .env
            .as_ref()
            .unwrap()
            .get("YUNXIAO_ACCESS_TOKEN")
            .unwrap(),
        "<YOUR_TOKEN>"
    );
    assert!(h.join(".arca/sources/local/manual.json").exists());

    // 2) A bare name->config map (no mcpServers wrapper).
    let added = import_pasted_config(r#"{"git":{"command":"npx","args":["-y","git-mcp"]}}"#.into())
        .unwrap();
    assert_eq!(added, vec!["git".to_string()]);
    assert!(block_on(list_registry()).unwrap().iter().any(|e| e.name == "git"));

    // 3) YAML maps use the same safe import path.
    let added = import_pasted_config(
        r#"mcpServers:
  docs:
    url: https://example.com/mcp
    headers:
      Authorization: Bearer test
"#
        .into(),
    )
    .unwrap();
    assert_eq!(added, vec!["docs".to_string()]);
    let docs = block_on(list_registry()).unwrap()
        .into_iter()
        .find(|entry| entry.name == "docs")
        .expect("YAML entry present");
    assert_eq!(
        docs.config.http.unwrap().headers.unwrap()["Authorization"],
        "Bearer test"
    );

    // 4) Junk / no servers → error.
    assert!(import_pasted_config("hello world".into()).is_err());
    assert!(import_pasted_config(r#"{"foo":123}"#.into()).is_err());
    assert!(import_pasted_config("".into()).is_err());
}
