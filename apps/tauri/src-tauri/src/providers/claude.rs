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

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const REFRESH_URL: &str = "https://platform.claude.com/v1/oauth/token";
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const OAUTH_BETA: &str = "oauth-2025-04-20";
// Anthropic's Cloudflare edge 1010-blocks automation User-Agents on the token
// endpoint, so every request must look like the CLI.
const CLAUDE_CODE_UA: &str = "claude-code/2.1.0";
// Treat the token as expired slightly early so we refresh before the server
// would start rejecting it.
const EXPIRY_SKEW_MS: i64 = 60_000;

fn creds_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join(".credentials.json")
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

#[derive(Debug, Deserialize)]
struct CredsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeOauth>,
}

#[derive(Debug, Deserialize, Clone)]
struct ClaudeOauth {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>,
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

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    #[serde(rename = "access_token")]
    access_token: String,
    #[serde(rename = "refresh_token")]
    refresh_token: Option<String>,
    #[serde(rename = "expires_in")]
    expires_in: Option<i64>,
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

/// Whether the stored access token is expired (or close enough that we should
/// refresh now). A missing `expiresAt` is treated as "try it" rather than dead.
fn token_expired(expires_at: Option<i64>, now: i64) -> bool {
    match expires_at {
        Some(e) => e <= now + EXPIRY_SKEW_MS,
        None => false,
    }
}

/// Atomically rewrite `~/.claude/.credentials.json` with rotated tokens,
/// preserving every other field Claude Code stores (org uuid, scopes, etc.).
fn persist_tokens(access: &str, refresh: &str, expires_at: i64) -> std::io::Result<()> {
    let path = creds_path();
    let raw = std::fs::read_to_string(&path)?;
    let mut value: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}));
    if !value
        .get("claudeAiOauth")
        .map(|x| x.is_object())
        .unwrap_or(false)
    {
        value["claudeAiOauth"] = serde_json::json!({});
    }
    if let Some(obj) = value["claudeAiOauth"].as_object_mut() {
        obj.insert("accessToken".into(), serde_json::json!(access));
        obj.insert("refreshToken".into(), serde_json::json!(refresh));
        obj.insert("expiresAt".into(), serde_json::json!(expires_at));
    }
    let data = serde_json::to_string(&value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    if std::fs::rename(&tmp, &path).is_err() {
        // Some Windows configurations refuse rename-over-existing; fall back.
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&tmp, &path)?;
    }
    Ok(())
}

enum RefreshOutcome {
    /// New access token (already persisted to disk).
    Refreshed(String),
    /// Token endpoint is rate limited; back off until the given instant.
    RateLimited(Option<DateTime<Utc>>),
    /// Refresh cannot succeed (bad/expired refresh token, network, etc.).
    Terminal(String),
}

/// Exchange the refresh token for a fresh access token via the OAuth token
/// endpoint and persist the rotated credentials.
async fn try_refresh(client: &reqwest::Client, refresh_token: &str) -> RefreshOutcome {
    let resp = client
        .post(REFRESH_URL)
        .header("Accept", "application/json")
        .header("User-Agent", CLAUDE_CODE_UA)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", OAUTH_CLIENT_ID),
        ])
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return RefreshOutcome::Terminal(format!("refresh network: {e}")),
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
        Err(e) => return RefreshOutcome::Terminal(format!("refresh body: {e}")),
    };
    let parsed: RefreshResponse = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => return RefreshOutcome::Terminal(format!("refresh parse: {e}")),
    };
    let expires_at = now_ms() + parsed.expires_in.unwrap_or(8 * 3600) * 1000;
    let new_refresh = parsed
        .refresh_token
        .clone()
        .unwrap_or_else(|| refresh_token.to_string());
    // Best effort: even if the disk write fails we still use the token this round.
    let _ = persist_tokens(&parsed.access_token, &new_refresh, expires_at);
    RefreshOutcome::Refreshed(parsed.access_token)
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

