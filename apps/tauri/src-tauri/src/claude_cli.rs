use crate::scheduler;
use crate::state::{AppState, SnapshotError};
use chrono::Utc;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::path::Path;
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

const MIN_FRESH_MS: i64 = 60_000;
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: chrono::Duration = chrono::Duration::seconds(20);
const LAUNCH_COOLDOWN: chrono::Duration = chrono::Duration::minutes(2);
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
struct CredsSnapshot {
    access_token: Option<String>,
    expires_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CredsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeOauth>,
}

#[derive(Debug, Deserialize)]
struct ClaudeOauth {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>,
}

fn creds_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join(".credentials.json")
}

#[cfg(target_os = "windows")]
fn resolve_claude_command() -> PathBuf {
    if let Ok(output) = Command::new("where")
        .arg("claude")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(path) = stdout
            .lines()
            .map(str::trim)
            .map(PathBuf::from)
            .find(|path| path.exists())
        {
            return path;
        }
    }

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let winget_link = Path::new(&local_app_data)
            .join("Microsoft")
            .join("WinGet")
            .join("Links")
            .join("claude.exe");
        if winget_link.exists() {
            return winget_link;
        }
    }

    PathBuf::from("claude")
}

#[cfg(not(target_os = "windows"))]
fn resolve_claude_command() -> PathBuf {
    PathBuf::from("claude")
}

async fn read_creds_snapshot() -> Option<CredsSnapshot> {
    let raw = tokio::fs::read_to_string(creds_path()).await.ok()?;
    let parsed: CredsFile = serde_json::from_str(&raw).ok()?;
    let oauth = parsed.claude_ai_oauth?;
    Some(CredsSnapshot {
        access_token: oauth.access_token.filter(|t| !t.is_empty()),
        expires_at: oauth.expires_at,
    })
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn is_fresh(snapshot: &CredsSnapshot) -> bool {
    snapshot.access_token.is_some()
        && snapshot
            .expires_at
            .is_some_and(|expires_at| expires_at > now_ms() + MIN_FRESH_MS)
}

fn advanced_after(before: Option<&CredsSnapshot>, after: &CredsSnapshot) -> bool {
    let Some(before) = before else {
        return is_fresh(after);
    };
    after.access_token != before.access_token
        || after.expires_at.unwrap_or_default() > before.expires_at.unwrap_or_default()
}

fn launch_hidden_claude() -> Result<Child, String> {
    let mut command = Command::new(resolve_claude_command());
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
        .map_err(|e| format!("Unable to launch hidden Claude process: {e}"))
}

fn stop_claude_child(child: &mut Option<Child>) {
    if let Some(child) = child.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    *child = None;
}

async fn set_claude_error(
    app: &AppHandle,
    state: &AppStateMutex,
    message: impl Into<String>,
    detail: Option<impl Into<String>>,
) {
    let mut s = state.lock().await;
    if let Some(snap) = s.snapshots.get_mut("claude") {
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
            set_claude_error(
                &app,
                &state,
                "Claude token expired. Waiting for hidden `claude` to refresh credentials.",
                Some("A hidden Claude refresh is already running. This app is polling ~/.claude/.credentials.json and will refresh usage automatically once the token changes."),
            )
            .await;
            return;
        }

        let before = read_creds_snapshot().await;
        let mut claude_child = None;
        if !launch_allowed().await {
            set_claude_error(
                &app,
                &state,
                "Claude token expired. Waiting for hidden `claude` to refresh credentials.",
                Some("A hidden Claude refresh was launched recently. This app is polling ~/.claude/.credentials.json and will refresh usage automatically once the token changes."),
            )
            .await;
        } else if let Err(e) = launch_hidden_claude().map(|child| claude_child = Some(child)) {
            REFRESH_RUNNING.store(false, Ordering::SeqCst);
            set_claude_error(
                &app,
                &state,
                "Claude token expired. Could not launch hidden `claude`.",
                Some(e),
            )
            .await;
            return;
        } else {
            set_claude_error(
                &app,
                &state,
                "Claude token expired. Launched hidden `claude`; waiting for refreshed credentials.",
                Some("This app is polling ~/.claude/.credentials.json and will refresh usage automatically once the token changes."),
            )
            .await;
        }

        let deadline = Utc::now() + POLL_TIMEOUT;
        while Utc::now() < deadline {
            tokio::time::sleep(POLL_INTERVAL).await;
            let Some(after) = read_creds_snapshot().await else {
                continue;
            };
            if is_fresh(&after) && advanced_after(before.as_ref(), &after) {
                REFRESH_RUNNING.store(false, Ordering::SeqCst);
                stop_claude_child(&mut claude_child);
                scheduler::run_once(app.clone(), state.clone()).await;
                return;
            }
        }

        REFRESH_RUNNING.store(false, Ordering::SeqCst);
        stop_claude_child(&mut claude_child);
        set_claude_error(
            &app,
            &state,
            "Claude token expired. Hidden `claude` did not refresh credentials within 20 seconds.",
            Some("Use Refresh now after Claude finishes updating ~/.claude/.credentials.json, or wait for the next scheduled poll."),
        )
        .await;
    });
}
