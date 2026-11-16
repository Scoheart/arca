use std::sync::Mutex;

use arca_core::application::playground::Playground;
use tauri_plugin_opener::OpenerExt;

#[derive(Default)]
pub struct PlaygroundState(Mutex<Option<Playground>>);

#[tauri::command]
pub fn open_model_playground(
    app: tauri::AppHandle,
    state: tauri::State<'_, PlaygroundState>,
) -> Result<(), String> {
    let url = {
        let mut server = state.0.lock().map_err(|_| "模型试验台不可用，请重启 Arca。".to_owned())?;
        if server.is_none() {
            *server = Some(Playground::start()?);
        }
        server.as_ref().expect("model playground started").launch_url()
    };
    app.opener().open_url(url, None::<&str>)
        .map_err(|_| "无法打开默认浏览器。".to_owned())
}
