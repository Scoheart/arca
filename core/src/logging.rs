//! Local end-to-end diagnostic logging for Arca frontends and Core operations.
//!
//! Design invariants:
//! - `~/.arca/logs` (resolved dynamically via `paths::logs_dir()`, honoring
//!   `ARCA_HOME` and `TestHome`) owns three rotating files:
//!   - `arca.log`: chronological single-line human-readable trace.
//!   - `arca-errors.log`: structured diagnostic blocks for `WARN`/`ERROR`/`PANIC`
//!     so users can immediately inspect failures in one file.
//!   - `arca.jsonl`: JSON Lines records for CLI (`arca logs`) and structured queries.
//! - Every record is sanitized before leaving memory or hitting disk: API keys,
//!   tokens, Authorization/Cookie headers, URL userinfo/query secrets, sensitive
//!   environment variables, and `findings_hash` tokens are redacted.
//! - Pure read-only calls on an uninitialized `ARCA_HOME` never create `~/.arca`
//!   as a side effect; disk persistence activates only when `~/.arca` already
//!   exists as a real directory or when `ensure_log_files()` is explicitly called
//!   after startup bootstrap.
//! - Logging is best-effort and fail-open for business operations: I/O errors
//!   in the logger never block mutations or latch `BackendStatus::ReadOnly`.

use crate::paths::{arca_dir, error_log_file, logs_dir, main_log_file, structured_log_file};
use chrono::SecondsFormat;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, Once};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const MAIN_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
const MAIN_LOG_ARCHIVES: usize = 5;
const ERROR_LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;
const ERROR_LOG_ARCHIVES: usize = 3;
const JSONL_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
const JSONL_LOG_ARCHIVES: usize = 3;

const MAX_MESSAGE_BYTES: usize = 4096;
const MAX_STRING_FIELD_BYTES: usize = 4096;
const MAX_FIELDS_COUNT: usize = 32;
const MEMORY_RING_CAPACITY: usize = 500;
const REDACTED: &str = "[REDACTED]";

static NEXT_REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);
static PANIC_HOOK_ONCE: Once = Once::new();
static LOG_WRITE_LOCK: Mutex<()> = Mutex::new(());

struct SessionState {
    surface: String,
    session_id: String,
}

static SESSION_STATE: LazyLock<Mutex<SessionState>> = LazyLock::new(|| {
    let short_id = uuid::Uuid::new_v4().simple().to_string();
    Mutex::new(SessionState {
        surface: "core".into(),
        session_id: format!("ses_{}", &short_id[..12]),
    })
});

static MEMORY_RING: LazyLock<Mutex<VecDeque<LogRecord>>> =
    LazyLock::new(|| Mutex::new(VecDeque::with_capacity(MEMORY_RING_CAPACITY)));

#[derive(Debug, Clone, Default)]
struct ThreadLogContext {
    surface_override: Option<String>,
    request_id: Option<String>,
    command: Option<String>,
    operation_id: Option<String>,
    agent_id: Option<String>,
}

