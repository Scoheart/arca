//! Explicit native Codex document editing. No central asset projection or auth copying.
use crate::{domain::error::{CoreError, CoreResult}, safe_write::{self, AnchoredPathState}};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::{Component, PathBuf}};

const MAX_BYTES: u64 = 1024 * 1024;
const FIELDS: &[&str] = &["model", "model_reasoning_effort", "service_tier", "approval_policy", "sandbox_mode", "web_search"];

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source { Native, Arca }

#[derive(Serialize)]
pub struct Rendered { pub text: String, pub fields: BTreeMap<String, String> }
#[derive(Serialize)]
pub struct Snapshot {
    pub path: String, pub exists: bool, pub revision: String, pub text: String,
    pub fields: BTreeMap<String, String>, pub valid: bool,
}

fn error(code: &str, message: &str) -> CoreError { CoreError::new(code, message) }
fn conflict() -> CoreError { error("codex_config_conflict", "配置文件已被其他程序修改，草稿已保留，请重新读取后再保存。") }
fn hash(bytes: impl AsRef<[u8]>) -> String { hex::encode(Sha256::digest(bytes.as_ref())) }
fn path(source: Source, codex_home: Option<&str>) -> CoreResult<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| error("codex_config_home", "无法找到用户目录。"))?;
    let configured = codex_home.map(str::to_owned).or_else(|| std::env::var("CODEX_HOME").ok());
    let root = match configured {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home.join(".codex"),
    };
    if !root.is_absolute() || root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(error("codex_config_path", "CODEX_HOME 必须是绝对目录，且不能包含上级路径。"));
    }
    Ok(root.join(match source { Source::Native => "config.toml", Source::Arca => "arca.config.toml" }))
}

fn parse(text: &str) -> CoreResult<toml_edit::Document> {
    if text.len() as u64 > MAX_BYTES || text.contains('\0') {
        return Err(error("codex_config_limit", "配置文件需要是不超过 1 MiB 的 UTF-8 TOML 文本。"));
    }
    text.trim_start_matches('\u{feff}').parse::<toml_edit::Document>().map_err(|e| {
        let line = e.span().map(|span| text.as_bytes()[..span.start.min(text.len())].iter().filter(|b| **b == b'\n').count() + 1).unwrap_or(1);
        error("codex_config_toml", &format!("TOML 格式无效，请检查第 {line} 行附近的内容。"))
    })
}
fn fields(document: &toml_edit::Document) -> BTreeMap<String, String> {
    FIELDS.iter().filter_map(|key| match document.get(key) {
        None => Some(((*key).into(), String::new())),
        Some(item) => item.as_str().map(|value| ((*key).into(), value.into())),
    }).collect()
}
fn render_inner(text: &str, patch: BTreeMap<String, String>) -> CoreResult<Rendered> {
    let mut document = parse(text)?;
    for (key, value) in patch {
        if !FIELDS.contains(&key.as_str()) { return Err(error("codex_config_field", "此字段需要在 TOML 源码模式中编辑。")); }
        if value.len() > 8192 || value.contains('\0') { return Err(error("codex_config_field", "配置值过长或包含无效字符。")); }
        if document.get(&key).and_then(toml_edit::Item::as_str) == Some(&value) { continue; }
        if value.is_empty() { document.remove(&key); }
        else if let Some(toml_edit::Item::Value(existing)) = document.get_mut(&key) {
            let decor = existing.decor().clone();
            *existing = toml_edit::Value::from(value);
            *existing.decor_mut() = decor;
        } else { document[&key] = toml_edit::value(value); }
    }
    let result = document.to_string();
    let result = super::instructions::preserve_text(text, &result);
    Ok(Rendered { fields: fields(&document), text: result })
}
pub fn render(text: &str, patch: BTreeMap<String, String>) -> CoreResult<Rendered> {
    super::gate::query(|| render_inner(text, patch))
}
fn read_inner(source: Source, codex_home: Option<&str>) -> CoreResult<Snapshot> {
    let path = path(source, codex_home)?;
    let parent = safe_write::capture_parent_directory(&path).map_err(|_| error("codex_config_unsafe", "无法安全访问配置目录。"))?;
    let state = safe_write::read_path_state_anchored_limited(&path, &parent, MAX_BYTES).map_err(|_| error("codex_config_read", "无法安全读取配置文件，文件可能过大或不是普通文件。"))?;
    let revision = safe_write::fingerprint_anchored_path_state(&state, &parent).map_err(|_| conflict())?;
    let (exists, text) = match &state {
        AnchoredPathState::File { bytes, .. } => (true, std::str::from_utf8(bytes).map_err(|_| error("codex_config_encoding", "配置文件不是 UTF-8，请使用原生编辑器。"))?.to_owned()),
        AnchoredPathState::Missing => (false, String::new()),
        _ => return Err(error("codex_config_unsafe", "配置文件不能是链接或特殊文件。")),
    };
    let parsed = parse(&text);
    let valid = parsed.is_ok();
    let fields = parsed.map(|d| fields(&d)).unwrap_or_default();
    Ok(Snapshot { path: path.to_string_lossy().into_owned(), exists, text, fields, revision, valid })
}
pub fn read(source: Source, codex_home: Option<&str>) -> CoreResult<Snapshot> { super::gate::query(|| read_inner(source, codex_home)) }

