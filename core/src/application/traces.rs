//! Read-only, local conversation traces. Formats and discovery paths belong to
//! Core; frontends receive summaries and fetch complete, redacted records lazily.
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::UNIX_EPOCH;

const MAX_RECORD: u64 = 8 * 1024 * 1024;
const MAX_DOCUMENT: u64 = 32 * 1024 * 1024;
const PAGE_RECORDS: usize = 60;
const INLINE_MESSAGE_BYTES: usize = 32 * 1024;
const INLINE_PAGE_BYTES: usize = 512 * 1024;
const MAX_FILES: usize = 5000;
const MAX_SCAN: usize = 25000;
const MAX_REDACTION_BYTES: u64 = 32 * 1024 * 1024;
const MAX_REDACTION_RECORDS: usize = 100_000;
const MAX_SECRETS: usize = 4096;
const MAX_SECRET_BYTES: usize = 64 * 1024;
const MAX_TOTAL_SECRET_BYTES: usize = 1024 * 1024;

#[derive(Clone, Deserialize)]
struct Definition {
    id: String,
    agent_ids: Vec<String>,
    format: String,
    roots: Vec<String>,
    extensions: Vec<String>,
    filename_prefix: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct TraceSource {
    pub id: String,
    pub agent_ids: Vec<String>,
    pub name: String,
    pub format: String,
    pub roots: Vec<String>,
}
#[derive(Clone, Serialize)]
pub struct TraceSession {
    pub id: String,
    pub source_id: String,
    pub agent_name: String,
    pub format: String,
    pub title: String,
    pub project: Option<String>,
    pub path: String,
    pub modified_at: String,
    pub bytes: u64,
    pub imported: bool,
}
#[derive(Serialize)]
pub struct TraceIndex {
    pub sources: Vec<TraceSource>,
    pub sessions: Vec<TraceSession>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Serialize)]
