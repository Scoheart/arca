//! Frontend-facing application facade for local diagnostic logs.

use crate::domain::error::CoreResult;
pub use crate::logging::{
    ensure_log_files, list, record_frontend_event, status, FrontendLogInput, LogLevel,
    LogListResponse, LogQuery, LogRecord, LogStatusView,
};
use std::path::PathBuf;

pub fn prepare_log_file_for_open(kind: Option<&str>) -> CoreResult<PathBuf> {
    ensure_log_files().map_err(crate::domain::error::CoreError::internal)?;
    let target = match kind.unwrap_or("errors") {
        "dir" => crate::paths::logs_dir(),
        "main" => crate::paths::main_log_file(),
        "jsonl" | "structured" => crate::paths::structured_log_file(),
        _ => crate::paths::error_log_file(),
    };
    Ok(target)
}