pub fn save(source: Source, codex_home: Option<&str>, expected: &str, text: &str, patch: BTreeMap<String, String>) -> CoreResult<Snapshot> {
    super::gate::mutate_independent_core("codex_configuration_edit", || {
        let current = read_inner(source, codex_home)?;
        if current.revision != expected { return Err(conflict()); }
        let content = render_inner(text, patch)?.text;
        let content = super::instructions::preserve_text(&current.text, &content);
        parse(&content)?;
        let path = path(source, codex_home)?;
        let parent = safe_write::capture_parent_directory(&path).map_err(|_| conflict())?;
        let state = safe_write::read_path_state_anchored_limited(&path, &parent, MAX_BYTES).map_err(|_| conflict())?;
        if safe_write::fingerprint_anchored_path_state(&state, &parent).map_err(|_| conflict())? != expected { return Err(conflict()); }
        let scope = crate::paths::arca_dir().join("configuration-edits").join(hash(path.as_os_str().as_encoded_bytes())).join("claims");
        safe_write::with_file_edit_claims(&scope, || match &state {
            AnchoredPathState::File { bytes, mode, identity } => {
                if bytes.as_slice() == content.as_bytes() { return Ok(()); }
                crate::resources::model::persist_private_file_snapshot(&format!("config-{}", hash(path.as_os_str().as_encoded_bytes())), &path, bytes)?;
                safe_write::write_bytes_if_hash_unchanged_in_parent(&path, &parent, Some((&hash(bytes), *mode, *identity)), content.as_bytes(), Some(0o600))
            }
            AnchoredPathState::Missing => safe_write::write_bytes_if_hash_unchanged_in_parent(&path, &parent, None, content.as_bytes(), Some(0o600)),
            _ => Err("changed".into()),
        }).map_err(|e| if e.contains("changed") { conflict() } else { error("codex_config_save", "无法安全备份或保存配置文件，草稿已保留。") })?;
        read_inner(source, codex_home)
    })
}

pub(crate) fn validate_launch(agent_id: &str, target: &crate::domain::agents::LaunchTarget) -> Result<(), String> {
    let crate::domain::agents::LaunchTarget::Cli { args, env, .. } = target else { return Ok(()); };
    if agent_id != "codex" { return Ok(()); }
    let mut profile = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" { break; }
        if arg == "--profile" || arg == "-p" { profile = iter.next().map(String::as_str); }
        else if let Some(name) = arg.strip_prefix("--profile=") { profile = Some(name); }
        else if let Some(name) = arg.strip_prefix("-p").filter(|name| !name.is_empty()) { profile = Some(name); }
    }
    if profile == Some("arca") {
        let snapshot = read_inner(Source::Arca, env.get("CODEX_HOME").map(String::as_str)).map_err(|e| e.message)?;
        if !snapshot.exists { return Err("Arca 专属配置尚未创建，请先编辑并保存。".into()); }
        if !snapshot.valid { return Err("Arca 专属配置的 TOML 格式无效，请先修正。".into()); }
    }
    Ok(())
}