pub struct TraceEvent {
    pub id: String,
    pub kind: String,
    pub timestamp: Option<String>,
    pub title: String,
    pub preview: String,
    pub call_id: Option<String>,
    pub is_error: bool,
    /// Bounded, redacted conversation text. Tool bodies stay on demand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub text_truncated: bool,
}
#[derive(Serialize)]
pub struct TracePage {
    pub session: TraceSession,
    pub events: Vec<TraceEvent>,
    pub next_cursor: Option<String>,
    pub revision: String,
    pub warnings: Vec<String>,
}
#[derive(Serialize)]
pub struct TraceRaw {
    pub label: String,
    pub raw: Value,
    pub text: String,
}
#[derive(Serialize)]
pub struct TraceDetail {
    pub event: TraceEvent,
    pub raw: Value,
    pub text: String,
    pub related: Vec<TraceRaw>,
    pub pairing_note: Option<String>,
    pub redacted: bool,
}
#[derive(Clone)]
struct Locator {
    path: PathBuf,
    home: PathBuf,
    source: TraceSource,
    imported: bool,
}
static LOCATORS: LazyLock<Mutex<BTreeMap<String, Locator>>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));
static DEFINITIONS: LazyLock<Vec<Definition>> = LazyLock::new(|| serde_json::from_str(include_str!("../../../data/trace-sources.json")).expect("trace-sources.json must be valid"));
static PASSWORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(?:密码|口令|password|passphrase)\s*[:：=]?\s*(?:([0-9]{4,16})|["\x27`]([^"\x27`\r\n]{4,128})["\x27`])"#).unwrap());
static QUERY_SECRET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)([?&](?:code|state|token|access_token|refresh_token|id_token|payload|session_id|verifier_id)=)([^\s&#"'<>\\]+)"#).unwrap());
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:sk-[A-Za-z0-9_-]{16,}|gh[pousr]_[A-Za-z0-9]{16,})\b").unwrap());
static BEARER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(\bBearer\s+)([A-Za-z0-9_.~+/-]{4,})").unwrap());
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)((?:["']?(?:api[-_]?key|access[-_]?token|refresh[-_]?token|client[-_]?secret|password|passphrase|authorization|cookie)["']?\s*[:=]\s*)["']?)([^\s,;"'<>}]{4,})"#).unwrap());

fn home() -> Result<PathBuf, String> {
    let path = dirs::home_dir().ok_or_else(|| "无法确定用户目录。".to_owned())?;
    fs::canonicalize(path).map_err(|_| "用户目录不可读。".into())
}
fn home_path(path: &Path) -> Result<PathBuf, String> {
    let original = dirs::home_dir().ok_or("无法确定用户目录。")?;
    // macOS temporary HOME roots can themselves use /var → /private/var.
    // Resolve only the declared HOME alias, never symlinks inside a source.
    Ok(path.strip_prefix(&original).map(|p| home().map(|h| h.join(p))).unwrap_or_else(|_| Ok(path.to_path_buf()))?)
}
fn source(def: &Definition) -> TraceSource {
    let agents = crate::agents::builtin_agents();
    let name = def.agent_ids.first().and_then(|id| agents.get(id)).and_then(|a| a.name.clone()).unwrap_or_else(|| def.id.clone());
    TraceSource { id: def.id.clone(), agent_ids: def.agent_ids.clone(), name, format: def.format.clone(), roots: def.roots.clone() }
}
fn display_path(path: &Path, home: &Path) -> String {
    path.strip_prefix(home).map(|p| format!("~/{}", p.display())).unwrap_or_else(|_| path.display().to_string())
}
fn bounded_preview(text: &str) -> String {
    text.chars().take(600).collect()
}
fn revision(file: &File) -> Result<String, String> {
    let m = file.metadata().map_err(|_| "无法读取记录元数据。".to_owned())?;
    let modified = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|t| t.as_nanos()).unwrap_or(0);
    Ok(format!("{}:{modified}", m.len()))
}
fn open_path(path: &Path, writing: bool) -> Result<File, String> {
    let mapped = home_path(path)?;
    let path = mapped.as_path();
    #[cfg(unix)] {
        use rustix::fs::{openat, Mode, OFlags};
        use std::path::Component;
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) { return Err("记录位置必须是绝对、规范路径。".into()); }
        let parts: Vec<_> = path.components().filter_map(|c| if let Component::Normal(part) = c { Some(part) } else { None }).collect();
        if parts.is_empty() { return Err("记录位置不是文件。".into()); }
        let mut directory = rustix::fs::open("/", OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty()).map_err(|_| "无法打开记录目录。".to_owned())?;
        for part in &parts[..parts.len() - 1] {
            directory = openat(&directory, *part, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).map_err(|_| "记录父目录不可读或包含符号链接。".to_owned())?;
        }
        let flags = if writing { OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL } else { OFlags::RDONLY };
        let fd = openat(&directory, parts[parts.len() - 1], flags | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK, Mode::from_bits_truncate(0o600)).map_err(|_| "记录不可读，或导出位置已存在 / 不可写。".to_owned())?;
        let file = File::from(fd);
        if !file.metadata().map_err(|_| "无法读取文件类型。".to_owned())?.is_file() { return Err("只接受普通记录文件。".into()); }
        Ok(file)
    }
    #[cfg(not(unix))] {
        let mut options = OpenOptions::new();
        if writing { options.write(true).create_new(true); } else { options.read(true); }
        options.open(path).map_err(|_| "记录位置不可读或不可写。".into())
    }
}
fn open(locator: &Locator) -> Result<File, String> {
    if locator.home != home()? || fs::canonicalize(&locator.path).ok().as_ref() != Some(&locator.path) {
        return Err("记录位置已改变或包含符号链接，请重新扫描。".into());
    }
    let file = open_path(&locator.path, false)?;
    if !file.metadata().map_err(|_| "无法读取记录元数据。".to_owned())?.is_file() { return Err("记录不是普通文件。".into()); }
    Ok(file)
}
fn lookup(id: &str) -> Result<Locator, String> {
    LOCATORS.lock().map_err(|_| "Trace 索引不可用。".to_owned())?.get(id).cloned().ok_or_else(|| "会话不在本次扫描中，请刷新或重新导入。".into())
}
fn read_line(reader: &mut BufReader<File>) -> Result<Option<(u64, Value)>, String> {
    let start = reader.stream_position().map_err(|_| "无法定位记录。".to_owned())?;
    loop {
    let offset = reader.stream_position().map_err(|_| "无法定位记录。".to_owned())?;
    if offset.saturating_sub(start) > MAX_RECORD { return Err("空行扫描超过 8 MiB，读取已停止。".into()); }
    let mut bytes = Vec::new();
    let read = (&mut *reader).take(MAX_RECORD + 1).read_until(b'\n', &mut bytes).map_err(|_| "读取记录失败。".to_owned())?;
    if read == 0 { return Ok(None); }
    if read as u64 > MAX_RECORD { return Err("单条记录超过 8 MiB；未截断或加载，请在 Agent 中导出较小的记录。".into()); }
    if bytes.iter().all(u8::is_ascii_whitespace) { continue; }
    return match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(Some((offset, value))),
        Err(error) if error.is_eof() && bytes.last() != Some(&b'\n') => Err("末条记录尚未写完，请稍后刷新；已读取内容不会被当作完整记录。".into()),
        Err(_) => Err(format!("偏移 {offset} 的记录不是有效 JSON，未跳过或猜测。")),
    };
    }
}
fn read_document(locator: &Locator, file: File) -> Result<Value, String> {
    if file.metadata().map_err(|_| "无法读取记录元数据。".to_owned())?.len() > MAX_DOCUMENT { return Err("JSON / Gemini 文档超过 32 MiB；未截断或加载。JSONL 流式格式可按页读取。".into()); }
    if locator.path.extension().and_then(|s| s.to_str()) == Some("json") {
        let document: Value = serde_json::from_reader(file.take(MAX_DOCUMENT + 1)).map_err(|_| "记录文档不是有效 JSON。".to_owned())?;
        if locator.source.format == "gemini" && !document.get("messages").is_some_and(Value::is_array) { return Err("Gemini messages 字段不是数组；未猜测文档格式。".into()); }
        return Ok(document);
    }
    // Gemini journals start with metadata, use $set checkpoints and append
    // message objects. Replace checkpoint messages rather than duplicating them.
    let mut reader = BufReader::new(file);
    let mut budget = RedactionReadBudget::default();
    let mut document = json!({"messages": []});
    while let Some((_, value)) = budget.read(&mut reader)? {
        if let Some(set) = value.get("$set").and_then(Value::as_object) {
            for (key, value) in set { document[key] = value.clone(); }
        } else if let Some(messages) = value.get("messages").and_then(Value::as_array) {
            document["messages"] = json!(messages);
        } else if value.get("type").is_some() {
            let messages = document["messages"].as_array_mut().ok_or("Gemini messages 字段不是数组。")?;
            if let Some(index) = value.get("id").and_then(|id| messages.iter().position(|m| m.get("id") == Some(id))) { messages[index] = value; }
            else { messages.push(value); }
        } else if let Some(object) = value.as_object() {
            for (key, value) in object { document[key] = value.clone(); }
        }
    }
    if !document.get("messages").is_some_and(Value::is_array) { return Err("Gemini messages 字段不是数组；未猜测文档格式。".into()); }
    Ok(document)
}
fn document_mode(locator: &Locator) -> bool {
    locator.source.format == "gemini" || locator.path.extension().and_then(|s| s.to_str()) == Some("json")
}
fn sensitive_key(key: &str) -> bool {
    let normalized: String = key.chars().filter(|ch| !matches!(*ch, '-' | '_')).flat_map(char::to_lowercase).collect();
    matches!(normalized.as_str(), "password" | "passwd" | "passphrase" | "token" | "secret" | "apikey" | "accesstoken" | "refreshtoken" | "idtoken" | "clientsecret" | "authorization" | "cookie" | "setcookie")
}
fn redaction_limit() -> String {
    "redaction_context_limit: Trace redaction context exceeds its bounded read or credential budget; no content was returned".into()
}
#[derive(Clone, Default)]
struct Redactor { secrets: BTreeSet<String>, secret_bytes: usize }
impl Redactor {
    fn insert(&mut self, secret: &str) -> Result<(), String> {
        if secret.is_empty() || self.secrets.contains(secret) { return Ok(()); }
        if secret.len() > MAX_SECRET_BYTES || self.secrets.len() >= MAX_SECRETS
            || self.secret_bytes.saturating_add(secret.len()) > MAX_TOTAL_SECRET_BYTES {
            return Err(redaction_limit());
        }
        self.secret_bytes += secret.len();
        self.secrets.insert(secret.to_owned());
        Ok(())
    }
    fn learn_sensitive_value(&mut self, value: &Value) -> Result<(), String> {
        match value {
            Value::String(text) => { self.insert(text)?; self.learn(value)?; },
            Value::Number(number) => self.insert(&number.to_string())?,
            Value::Array(values) => for value in values { self.learn_sensitive_value(value)?; },
            Value::Object(values) => for value in values.values() { self.learn_sensitive_value(value)?; },
            _ => {},
        }
        Ok(())
    }
    fn learn(&mut self, value: &Value) -> Result<(), String> {
        match value {
            Value::String(text) => {
                for capture in PASSWORD.captures_iter(text) {
                    for secret in capture.iter().skip(1).flatten() { self.insert(secret.as_str())?; }
                }
                for pattern in [&*ASSIGNMENT, &*BEARER, &*QUERY_SECRET] {
                    for capture in pattern.captures_iter(text) {
                        if let Some(secret) = capture.get(2) { self.insert(secret.as_str())?; }
                    }
                }
            }
            Value::Array(values) => for value in values { self.learn(value)?; },
            Value::Object(values) => for (key, value) in values {
                if sensitive_key(key) { self.learn_sensitive_value(value)?; }
                else { self.learn(value)?; }
            },
            _ => {},
        }
        Ok(())
    }
    fn text(&self, text: &str) -> String {
        // Longest values must win: replacing a short prefix first can expose
        // the remaining suffix of a longer credential.
        let mut secrets: Vec<_> = self.secrets.iter().collect();
        secrets.sort_unstable_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        let mut text = text.to_owned();
        for secret in secrets { text = text.replace(secret, "[密码已脱敏]"); }
        text = QUERY_SECRET.replace_all(&text, "${1}[凭据已脱敏]").into_owned();
        text = TOKEN.replace_all(&text, "[令牌已脱敏]").into_owned();
        text = BEARER.replace_all(&text, "${1}[令牌已脱敏]").into_owned();
        ASSIGNMENT.replace_all(&text, "${1}[凭据已脱敏]").into_owned()
    }
    fn value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.text(text)),
            Value::Number(number) if self.secrets.contains(&number.to_string()) => json!("[凭据已脱敏]"),
            Value::Array(values) => Value::Array(values.iter().map(|v| self.value(v)).collect()),
            Value::Object(values) => Value::Object(values.iter().map(|(key, value)| {
                let value = if sensitive_key(key) && !value.is_null() { json!("[凭据已脱敏]") } else { self.value(value) };
                (self.text(key), value)
            }).collect()),
            _ => value.clone(),
        }
    }
}

