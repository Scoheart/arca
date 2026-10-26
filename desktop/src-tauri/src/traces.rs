use arca_core::application::traces as core;
use tauri_plugin_dialog::DialogExt;

async fn worker<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| "Trace 后台读取失败。".to_owned())?
}
#[tauri::command]
pub async fn trace_index() -> Result<core::TraceIndex, String> { worker(core::index).await }
#[tauri::command]
pub async fn trace_page(session_id: String, cursor: Option<String>) -> Result<core::TracePage, String> {
    worker(move || core::page(&session_id, cursor.as_deref())).await
}
#[tauri::command]
pub async fn trace_detail(session_id: String, event_id: String, revision: String) -> Result<core::TraceDetail, String> {
    worker(move || core::detail(&session_id, &event_id, &revision)).await
}
#[tauri::command]
pub async fn trace_import(app: tauri::AppHandle) -> Result<Option<core::TraceSession>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().add_filter("Agent Trace", &["json", "jsonl"]).blocking_pick_file()).await.map_err(|_| "无法打开文件选择窗口。".to_owned())?;
    let Some(picked) = picked else { return Ok(None); };
    let path = picked.into_path().map_err(|_| "导入位置无效。".to_owned())?;
    worker(move || core::import(&path).map(Some)).await
}
#[tauri::command]
pub async fn trace_export(app: tauri::AppHandle, session_id: String, event_id: String, revision: String) -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().add_filter("Trace JSON", &["json"]).set_file_name("arca-trace-record.json").blocking_save_file()).await.map_err(|_| "无法打开导出窗口。".to_owned())?;
    let Some(picked) = picked else { return Ok(None); };
    let path = picked.into_path().map_err(|_| "导出位置无效。".to_owned())?;
    worker(move || { core::export_detail(&session_id, &event_id, &revision, &path)?; Ok(Some(path.to_string_lossy().into_owned())) }).await
}
