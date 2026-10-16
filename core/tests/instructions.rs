//! Native-document fixtures never touch real Agent files or Keychain entries.
use arca_core::{application::instructions, testenv::TestHome};
use std::fs;
#[test]
fn viewing_is_read_only_and_initialization_never_upserts() {
    let home = TestHome::new("instructions-create");
    let inventory = instructions::list("claude-code").unwrap();
    assert!(inventory.create.is_some());
    assert!(!home.home.join(".claude").exists());
    let plan = instructions::prepare_create("claude-code", &inventory.revision, "User instructions\n").unwrap();
    assert!(!home.home.join(".claude").exists());
    fs::create_dir(home.home.join(".claude")).unwrap();
    fs::write(home.home.join(".claude/CLAUDE.md"), "external content").unwrap();
    assert!(instructions::commit(&plan.operation_id, &plan.candidate_hash).is_err());
    assert_eq!(fs::read_to_string(home.home.join(".claude/CLAUDE.md")).unwrap(), "external content");
    assert!(instructions::list("claude-code").unwrap().create.is_none());
}
#[test]
fn edits_keep_original_bytes_and_refuse_external_replacement() {
    let home = TestHome::new("instructions-save");
    let path = home.home.join(".claude/CLAUDE.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "\u{feff}---\r\ncustom: preserved\r\n---\nHello\r\nTail").unwrap();
    let file = instructions::list("claude-code").unwrap().files.remove(0);
    let snapshot = instructions::read("claude-code", &file.id).unwrap();
    let plan = instructions::prepare_save("claude-code", &file.id, &snapshot.revision, &snapshot.text.replace("Hello", "Edited").replace("\r\n", "\n")).unwrap();
    #[cfg(unix)] let inode = { use std::os::unix::fs::MetadataExt; fs::metadata(&path).unwrap().ino() };
    instructions::commit(&plan.operation_id, &plan.candidate_hash).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "\u{feff}---\r\ncustom: preserved\r\n---\nEdited\r\nTail");
    #[cfg(unix)] { use std::os::unix::fs::MetadataExt; assert_eq!(fs::metadata(&path).unwrap().ino(), inode); }
    let current = instructions::read("claude-code", &file.id).unwrap();
    let stale = instructions::prepare_save("claude-code", &file.id, &current.revision, "draft").unwrap();
    fs::write(&path, "external change").unwrap();
    assert!(instructions::commit(&stale.operation_id, &stale.candidate_hash).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "external change");
    fs::remove_file(&path).unwrap();
    assert!(instructions::prepare_save("claude-code", &file.id, &current.revision, "draft").is_err());
    assert!(!path.exists());
}
#[test]
fn existing_rules_or_empty_override_block_extra_initialization() {
    let home = TestHome::new("instructions-rules");
    fs::create_dir_all(home.home.join(".claude/rules")).unwrap();
    let path = home.home.join(".claude/rules/user.md");
    fs::write(&path, "---\npaths: src/**\ncustom: keep\n---\nExisting rule").unwrap();
    let inventory = instructions::list("claude-code").unwrap();
    assert_eq!(inventory.files.len(), 1); assert!(inventory.create.is_none());
    let before = fs::read(&path).unwrap(); instructions::read("claude-code", &inventory.files[0].id).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::create_dir(home.home.join(".codex")).unwrap();
    fs::write(home.home.join(".codex/AGENTS.override.md"), "").unwrap();
    assert!(instructions::list("codex").unwrap().create.is_none());
}
