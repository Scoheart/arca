//! Thin configuration editor IPC. Core owns parsing, revisions and private writes.
use arca_core::{application::codex_configuration as core, domain::error::{CoreError, CoreResult}};
use std::collections::BTreeMap;
async fn worker<T: Send + 'static>(f: impl FnOnce() -> CoreResult<T> + Send + 'static) -> CoreResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| CoreError::new("codex_config_worker", "配置编辑器暂时无法响应，请重试。"))?
}
#[tauri::command]
pub async fn codex_configuration_read(source: core::Source, codex_home: Option<String>) -> CoreResult<core::Snapshot> {
    worker(move || core::read(source, codex_home.as_deref())).await
}
#[tauri::command]
pub async fn codex_configuration_render(text: String, patch: BTreeMap<String, String>) -> CoreResult<core::Rendered> {
    worker(move || core::render(&text, patch)).await
}
#[tauri::command]
pub async fn codex_configuration_save(source: core::Source, codex_home: Option<String>, expected_revision: String, text: String, patch: BTreeMap<String, String>) -> CoreResult<core::Snapshot> {
    worker(move || core::save(source, codex_home.as_deref(), &expected_revision, &text, patch)).await
}