thread_local! {
    static THREAD_CONTEXT: RefCell<ThreadLogContext> = RefCell::new(ThreadLogContext::default());
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Panic,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Panic => "PANIC",
        }
    }

    pub fn is_error_or_warn(self) -> bool {
        matches!(self, Self::Warn | Self::Error | Self::Panic)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogRecord {
    pub timestamp: String,
    pub level: LogLevel,
    pub surface: String,
    pub version: String,
    pub pid: u32,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogStatusView {
    pub logs_dir: String,
    pub main_log_path: String,
    pub error_log_path: String,
    pub structured_log_path: String,
    pub main_log_bytes: u64,
    pub error_log_bytes: u64,
    pub structured_log_bytes: u64,
    pub session_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogQuery {
    #[serde(default)]
    pub errors_only: bool,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogListResponse {
    pub status: LogStatusView,
    pub total_matched: usize,
    pub records: Vec<LogRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendLogInput {
    #[serde(default)]
    pub level: Option<String>,
    pub stage: String,
    #[serde(default)]
    pub code: Option<String>,
    pub message: String,
    #[serde(default)]
    pub details: Option<Value>,
}

/// Initialize the process logging surface (`"desktop"`, `"cli"`, `"tui"`) and
/// install the process-wide panic hook once.
pub fn init_session(surface: &str) {
    if let Ok(mut state) = SESSION_STATE.lock() {
        state.surface = surface.to_owned();
    }
    install_panic_hook();
}

pub fn session_id() -> String {
    SESSION_STATE
        .lock()
        .map(|state| state.session_id.clone())
        .unwrap_or_else(|error| error.into_inner().session_id.clone())
}

fn current_surface() -> String {
    let override_surface = THREAD_CONTEXT.with(|ctx| ctx.borrow().surface_override.clone());
    if let Some(surface) = override_surface {
        return surface;
    }
    SESSION_STATE
        .lock()
        .map(|state| state.surface.clone())
        .unwrap_or_else(|error| error.into_inner().surface.clone())
}

pub fn current_request_id() -> Option<String> {
    THREAD_CONTEXT.with(|ctx| ctx.borrow().request_id.clone())
}

pub fn current_operation_id() -> Option<String> {
    THREAD_CONTEXT.with(|ctx| ctx.borrow().operation_id.clone())
}

pub fn set_current_operation_id(operation_id: &str) {
    if operation_id.trim().is_empty() {
        return;
    }
    THREAD_CONTEXT.with(|ctx| {
        ctx.borrow_mut().operation_id = Some(operation_id.to_owned());
    });
}

pub fn next_request_id() -> String {
    let seq = NEXT_REQUEST_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("req_{seq:06x}")
}

struct ContextRestoreGuard {
    previous: ThreadLogContext,
}

impl Drop for ContextRestoreGuard {
    fn drop(&mut self) {
        THREAD_CONTEXT.with(|ctx| {
            *ctx.borrow_mut() = std::mem::take(&mut self.previous);
        });
    }
}

/// Run a closure within a correlation request scope (`req_...`), inheriting any
/// outer operation ID and restoring the prior thread context on exit.
pub fn with_request_scope<T>(
    surface_override: Option<&str>,
    command: &str,
    operation: impl FnOnce(&str) -> T,
) -> T {
    let request_id = next_request_id();
    let previous = THREAD_CONTEXT.with(|ctx| {
        let mut current = ctx.borrow().clone();
        let prev = current.clone();
        if let Some(surface) = surface_override {
            current.surface_override = Some(surface.to_owned());
        }
        current.request_id = Some(request_id.clone());
        current.command = Some(command.to_owned());
        *ctx.borrow_mut() = current;
        prev
    });
    let _guard = ContextRestoreGuard { previous };
    operation(&request_id)
}

/// Run a closure bound to an explicit `operation_id` (and optional `agent_id`).
pub fn with_operation_scope<T>(
    operation_id: &str,
    agent_id: Option<&str>,
    operation: impl FnOnce() -> T,
) -> T {
    let previous = THREAD_CONTEXT.with(|ctx| {
        let mut current = ctx.borrow().clone();
        let prev = current.clone();
        if !operation_id.trim().is_empty() {
            current.operation_id = Some(operation_id.to_owned());
        }
        if let Some(agent) = agent_id.filter(|value| !value.trim().is_empty()) {
            current.agent_id = Some(agent.to_owned());
        }
        *ctx.borrow_mut() = current;
        prev
    });
    let _guard = ContextRestoreGuard { previous };
    operation()
}

fn install_panic_hook() {
    PANIC_HOOK_ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let payload = if let Some(text) = info.payload().downcast_ref::<&str>() {
                (*text).to_owned()
            } else if let Some(text) = info.payload().downcast_ref::<String>() {
                text.clone()
            } else {
                "non-string panic payload".to_owned()
            };
            let location = info
                .location()
                .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
                .unwrap_or_else(|| "unknown".into());
            let thread_name = std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_owned();
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            let truncated_bt = truncate_utf8(&backtrace, 3072);

            LogEvent::new(LogLevel::Panic, "runtime.panic", &payload)
                .code("panic")
                .field("location", location)
                .field("thread", thread_name)
                .field("backtrace", truncated_bt)
                .emit();

            previous(info);
        }));
    });
}

