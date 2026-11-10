use serde::Serialize;

#[derive(Serialize)]
pub struct FileEditor {
    name: String,
    path: String,
}

#[tauri::command]
pub async fn list_file_editors() -> Result<Vec<FileEditor>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let names = ["Visual Studio Code", "Visual Studio Code - Insiders", "Cursor", "Devin", "Antigravity", "Qoder", "Qoder IDE", "ZCode", "Sublime Text", "Zed", "TextEdit", "BBEdit", "Nova", "CotEditor", "TextMate", "Xcode", "Android Studio", "IntelliJ IDEA", "IntelliJ IDEA CE", "WebStorm", "PyCharm", "PyCharm CE", "GoLand", "RustRover", "CLion", "PhpStorm", "Rider"];
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

#[tauri::command]
pub async fn ensure_file_path(path: String, kind: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = std::path::Path::new(&path);
        if p.exists() {
            return Ok(());
        }
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if kind.as_deref() == Some("folder") {
            let _ = std::fs::create_dir_all(p);
        } else {
            let initial = if path.ends_with(".json") { "{}\n" } else { "" };
            let _ = std::fs::write(p, initial);
        }
        Ok(())
    }).await.map_err(|error| error.to_string())?
}
