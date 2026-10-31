use crate::accounts::OauthCreds;
use crate::oauth::{TokenResponse, CLAUDE_CLIENT_ID, CLAUDE_CODE_UA, CLAUDE_TOKEN_URL};
use crate::providers::{refresh_failed, save_rotated, FetchResult, SharedState};
use crate::state::UsageBlock;
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

// Anthropic's Retry-After after a 429, per account id.
static CLAUDE_RETRY_AT: Lazy<Mutex<HashMap<String, DateTime<Utc>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
// Treat the token as expired slightly early so we refresh before the server
// would start rejecting it.
const EXPIRY_SKEW_MS: i64 = 60_000;
const SIGN_IN_AGAIN: &str = "Claude sign-in expired. Sign in again in Settings.";

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
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

async fn current_retry_at(id: &str) -> Option<DateTime<Utc>> {
    let mut map = CLAUDE_RETRY_AT.lock().await;
    match map.get(id).copied() {
        Some(at) if at > Utc::now() => Some(at),
        Some(_) => {
            map.remove(id);
            None
        }
        None => None,
    }
}

async fn set_retry_at(id: &str, next_retry_at: Option<DateTime<Utc>>) {
    let mut map = CLAUDE_RETRY_AT.lock().await;
    match next_retry_at {
        Some(at) => {
            map.insert(id.to_string(), at);
        }
        None => {
            map.remove(id);
        }
    }
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

/// Whether the stored access token is expired (or close enough that we should
/// refresh now). A missing `expiresAt` is treated as "try it" rather than dead.
fn token_expired(expires_at: Option<i64>, now: i64) -> bool {
    match expires_at {
        Some(e) => e <= now + EXPIRY_SKEW_MS,
        None => false,
    }
}

enum RefreshOutcome {
    /// Rotated credentials; the caller saves them.
    Refreshed(OauthCreds),
    /// Token endpoint is rate limited; back off until the given instant.
    RateLimited(Option<DateTime<Utc>>),
    /// Refresh cannot succeed (bad/expired refresh token, network, etc.).
    Terminal(String),
}

async fn try_refresh(client: &reqwest::Client, creds: &OauthCreds) -> RefreshOutcome {
    if creds.refresh.is_empty() {
        return RefreshOutcome::Terminal("no refresh token stored".into());
    }
    let resp = client
        .post(CLAUDE_TOKEN_URL)
        .header("Accept", "application/json")
        .header("User-Agent", CLAUDE_CODE_UA)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", creds.refresh.as_str()),
            ("client_id", CLAUDE_CLIENT_ID),
        ])
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return RefreshOutcome::Terminal(format!("network: {e}")),
    };
    let status = resp.status();
    if status.as_u16() == 429 {
        let ra = retry_at_from_headers(resp.headers(), Utc::now())
            .or_else(|| Some(Utc::now() + chrono::Duration::minutes(5)));
        return RefreshOutcome::RateLimited(ra);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return RefreshOutcome::Terminal(format!(
            "refresh HTTP {}: {}",
            status.as_u16(),
            truncate(body.trim(), 200)
        ));
    }
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => return RefreshOutcome::Terminal(format!("network: {e}")),
    };
    match serde_json::from_str::<TokenResponse>(&body) {
        Ok(parsed) => RefreshOutcome::Refreshed(parsed.into_creds(&creds.refresh, None)),
        Err(e) => RefreshOutcome::Terminal(format!("refresh parse: {e}")),
    }
}

async fn request_usage(
    client: &reqwest::Client,
    token: &str,
) -> reqwest::Result<reqwest::Response> {
    client
        .get(USAGE_URL)
        .bearer_auth(token)
        .header("anthropic-beta", OAUTH_BETA)
        .header("User-Agent", CLAUDE_CODE_UA)
        .header("Accept", "application/json")
        .send()
        .await
}

