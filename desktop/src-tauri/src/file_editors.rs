use serde::Serialize;

#[derive(Serialize)]
pub struct FileEditor {
    name: String,
    path: String,
}

#[tauri::command]
pub async fn list_file_editors() -> Result<Vec<FileEditor>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let names = ["Visual Studio Code", "Visual Studio Code - Insiders", "Cursor", "Windsurf", "Antigravity", "Qoder", "Qoder IDE", "ZCode", "Sublime Text", "Zed", "TextEdit", "BBEdit", "Nova", "CotEditor", "TextMate", "Xcode", "Android Studio", "IntelliJ IDEA", "IntelliJ IDEA CE", "WebStorm", "PyCharm", "PyCharm CE", "GoLand", "RustRover", "CLion", "PhpStorm", "Rider"];
        let mut roots = vec![std::path::PathBuf::from("/Applications"), std::path::PathBuf::from("/System/Applications")];
        if let Some(home) = dirs::home_dir() { roots.push(home.join("Applications")); }
        let mut found = Vec::new();
        for name in names {
            for root in &roots {
                let path = root.join(format!("{name}.app"));
                if path.join("Contents/Info.plist").is_file() {
                    found.push(FileEditor { name: name.into(), path: path.to_string_lossy().into_owned() });
                    break;
                }
            }
        }
        found
    }).await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn detect_agent_installation(agent_id: String) -> Result<Option<bool>, String> {
    tauri::async_runtime::spawn_blocking(move || arca_core::application::agents::runtime_detected(&agent_id))
        .await.map_err(|error| error.to_string())
}