#[derive(Default)]
struct RedactionReadBudget { records: usize }
impl RedactionReadBudget {
    fn read(&mut self, reader: &mut BufReader<File>) -> Result<Option<(u64, Value)>, String> {
        let record = read_line(reader).map_err(|_| {
            "redaction_context_incomplete: Trace context is invalid, unfinished or exceeds the record limit; no content was returned".to_owned()
        })?;
        if record.is_some() { self.records += 1; }
        let position = reader.stream_position().map_err(|_| "无法定位脱敏上下文。".to_owned())?;
        if position > MAX_REDACTION_BYTES || self.records > MAX_REDACTION_RECORDS { return Err(redaction_limit()); }
        Ok(record)
    }
}

/// Rebuild the context from the same open file before seeking to a page/event.
/// It is deliberately independent of prior pages, locator cache or CLI process.
fn context_reader(file: File, offset: u64, redactor: &mut Redactor) -> Result<(BufReader<File>, RedactionReadBudget), String> {
    if offset > MAX_REDACTION_BYTES { return Err(redaction_limit()); }
    let mut reader = BufReader::new(file);
    let mut budget = RedactionReadBudget::default();
    while reader.stream_position().map_err(|_| "无法定位脱敏上下文。".to_owned())? < offset {
        let (record_offset, value) = budget.read(&mut reader)?.ok_or("redaction_context_incomplete: Requested Trace context is unavailable")?;
        // A detail offset can follow blank lines skipped by read_line.
        if record_offset == offset {
            reader.seek(SeekFrom::Start(offset)).map_err(|_| "无法定位脱敏上下文。".to_owned())?;
            break;
        }
        if record_offset > offset || reader.stream_position().map_err(|_| "无法定位脱敏上下文。".to_owned())? > offset {
            return Err("分页游标不是完整记录边界。".into());
        }
        redactor.learn(&value)?;
    }
    Ok((reader, budget))
}
fn summary(locator: &Locator, id: String) -> Result<TraceSession, String> {
    summary_with_context(locator, id, &Redactor::default())
}
fn summary_with_context(locator: &Locator, id: String, context: &Redactor) -> Result<TraceSession, String> {
    let file = open(locator)?;
    let metadata = file.metadata().map_err(|_| "无法读取会话元数据。".to_owned())?;
    let mut prefix = String::new();
    file.take(64 * 1024).read_to_string(&mut prefix).ok();
    let mut title = locator.path.file_stem().and_then(|s| s.to_str()).unwrap_or("Trace").to_owned();
    let mut project = None;
    let mut redactor = context.clone();
    for line in prefix.lines().take(30) {
        let Ok(value) = serde_json::from_str::<Value>(line) else { continue; };
        redactor.learn(&value)?;
        let metadata = value.get("payload").unwrap_or(&value);
        if let Some(cwd) = metadata.get("cwd").and_then(Value::as_str) { project = Some(cwd.to_owned()); }
        if value.get("type").and_then(Value::as_str) == Some("session_info") {
            if let Some(name) = value.get("name").and_then(Value::as_str) { title = name.to_owned(); }
        }
        if let Some(name) = value.get("title").and_then(Value::as_str) { title = name.to_owned(); }
    }
    let modified: chrono::DateTime<chrono::Utc> = metadata.modified().unwrap_or(UNIX_EPOCH).into();
    Ok(TraceSession { id, source_id: locator.source.id.clone(), agent_name: locator.source.name.clone(), format: locator.source.format.clone(), title: bounded_preview(&redactor.text(&title)), project: project.map(|value| redactor.text(&value)), path: redactor.text(&display_path(&locator.path, &locator.home)), modified_at: modified.to_rfc3339(), bytes: metadata.len(), imported: locator.imported })
}
fn register(path: PathBuf, source: TraceSource, imported: bool) -> Result<TraceSession, String> {
    let path = home_path(&path)?;
    if fs::symlink_metadata(&path).map_err(|_| "记录不存在。".to_owned())?.file_type().is_symlink() { return Err("不读取符号链接记录。".into()); }
    let home = home()?;
    let canonical = fs::canonicalize(&path).map_err(|_| "无法解析记录位置。".to_owned())?;
    if canonical != path { return Err("记录父目录包含符号链接；未读取。".into()); }
    let id = hex::encode(Sha256::digest(format!("{}:{}", source.id, canonical.display()).as_bytes()));
    let mut cache = LOCATORS.lock().map_err(|_| "Trace 索引不可用。".to_owned())?;
    let locator = Locator { path: canonical, home, source, imported };
    let session = summary(&locator, id.clone())?;
    cache.insert(id, locator);
    Ok(session)
}
fn scan(path: &Path, def: &Definition, depth: usize, visited: &mut usize, paths: &mut Vec<PathBuf>) {
    if depth > 8 || *visited >= MAX_SCAN || paths.len() >= MAX_FILES { return; }
    let Ok(metadata) = fs::symlink_metadata(path) else { return; };
    if metadata.file_type().is_symlink() { return; }
    *visited += 1;
    if metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) { for entry in entries.flatten() { scan(&entry.path(), def, depth + 1, visited, paths); } }
    } else if metadata.is_file() && def.extensions.iter().any(|ext| path.extension().and_then(|s| s.to_str()) == Some(ext.as_str())) && def.filename_prefix.as_ref().is_none_or(|prefix| path.file_name().and_then(|s| s.to_str()).is_some_and(|s| s.starts_with(prefix))) {
        paths.push(path.to_path_buf());
    }
}
pub fn index() -> Result<TraceIndex, String> {
    let home = home()?;
    let sources: Vec<_> = DEFINITIONS.iter().map(source).collect();
    let mut sessions = Vec::new();
    let mut warnings = Vec::new();
    for (def, source) in DEFINITIONS.iter().zip(&sources) {
        let mut paths = Vec::new();
        let mut visited = 0;
        for root in &def.roots { scan(&home.join(root.trim_start_matches("~/")), def, 0, &mut visited, &mut paths); }
        if visited >= MAX_SCAN || paths.len() >= MAX_FILES { warnings.push(format!("{} 的扫描达到上限（5000 个文件 / 25000 个目录项）；列表不是完整清单。", source.name)); }
        for path in paths { match register(path, source.clone(), false) { Ok(session) => sessions.push(session), Err(_) => warnings.push(format!("{} 有不可读或不安全的记录；未读取。", source.name)) } }
    }
    let imported: Vec<_> = LOCATORS.lock().map_err(|_| "Trace 索引不可用。".to_owned())?.iter().filter(|(_, l)| l.imported && l.home == home).map(|(id, l)| (id.clone(), l.clone())).collect();
    for (id, locator) in imported { if let Ok(session) = summary(&locator, id) { sessions.push(session); } }
    sessions.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    warnings.sort(); warnings.dedup();
    Ok(TraceIndex { sources, sessions, warnings })
}
pub fn import(path: &Path) -> Result<TraceSession, String> {
    if !matches!(path.extension().and_then(|s| s.to_str()), Some("json" | "jsonl")) { return Err("只接受用户选择的 JSON / JSONL 会话文件。".into()); }
    let mut prefix = String::new();
    open_path(path, false)?.take(64 * 1024).read_to_string(&mut prefix).ok();
    let first = if path.extension().and_then(|s| s.to_str()) == Some("json") { serde_json::from_str::<Value>(&prefix).ok() } else { prefix.lines().find_map(|line| serde_json::from_str::<Value>(line).ok()) };
    let format = first.as_ref().map(|v| {
        match v.get("type").and_then(Value::as_str) {
            Some("session" | "model_change") => "pi",
            Some("session_meta" | "response_item" | "event_msg") => "codex",
            Some("user" | "assistant") if v.get("message").is_some() => "claude",
            _ if v.get("sessionId").is_some() || v.get("projectHash").is_some() => "gemini",
            _ => "generic",
        }
    }).unwrap_or("generic");
    let source = DEFINITIONS.iter().find(|d| d.format == format).map(source).unwrap_or(TraceSource { id: "imported".into(), agent_ids: vec![], name: "导入记录".into(), format: "generic".into(), roots: vec![] });
    register(path.to_path_buf(), source, true)
}