pub struct LogEvent {
    level: LogLevel,
    stage: String,
    code: Option<String>,
    message: String,
    operation_id: Option<String>,
    request_id: Option<String>,
    duration_ms: Option<u64>,
    fields: BTreeMap<String, Value>,
}

impl LogEvent {
    pub fn new(level: LogLevel, stage: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level,
            stage: stage.into(),
            code: None,
            message: message.into(),
            operation_id: None,
            request_id: None,
            duration_ms: None,
            fields: BTreeMap::new(),
        }
    }

    pub fn info(stage: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(LogLevel::Info, stage, message)
    }

    pub fn warn(stage: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(LogLevel::Warn, stage, message)
    }

    pub fn error(stage: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(LogLevel::Error, stage, message)
    }

    pub fn code(mut self, code: impl Into<String>) -> Self {
        let code = code.into();
        if !code.trim().is_empty() {
            self.code = Some(code);
        }
        self
    }

    pub fn maybe_code(mut self, code: Option<&str>) -> Self {
        if let Some(code) = code.filter(|value| !value.trim().is_empty()) {
            self.code = Some(code.to_owned());
        }
        self
    }

    pub fn operation_id(mut self, operation_id: impl Into<String>) -> Self {
        let operation_id = operation_id.into();
        if !operation_id.trim().is_empty() {
            self.operation_id = Some(operation_id);
        }
        self
    }

    pub fn request_id(mut self, request_id: impl Into<String>) -> Self {
        let request_id = request_id.into();
        if !request_id.trim().is_empty() {
            self.request_id = Some(request_id);
        }
        self
    }

    pub fn duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    pub fn field(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        if self.fields.len() < MAX_FIELDS_COUNT {
            self.fields.insert(key.into(), value.into());
        }
        self
    }

    pub fn fields(mut self, extra: BTreeMap<String, Value>) -> Self {
        for (key, value) in extra {
            if self.fields.len() >= MAX_FIELDS_COUNT {
                break;
            }
            self.fields.insert(key, value);
        }
        self
    }

    pub fn into_record(mut self) -> LogRecord {
        let (ctx_req, ctx_cmd, ctx_op, ctx_agent) = THREAD_CONTEXT.with(|ctx| {
            let borrowed = ctx.borrow();
            (
                borrowed.request_id.clone(),
                borrowed.command.clone(),
                borrowed.operation_id.clone(),
                borrowed.agent_id.clone(),
            )
        });
        if let Some(cmd) = ctx_cmd {
            self.fields
                .entry("command".into())
                .or_insert_with(|| Value::String(cmd));
        }
        if let Some(agent_id) = ctx_agent {
            self.fields
                .entry("agent_id".into())
                .or_insert_with(|| Value::String(agent_id));
        }
        let sanitized_fields = redact_fields(self.fields);
        LogRecord {
            timestamp: chrono::Local::now().to_rfc3339_opts(SecondsFormat::Millis, false),
            level: self.level,
            surface: current_surface(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            pid: std::process::id(),
            session_id: session_id(),
            request_id: self.request_id.or(ctx_req),
            operation_id: self.operation_id.or(ctx_op),
            stage: truncate_utf8(&self.stage, 128),
            code: self.code.map(|code| truncate_utf8(&code, 128)),
            message: redact_text(&self.message, MAX_MESSAGE_BYTES),
            duration_ms: self.duration_ms,
            fields: sanitized_fields,
        }
    }

    pub fn emit(self) {
        let record = self.into_record();
        emit_record(record);
    }
}

