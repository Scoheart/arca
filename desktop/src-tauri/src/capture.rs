use arca_core::application::capture as core;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

async fn worker<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| "抓包后台任务失败。".to_owned())?
}
#[tauri::command]
pub async fn capture_environment() -> Result<core::CaptureEnvironment, String> { worker(|| Ok(core::environment())).await }
#[tauri::command]
pub async fn capture_install_engine() -> Result<(), String> { worker(|| core::install_engine().map(|_| ())).await }
#[tauri::command]
pub async fn capture_open_certificate(app: tauri::AppHandle) -> Result<(), String> {
    let path = worker(core::prepare_certificate).await?;
    app.opener().open_path(path.to_string_lossy(), Some("Keychain Access"))
        .map_err(|_| "无法打开证书设置，请在钥匙串访问中导入抓包证书。".to_owned())
}
#[tauri::command]
pub async fn capture_launch_agent(agent_id: String, directory: Option<String>) -> Result<core::CaptureTarget, String> {
    worker(move || core::launch_agent(&agent_id, directory.as_deref())).await
}
#[tauri::command]
pub async fn capture_sessions() -> Result<Vec<core::CaptureSession>, String> { worker(core::sessions).await }
#[tauri::command]
pub async fn capture_snapshot(session_id: String) -> Result<core::CaptureSnapshot, String> { worker(move || core::snapshot(&session_id)).await }
#[tauri::command]
pub async fn capture_delta(session_id: String, revision: Option<String>) -> Result<core::CaptureDelta, String> {
    worker(move || core::delta(&session_id, revision.as_deref())).await
}
#[tauri::command]
pub async fn capture_detail(session_id: String, flow_id: String) -> Result<core::CapturedFlow, String> { worker(move || core::detail(&session_id, &flow_id)).await }
#[tauri::command]
pub async fn capture_start(request: core::StartCapture, launch: Option<bool>, directory: Option<String>) -> Result<core::CaptureSnapshot, String> {
    worker(move || if launch.unwrap_or(false) { core::start_fresh(request, directory.as_deref()) } else { core::start(request) }).await
}
#[tauri::command]
pub async fn capture_stop(session_id: String) -> Result<core::CaptureSnapshot, String> { worker(move || core::stop(&session_id)).await }
#[tauri::command]
pub async fn capture_export(app: tauri::AppHandle, session_id: String, flow_id: Option<String>) -> Result<Option<String>, String> {
    let filename = if flow_id.is_some() { "arca-request-response.json" } else { "arca-capture-session.json" };
    let picked = tauri::async_runtime::spawn_blocking(move || app.dialog().file().add_filter("抓包记录", &["json"]).set_file_name(filename).blocking_save_file()).await.map_err(|_| "无法打开导出窗口。".to_owned())?;
    let Some(picked) = picked else { return Ok(None); };
    let path = picked.into_path().map_err(|_| "导出位置无效。".to_owned())?;
    worker(move || { core::export(&session_id, flow_id.as_deref(), &path)?; Ok(Some(path.to_string_lossy().into_owned())) }).await
}