// The retry gate is set by the caller before this runs, so this only builds
// the user-facing error.
fn rate_limited_result(retry_at: Option<DateTime<Utc>>, source: &str) -> FetchResult {
    let wait = retry_at
        .map(|a| format_duration(a.signed_duration_since(Utc::now())))
        .unwrap_or_else(|| "a few minutes".into());
    FetchResult::err_with_detail(
        format!("Claude {source} is rate limited. Retrying in {wait}."),
        "rate-limited",
        Some(format!(
            "Anthropic returned HTTP 429 on the Claude {source}; requests are paused until the backoff expires."
        )),
        retry_at.map(|a| a.to_rfc3339()),
    )
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
    if let Some(retry_at) = current_retry_at(id).await {
        let wait = format_duration(retry_at.signed_duration_since(Utc::now()));
        return FetchResult::err_with_detail(
            format!("Claude usage API is rate limited. Retrying in {wait}."),
            "rate-limited",
            Some("Anthropic previously returned HTTP 429; usage requests are paused until the Retry-After window expires."),
            Some(retry_at.to_rfc3339()),
        );
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => return FetchResult::err(format!("Claude client: {e}"), "upstream"),
    };

    // Pre-flight expiry guard. Sending a known-dead token only trips Anthropic's
    // edge rate limiter (HTTP 429), which masks the underlying 401.
    let mut token = creds.access.clone();
    if token_expired(Some(creds.expires_at), now_ms()) {
        match try_refresh(&client, creds).await {
            RefreshOutcome::Refreshed(new) => {
                save_rotated(state, id, &new).await;
                token = new.access.clone();
                *rotated = Some(new);
                set_retry_at(id, None).await;
            }
            RefreshOutcome::RateLimited(retry_at) => {
                set_retry_at(id, retry_at).await;
                return rate_limited_result(retry_at, "token refresh endpoint");
            }
            RefreshOutcome::Terminal(detail) => return refresh_failed("Claude", SIGN_IN_AGAIN, detail),
        }
    }

    let mut resp = match request_usage(&client, &token).await {
        Ok(r) => r,
        Err(e) => return FetchResult::err(format!("Claude network: {e}"), "offline"),
    };

    // One in-process refresh on 401, unless this round already refreshed:
    // the old refresh token is spent by then.
    if resp.status().as_u16() == 401 && rotated.is_none() {
        match try_refresh(&client, creds).await {
            RefreshOutcome::Refreshed(new) => {
                save_rotated(state, id, &new).await;
                token = new.access.clone();
                *rotated = Some(new);
                set_retry_at(id, None).await;
                resp = match request_usage(&client, &token).await {
                    Ok(r) => r,
                    Err(e) => {
                        return FetchResult::err(format!("Claude network: {e}"), "offline")
                    }
                };
            }
            RefreshOutcome::RateLimited(retry_at) => {
                set_retry_at(id, retry_at).await;
                return rate_limited_result(retry_at, "token refresh endpoint");
            }
            RefreshOutcome::Terminal(detail) => return refresh_failed("Claude", SIGN_IN_AGAIN, detail),
        }
    }

    let status = resp.status();
    if status.as_u16() == 401 {
        set_retry_at(id, None).await;
        return FetchResult::err(SIGN_IN_AGAIN, "signed-out");
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
        let detail = http_error_detail(status, &body, retry_at);
        if status.as_u16() == 429 {
            set_retry_at(id, retry_at).await;
            let message = if let Some(at) = retry_at {
                let wait = format_duration(at.signed_duration_since(now));
                format!("Claude usage API is rate limited. Try again in {wait}.")
            } else {
                "Claude usage API is rate limited. Try again later.".to_string()
            };
            return FetchResult::err_with_detail(message, "rate-limited", Some(detail), retry_at_iso);
        }
        return FetchResult::err_with_detail(
            format!("Claude HTTP {}", status.as_u16()),
            "upstream",
            Some(detail),
            None::<String>,
        );
    }
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => return FetchResult::err(format!("Claude body: {e}"), "upstream"),
    };
    set_retry_at(id, None).await;
    let data: Resp = match serde_json::from_str(&body) {
        Ok(d) => d,
        Err(e) => {
            return FetchResult::err_with_detail(
                format!("Claude parse: {e}"),
                "upstream",
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
        rotated: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_when_at_or_past_skew() {
        assert!(token_expired(Some(1_000), 1_000));
        assert!(token_expired(Some(1_000 + EXPIRY_SKEW_MS), 1_000));
    }

    #[test]
    fn fresh_when_far_in_future() {
        assert!(!token_expired(Some(10_000_000_000), 0));
    }

    #[test]
    fn missing_expiry_is_not_expired() {
        assert!(!token_expired(None, 12_345));
    }

    #[test]
    fn parses_usage_response_snake_case() {
        let data: Resp = serde_json::from_str(
            r#"{"five_hour":{"utilization":16.0,"resets_at":"2026-06-21T14:50:00Z"},
                "seven_day":{"utilization":17.0,"resets_at":"2026-06-26T12:00:00Z"}}"#,
        )
        .expect("usage response should parse");
        assert_eq!(data.five_hour.unwrap().utilization, Some(16.0));
        assert_eq!(data.seven_day.unwrap().utilization, Some(17.0));
    }

    #[tokio::test]
    async fn retry_gate_is_per_account() {
        set_retry_at("a", Some(Utc::now() + chrono::Duration::minutes(5))).await;
        assert!(current_retry_at("a").await.is_some());
        assert!(current_retry_at("b").await.is_none());
        set_retry_at("a", None).await;
        assert!(current_retry_at("a").await.is_none());
    }
}