pub fn emit_record(record: LogRecord) {
    if let Ok(mut ring) = MEMORY_RING.lock() {
        if ring.len() >= MEMORY_RING_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(record.clone());
    }
    let _ = write_record_to_disk(&record);
}

/// Ensure `~/.arca/logs` and the three log files (`arca.log`, `arca-errors.log`,
/// `arca.jsonl`) exist with private permissions when root validation permits.
pub fn ensure_log_files() -> Result<PathBuf, String> {
    crate::brand::validate_roots().map_err(|error| error.to_string())?;
    let _guard = LOG_WRITE_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    ensure_logs_dir_unlocked()
}

/// Called at the end of `ArcaCore::bootstrap()` to guarantee the log files are
/// ready on disk and to record the startup report and any bootstrap warnings.
pub fn record_bootstrap_outcome(
    report: &crate::application::bootstrap::BootstrapReport,
    duration_ms: u64,
) {
    let _ = ensure_log_files();
    for warning in &report.warnings {
        LogEvent::warn("bootstrap.warning", &warning.message)
            .code(warning.stage.code())
            .field("bootstrap_stage", warning.stage.code())
            .emit();
    }
    match &report.status {
        crate::application::BackendStatus::Ready => {
            LogEvent::info("bootstrap.complete", "Arca backend bootstrap completed")
                .duration_ms(duration_ms)
                .field("status", "ready")
                .field("warnings_count", report.warnings.len() as u64)
                .field("skill_updates_allowed", report.skill_updates_allowed)
                .emit();
        }
        crate::application::BackendStatus::CapabilityUnavailable {
            capability,
            stage,
            code,
            message,
        } => {
            LogEvent::warn("bootstrap.degraded", message)
                .code(code)
                .duration_ms(duration_ms)
                .field("status", "capability_unavailable")
                .field("capability", json!(capability))
                .field("bootstrap_stage", stage.clone())
                .field("warnings_count", report.warnings.len() as u64)
                .emit();
        }
        crate::application::BackendStatus::ReadOnly { stage, message } => {
            LogEvent::error("bootstrap.read_only", message)
                .code("recovery_required")
                .duration_ms(duration_ms)
                .field("status", "read_only")
                .field("bootstrap_stage", stage.clone())
                .field("warnings_count", report.warnings.len() as u64)
                .emit();
        }
        crate::application::BackendStatus::Starting => {}
    }
}

pub fn status() -> LogStatusView {
    let dir = logs_dir();
    let main = main_log_file();
    let error = error_log_file();
    let structured = structured_log_file();
    LogStatusView {
        logs_dir: dir.to_string_lossy().into_owned(),
        main_log_path: main.to_string_lossy().into_owned(),
        error_log_path: error.to_string_lossy().into_owned(),
        structured_log_path: structured.to_string_lossy().into_owned(),
        main_log_bytes: file_len(&main),
        error_log_bytes: file_len(&error),
        structured_log_bytes: file_len(&structured),
        session_id: session_id(),
    }
}

pub fn list(query: LogQuery) -> LogListResponse {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let mut records = read_structured_records_from_disk();
    if records.is_empty() {
        if let Ok(ring) = MEMORY_RING.lock() {
            records = ring.iter().cloned().collect();
        }
    }
    let matched: Vec<LogRecord> = records
        .into_iter()
        .filter(|record| {
            if query.errors_only && !record.level.is_error_or_warn() {
                return false;
            }
            if let Some(stage) = query.stage.as_deref().filter(|s| !s.is_empty()) {
                if !record.stage.contains(stage) {
                    return false;
                }
            }
            if let Some(op_id) = query.operation_id.as_deref().filter(|s| !s.is_empty()) {
                if record.operation_id.as_deref() != Some(op_id) {
                    return false;
                }
            }
            if let Some(req_id) = query.request_id.as_deref().filter(|s| !s.is_empty()) {
                if record.request_id.as_deref() != Some(req_id) {
                    return false;
                }
            }
            true
        })
        .collect();
    let total_matched = matched.len();
    let tail: Vec<LogRecord> = matched
        .into_iter()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    LogListResponse {
        status: status(),
        total_matched,
        records: tail,
    }
}

