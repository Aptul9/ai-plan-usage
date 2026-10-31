use crate::accounts::{self, Account, Provider, Source};
use crate::oauth;
use crate::opencode_import;
use crate::popover;
use crate::scheduler;
use crate::settings_store;
use crate::settings_win;
use crate::state::AppState;
use crate::tray;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

type AppStateMutex = Arc<Mutex<AppState>>;

// How long a browser sign-in waits for the redirect.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Save, tell both windows, redraw the tray.
fn publish(app: &AppHandle, s: &mut AppState) {
    s.ensure_snapshots();
    s.primary = accounts::normalize_primary(&s.accounts, &s.primary);
    s.mirror_primary();
    settings_store::persist(s);
    let _ = app.emit("snapshot-updated", s.public_state());
    tray::refresh_icon(app, s);
}

fn refresh_soon(app: &AppHandle) {
    let app = app.clone();
    let state = app.state::<AppStateMutex>().inner().clone();
    tauri::async_runtime::spawn(async move { scheduler::run_once(app, state).await });
}

async fn add_account(app: &AppHandle, state: &AppStateMutex, account: Account) -> String {
    let id = account.id.clone();
    {
        let mut s = state.lock().await;
        if s.primary.is_empty() {
            s.primary = id.clone();
        }
        s.accounts.push(account);
        publish(app, &mut s);
    }
    refresh_soon(app);
    id
}

async fn ensure_new_token(state: &AppStateMutex, token: &str) -> Result<(), String> {
    let s = state.lock().await;
    if s.accounts.iter().any(|a| a.token.as_deref() == Some(token)) {
        return Err("That token is already added".into());
    }
    Ok(())
}

fn label_or(label: &str, fallback: &str) -> String {
    let l = label.trim();
    if l.is_empty() {
        fallback.to_string()
    } else {
        l.to_string()
    }
}

// OpenCode's SQLite store can wait up to its busy timeout, so it stays off
// the async runtime.
async fn opencode_logins() -> Result<Vec<opencode_import::CopilotLogin>, String> {
    tokio::task::spawn_blocking(opencode_import::list)
        .await
        .map_err(|e| e.to_string())?
}

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