// The retry gate is set by the caller (which awaits `set_retry_at`) before this
// runs, so this only builds the user-facing error.
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
    let oauth = match serde_json::from_str::<CredsFile>(&creds_raw) {
        Ok(c) => c.claude_ai_oauth,
        Err(_) => return FetchResult::err("Claude credentials malformed", "bad-credentials"),
    };
    let Some(oauth) = oauth else {
        return FetchResult::err("Claude credentials missing accessToken", "bad-credentials");
    };
    let Some(mut token) = oauth.access_token.clone().filter(|t| !t.is_empty()) else {
        return FetchResult::err("Claude credentials missing accessToken", "bad-credentials");
    };
    let refresh_token = oauth.refresh_token.clone().filter(|t| !t.is_empty());

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return FetchResult::err(format!("Claude client: {e}"), "unknown");
        }
    };

    // Pre-flight expiry guard. Sending a known-dead token only trips Anthropic's
    // edge rate limiter (HTTP 429), which masks the underlying 401 and prevents
    // the refresh path from ever firing. Detect expiry locally and refresh first.
    if token_expired(oauth.expires_at, now_ms()) {
        match &refresh_token {
            Some(rt) => match try_refresh(&client, rt).await {
                RefreshOutcome::Refreshed(new_token) => {
                    token = new_token;
                    set_retry_at(None).await;
                }
                RefreshOutcome::RateLimited(retry_at) => {
                    set_retry_at(retry_at).await;
                    return rate_limited_result(retry_at, "token refresh endpoint");
                }
                RefreshOutcome::Terminal(detail) => {
                    return FetchResult::err_with_detail(
                        "Claude token expired. Refresh failed; run `claude`.",
                        "token-expired",
                        Some(detail),
                        None::<String>,
                    );
                }
            },
            None => {
                return FetchResult::err_with_detail(
                    "Claude token expired. Run `claude` to re-authenticate.",
                    "token-expired",
                    Some("No refresh token is stored (e.g. a `claude setup-token` token), so the app cannot refresh it in the background."),
                    None::<String>,
                );
            }
        }
    }

    let mut resp = match request_usage(&client, &token).await {
        Ok(r) => r,
        Err(e) => return FetchResult::err(format!("Claude network: {e}"), "network-error"),
    };

    // If the server still rejects the token, try exactly one in-process refresh
    // and retry the usage call before giving up.
    if resp.status().as_u16() == 401 {
        if let Some(rt) = &refresh_token {
            match try_refresh(&client, rt).await {
                RefreshOutcome::Refreshed(new_token) => {
                    token = new_token;
                    set_retry_at(None).await;
                    resp = match request_usage(&client, &token).await {
                        Ok(r) => r,
                        Err(e) => {
                            return FetchResult::err(
                                format!("Claude network: {e}"),
                                "network-error",
                            )
                        }
                    };
                }
                RefreshOutcome::RateLimited(retry_at) => {
                    set_retry_at(retry_at).await;
                    return rate_limited_result(retry_at, "token refresh endpoint");
                }
                RefreshOutcome::Terminal(detail) => {
                    return FetchResult::err_with_detail(
                        "Claude token expired. Run `claude`.",
                        "token-expired",
                        Some(detail),
                        None::<String>,
                    );
                }
            }
        }
    }

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
        let detail = http_error_detail(status, &body, retry_at);
        if status.as_u16() == 429 {
            set_retry_at(retry_at).await;
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
    fn parses_refresh_response() {
        let r: RefreshResponse = serde_json::from_str(
            r#"{"access_token":"a","refresh_token":"b","expires_in":28800,"token_type":"Bearer"}"#,
        )
        .expect("refresh response should parse");
        assert_eq!(r.access_token, "a");
        assert_eq!(r.refresh_token.as_deref(), Some("b"));
        assert_eq!(r.expires_in, Some(28_800));
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
}
