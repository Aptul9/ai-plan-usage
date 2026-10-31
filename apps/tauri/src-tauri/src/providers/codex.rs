use crate::accounts::OauthCreds;
use crate::oauth;
use crate::opencode_import;
use crate::providers::{refresh_failed, save_rotated, FetchResult, SharedState};
use crate::state::UsageBlock;
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use std::time::Duration;

// OpenCode refreshes when less than 5 minutes remain.
const REFRESH_SKEW_MS: i64 = 5 * 60_000;
const SIGN_IN_AGAIN: &str = "Codex sign-in expired. Sign in again in Settings.";
const OPENCODE_EXPIRED: &str = "OpenCode's Codex login expired. Use Codex in OpenCode once to renew it.";

#[derive(Debug, Deserialize)]
struct Resp {
    rate_limit: Option<RateLimit>,
    plan_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RateLimit {
    primary_window: Option<RateWindow>,
    secondary_window: Option<RateWindow>,
}

#[derive(Debug, Deserialize)]
struct RateWindow {
    used_percent: Option<f64>,
    reset_at: Option<i64>,
}

fn iso_from_unix(secs: i64) -> Option<String> {
    Utc.timestamp_opt(secs, 0).single().map(|d| d.to_rfc3339())
}

async fn request(
    client: &reqwest::Client,
    creds: &OauthCreds,
) -> reqwest::Result<reqwest::Response> {
    let mut req = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .bearer_auth(&creds.access)
        .header("User-Agent", "ai-plan-usage")
        .header("Accept", "application/json");
    if let Some(aid) = &creds.account_id {
        req = req.header("ChatGPT-Account-Id", aid);
    }
    req.send().await
}

fn client() -> Result<reqwest::Client, FetchResult> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| FetchResult::err(format!("Codex client: {e}"), "upstream"))
}

/// Usage with the login OpenCode holds, read fresh each poll. Never refreshed
/// here: OpenCode owns the refresh token, and spending it would sign OpenCode out.
pub async fn fetch_opencode(cred_id: &str) -> FetchResult {
    let id = cred_id.to_string();
    let found = tokio::task::spawn_blocking(move || {
        opencode_import::codex_logins().map(|l| l.into_iter().find(|l| l.id == id))
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    let creds = match found {
        Ok(Some(l)) => l.creds,
        Ok(None) => return FetchResult::err("This Codex login is no longer in OpenCode.", "signed-out"),
        Err(e) => return FetchResult::err(format!("OpenCode store: {e}"), "upstream"),
    };
    if creds.expires_at <= Utc::now().timestamp_millis() {
        return FetchResult::err(OPENCODE_EXPIRED, "signed-out");
    }
    let client = match client() {
        Ok(c) => c,
        Err(e) => return e,
    };
    let resp = match request(&client, &creds).await {
        Ok(r) => r,
        Err(e) => return FetchResult::err(format!("Codex network: {e}"), "offline"),
    };
    if resp.status().as_u16() == 401 {
        return FetchResult::err(OPENCODE_EXPIRED, "signed-out");
    }
    read_usage(resp).await
}

pub async fn fetch(id: &str, creds: &OauthCreds, state: &SharedState) -> FetchResult {
    let mut rotated = None;
    let result = fetch_inner(id, creds, state, &mut rotated).await;
    result.with_rotated(rotated)
}

async fn fetch_inner(
    id: &str,
    creds: &OauthCreds,
    state: &SharedState,
    rotated: &mut Option<OauthCreds>,
) -> FetchResult {
    let client = match client() {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mut current = creds.clone();
    if current.expires_at - Utc::now().timestamp_millis() < REFRESH_SKEW_MS {
        match oauth::codex_refresh(&current).await {
            Ok(new) => {
                save_rotated(state, id, &new).await;
                current = new.clone();
                *rotated = Some(new);
            }
            Err(detail) => return refresh_failed("Codex", SIGN_IN_AGAIN, detail),
        }
    }
    let mut resp = match request(&client, &current).await {
        Ok(r) => r,
        Err(e) => return FetchResult::err(format!("Codex network: {e}"), "offline"),
    };
    // One refresh on 401, unless this round already refreshed.
    if resp.status().as_u16() == 401 && rotated.is_none() {
        match oauth::codex_refresh(&current).await {
            Ok(new) => {
                save_rotated(state, id, &new).await;
                current = new.clone();
                *rotated = Some(new);
                resp = match request(&client, &current).await {
                    Ok(r) => r,
                    Err(e) => return FetchResult::err(format!("Codex network: {e}"), "offline"),
                };
            }
            Err(detail) => return refresh_failed("Codex", SIGN_IN_AGAIN, detail),
        }
    }
    if resp.status().as_u16() == 401 {
        return FetchResult::err(SIGN_IN_AGAIN, "signed-out");
    }
    read_usage(resp).await
}

async fn read_usage(resp: reqwest::Response) -> FetchResult {
    let status = resp.status();
    if !status.is_success() {
        return FetchResult::err(format!("Codex HTTP {}", status.as_u16()), "upstream");
    }
    let data: Resp = match resp.json().await {
        Ok(d) => d,
        Err(e) => return FetchResult::err(format!("Codex parse: {e}"), "upstream"),
    };
    let window = |w: Option<&RateWindow>| {
        w.map(|w| UsageBlock {
            used_pct: w.used_percent,
            resets_at: w.reset_at.and_then(iso_from_unix),
            ..Default::default()
        })
    };
    let limits = data.rate_limit.as_ref();
    FetchResult {
        session: window(limits.and_then(|r| r.primary_window.as_ref())),
        weekly: window(limits.and_then(|r| r.secondary_window.as_ref())),
        plan_type: data.plan_type,
        error: None,
        rotated: None,
    }
}