/// Async: building the window from a sync command deadlocks on Windows.
#[tauri::command]
pub async fn settings_open(app: AppHandle) -> Result<(), String> {
    settings_win::open(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn settings_get_autostart(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
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
    let mut v = s.public_state();
    v["intervals"] = json!(AppState::poll_intervals());
    v["iconStyles"] = json!(AppState::icon_styles());
    Ok(v)
}

#[tauri::command]
pub async fn settings_set_primary(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    id: String,
) -> Result<String, String> {
    let mut s = state.lock().await;
    if s.accounts.iter().any(|a| a.id == id && a.enabled) {
        s.primary = id;
        publish(&app, &mut s);
    }
    Ok(s.primary.clone())
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
            publish(&app, &mut s);
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

#[tauri::command]
pub async fn account_set_enabled(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    id: String,
    on: bool,
) -> Result<(), String> {
    {
        let mut s = state.lock().await;
        if let Some(a) = s.accounts.iter_mut().find(|a| a.id == id) {
            a.enabled = on;
        }
        publish(&app, &mut s);
    }
    if on {
        refresh_soon(&app);
    }
    Ok(())
}

#[tauri::command]
pub async fn account_rename(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    id: String,
    label: String,
) -> Result<(), String> {
    let label = label.trim().to_string();
    if label.is_empty() {
        return Err("Name cannot be empty".into());
    }
    let mut s = state.lock().await;
    if let Some(a) = s.accounts.iter_mut().find(|a| a.id == id) {
        a.label = label;
    }
    publish(&app, &mut s);
    Ok(())
}

#[tauri::command]
pub async fn account_move(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    id: String,
    to: usize,
) -> Result<(), String> {
    let mut s = state.lock().await;
    if let Some(from) = s.accounts.iter().position(|a| a.id == id) {
        let acc = s.accounts.remove(from);
        let to = to.min(s.accounts.len());
        s.accounts.insert(to, acc);
        publish(&app, &mut s);
    }
    Ok(())
}

#[tauri::command]
pub async fn account_remove(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    id: String,
) -> Result<(), String> {
    let mut s = state.lock().await;
    // Remembered so the next poll does not add the Meridian profile back.
    if let Some(pid) = id.strip_prefix(accounts::MERIDIAN_PREFIX) {
        if !s.meridian.removed.iter().any(|r| r == pid) {
            s.meridian.removed.push(pid.to_string());
        }
    }
    s.accounts.retain(|a| a.id != id);
    publish(&app, &mut s);
    Ok(())
}

/// `api_key`: None keeps the stored key, an empty string removes it.
#[tauri::command]
pub async fn meridian_set(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    url: String,
    api_key: Option<String>,
) -> Result<(), String> {
    {
        let mut s = state.lock().await;
        s.meridian.url = url.trim().trim_end_matches('/').to_string();
        if let Some(k) = api_key {
            let k = k.trim().to_string();
            s.meridian.api_key = if k.is_empty() { None } else { Some(k) };
        }
        publish(&app, &mut s);
    }
    refresh_soon(&app);
    Ok(())
}

/// Turns Meridian on or off. Off drops its accounts; on adds every profile
/// back on the next poll, removed ones included.
#[tauri::command]
pub async fn meridian_set_enabled(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    on: bool,
) -> Result<(), String> {
    {
        let mut s = state.lock().await;
        s.meridian.enabled = on;
        if on {
            s.meridian.removed.clear();
        } else {
            s.accounts.retain(|a| a.source != Source::Meridian);
        }
        publish(&app, &mut s);
    }
    refresh_soon(&app);
    Ok(())
}

#[tauri::command]
pub async fn claude_login_start(state: State<'_, AppStateMutex>) -> Result<String, String> {
    let p = oauth::pkce();
    let url = oauth::claude_authorize_url(&p);
    state.lock().await.pending.claude_pkce = Some(p);
    oauth::open_browser(&url);
    Ok(url)
}

#[tauri::command]
pub async fn claude_login_finish(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    code: String,
    label: String,
) -> Result<String, String> {
    let pkce = state
        .lock()
        .await
        .pending
        .claude_pkce
        .clone()
        .ok_or("Open the sign-in page first")?;
    let creds = oauth::claude_exchange(&code, &pkce).await?;
    state.lock().await.pending.claude_pkce = None;
    let mut acc = Account::new(Provider::Claude, Source::Oauth, &label_or(&label, "Claude"));
    acc.oauth = Some(creds);
    Ok(add_account(&app, state.inner(), acc).await)
}

fn clear_codex_pending(s: &mut AppState, owner: &str) {
    if s.pending.codex.as_deref() == Some(owner) {
        s.pending.codex = None;
    }
}

/// The page the browser lands on after the redirect.
fn callback_page(err: Option<&String>) -> String {
    let body = match err {
        None => "<h2>Signed in</h2><p>Codex is added to ai-plan-usage. You can close this tab.</p>".to_string(),
        Some(e) => {
            let e = e.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            format!("<h2>Sign-in failed</h2><p>{e}</p><p>Go back to ai-plan-usage and try again.</p>")
        }
    };
    let html = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>ai-plan-usage</title>         <body style=\"font-family:Segoe UI,system-ui,sans-serif;padding:40px;color:#18181b\">{body}</body>"
    );
    format!(
        "HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: {}
Connection: close

{html}",
        html.len()
    )
}

/// Browser sign-in: a one-shot server on a registered localhost port catches
/// OpenAI's redirect. A newer click replaces this one, which then lets go of
/// its port within a second.
#[tauri::command]
pub async fn codex_login(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    label: String,
) -> Result<String, String> {
    let p = oauth::pkce();
    state.lock().await.pending.codex = Some(p.state.clone());
    let mut bound = None;
    for port in oauth::CODEX_PORTS {
        if let Ok(l) = TcpListener::bind(("127.0.0.1", port)).await {
            bound = Some((l, port));
            break;
        }
    }
    let Some((listener, port)) = bound else {
        clear_codex_pending(&mut *state.lock().await, &p.state);
        return Err("Ports 1455 and 1457 are busy: another Codex or OpenCode sign-in is open. Close it and try again.".into());
    };
    let redirect = oauth::codex_redirect(port);
    oauth::open_browser(&oauth::codex_authorize_url(&p, &redirect));
    let started = Instant::now();
    loop {
        if state.lock().await.pending.codex.as_deref() != Some(p.state.as_str()) {
            return Err("Replaced by a newer sign-in".into());
        }
        if started.elapsed() > LOGIN_TIMEOUT {
            clear_codex_pending(&mut *state.lock().await, &p.state);
            return Err("Sign-in timed out. Try again.".into());
        }
        let Ok(Ok((mut sock, _))) = tokio::time::timeout(Duration::from_secs(1), listener.accept()).await else {
            continue;
        };
        let mut buf = [0u8; 8192];
        let Ok(Ok(n)) = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf)).await else {
            continue;
        };
        let req = String::from_utf8_lossy(&buf[..n]);
        let Some(result) = oauth::callback_code(req.lines().next().unwrap_or(""), &p) else {
            let _ = sock
                .write_all(b"HTTP/1.1 404 Not Found
Content-Length: 0
Connection: close

")
                .await;
            continue;
        };
        let result = match result {
            Ok(code) => oauth::codex_exchange(&code, &redirect, &p).await,
            Err(e) => Err(e),
        };
        let _ = sock.write_all(callback_page(result.as_ref().err()).as_bytes()).await;
        clear_codex_pending(&mut *state.lock().await, &p.state);
        let mut acc = Account::new(Provider::Codex, Source::Oauth, &label_or(&label, "Codex"));
        acc.oauth = Some(result?);
        return Ok(add_account(&app, state.inner(), acc).await);
    }
}

#[tauri::command]
pub async fn copilot_add_token(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    token: String,
    label: String,
) -> Result<String, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("Paste a token".into());
    }
    ensure_new_token(state.inner(), &token).await?;
    let mut acc = Account::new(Provider::Copilot, Source::Token, &label_or(&label, "Copilot"));
    acc.token = Some(token);
    Ok(add_account(&app, state.inner(), acc).await)
}

#[tauri::command]
pub async fn copilot_import_list() -> Result<Value, String> {
    let logins = opencode_logins().await?;
    Ok(json!(logins
        .iter()
        .map(|l| json!({ "id": l.id, "label": l.label }))
        .collect::<Vec<_>>()))
}

#[tauri::command]
pub async fn copilot_import(
    app: AppHandle,
    state: State<'_, AppStateMutex>,
    id: String,
    label: String,
) -> Result<String, String> {
    let login = opencode_logins()
        .await?
        .into_iter()
        .find(|l| l.id == id)
        .ok_or("That OpenCode login is gone")?;
    ensure_new_token(state.inner(), &login.token).await?;
    let mut acc = Account::new(Provider::Copilot, Source::Token, &label_or(&label, &login.label));
    acc.token = Some(login.token);
    Ok(add_account(&app, state.inner(), acc).await)
}
