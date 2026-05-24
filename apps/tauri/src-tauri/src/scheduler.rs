use crate::providers;
use crate::state::AppState;
use crate::tray;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

static FETCH_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub fn start(app: AppHandle, state: Arc<Mutex<AppState>>) {
    let app_clone = app.clone();
    let state_clone = state.clone();
    tauri::async_runtime::spawn(async move {
        // initial fetch after small grace
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        run_once(app_clone.clone(), state_clone.clone()).await;
        loop {
            let interval_ms = {
                let s = state_clone.lock().await;
                s.poll_interval_ms
            };
            tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
            run_once(app_clone.clone(), state_clone.clone()).await;
        }
    });
}

pub async fn run_once(app: AppHandle, state: Arc<Mutex<AppState>>) {
    let Ok(_fetch_guard) = FETCH_LOCK.try_lock() else {
        return;
    };

    let (copilot_token, enabled_provider_ids) = {
        let s = state.lock().await;
        let enabled = s
            .providers
            .iter()
            .filter(|p| p.enabled && p.available)
            .map(|p| p.id.clone())
            .collect();
        (s.copilot_token.clone(), enabled)
    };
    let results = providers::fetch_all(copilot_token, enabled_provider_ids).await;
    let claude_token_expired = results.iter().any(|(id, res)| {
        id == "claude"
            && res
                .error
                .as_ref()
                .is_some_and(|err| err.kind == "token-expired")
    });
    let codex_token_expired = results.iter().any(|(id, res)| {
        id == "codex"
            && res
                .error
                .as_ref()
                .is_some_and(|err| err.kind == "token-expired")
    });
    {
        let mut s = state.lock().await;
        for (id, res) in results {
            if let Some(snap) = s.snapshots.get_mut(&id) {
                res.apply_to(snap);
            }
        }
        s.mirror_primary();
        s.fetched_at = chrono::Utc::now().to_rfc3339();
        let public = s.public_state();
        let _ = app.emit("snapshot-updated", public);
        tray::refresh_icon(&app, &s);
    }
    if claude_token_expired {
        crate::claude_cli::refresh_after_token_expired(app.clone(), state.clone());
    }
    if codex_token_expired {
        crate::codex_cli::refresh_after_token_expired(app, state);
    }
}