struct Parsed { event: TraceEvent, raw: Value, text: String }
fn content_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|block| {
            if matches!(block.get("type").and_then(Value::as_str), Some("thinking" | "reasoning" | "redacted_thinking")) { None }
            else { block.get("text").and_then(Value::as_str).or_else(|| block.get("text_delta").and_then(Value::as_str)) }
        }).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}
fn public_conversation(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        for key in ["thoughts", "thinking", "reasoning", "thinkingSignature", "reasoning_content", "encrypted_content", "base_instructions", "system_prompt", "systemPrompt", "developer_prompt", "developerPrompt"] { object.remove(key); }
        if let Some(Value::Array(blocks)) = object.get_mut("content") { blocks.retain(|b| !matches!(b.get("type").and_then(Value::as_str), Some("thinking" | "reasoning" | "redacted_thinking"))); }
        if let Some(message) = object.get_mut("message") { *message = public_conversation(message); }
        if let Some(payload) = object.get_mut("payload") { *payload = public_conversation(payload); }
    }
    value
}
fn string(value: &Value, key: &str) -> Option<String> { value.get(key).and_then(Value::as_str).map(str::to_owned) }
fn emit(items: &mut Vec<Parsed>, offset: u64, kind: &str, timestamp: Option<String>, title: String, call_id: Option<String>, text: String, raw: Value, is_error: bool, _redactor: &Redactor) {
    // Keep complete originals inside this read until the final redaction
    // context is known. Early prefix replacement/truncation can expose suffixes.
    items.push(Parsed { event: TraceEvent { id: format!("{offset}.{}", items.len()), kind: kind.into(), timestamp, title, preview: text.clone(), call_id, is_error, text: None, text_truncated: false }, raw, text });
}
fn redact_event(event: &mut TraceEvent, redactor: &Redactor, inline_budget: Option<&mut usize>) {
    event.title = redactor.text(&event.title);
    let text = redactor.text(&event.preview);
    event.preview = bounded_preview(&text);
    if let Some(budget) = inline_budget.filter(|_| matches!(event.kind.as_str(), "user" | "assistant")) {
        // Redact complete values before taking any prefix, so a secret crossing
        // the display boundary cannot leak its suffix in another representation.
        let mut end = text.len().min(INLINE_MESSAGE_BYTES).min(*budget);
        while !text.is_char_boundary(end) { end -= 1; }
        event.text = Some(text[..end].to_owned());
        event.text_truncated = end < text.len();
        *budget -= end;
    }
    event.timestamp = event.timestamp.as_deref().map(|s| redactor.text(s));
    event.call_id = event.call_id.as_deref().map(|s| redactor.text(s));
}
fn normalize(value: &Value, format: &str, offset: u64, redactor: &Redactor) -> Vec<Parsed> {
    let mut items = Vec::new();
    let timestamp = string(value, "timestamp");
    match format {
        "pi" | "claude" => {
            let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
            let Some(message) = value.get("message") else {
                if matches!(kind, "compaction" | "branch_summary") {
                    emit(&mut items, offset, "event", timestamp, kind.into(), None, value.get("summary").and_then(Value::as_str).unwrap_or("").into(), value.clone(), false, redactor);
                }
                return items;
            };
            let role = message.get("role").and_then(Value::as_str).unwrap_or(kind);
            if matches!(role, "system" | "developer" | "analysis") { return items; }
            let content = message.get("content").unwrap_or(&Value::Null);
            let text = content_text(content);
            if !text.is_empty() || content.as_array().is_some_and(|bs| bs.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("image"))) {
                if matches!(role, "user" | "assistant") { emit(&mut items, offset, role, timestamp.clone(), role.into(), None, text, public_conversation(value), false, redactor); }
            }
            if role == "toolResult" {
                emit(&mut items, offset, "tool_result", timestamp.clone(), string(message, "toolName").unwrap_or_else(|| "toolResult".into()), string(message, "toolCallId"), content_text(content), value.clone(), message.get("isError").and_then(Value::as_bool).unwrap_or(false), redactor);
            }
            if let Some(blocks) = content.as_array() {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("toolCall" | "tool_use") => emit(&mut items, offset, "tool_call", timestamp.clone(), string(block, "name").unwrap_or_else(|| "tool".into()), string(block, "id"), serde_json::to_string(block.get("arguments").or_else(|| block.get("input")).unwrap_or(&Value::Null)).unwrap_or_default(), block.clone(), false, redactor),
                        Some("tool_result") => emit(&mut items, offset, "tool_result", timestamp.clone(), "tool_result".into(), string(block, "tool_use_id"), content_text(block.get("content").unwrap_or(&Value::Null)), block.clone(), block.get("is_error").and_then(Value::as_bool).unwrap_or(false), redactor),
                        _ => {}
                    }
                }
            }
        }
        "codex" => {
            let Some(payload) = value.get("payload") else { return items; };
            // response_item is authoritative. event_msg user_message/agent_message
            // mirrors must not double-count the same conversation.
            if value.get("type").and_then(Value::as_str) != Some("response_item") { return items; }
            match payload.get("type").and_then(Value::as_str) {
                Some("message") => {
                    let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
                    if matches!(role, "user" | "assistant") && payload.get("channel").and_then(Value::as_str) != Some("analysis") {
                        emit(&mut items, offset, role, timestamp, role.into(), None, content_text(payload.get("content").unwrap_or(&Value::Null)), public_conversation(value), false, redactor);
                    }
                }
                Some("function_call" | "custom_tool_call" | "tool_call") => emit(&mut items, offset, "tool_call", timestamp, string(payload, "name").unwrap_or_else(|| "tool".into()), string(payload, "call_id"), payload.get("arguments").or_else(|| payload.get("input")).map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())).unwrap_or_default(), value.clone(), false, redactor),
                Some("function_call_output" | "custom_tool_call_output" | "tool_result") => emit(&mut items, offset, "tool_result", timestamp, "tool_result".into(), string(payload, "call_id"), payload.get("output").map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())).unwrap_or_default(), value.clone(), false, redactor),
                _ => {}
            }
        }
        "gemini" => {
            let kind = value.get("type").and_then(Value::as_str).unwrap_or("event");
            if matches!(kind, "system" | "developer" | "thinking" | "reasoning" | "analysis") { return items; }
            let role = if kind == "gemini" { "assistant" } else if kind == "user" { "user" } else { "event" };
            let text = content_text(value.get("content").unwrap_or(&Value::Null));
            if !text.is_empty() { emit(&mut items, offset, role, timestamp.clone(), kind.into(), None, text, public_conversation(value), false, redactor); }
            if let Some(calls) = value.get("toolCalls").and_then(Value::as_array) {
                for call in calls {
                    let id = string(call, "id");
                    emit(&mut items, offset, "tool_call", timestamp.clone(), string(call, "name").unwrap_or_else(|| "tool".into()), id.clone(), call.get("args").unwrap_or(&Value::Null).to_string(), call.clone(), false, redactor);
                    if let Some(result) = call.get("result") { emit(&mut items, offset, "tool_result", timestamp.clone(), string(call, "name").unwrap_or_else(|| "tool_result".into()), id, result.to_string(), result.clone(), call.get("status").and_then(Value::as_str) == Some("error"), redactor); }
                }
            }
        }
        _ => {
            let role = value.get("role").and_then(Value::as_str).unwrap_or("event");
            if matches!(role, "system" | "developer" | "analysis") || matches!(value.get("type").and_then(Value::as_str), Some("system" | "developer" | "thinking" | "reasoning" | "redacted_thinking" | "analysis")) { return items; }
            let kind = if matches!(role, "user" | "assistant") { role } else { "event" };
            emit(&mut items, offset, kind, timestamp, string(value, "type").unwrap_or_else(|| kind.into()), None, content_text(value.get("content").unwrap_or(value)), public_conversation(value), false, redactor);
        }
    }
    items
}
fn document_items<'a>(document: &'a Value, locator: &'a Locator, redactor: &'a Redactor) -> impl Iterator<Item = Parsed> + 'a {
    let values = document.as_array().or_else(|| document.get("messages").and_then(Value::as_array));
    values.map(|v| v.as_slice()).unwrap_or(std::slice::from_ref(document)).iter()
        .flat_map(move |value| normalize(value, &locator.source.format, 0, redactor))
        .enumerate().map(|(index, mut item)| { item.event.id = format!("0.{index}"); item })
}

