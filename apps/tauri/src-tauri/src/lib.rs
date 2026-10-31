mod accounts;
mod color;
mod commands;
mod icon;
mod meridian;
mod oauth;
mod opencode_import;
mod popover;
mod providers;
mod scheduler;
mod settings_store;
mod settings_win;
mod state;
mod tray;

use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

pub fn run() {
    let initial_state = settings_store::load_or_default();
    let state = Arc::new(Mutex::new(initial_state));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("popover") {
                let _ = popover::toggle(&win, app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::snapshot_get,
            commands::app_quit,
            commands::settings_open,
            commands::settings_get_autostart,
            commands::settings_set_autostart,
            commands::settings_get_all,
            commands::settings_set_primary,
            commands::settings_set_interval,
            commands::settings_set_icon_style,
            commands::settings_set_dev_mode,
            commands::popover_resize,
            commands::account_set_enabled,
            commands::account_rename,
            commands::account_move,
            commands::account_remove,
            commands::meridian_set,
            commands::meridian_set_enabled,
            commands::claude_login_start,
            commands::claude_login_finish,
            commands::codex_login_start,
            commands::codex_login_finish,
            commands::copilot_add_token,
            commands::copilot_import_list,
            commands::copilot_import,
            commands::codex_import_list,
            commands::codex_import,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            popover::create(&handle)?;
            settings_win::prewarm(&handle)?;
            tray::create(&handle, state.clone())?;
            scheduler::start(handle.clone(), state.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "popover" {
                if let tauri::WindowEvent::Focused(false) = event {
                    let app = window.app_handle();
                    // Tray click: cursor is on the tray icon → skip auto-hide.
                    // The tray Click handler owns visibility in that case.
                    if popover::cursor_over_tray(app) {
                        return;
                    }
                    let settings_focused = app
                        .get_webview_window("settings")
                        .map(|w| w.is_focused().unwrap_or(false))
                        .unwrap_or(false);
                    if !settings_focused {
                        let _ = window.hide();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
