//! Native macOS window commands. Closing the main window preserves its WebView
//! and background tasks; Quit remains the application's normal exit path.
use tauri::{menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem as Native, Submenu}, Manager};

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let handle = app.handle();
    let close = MenuItem::with_id(handle, "arca-close-window", "Close Window", true, Some("CmdOrCtrl+W"))?;
    let menu = Menu::with_items(handle, &[
        &Submenu::with_items(handle, "Arca", true, &[
            &Native::about(handle, None, Some(AboutMetadata {
                name: Some("Arca".into()), version: Some(app.package_info().version.to_string()), ..Default::default()
            }))?,
            &Native::separator(handle)?, &Native::services(handle, None)?,
            &Native::separator(handle)?, &Native::hide(handle, None)?, &Native::hide_others(handle, None)?,
            &Native::separator(handle)?, &Native::quit(handle, None)?,
        ])?,
        &Submenu::with_items(handle, "File", true, &[&close])?,
        &Submenu::with_items(handle, "Edit", true, &[
            &Native::undo(handle, None)?, &Native::redo(handle, None)?, &Native::separator(handle)?,
            &Native::cut(handle, None)?, &Native::copy(handle, None)?, &Native::paste(handle, None)?,
            &Native::select_all(handle, None)?,
        ])?,
        &Submenu::with_items(handle, "View", true, &[&Native::fullscreen(handle, None)?])?,
        &Submenu::with_id_and_items(handle, "__tauri_window_menu__", "Window", true, &[
            &Native::minimize(handle, None)?, &Native::maximize(handle, None)?,
        ])?,
    ])?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        if event.id().as_ref() == "arca-close-window" {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.close();
            }
        }
    });
    Ok(())
}
