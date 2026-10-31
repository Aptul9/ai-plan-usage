use crate::accounts::OauthCreds;
use crate::oauth;
use crate::providers::{refresh_failed, save_rotated, FetchResult, SharedState};
use crate::state::UsageBlock;
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use std::time::Duration;

// OpenCode refreshes when less than 5 minutes remain.
const REFRESH_SKEW_MS: i64 = 5 * 60_000;
const SIGN_IN_AGAIN: &str = "Codex sign-in expired. Sign in again in Settings.";

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
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => return FetchResult::err(format!("Codex client: {e}"), "upstream"),
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
    let status = resp.status();
    if status.as_u16() == 401 {
        return FetchResult::err(SIGN_IN_AGAIN, "signed-out");
    }
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