#[derive(Serialize, Deserialize)]
struct Cursor { offset: u64, index: usize, revision: String }
fn encode(cursor: &Cursor) -> Result<String, String> { serde_json::to_vec(cursor).map(hex::encode).map_err(|_| "无法生成分页游标。".into()) }
fn decode(cursor: Option<&str>, revision: &str) -> Result<Cursor, String> {
    let cursor: Cursor = match cursor { Some(value) if value.len() <= 512 => serde_json::from_slice(&hex::decode(value).map_err(|_| "分页游标无效。")?).map_err(|_| "分页游标无效。")?, Some(_) => return Err("分页游标无效。".into()), None => Cursor { offset: 0, index: 0, revision: revision.into() } };
    if cursor.revision != revision { return Err("记录已变化，请刷新会话后继续读取。".into()); }
    Ok(cursor)
}
pub fn page(session_id: &str, cursor: Option<&str>) -> Result<TracePage, String> {
    let locator = lookup(session_id)?;
    let file = open(&locator)?;
    let rev = revision(&file)?;
    let cursor = decode(cursor, &rev)?;
    let mut redactor = Redactor::default();
    let warnings = Vec::new();
    let document_mode = document_mode(&locator);
    let mut inline_budget = INLINE_PAGE_BYTES;
    let (mut events, next_cursor): (Vec<TraceEvent>, Option<String>) = if document_mode {
        let document = read_document(&locator, file)?;
        redactor.learn(&document)?;
        // Normalize one source record at a time, never retain a second full
        // document of raw bodies just to return a page of small summaries.
        let mut events = Vec::new();
        let mut total = 0;
        for item in document_items(&document, &locator, &redactor) {
            if total >= cursor.index && events.len() < PAGE_RECORDS {
                let mut event = item.event;
                // The complete document has already supplied the redaction
                // context, so no unbounded preview needs to survive this loop.
                redact_event(&mut event, &redactor, Some(&mut inline_budget));
                events.push(event);
            }
            total += 1;
        }
        if cursor.index > total { return Err("分页位置无效。".into()); }
        let end = cursor.index + events.len();
        let next = if end < total { Some(encode(&Cursor { offset: 0, index: end, revision: rev.clone() })?) } else { None };
        (events, next)
    } else {
        let (mut reader, mut budget) = context_reader(file, cursor.offset, &mut redactor)?;
        let mut events = Vec::new();
        let mut complete = false;
        for _ in 0..PAGE_RECORDS {
            match budget.read(&mut reader)? {
                Some((offset, value)) => { redactor.learn(&value)?; events.extend(normalize(&value, &locator.source.format, offset, &redactor).into_iter().map(|i| i.event)); }
                None => { complete = true; break; }
            }
        }
        let offset = reader.stream_position().map_err(|_| "无法读取分页位置。".to_owned())?;
        if offset >= reader.get_ref().metadata().map_err(|_| "无法读取记录元数据。".to_owned())?.len() { complete = true; }
        let next = if complete { None } else { Some(encode(&Cursor { offset, index: 0, revision: rev.clone() })?) };
        (events, next)
    };
    if !document_mode {
        for event in &mut events { redact_event(event, &redactor, Some(&mut inline_budget)); }
    }
    let session = summary_with_context(&locator, session_id.into(), &redactor)?;
    if revision(&open(&locator)?)? != rev { return Err("记录在读取期间发生变化，请刷新；未返回混合快照。".into()); }
    Ok(TracePage { session, events, next_cursor, revision: rev, warnings })
}
pub fn detail(session_id: &str, event_id: &str, expected_revision: &str) -> Result<TraceDetail, String> {
    let locator = lookup(session_id)?;
    let file = open(&locator)?;
    if revision(&file)? != expected_revision { return Err("记录已变化，请刷新会话。".into()); }
    let (offset, index) = event_id.split_once('.').ok_or("记录 ID 无效。")?;
    let offset: u64 = offset.parse().map_err(|_| "记录 ID 无效。")?;
    let index: usize = index.parse().map_err(|_| "记录 ID 无效。")?;
    let mut redactor = Redactor::default();
    let mut related = Vec::new();
    let mut pairing_note = None;
    let mut selected = if document_mode(&locator) {
        if offset != 0 { return Err("记录 ID 无效。".into()); }
        let document = read_document(&locator, file)?;
        redactor.learn(&document)?;
        let selected = document_items(&document, &locator, &redactor).nth(index).ok_or("记录不存在。")?;
        if selected.event.kind == "tool_call" {
            if let Some(id) = &selected.event.call_id {
                for result in document_items(&document, &locator, &redactor)
                    .filter(|i| i.event.kind == "tool_result" && i.event.call_id.as_ref() == Some(id)) {
                    related.push(TraceRaw { label: "tool_result".into(), raw: result.raw, text: result.text });
                }
            }
        }
        selected
    } else {
        let (mut reader, mut budget) = context_reader(file, offset, &mut redactor)?;
        let (actual_offset, value) = budget.read(&mut reader)?.ok_or("记录不存在。")?;
        if actual_offset != offset { return Err("记录 ID 不是完整记录边界。".into()); }
        redactor.learn(&value)?;
        let mut items = normalize(&value, &locator.source.format, offset, &redactor);
        if index >= items.len() { return Err("记录不存在。".into()); }
        let selected = items.remove(index);
        let call_id = (selected.event.kind == "tool_call").then_some(selected.event.call_id.as_ref()).flatten();
        if let Some(id) = call_id {
            for item in &items { if item.event.kind == "tool_result" && item.event.call_id.as_ref() == Some(id) { related.push(TraceRaw { label: "tool_result".into(), raw: item.raw.clone(), text: item.text.clone() }); } }
        }
        // Any page containing this record can learn from up to 59 following
        // records. Detail must learn at least that same context before output,
        // even for a plain message or an already paired tool call.
        for forward in 0..1000 {
            if forward >= PAGE_RECORDS - 1 && (call_id.is_none() || !related.is_empty()) { break; }
            let Some(record) = budget.read(&mut reader)? else { break; };
            redactor.learn(&record.1)?;
            if related.is_empty() {
                if let Some(id) = call_id {
                    for result in normalize(&record.1, &locator.source.format, record.0, &redactor) {
                        if result.event.kind == "tool_result" && result.event.call_id.as_ref() == Some(id) { related.push(TraceRaw { label: "tool_result".into(), raw: result.raw, text: result.text }); }
                    }
                }
            }
        }
        selected
    };
    if selected.event.kind == "tool_call" && related.is_empty() { pairing_note = Some("在当前文件的配对范围内未找到返回（最多后续 1000 条 / 32 MiB）；可能仍在执行、返回被省略或在另一条分支。".into()); }
    if revision(&open(&locator)?)? != expected_revision { return Err("记录在读取期间发生变化，请刷新。".into()); }
    redact_event(&mut selected.event, &redactor, None);
    for item in &mut related { item.raw = redactor.value(&item.raw); item.text = redactor.text(&item.text); }
    Ok(TraceDetail { event: selected.event, raw: redactor.value(&selected.raw), text: redactor.text(&selected.text), related, pairing_note, redacted: true })
}
pub fn export_detail(session_id: &str, event_id: &str, revision: &str, destination: &Path) -> Result<(), String> {
    let detail = detail(session_id, event_id, revision)?;
    let bytes = serde_json::to_vec_pretty(&detail).map_err(|_| "无法编码记录。".to_owned())?;
    // Never overwrite a transcript or other existing user data. Explicit file
    // selection does not authorize silently replacing unrelated files.
    let mut file = open_path(destination, true)?;
    file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|_| "导出未完成，请检查所选文件。".to_owned())
}