pub fn record_frontend_event(input: FrontendLogInput) {
    let level = match input
        .level
        .as_deref()
        .unwrap_or("error")
        .to_ascii_lowercase()
        .as_str()
    {
        "info" => LogLevel::Info,
        "warn" | "warning" => LogLevel::Warn,
        _ => LogLevel::Error,
    };
    let stage = if input.stage.trim().is_empty() {
        "webview.error".to_owned()
    } else if input.stage.starts_with("webview.") || input.stage.starts_with("ui.") {
        input.stage
    } else {
        format!("webview.{}", input.stage)
    };
    let mut event = LogEvent::new(level, stage, input.message);
    if let Some(code) = input.code {
        event = event.code(code);
    }
    if let Some(details) = input.details {
        match details {
            Value::Object(map) => {
                for (key, value) in map {
                    event = event.field(key, value);
                }
            }
            other => {
                event = event.field("details", other);
            }
        }
    }
    let mut record = event.into_record();
    record.surface = "webview".into();
    emit_record(record);
}

fn file_len(path: &Path) -> u64 {
    fs::symlink_metadata(path)
        .ok()
        .filter(|meta| meta.is_file() && !meta.file_type().is_symlink())
        .map(|meta| meta.len())
        .unwrap_or(0)
}

fn read_structured_records_from_disk() -> Vec<LogRecord> {
    let path = structured_log_file();
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return Vec::new();
    };
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Vec::new();
    }
    let Ok(file) = File::open(&path) else {
        return Vec::new();
    };
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<LogRecord>(&line) {
            records.push(record);
        }
    }
    records
}

fn can_persist_to_disk() -> bool {
    let root = arca_dir();
    match fs::symlink_metadata(&root) {
        Ok(meta) => meta.is_dir(),
        Err(_) => false,
    }
}

