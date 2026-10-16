//! Native-document fixtures never touch real Agent files or Keychain entries.
use arca_core::{application::instructions, testenv::TestHome};
use std::fs;
use instructions::InstructionPathUpdate;
struct NativeEnv(&'static str, Option<std::ffi::OsString>);
impl NativeEnv {
    fn set(name: &'static str, value: &std::path::Path) -> Self {
        let old = std::env::var_os(name); std::env::set_var(name, value); Self(name, old)
    }
}
impl Drop for NativeEnv {
    fn drop(&mut self) { if let Some(value) = &self.1 { std::env::set_var(self.0, value); } else { std::env::remove_var(self.0); } }
}

#[test]
fn opencode_initialization_is_explicit_for_cli_and_desktop() {
    for agent in ["opencode", "opencode-desktop"] {
        let home = TestHome::new("opencode-instructions-create");
        let directory = home.home.join(".config/opencode");
        let _native_root = NativeEnv::set("OPENCODE_CONFIG_DIR", &directory);
        let target = directory.join("AGENTS.md");
        let inventory = instructions::list(agent).unwrap();
        assert!(inventory.complete);
        assert_eq!(inventory.create.as_ref().unwrap().path, target.to_string_lossy());
        assert!(!directory.exists());
        let plan = instructions::prepare_create(agent, &inventory.revision, "My global instructions\n").unwrap();
        assert!(!directory.exists());
        let saved = instructions::commit(&plan.operation_id, &plan.candidate_hash).unwrap();
        assert_eq!(saved.text, "My global instructions\n");
        assert_eq!(fs::read_to_string(&target).unwrap(), "My global instructions\n");
        assert!(instructions::list(agent).unwrap().create.is_none());
        assert!(!home.home.join(".claude").exists());
    }
}

#[test]
fn opencode_creation_uses_selected_directory_and_preserves_compatibility_files() {
    let home = TestHome::new("opencode-instructions-selected");
    let native = home.home.join(".config/opencode");
    let _native_root = NativeEnv::set("OPENCODE_CONFIG_DIR", &native);
    let config = instructions::get_paths("opencode").unwrap();
    instructions::set_paths("opencode", &config.revision, vec![InstructionPathUpdate { id: "global".into(), path: Some("~/selected-opencode".into()) }]).unwrap();
    let inventory = instructions::list("opencode").unwrap();
    assert_eq!(inventory.create.as_ref().unwrap().path, home.home.join("selected-opencode/AGENTS.md").to_string_lossy());
    let plan = instructions::prepare_create("opencode", &inventory.revision, "New content\n").unwrap();
    let fallback = home.home.join(".claude/CLAUDE.md");
    fs::create_dir_all(fallback.parent().unwrap()).unwrap();
    fs::write(&fallback, "Existing compatibility instructions").unwrap();
    assert!(instructions::commit(&plan.operation_id, &plan.candidate_hash).is_err());
    assert!(instructions::list("opencode").unwrap().create.is_none());
    assert_eq!(fs::read_to_string(&fallback).unwrap(), "Existing compatibility instructions");
    assert!(!home.home.join("selected-opencode").exists());
    assert!(!native.exists());
}

#[test]
fn directory_preferences_select_files_without_migration_and_reject_stale_edits() {
    let home = TestHome::new("instructions-custom-directory");
    let _native_root = NativeEnv::set("CODEX_HOME", &home.home.join(".codex"));
    let original = home.home.join(".codex/AGENTS.md");
    let selected = home.home.join("custom-codex/AGENTS.md");
    for path in [&original, &selected] { fs::create_dir_all(path.parent().unwrap()).unwrap(); }
    fs::write(&original, "native default").unwrap(); fs::write(&selected, "selected native file").unwrap();
    let before = instructions::list("codex").unwrap();
    let snapshot = instructions::read("codex", &before.files[0].id).unwrap();
    let pending = instructions::prepare_save("codex", &snapshot.file.id, &snapshot.revision, "old draft").unwrap();
    let config = instructions::get_paths("codex").unwrap();
    let changed = instructions::set_paths("codex", &config.revision, vec![InstructionPathUpdate { id: config.sources[0].id.clone(), path: Some("~/custom-codex".into()) }]).unwrap();
    assert_eq!(instructions::list("codex").unwrap().files[0].path, selected.to_string_lossy());
    assert!(instructions::commit(&pending.operation_id, &pending.candidate_hash).is_err());
    assert_eq!(fs::read_to_string(&original).unwrap(), "native default");
    assert_eq!(fs::read_to_string(&selected).unwrap(), "selected native file");
    assert!(instructions::set_paths("codex", &config.revision, vec![InstructionPathUpdate { id: config.sources[0].id.clone(), path: None }]).is_err());
    instructions::set_paths("codex", &changed.revision, vec![InstructionPathUpdate { id: changed.sources[0].id.clone(), path: None }]).unwrap();
    assert_eq!(instructions::list("codex").unwrap().files[0].path, original.to_string_lossy());
}

#[test]
fn rule_directory_preferences_never_initialize_until_explicit_content_save() {
    let home = TestHome::new("instructions-custom-rules");
    let config = instructions::get_paths("augment").unwrap();
    instructions::set_paths("augment", &config.revision, vec![InstructionPathUpdate { id: config.sources[0].id.clone(), path: Some("~/custom-rules".into()) }]).unwrap();
    let target = home.home.join("custom-rules/global.md");
    assert!(!target.parent().unwrap().exists());
    let inventory = instructions::list("augment").unwrap();
    assert_eq!(inventory.create.as_ref().unwrap().path, target.to_string_lossy());
    let plan = instructions::prepare_create("augment", &inventory.revision, "Explicit instructions\n").unwrap();
    assert!(!target.exists());
    instructions::commit(&plan.operation_id, &plan.candidate_hash).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "Explicit instructions\n");
    assert!(!home.home.join(".augment").exists());
}
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