#[cfg(test)]
mod redaction_tests {
    use super::*;

    #[test]
    fn learns_structured_assignment_and_bearer_values_with_bounds() {
        let mut redactor = Redactor::default();
        redactor.learn(&json!({"api_key":"opaque-fixture", "content":"api_key=assigned-fixture Bearer bearer-fixture"})).unwrap();
        let text = redactor.text("opaque-fixture assigned-fixture bearer-fixture");
        for value in ["opaque-fixture", "assigned-fixture", "bearer-fixture"] { assert!(!text.contains(value)); }
        assert!(redactor.learn(&json!({"api_key":"x".repeat(MAX_SECRET_BYTES + 1)})).is_err());
        let mut redactor = Redactor::default();
        for index in 0..MAX_SECRETS { redactor.insert(&format!("bounded-{index}")).unwrap(); }
        assert!(redactor.insert("overflow").unwrap_err().starts_with("redaction_context_limit:"));
    }

    #[test]
    fn missing_or_excessive_prefix_context_fails_closed() {
        let home = crate::testenv::TestHome::new("trace-invalid-context");
        let path = home.home.join("broken.jsonl");
        fs::write(&path, "{}\n{broken}\n{}\n").unwrap();
        assert!(context_reader(File::open(&path).unwrap(), 12, &mut Redactor::default()).is_err());
        assert!(context_reader(File::open(&path).unwrap(), MAX_REDACTION_BYTES + 1, &mut Redactor::default()).is_err());
    }
}
