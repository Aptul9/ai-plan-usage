use crate::providers::FetchResult;
use crate::state::UsageBlock;
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

fn creds_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".codex")
        .join("auth.json")
}

#[derive(Debug, Deserialize)]
struct CredsFile {
    #[serde(rename = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,
    tokens: Option<Tokens>,
}

#[derive(Debug, Deserialize)]
struct Tokens {
    access_token: Option<String>,
    account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Resp {
    rate_limit: Option<RateLimit>,
    plan_type: Option<String>,
    email: Option<String>,
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

pub async fn fetch() -> FetchResult {
    let creds_raw = match tokio::fs::read_to_string(creds_path()).await {
        Ok(r) => r,
        Err(_) => {
            return FetchResult::err("Codex not logged in. Run `codex`.", "not-authenticated");
        }
    };
    let creds: CredsFile = match serde_json::from_str(&creds_raw) {
        Ok(c) => c,
        Err(_) => return FetchResult::err("Codex credentials malformed", "bad-credentials"),
    };
    let access_token = creds
        .tokens
        .as_ref()
        .and_then(|t| t.access_token.clone())
        .filter(|t| !t.is_empty())
        .or(creds.openai_api_key.clone().filter(|t| !t.is_empty()));
    let account_id = creds.tokens.as_ref().and_then(|t| t.account_id.clone());
    let Some(token) = access_token else {
        return FetchResult::err("Codex credentials missing token", "bad-credentials");
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => return FetchResult::err(format!("Codex client: {e}"), "unknown"),
    };
    let mut req = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .bearer_auth(&token)
        .header("User-Agent", "ai-plan-usage")
        .header("Accept", "application/json");
    if let Some(aid) = account_id {
        req = req.header("ChatGPT-Account-Id", aid);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return FetchResult::err(format!("Codex network: {e}"), "network-error"),
    };
    let status = resp.status();
    if status.as_u16() == 401 {
        return FetchResult::err("Codex token expired. Run `codex`.", "token-expired");
    }
    if !status.is_success() {
        return FetchResult::err(format!("Codex HTTP {}", status.as_u16()), "server-error");
    }
    let data: Resp = match resp.json().await {
        Ok(d) => d,
        Err(e) => return FetchResult::err(format!("Codex parse: {e}"), "server-error"),
    };
    let primary = data
        .rate_limit
        .as_ref()
        .and_then(|r| r.primary_window.as_ref());
    let secondary = data
        .rate_limit
        .as_ref()
        .and_then(|r| r.secondary_window.as_ref());
    let session = primary.map(|w| UsageBlock {
        used_pct: w.used_percent,
        resets_at: w.reset_at.and_then(iso_from_unix),
        ..Default::default()
    });
    let weekly = secondary.map(|w| UsageBlock {
        used_pct: w.used_percent,
        resets_at: w.reset_at.and_then(iso_from_unix),
        ..Default::default()
    });
    let _ = data.email;
    FetchResult {
        session,
        weekly,
        plan_type: data.plan_type,
        error: None,
    }
}
