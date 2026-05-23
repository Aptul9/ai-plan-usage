use crate::providers::FetchResult;
use crate::state::UsageBlock;
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::Mutex;

static CLAUDE_RETRY_AT: Lazy<Mutex<Option<DateTime<Utc>>>> = Lazy::new(|| Mutex::new(None));

fn creds_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join(".credentials.json")
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
}

#[derive(Debug, Deserialize)]
struct Resp {
    five_hour: Option<Window>,
    seven_day: Option<Window>,
}

#[derive(Debug, Deserialize)]
struct Window {
    utilization: Option<f64>,
    resets_at: Option<String>,
}

async fn current_retry_at() -> Option<DateTime<Utc>> {
    let mut retry_at = CLAUDE_RETRY_AT.lock().await;
    if let Some(at) = retry_at.clone() {
        if at > Utc::now() {
            return Some(at);
        }
        *retry_at = None;
    }
    None
}

async fn set_retry_at(next_retry_at: Option<DateTime<Utc>>) {
    *CLAUDE_RETRY_AT.lock().await = next_retry_at;
}

fn format_duration(duration: chrono::Duration) -> String {
    let total = duration.num_seconds().max(0);
    let hours = total / 3_600;
    let minutes = (total % 3_600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn retry_at_from_headers(headers: &HeaderMap, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<i64>() {
        return Some(now + chrono::Duration::seconds(seconds.max(0)));
    }
    DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn truncate(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let mut out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

fn anthropic_error_summary(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = value.get("error")?;
    let error_type = error.get("type").and_then(|v| v.as_str());
    let message = error.get("message").and_then(|v| v.as_str());
    match (error_type, message) {
        (Some(t), Some(m)) => Some(format!("{t}: {m}")),
        (Some(t), None) => Some(t.to_string()),
        (None, Some(m)) => Some(m.to_string()),
        (None, None) => None,
    }
}

fn http_error_detail(
    status: reqwest::StatusCode,
    body: &str,
    retry_at: Option<DateTime<Utc>>,
) -> String {
    let now = Utc::now();
    let mut parts = vec![format!("HTTP {}", status.as_u16())];
    if let Some(summary) = anthropic_error_summary(body) {
        parts.push(summary);
    } else if !body.trim().is_empty() {
        parts.push(format!("Body: {}", truncate(body.trim(), 600)));
    }
    if let Some(at) = retry_at {
        parts.push(format!(
            "Retry-After: {} ({})",
            format_duration(at.signed_duration_since(now)),
            at.to_rfc3339()
        ));
    }
    parts.join("\n")
}

pub async fn fetch() -> FetchResult {
    if let Some(retry_at) = current_retry_at().await {
        let wait = format_duration(retry_at.signed_duration_since(Utc::now()));
        return FetchResult::err_with_detail(
            format!("Claude usage API is rate limited. Retrying in {wait}."),
            "rate-limited",
            Some("Anthropic previously returned HTTP 429; usage requests are paused until the Retry-After window expires."),
            Some(retry_at.to_rfc3339()),
        );
    }

    let creds_raw = match tokio::fs::read_to_string(creds_path()).await {
        Ok(r) => r,
        Err(_) => {
            return FetchResult::err("Claude not logged in. Run `claude`.", "not-authenticated");
        }
    };
    let creds: CredsFile = match serde_json::from_str(&creds_raw) {
        Ok(c) => c,
        Err(_) => {
            return FetchResult::err("Claude credentials malformed", "bad-credentials");
        }
    };
    let token = creds
        .claude_ai_oauth
        .and_then(|c| c.access_token)
        .filter(|t| !t.is_empty());
    let Some(token) = token else {
        return FetchResult::err("Claude credentials missing accessToken", "bad-credentials");
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return FetchResult::err(format!("Claude client: {e}"), "unknown");
        }
    };
    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-code/2.1.0")
        .header("Accept", "application/json")
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return FetchResult::err(format!("Claude network: {e}"), "network-error"),
    };
    let status = resp.status();
    if status.as_u16() == 401 {
        set_retry_at(None).await;
        return FetchResult::err("Claude token expired. Run `claude`.", "token-expired");
    }
    if !status.is_success() {
        let now = Utc::now();
        let retry_at = if status.as_u16() == 429 {
            retry_at_from_headers(resp.headers(), now)
                .or_else(|| Some(now + chrono::Duration::minutes(5)))
        } else {
            None
        };
        let retry_at_iso = retry_at.as_ref().map(DateTime::to_rfc3339);
        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => format!("Unable to read response body: {e}"),
        };
        let detail = http_error_detail(status, &body, retry_at.clone());
        if status.as_u16() == 429 {
            set_retry_at(retry_at.clone()).await;
            let message = if let Some(at) = retry_at {
                let wait = format_duration(at.signed_duration_since(now));
                format!("Claude usage API is rate limited. Try again in {wait}.")
            } else {
                "Claude usage API is rate limited. Try again later.".to_string()
            };
            return FetchResult::err_with_detail(
                message,
                "rate-limited",
                Some(detail),
                retry_at_iso,
            );
        }
        return FetchResult::err_with_detail(
            format!("Claude HTTP {}", status.as_u16()),
            "server-error",
            Some(detail),
            None::<String>,
        );
    }
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => return FetchResult::err(format!("Claude body: {e}"), "server-error"),
    };
    set_retry_at(None).await;
    let data: Resp = match serde_json::from_str(&body) {
        Ok(d) => d,
        Err(e) => {
            return FetchResult::err_with_detail(
                format!("Claude parse: {e}"),
                "server-error",
                Some(format!("Body: {}", truncate(body.trim(), 600))),
                None::<String>,
            )
        }
    };
    let session = data.five_hour.map(|w| UsageBlock {
        used_pct: w.utilization,
        resets_at: w.resets_at,
        ..Default::default()
    });
    let weekly = data.seven_day.map(|w| UsageBlock {
        used_pct: w.utilization,
        resets_at: w.resets_at,
        ..Default::default()
    });
    FetchResult {
        session,
        weekly,
        plan_type: None,
        error: None,
    }
}