fn ensure_logs_dir_unlocked() -> Result<PathBuf, String> {
    let root = arca_dir();
    if let Ok(meta) = fs::symlink_metadata(&root) {
        if !meta.is_dir() {
            return Err("Arca data root is not a directory".into());
        }
    } else {
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    }
    let dir = logs_dir();
    match fs::symlink_metadata(&dir) {
        Ok(meta) => {
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err("Arca logs path is not a real directory".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    #[cfg(unix)]
    {
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    for file_path in [main_log_file(), error_log_file(), structured_log_file()] {
        let _ = open_private_append_file(&file_path);
    }
    Ok(dir)
}

fn write_record_to_disk(record: &LogRecord) -> Result<(), String> {
    if !can_persist_to_disk() {
        return Ok(());
    }
    let _guard = LOG_WRITE_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    ensure_logs_dir_unlocked()?;

    let main_path = main_log_file();
    let main_line = format_main_line(record);
    append_with_rotation(
        &main_path,
        main_line.as_bytes(),
        MAIN_LOG_MAX_BYTES,
        MAIN_LOG_ARCHIVES,
    )?;

    let jsonl_path = structured_log_file();
    if let Ok(mut encoded) = serde_json::to_vec(record) {
        encoded.push(b'\n');
        append_with_rotation(
            &jsonl_path,
            &encoded,
            JSONL_LOG_MAX_BYTES,
            JSONL_LOG_ARCHIVES,
        )?;
    }

    if record.level.is_error_or_warn() {
        let err_path = error_log_file();
        let err_block = format_error_block(record);
        append_with_rotation(
            &err_path,
            err_block.as_bytes(),
            ERROR_LOG_MAX_BYTES,
            ERROR_LOG_ARCHIVES,
        )?;
    }

    Ok(())
}

fn format_main_line(record: &LogRecord) -> String {
    let req = record
        .request_id
        .as_deref()
        .map(|id| format!(" [req={id}]"))
        .unwrap_or_default();
    let op = record
        .operation_id
        .as_deref()
        .map(|id| format!(" [op={id}]"))
        .unwrap_or_default();
    let code = record
        .code
        .as_deref()
        .map(|c| format!(" code={c}"))
        .unwrap_or_default();
    let dur = record
        .duration_ms
        .map(|ms| format!(" duration={ms}ms"))
        .unwrap_or_default();
    let fields = if record.fields.is_empty() {
        String::new()
    } else {
        match serde_json::to_string(&record.fields) {
            Ok(json) => format!(" fields={json}"),
            Err(_) => String::new(),
        }
    };
    let clean_msg = record.message.replace(['\r', '\n'], " ");
    format!(
        "{} [{}] [{}:{}] [ses={}]{}{} stage={}{}{} — {}{}\n",
        record.timestamp,
        record.level.as_str(),
        record.surface,
        record.pid,
        record.session_id,
        req,
        op,
        record.stage,
        code,
        dur,
        clean_msg,
        fields
    )
}

fn format_error_block(record: &LogRecord) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("================================================================================\n");
    out.push_str(&format!("Time:       {}\n", record.timestamp));
    out.push_str(&format!("Level:      {}\n", record.level.as_str()));
    out.push_str(&format!(
        "Process:    {} (pid={}, session={}, version={})\n",
        record.surface, record.pid, record.session_id, record.version
    ));
    out.push_str(&format!("Stage:      {}\n", record.stage));
    if let Some(req) = &record.request_id {
        out.push_str(&format!("Request ID: {req}\n"));
    }
    if let Some(op) = &record.operation_id {
        out.push_str(&format!("Operation:  {op}\n"));
    }
    if let Some(code) = &record.code {
        out.push_str(&format!("Error Code: {code}\n"));
    }
    if let Some(ms) = record.duration_ms {
        out.push_str(&format!("Duration:   {ms}ms\n"));
    }
    out.push_str(&format!("Message:    {}\n", record.message));
    if !record.fields.is_empty() {
        if let Ok(pretty) = serde_json::to_string_pretty(&record.fields) {
            out.push_str("Context:\n");
            for line in pretty.lines() {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out.push_str("--------------------------------------------------------------------------------\n");
    out
}

fn append_with_rotation(
    path: &Path,
    bytes: &[u8],
    max_bytes: u64,
    archives: usize,
) -> Result<(), String> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err("refusing to write to symlinked log file".into());
        }
        if meta.len().saturating_add(bytes.len() as u64) > max_bytes {
            rotate_log_files(path, archives);
        }
    }
    let mut file = open_private_append_file(path)?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    Ok(())
}

fn rotate_log_files(path: &Path, archives: usize) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("log");
    let archived_path = |index: usize| parent.join(format!("{stem}.{index}.{ext}"));

    for index in (1..archives).rev() {
        let from = archived_path(index);
        let to = archived_path(index + 1);
        if fs::symlink_metadata(&from)
            .ok()
            .is_some_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
        {
            let _ = fs::rename(&from, &to);
        }
    }
    if archives >= 1 {
        let first_archive = archived_path(1);
        let _ = fs::rename(path, first_archive);
    }
}

fn open_private_append_file(path: &Path) -> Result<File, String> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("refusing non-regular or symlinked log file".into());
        }
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
        options.custom_flags(libc_o_nofollow());
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
    }
    Ok(file)
}

#[cfg(unix)]
const fn libc_o_nofollow() -> i32 {
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd"))]
    {
        0x0100
    }
    #[cfg(target_os = "linux")]
    {
        0o400000
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "linux"
    )))]
    {
        0
    }
}

// ── Redaction Engine ──────────────────────────────────────────────────────

