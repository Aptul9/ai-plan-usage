use crate::popover;
use crate::scheduler;
use crate::settings_store;
use crate::settings_win;
use crate::state::{AppState, ProviderEntry, ProviderId};
use crate::tray;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

type AppStateMutex = Arc<Mutex<AppState>>;

#[tauri::command]
pub async fn snapshot_get(state: State<'_, AppStateMutex>) -> Result<Value, String> {
    let s = state.lock().await;
    Ok(s.public_state())
}

#[tauri::command]
pub async fn app_quit(app: AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn settings_get_autostart(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    mgr.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn settings_set_autostart(app: AppHandle, on: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if on {
        mgr.enable().map_err(|e| e.to_string())?;
    } else {
        mgr.disable().map_err(|e| e.to_string())?;
    }
    mgr.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn settings_get_all(state: State<'_, AppStateMutex>) -> Result<Value, String> {
    let s = state.lock().await;
    Ok(json!({
        "providers": s.providers,
        "primaryProvider": s.primary_provider,
        "pollIntervalMs": s.poll_interval_ms,
        "iconStyle": s.icon_style,
        "dataSource": s.data_source,
        "intervals": AppState::poll_intervals(),
        "iconStyles": AppState::icon_styles(),
        "devMode": s.dev_mode,
    }))
}

#[tauri::command]
pub async fn settings_set_provider_enabled(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    id: ProviderId,
    on: bool,
) -> Result<Value, String> {
    let mut s = state.lock().await;
    if let Some(p) = s.providers.iter_mut().find(|x| x.id == id) {
        if p.available {
            p.enabled = on;
        }
    }
    let primary_ok = s
        .providers
        .iter()
        .any(|x| x.id == s.primary_provider && x.enabled);
    if !primary_ok {
        if let Some(first) = s.providers.iter().find(|x| x.enabled && x.available) {
            s.primary_provider = first.id.clone();
        }
    }
    s.mirror_primary();
    settings_store::persist(&s);
    let providers_clone: Vec<ProviderEntry> = s.providers.clone();
    let primary_clone = s.primary_provider.clone();
    let public = s.public_state();
    let _ = app.emit("snapshot-updated", public);
    tray::refresh_icon(&app, &s);
    Ok(json!({
        "providers": providers_clone,
        "primaryProvider": primary_clone,
    }))
}

#[tauri::command]
pub async fn settings_set_primary(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    id: ProviderId,
) -> Result<String, String> {
    let mut s = state.lock().await;
    let usable = s
        .providers
        .iter()
        .any(|x| x.id == id && x.enabled && x.available);
    if usable {
        s.primary_provider = id;
        s.mirror_primary();
        settings_store::persist(&s);
        let public = s.public_state();
        let _ = app.emit("snapshot-updated", public);
        tray::refresh_icon(&app, &s);
    }
    Ok(s.primary_provider.clone())
}

#[tauri::command]
pub async fn settings_set_interval(
    state: State<'_, AppStateMutex>,
    ms: u64,
) -> Result<u64, String> {
    let mut s = state.lock().await;
    if AppState::poll_intervals().iter().any(|x| x.ms == ms) {
        s.poll_interval_ms = ms;
        settings_store::persist(&s);
    }
    Ok(s.poll_interval_ms)
}

#[tauri::command]
pub async fn settings_set_icon_style(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    style: String,
) -> Result<String, String> {
    {
        let mut s = state.lock().await;
        if AppState::icon_styles().contains(&style.as_str()) {
            s.icon_style = style;
            settings_store::persist(&s);
            let public = s.public_state();
            let _ = app.emit("snapshot-updated", public);
            tray::refresh_icon(&app, &s);
        }
    }
    let app_clone = app.clone();
    let state_arc = app.state::<AppStateMutex>().inner().clone();
    tauri::async_runtime::spawn(async move {
        tray::rebuild_menu(&app_clone, &state_arc).await;
    });
    let s = state.lock().await;
    Ok(s.icon_style.clone())
}

#[tauri::command]
pub async fn settings_get_copilot_status(state: State<'_, AppStateMutex>) -> Result<Value, String> {
    let s = state.lock().await;
    Ok(json!({
        "hasToken": s.copilot_token.is_some(),
        "encryptionAvailable": true,
    }))
}

#[tauri::command]
pub async fn settings_set_copilot_token(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    token: String,
) -> Result<bool, String> {
    let trimmed = token.trim().to_string();
    {
        let mut s = state.lock().await;
        let has_token = !trimmed.is_empty();
        s.copilot_token = if has_token { Some(trimmed) } else { None };
        if let Some(p) = s.providers.iter_mut().find(|p| p.id == "copilot") {
            p.available = has_token;
            if !has_token {
                p.enabled = false;
            }
        }
        let primary_ok = s
            .providers
            .iter()
            .any(|x| x.id == s.primary_provider && x.enabled && x.available);
        if !primary_ok {
            if let Some(first) = s.providers.iter().find(|x| x.enabled && x.available) {
                s.primary_provider = first.id.clone();
            }
        }
        s.mirror_primary();
        settings_store::persist(&s);
        let public = s.public_state();
        let _ = app.emit("snapshot-updated", public);
        crate::tray::refresh_icon(&app, &s);
    }
    let app_clone = app.clone();
    let state_arc = app.state::<AppStateMutex>().inner().clone();
    tauri::async_runtime::spawn(async move {
        scheduler::run_once(app_clone, state_arc).await;
    });
    let s = state.lock().await;
    Ok(s.copilot_token.is_some())
}

#[tauri::command]
pub async fn settings_set_dev_mode(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    on: bool,
) -> Result<bool, String> {
    {
        let mut s = state.lock().await;
        s.dev_mode = on;
        settings_store::persist(&s);
    }
    let app_clone = app.clone();
    let state_arc = app.state::<AppStateMutex>().inner().clone();
    tauri::async_runtime::spawn(async move {
        tray::rebuild_menu(&app_clone, &state_arc).await;
    });
    let s = state.lock().await;
    Ok(s.dev_mode)
}

#[tauri::command]
pub async fn popover_resize(app: AppHandle, w: u32, h: u32) -> Result<(), String> {
    popover::resize(&app, w, h).map_err(|e| e.to_string())
}

// Keep settings_win in scope for completion
#[allow(dead_code)]
fn _ref_settings_win(app: &AppHandle) {
    let _ = settings_win::open(app);
}
