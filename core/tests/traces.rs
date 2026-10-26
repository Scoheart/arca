//! Synthetic, isolated transcript fixtures: no real HOME, credentials or sessions.
use arca_core::{application::traces, testenv::TestHome};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn jsonl(path: &Path, records: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, records.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
}
#[test]
fn pi_keeps_full_tool_inputs_results_and_redacts_password_context() {
    let home = TestHome::new("trace-pi");
    let file = home.home.join(".pi/agent/sessions/project/session.jsonl");
    let original = vec![
        json!({"type":"session","id":"s","cwd":"/synthetic","timestamp":"2026-01-01T00:00:00Z"}),
        json!({"type":"message","id":"a","timestamp":"2026-01-01T00:00:01Z","message":{"role":"user","content":"密码 739182"}}),
        json!({"type":"message","id":"b","parentId":"a","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hidden fixture"},{"type":"toolCall","id":"call1","name":"act_ui","arguments":{"text":"739182","extra":{"preserved":true}}}]}}),
        json!({"type":"message","id":"c","parentId":"b","message":{"role":"toolResult","toolName":"act_ui","toolCallId":"call1","content":[{"type":"text","text":"original returned data"}],"isError":true,"details":{"kept":123}}}),
    ];
    jsonl(&file, &original);
    let bytes = fs::read(&file).unwrap();
    let session = traces::index().unwrap().sessions.into_iter().find(|s| s.format == "pi").unwrap();
    let page = traces::page(&session.id, None).unwrap();
    assert_eq!(page.events.len(), 3);
    let call = page.events.iter().find(|e| e.kind == "tool_call").unwrap();
    let detail = traces::detail(&session.id, &call.id, &page.revision).unwrap();
    assert_eq!(detail.raw["arguments"]["text"], "[密码已脱敏]");
    assert_eq!(detail.raw["arguments"]["extra"]["preserved"], true);
    assert_eq!(detail.related[0].raw["message"]["details"]["kept"], 123);
    assert_eq!(detail.related[0].text, "original returned data");
    assert!(detail.related[0].raw["message"]["isError"].as_bool().unwrap());
    let output = serde_json::to_string(&detail).unwrap();
    assert!(!output.contains("739182"));
    assert!(!output.contains("hidden fixture"));
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert!(traces::export_detail(&session.id, &call.id, &page.revision, &file).is_err());
    let exported = home.home.join("export.json");
    traces::export_detail(&session.id, &call.id, &page.revision, &exported).unwrap();
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(fs::metadata(exported).unwrap().permissions().mode() & 0o777, 0o600); }
}
#[test]
fn inline_conversation_obeys_utf8_page_budgets_and_redacts_before_slicing() {
    let home = TestHome::new("trace-inline");
    let file = home.home.join(".pi/agent/sessions/project/session.jsonl");
    let secret = "synthetic-inline-secret";
    let text = format!("{}{}🧰", "对".repeat(10920), secret);
    let mut records = vec![json!({"type":"session","id":"inline"})];
    for _ in 0..58 { records.push(json!({"type":"message","message":{"role":"assistant","content":text}})); }
    records.push(json!({"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"s","name":"fixture","arguments":{"api_key":secret}}]}}));
    jsonl(&file, &records);
    let session = traces::index().unwrap().sessions.into_iter().find(|s| s.format == "pi").unwrap();
    let page = traces::page(&session.id, None).unwrap();
    let bodies: Vec<_> = page.events.iter().filter_map(|e| e.text.as_deref()).collect();
    assert!(bodies.iter().all(|text| text.len() <= 32 * 1024));
    assert!(bodies.iter().map(|text| text.len()).sum::<usize>() <= 512 * 1024);
    assert!(page.events.iter().any(|e| e.text_truncated));
    assert!(!serde_json::to_string(&page).unwrap().contains(secret));
    let first = &page.events[0];
    let full = traces::detail(&session.id, &first.id, &page.revision).unwrap();
    assert!(!full.text.contains(secret));
    assert!(full.text.ends_with('🧰'));
}
#[test]
fn codex_paginates_without_mirrored_messages_or_reasoning() {
    let home = TestHome::new("trace-codex");
    let file = home.home.join(".codex/sessions/2026/01/01/rollout-fixture.jsonl");
    let mut values = vec![json!({"type":"session_meta","payload":{"id":"s","cwd":"/synthetic","base_instructions":"hidden fixture"}})];
    for _ in 0..70 { values.push(json!({"type":"event_msg","payload":{"type":"user_message","message":"mirror"}})); }
    values.extend([
        json!({"type":"response_item","payload":{"type":"reasoning","encrypted_content":"hidden"}}),
        json!({"type":"response_item","payload":{"type":"function_call","name":"shell","call_id":"c","arguments":"{\"command\":\"echo fixture\"}"}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"c","output":"original stdout"}}),
    ]);
    jsonl(&file, &values);
    let session = traces::index().unwrap().sessions.into_iter().find(|s| s.format == "codex").unwrap();
    let first = traces::page(&session.id, None).unwrap();
    assert!(first.events.is_empty()); assert!(first.next_cursor.is_some());
    let second = traces::page(&session.id, first.next_cursor.as_deref()).unwrap();
    assert_eq!(second.events.len(), 2);
    let call = traces::detail(&session.id, &second.events[0].id, &second.revision).unwrap();
    assert_eq!(call.related[0].raw["payload"]["output"], "original stdout");
    fs::write(&file, "{}\n").unwrap();
    assert!(traces::page(&session.id, first.next_cursor.as_deref()).is_err());
    assert!(traces::detail(&session.id, &second.events[0].id, &second.revision).is_err());
}
#[test]
fn claude_tool_blocks_are_paired_by_id_not_adjacency() {
    let home = TestHome::new("trace-claude");
    let file = home.home.join(".claude/projects/project/fixture.jsonl");
    jsonl(&file, &[
        json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"a","name":"Read","input":{"file_path":"/fixture"}},{"type":"tool_use","id":"b","name":"Bash","input":{"command":"echo fixture"}}]}}),
        json!({"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b","content":"second result"},{"type":"tool_result","tool_use_id":"a","content":"first result"}]}}),
    ]);
    let session = traces::index().unwrap().sessions.into_iter().find(|s| s.format == "claude").unwrap();
    let page = traces::page(&session.id, None).unwrap();
    let detail = traces::detail(&session.id, &page.events[0].id, &page.revision).unwrap();
    assert_eq!(detail.related.len(), 1);
    assert_eq!(detail.related[0].raw["content"], "first result");
}
#[test]
fn gemini_journal_checkpoint_replaces_messages_and_preserves_native_tools() {
    let home = TestHome::new("trace-gemini");
    let file = home.home.join(".gemini/tmp/project/chats/session-fixture.jsonl");
    jsonl(&file, &[
        json!({"sessionId":"s","projectHash":"fixture"}),
        json!({"$set":{"messages":[{"id":"u","type":"user","content":[{"text":"hello"}]}]}}),
        json!({"id":"a","type":"gemini","content":"answer","thoughts":["hidden fixture"],"toolCalls":[{"id":"t","name":"read_file","args":{"path":"/fixture"},"result":{"stdout":"original result"},"status":"success"}]}),
    ]);
    let session = traces::index().unwrap().sessions.into_iter().find(|s| s.format == "gemini").unwrap();
    let page = traces::page(&session.id, None).unwrap();
    assert_eq!(page.events.iter().filter(|e| e.kind == "user").count(), 1);
    let call = page.events.iter().find(|e| e.kind == "tool_call").unwrap();
    let detail = traces::detail(&session.id, &call.id, &page.revision).unwrap();
    assert_eq!(detail.related[0].raw["stdout"], "original result");
    let answer = page.events.iter().find(|e| e.kind == "assistant").unwrap();
    assert!(traces::detail(&session.id, &answer.id, &page.revision).unwrap().raw.get("thoughts").is_none());
}
#[test]
fn unknown_import_stays_raw_and_rejects_symlink_records() {
    let home = TestHome::new("trace-import");
    let file = home.home.join("export.json");
    fs::write(&file, r#"[{"type":"vendor-event","unknown":{"kept":true},"api_key":"synthetic-credential"}]"#).unwrap();
    let session = traces::import(&file).unwrap();
    assert_eq!(session.format, "generic"); assert!(session.imported);
    let page = traces::page(&session.id, None).unwrap();
    let detail = traces::detail(&session.id, &page.events[0].id, &page.revision).unwrap();
    assert_eq!(detail.raw["unknown"]["kept"], true);
    assert_eq!(detail.raw["api_key"], "[凭据已脱敏]");
    #[cfg(unix)] {
        let alias = home.home.join("alias.json");
        std::os::unix::fs::symlink(&file, &alias).unwrap();
        assert!(traces::import(&alias).is_err());
    }
}