static URL_USERINFO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(\b(?:https?|socks4a?|socks5h?)://)[^\s/@]+@").unwrap());
static QUERY_SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)([?&](?:key|api[_-]?key|token|access[_-]?token|refresh[_-]?token|id[_-]?token|secret|client[_-]?secret|password|passphrase|signature|sig|auth)=)([^\s&#"'<>\\]+)"#,
    )
    .unwrap()
});
static HEADER_SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(\b(?:proxy-authorization|authorization|set-cookie|cookie|x-api-key|api-key)["']?\s*[:=][ \t]*)("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\r\n,;}\]]+)"#,
    )
    .unwrap()
});
static KV_SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(\b[\w.-]*(?:password|passphrase|secret|token|api[ _-]?key|access[ _-]?key|private[ _-]?key|credential|findings_hash|findings_confirmation)[\w.-]*["']?\s*[:=][ \t]*)("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\s&,;}\]]+)"#,
    )
    .unwrap()
});
static BEARER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(\b(?:bearer|basic)\s+)[A-Za-z0-9_.+/~=-]{6,}").unwrap());
static TOKEN_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:sk-[A-Za-z0-9_-]{12,}|gh[pousr]_[A-Za-z0-9_]{16,}|github_pat_[A-Za-z0-9_]{16,}|xox[baprs]-[A-Za-z0-9-]{12,}|AIza[0-9A-Za-z_-]{20,})\b",
    )
    .unwrap()
});

pub fn is_sensitive_field_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "api-key"
            | "api_key"
            | "apikey"
            | "token"
            | "access_token"
            | "refresh_token"
            | "id_token"
            | "secret"
            | "client_secret"
            | "password"
            | "passphrase"
            | "credential"
            | "credentials"
            | "private_key"
            | "findings_hash"
            | "findings_confirmation"
            | "github_token"
    ) || lower.ends_with("_api_key")
        || lower.ends_with("-api-key")
        || lower.ends_with("_token")
        || lower.ends_with("-token")
        || lower.ends_with("_secret")
        || lower.ends_with("-secret")
        || lower.ends_with("_password")
        || lower.ends_with("_credential")
}

pub fn redact_text(input: &str, max_bytes: usize) -> String {
    let step1 = URL_USERINFO_RE.replace_all(input, "${1}[REDACTED]@");
    let step2 = QUERY_SECRET_RE.replace_all(&step1, "${1}[REDACTED]");
    let step3 = HEADER_SECRET_RE.replace_all(&step2, "${1}[REDACTED]");
    let step4 = KV_SECRET_RE.replace_all(&step3, "${1}[REDACTED]");
    let step5 = BEARER_RE.replace_all(&step4, "${1}[REDACTED]");
    let step6 = TOKEN_PREFIX_RE.replace_all(&step5, REDACTED);
    truncate_utf8(&step6, max_bytes)
}

fn redact_value(key_hint: Option<&str>, value: Value, depth: usize) -> Value {
    if let Some(key) = key_hint {
        if is_sensitive_field_key(key) {
            return Value::String(REDACTED.into());
        }
    }
    if depth > 6 {
        return Value::String("[TRUNCATED_DEPTH]".into());
    }
    match value {
        Value::String(text) => Value::String(redact_text(&text, MAX_STRING_FIELD_BYTES)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .take(32)
                .map(|item| redact_value(None, item, depth + 1))
                .collect(),
        ),
        Value::Object(map) => {
            let mut sanitized = serde_json::Map::new();
            for (k, v) in map.into_iter().take(MAX_FIELDS_COUNT) {
                let redacted_v = redact_value(Some(&k), v, depth + 1);
                sanitized.insert(k, redacted_v);
            }
            Value::Object(sanitized)
        }
        other => other,
    }
}

fn redact_fields(fields: BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    fields
        .into_iter()
        .take(MAX_FIELDS_COUNT)
        .map(|(key, value)| {
            let clean_val = redact_value(Some(&key), value, 0);
            (truncate_utf8(&key, 64), clean_val)
        })
        .collect()
}

