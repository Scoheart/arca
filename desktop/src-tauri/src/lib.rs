pub mod cli_tool;
pub mod commands;
mod capture;
mod traces;
mod instructions;
mod observation_watcher;
mod file_editors;
mod agent_launch;
mod trackpad_gestures;
#[cfg(target_os = "macos")]
mod window_lifecycle;
pub mod updater_guard;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Bootstrap before constructing Tauri. A configured WebView can exist
    // before the user setup hook runs, so setup is too late to be the IPC write
    // boundary. Desktop failures publish a read-only backend and still return a
    // diagnostic report. CLI read queries use the same degraded report while
    // every mutation remains constrained by the published backend status.
    let bootstrap = arca_core::application::ArcaCore::bootstrap();
    for warning in &bootstrap.warnings {
        eprintln!("Arca startup warning: {warning:?}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // Self-update: checks the stable-release channel (latest.json on the
        // newest vX.Y.Z GitHub Release); the frontend drives the UX.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Needed to relaunch the app after an update is installed.
        .plugin(tauri_plugin_process::init())
        .on_window_event(|window, event| {
            #[cfg(target_os = "macos")]
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (window, event);
        })
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            window_lifecycle::install(app)?;
            // Catch native selections changed while Arca was closed.
            if let Err(error) = arca_core::application::models::reconcile_native_selection() {
                eprintln!("Arca native model selection reconciliation: {error}");
            }
            if let Err(error) = observation_watcher::start(app.handle().clone()) {
                eprintln!("Arca observation watcher warning: {error}");
            }
            if bootstrap.skill_updates_allowed {
                std::thread::spawn(|| {
                    let _ = arca_core::application::skills::check_updates_if_due();
                });
            }

            // macOS may keep the process alive after the last window closes.
            // Always restore the configured main window on a fresh launch.
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(debug_assertions)]
                window.set_title("Arca · 本地开发预览")?;
                #[cfg(target_os = "macos")]
                if let Err(error) = trackpad_gestures::install(window.clone()) {
                    eprintln!("Arca trackpad phase bridge unavailable: {error}");
                }
                window.show()?;
                window.set_focus()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            trackpad_gestures::trackpad_gestures_available,
            agent_launch::get_agent_launch_info,
            agent_launch::get_agent_runtime_version,
            agent_launch::configure_agent_launch,
            agent_launch::launch_agent,
            agent_launch::get_terminal_settings,
            agent_launch::set_terminal_preference,
            file_editors::list_file_editors,
            file_editors::detect_agent_installation,
            commands::get_backend_status,
            commands::copy_to_clipboard,
            commands::get_workspace_snapshot,
            commands::list_agent_capabilities,
            commands::plan_operation,
            commands::commit_operation,
            commands::cancel_operation,
            commands::list_consumption_inventory,
            commands::get_resource_observation,
            commands::list_model_adoption_candidates,
            commands::plan_set_agent_consumption,
            commands::plan_set_mcp_enabled,
            commands::plan_set_skill_enabled,
            commands::plan_set_model_enabled,
            commands::plan_set_active_model,
            commands::plan_set_asset_consumers,
            commands::plan_update_agent_capabilities,
            commands::plan_update_central_asset,
            commands::plan_delete_central_asset,
            commands::commit_asset_operation,
            commands::cancel_asset_operation,
            commands::list_skills_inventory,
            commands::list_skill_agents,
            commands::get_skill_detail,
            commands::resolve_skill_source,
            commands::resolve_local_skill_source_dialog,
            commands::resolve_archive_skill_source_dialog,
            commands::plan_skill_asset_install,
            commands::commit_skill_install,
            commands::plan_skill_asset_import,
            commands::plan_skill_import,
            commands::commit_skill_import,
            commands::plan_skill_update,
            commands::commit_skill_update,
            commands::plan_skill_remove,
            commands::commit_skill_remove,
            commands::commit_skill_assignment,
            commands::plan_skill_repair,
            commands::commit_skill_repair,
            commands::check_skill_updates,
            commands::cancel_skill_operation,
            commands::list_registry,
            commands::get_registry_snapshot,
            commands::list_model_profiles,
            commands::export_model_curl,
            commands::list_model_providers,
            commands::list_model_provider_instances,
            commands::discover_provider_models,
            commands::reveal_model_provider_credential,
            commands::infer_model_provider,
            commands::validate_model_credential_source,
            commands::list_model_agents,
            commands::set_model_credential_delivery,
            commands::set_agent_credential_delivery,
            commands::list_registry_all,
            commands::list_custom_registry_keys,
            commands::preview_pasted_config,
            commands::import_pasted_config,
            commands::list_sources,
            commands::subscribe_source,
            commands::add_local_source,
            commands::add_local_source_dialog,
            commands::export_effective_dialog,
            commands::add_builtin_collection,
            commands::refresh_source,
            commands::set_source_enabled,
            commands::remove_source,
            commands::list_agents,
            commands::get_pinned_agents,
            commands::set_pinned_agents,
            commands::get_ui_locale,
            commands::set_ui_locale,
            commands::list_mcp_icon_preferences,
            commands::set_mcp_builtin_icon,
            commands::import_mcp_icon_dialog,
            commands::reset_mcp_icon,
            traces::trace_index,
            instructions::instruction_list,
            instructions::instruction_get_paths,
            instructions::instruction_set_paths,
            instructions::instruction_read,
            instructions::instruction_prepare_save,
            instructions::instruction_prepare_create,
            instructions::instruction_commit,
            traces::trace_page,
            traces::trace_detail,
            traces::trace_import,
            traces::trace_export,
            capture::capture_environment,
            capture::capture_proxy,
            capture::capture_install_engine,
            capture::capture_open_certificate,
            capture::capture_launch_agent,
            capture::capture_sessions,
            capture::capture_snapshot,
            capture::capture_delta,
            capture::capture_detail,
            capture::capture_start,
            capture::capture_stop,
            capture::capture_export,
            commands::get_proxy_settings,
            commands::set_proxy_settings,
            commands::add_agent,
            commands::update_agent,
            commands::scan_installed,
            commands::preview_install,
            cli_tool::cli_status,
            cli_tool::install_cli,
            updater_guard::update_environment
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(&event, tauri::RunEvent::Exit) { arca_core::capture::shutdown(); }
            #[cfg(target_os = "macos")]
            if matches!(&event, tauri::RunEvent::Exit) { trackpad_gestures::stop(); }
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = event
            {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
        });
}
