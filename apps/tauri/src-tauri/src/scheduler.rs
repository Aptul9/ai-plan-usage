use crate::accounts;
use crate::meridian;
use crate::providers;
use crate::settings_store;
use crate::state::{AppState, MeridianStatus};
use crate::tray;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

static FETCH_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
// Set when run_once found a round already running.
static RERUN: AtomicBool = AtomicBool::new(false);

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

    // Repaint the tray icon every 60s, independent of the fetch loop. The
    // pace-based color depends on the current time, so it must re-render as
    // time elapses even when no new usage data arrived. Local render only,
    // no network call, so it is cheap to run on a short cadence.
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let s = state.lock().await;
            tray::refresh_icon(&app, &s);
        }
    });
}

pub async fn run_once(app: AppHandle, state: Arc<Mutex<AppState>>) {
    let Ok(_fetch_guard) = FETCH_LOCK.try_lock() else {
        // A round is running: it runs once more when it ends.
        RERUN.store(true, Ordering::SeqCst);
        return;
    };
    loop {
        round(&app, &state).await;
        if !RERUN.swap(false, Ordering::SeqCst) {
            break;
        }
    }
}

async fn round(app: &AppHandle, state: &Arc<Mutex<AppState>>) {
    let (list, meridian_cfg) = {
        let s = state.lock().await;
        (s.accounts.clone(), s.meridian.clone())
    };
    let out = providers::fetch_all(list, meridian_cfg, state).await;

    let mut s = state.lock().await;
    let mut dirty = false;
    let profiles = out.meridian.as_deref();
    s.meridian_status = MeridianStatus {
        reachable: profiles.is_some(),
        profiles: profiles.map_or(0, |p| p.len()),
        checked_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    if let Some(found) = profiles {
        let ids: Vec<String> = found.iter().map(|p| p.id.clone()).collect();
        dirty |= accounts::merge_meridian(&mut s.accounts, &ids);
    }
    s.ensure_snapshots();

    let meridian_accounts: Vec<(String, String)> = s
        .accounts
        .iter()
        .filter(|a| a.enabled)
        .filter_map(|a| a.profile_id().map(|p| (a.id.clone(), p.to_string())))
        .collect();
    for (id, profile_id) in meridian_accounts {
        let res = meridian::result_for(profiles, &profile_id);
        if let Some(snap) = s.snapshots.get_mut(&id) {
            res.apply_to(snap);
        }
    }

    for (id, res) in out.results {
        if let Some(new) = &res.rotated {
            if let Some(acc) = s.accounts.iter_mut().find(|a| a.id == id) {
                acc.oauth = Some(new.clone());
                dirty = true;
            }
        }
        if let Some(snap) = s.snapshots.get_mut(&id) {
            res.apply_to(snap);
        }
    }

    let primary = accounts::normalize_primary(&s.accounts, &s.primary);
    s.primary = primary;
    s.mirror_primary();
    s.fetched_at = chrono::Utc::now().to_rfc3339();
    if dirty {
        settings_store::persist(&s);
    }
    let _ = app.emit("snapshot-updated", s.public_state());
    tray::refresh_icon(app, &s);
}