fn truncate_utf8(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = input[..end].to_owned();
    result.push_str("…[truncated]");
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    #[test]
    fn redacts_tokens_headers_urls_and_sensitive_fields() {
        let raw = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9 sk-proj-1234567890abcdef https://user:mysecret@example.com/v1?api_key=sec_999&mode=fast api_key=\"topsecret\"";
        let clean = redact_text(raw, 4096);
        assert!(!clean.contains("eyJhbGci"));
        assert!(!clean.contains("sk-proj-1234567890abcdef"));
        assert!(!clean.contains("mysecret"));
        assert!(!clean.contains("sec_999"));
        assert!(!clean.contains("topsecret"));
        assert!(clean.contains("[REDACTED]"));

        let record = LogEvent::error("test.redact", raw)
            .code("auth_failed")
            .field("api_key", "sk-live-secret-value")
            .field("nested", json!({"Authorization": "Bearer secret12345678", "safe": "ok"}))
            .into_record();
        assert_eq!(record.fields["api_key"], Value::String("[REDACTED]".into()));
        assert_eq!(
            record.fields["nested"]["Authorization"],
            Value::String("[REDACTED]".into())
        );
        assert_eq!(record.fields["nested"]["safe"], Value::String("ok".into()));
    }

    #[test]
    fn does_not_create_arca_dir_on_empty_uninitialized_home() {
        let home = TestHome::new("logging-empty-home");
        assert!(!home.home.join(".arca").exists());

        LogEvent::info("test.readonly", "pure read event on empty home").emit();

        assert!(
            !home.home.join(".arca").exists(),
            "logging must not materialize ~/.arca before bootstrap or storage init"
        );
    }

    #[test]
    fn writes_main_error_and_structured_logs_once_initialized() {
        let _home = TestHome::new("logging-writes-files");
        ensure_log_files().unwrap();

        with_request_scope(Some("desktop"), "commit_operation", |_req_id| {
            with_operation_scope("op_test_123", Some("claude-code"), || {
                LogEvent::info("operations.plan", "planned update")
                    .duration_ms(12)
                    .emit();
                LogEvent::error(
                    "operations.commit",
                    "failed to write ~/.claude.json with token=sk-secret1234567890",
                )
                .code("asset_target_unsafe")
                .duration_ms(34)
                .field("path", "~/.claude.json")
                .emit();
            });
        });

        let status = status();
        assert!(status.main_log_bytes > 0);
        assert!(status.error_log_bytes > 0);
        assert!(status.structured_log_bytes > 0);

        let error_text = fs::read_to_string(error_log_file()).unwrap();
        assert!(error_text.contains("operations.commit"));
        assert!(error_text.contains("op_test_123"));
        assert!(error_text.contains("asset_target_unsafe"));
        assert!(!error_text.contains("sk-secret1234567890"));

        let errors_only = list(LogQuery {
            errors_only: true,
            operation_id: Some("op_test_123".into()),
            ..Default::default()
        });
        assert_eq!(errors_only.records.len(), 1);
        assert_eq!(errors_only.records[0].code.as_deref(), Some("asset_target_unsafe"));
        assert_eq!(
            errors_only.records[0].fields.get("agent_id"),
            Some(&Value::String("claude-code".into()))
        );
        assert_eq!(
            errors_only.records[0].fields.get("command"),
            Some(&Value::String("commit_operation".into()))
        );
    }

    #[test]
    fn rotates_log_file_when_exceeding_max_bytes() {
        let _home = TestHome::new("logging-rotation");
        ensure_log_files().unwrap();
        let path = main_log_file();
        append_with_rotation(&path, &[b'a'; 128], 100, 2).unwrap();
        append_with_rotation(&path, &[b'b'; 64], 100, 2).unwrap();
        let rotated = logs_dir().join("arca.1.log");
        assert!(rotated.is_file());
        assert_eq!(fs::read(&rotated).unwrap().len(), 128);
        assert_eq!(fs::read(&path).unwrap().len(), 64);
    }
}