#[test]
fn independent_pages_and_details_rebuild_credential_context() {
    let home = TestHome::new("trace-cold-redaction");
    let file = home.home.join("context.jsonl");
    let short = "opaqueFixtureCredential";
    let long = "opaqueFixtureCredentialTAIL_SHOULD_NOT_LEAK";
    let mut records: Vec<_> = (0..65).map(|index| json!({"role":"assistant", "content":format!("fixture {index}")})).collect();
    records[0]["title"] = json!(long);
    records[0]["cwd"] = json!(format!("/fixture/{long}"));
    records[39]["api_key"] = json!(short);
    // A longer credential can be declared after its first echo on this page.
    records[40]["content"] = json!(long);
    records[49]["client-secret"] = json!(long);
    records[61]["content"] = json!(format!("echo {short} and {long}"));
    records[61]["kept"] = json!(true);
    jsonl(&file, &records);
    let session = traces::import(&file).unwrap();
    let first = traces::page(&session.id, None).unwrap();
    let encoded = serde_json::to_string(&first).unwrap();
    assert!(!encoded.contains(short));
    assert!(!encoded.contains("TAIL_SHOULD_NOT_LEAK"));
    // Detail may be requested directly, without retaining the page's redactor.
    let session = traces::import(&file).unwrap();
    let early_detail = traces::detail(&session.id, &first.events[40].id, &first.revision).unwrap();
    let encoded = serde_json::to_string(&early_detail).unwrap();
    assert!(!encoded.contains(short));
    assert!(!encoded.contains("TAIL_SHOULD_NOT_LEAK"));
    let session = traces::import(&file).unwrap();
    let next = traces::page(&session.id, first.next_cursor.as_deref()).unwrap();
    assert!(!serde_json::to_string(&next).unwrap().contains(short));
    let session = traces::import(&file).unwrap();
    let detail = traces::detail(&session.id, &next.events[1].id, &next.revision).unwrap();
    let encoded = serde_json::to_string(&detail).unwrap();
    assert!(!encoded.contains(short));
    assert!(!encoded.contains("TAIL_SHOULD_NOT_LEAK"));
    assert_eq!(detail.raw["kept"], true);
}
