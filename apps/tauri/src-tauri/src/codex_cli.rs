use crate::scheduler;
use crate::state::{AppState, SnapshotError};
use chrono::Utc;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

type AppStateMutex = Arc<Mutex<AppState>>;

static REFRESH_RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_LAUNCH_AT: Lazy<Mutex<Option<chrono::DateTime<Utc>>>> = Lazy::new(|| Mutex::new(None));

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: chrono::Duration = chrono::Duration::seconds(20);
const LAUNCH_COOLDOWN: chrono::Duration = chrono::Duration::minutes(2);
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
struct CredsSnapshot {
    access_token: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CredsFile {
    tokens: Option<Tokens>,
}

#[derive(Debug, Deserialize)]
struct Tokens {
    access_token: Option<String>,
    refresh_token: Option<String>,
}

fn creds_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".codex")
        .join("auth.json")
}

async fn read_creds_snapshot() -> Option<CredsSnapshot> {
    let raw = tokio::fs::read_to_string(creds_path()).await.ok()?;
    let parsed: CredsFile = serde_json::from_str(&raw).ok()?;
    let tokens = parsed.tokens?;
    Some(CredsSnapshot {
        access_token: tokens.access_token.filter(|t| !t.is_empty()),
        refresh_token: tokens.refresh_token.filter(|t| !t.is_empty()),
    })
}

fn advanced_after(before: Option<&CredsSnapshot>, after: &CredsSnapshot) -> bool {
    let Some(access_token) = after.access_token.as_ref() else {
        return false;
    };
    let Some(before) = before else {
        return !access_token.is_empty();
    };
    before.access_token.as_ref() != Some(access_token)
}

fn launch_hidden_codex_refresh() -> Result<Child, String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/d", "/s", "/c", "codex debug models"]);
        command
    };

    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut command = Command::new("codex");
        command.args(["debug", "models"]);
        command
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map_err(|e| format!("Unable to launch hidden Codex refresh: {e}"))
}

fn stop_codex_child(child: &mut Option<Child>) {
    if let Some(child) = child.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    *child = None;
}

async fn set_codex_error(
    app: &AppHandle,
    state: &AppStateMutex,
    message: impl Into<String>,
    detail: Option<impl Into<String>>,
) {
    let mut s = state.lock().await;
    if let Some(snap) = s.snapshots.get_mut("codex") {
        snap.error = Some(SnapshotError {
            message: message.into(),
            kind: "token-expired".into(),
            detail: detail.map(Into::into),
            occurred_at: Some(Utc::now().to_rfc3339()),
            retry_at: None,
        });
    }
    s.mirror_primary();
    let public = s.public_state();
    let _ = app.emit("snapshot-updated", public);
    crate::tray::refresh_icon(app, &s);
}

async fn launch_allowed() -> bool {
    let mut last_launch_at = LAST_LAUNCH_AT.lock().await;
    let now = Utc::now();
    if last_launch_at
        .as_ref()
        .is_some_and(|at| now.signed_duration_since(*at) < LAUNCH_COOLDOWN)
    {
        return false;
    }
    *last_launch_at = Some(now);
    true
}

pub fn refresh_after_token_expired(app: AppHandle, state: AppStateMutex) {
    tauri::async_runtime::spawn(async move {
        if REFRESH_RUNNING.swap(true, Ordering::SeqCst) {
            set_codex_error(
                &app,
                &state,
                "Codex token expired. Waiting for hidden `codex debug models` to refresh credentials.",
                Some("A hidden Codex refresh is already running. This app is polling ~/.codex/auth.json and will refresh usage automatically once the token changes."),
            )
            .await;
            return;
        }

        let before = read_creds_snapshot().await;
        let mut codex_child = None;
        if !before
            .as_ref()
            .and_then(|snapshot| snapshot.refresh_token.as_ref())
            .is_some_and(|token| !token.is_empty())
        {
            REFRESH_RUNNING.store(false, Ordering::SeqCst);
            set_codex_error(
                &app,
                &state,
                "Codex token expired. Run `codex` interactively.",
                Some("No Codex refresh token was found in ~/.codex/auth.json, so the app cannot refresh credentials in the background."),
            )
            .await;
            return;
        }

        if !launch_allowed().await {
            set_codex_error(
                &app,
                &state,
                "Codex token expired. Waiting for hidden `codex debug models` to refresh credentials.",
                Some("A hidden Codex refresh was launched recently. This app is polling ~/.codex/auth.json and will refresh usage automatically once the token changes."),
            )
            .await;
        } else if let Err(e) = launch_hidden_codex_refresh().map(|child| codex_child = Some(child))
        {
            REFRESH_RUNNING.store(false, Ordering::SeqCst);
            set_codex_error(
                &app,
                &state,
                "Codex token expired. Could not launch hidden `codex debug models`.",
                Some(e),
            )
            .await;
            return;
        } else {
            set_codex_error(
                &app,
                &state,
                "Codex token expired. Launched hidden `codex debug models`; waiting for refreshed credentials.",
                Some("Bare `codex` requires a terminal, so this app runs `codex debug models` hidden and polls ~/.codex/auth.json for a refreshed access token."),
            )
            .await;
        }

        let deadline = Utc::now() + POLL_TIMEOUT;
        while Utc::now() < deadline {
            tokio::time::sleep(POLL_INTERVAL).await;
            let child_failed = codex_child
                .as_mut()
                .and_then(|child| child.try_wait().ok().flatten())
                .is_some_and(|status| !status.success());
            let Some(after) = read_creds_snapshot().await else {
                if child_failed {
                    break;
                }
                continue;
            };
            if advanced_after(before.as_ref(), &after) {
                REFRESH_RUNNING.store(false, Ordering::SeqCst);
                stop_codex_child(&mut codex_child);
                scheduler::run_once(app.clone(), state.clone()).await;
                return;
            }
            if child_failed {
                break;
            }
        }

        REFRESH_RUNNING.store(false, Ordering::SeqCst);
        stop_codex_child(&mut codex_child);
        set_codex_error(
            &app,
            &state,
            "Codex token expired. Hidden `codex debug models` did not refresh credentials within 20 seconds.",
            Some("Run Codex interactively if it asks for an update or login, then use Refresh now or wait for the next scheduled poll."),
        )
        .await;
    });
}
