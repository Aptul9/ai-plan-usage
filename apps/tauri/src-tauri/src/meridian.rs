//! Claude usage from a local Meridian (`GET /v1/usage/quota/all`). The app
//! never reads or refreshes Meridian's own credential files.
use crate::providers::FetchResult;
use crate::state::UsageBlock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_URL: &str = "http://127.0.0.1:3456";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeridianConfig {
    /// Empty turns Meridian off.
    pub url: String,
    #[serde(rename = "apiKey", default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Default for MeridianConfig {
    fn default() -> Self {
        Self { url: DEFAULT_URL.into(), api_key: None }
    }
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub id: String,
    pub session: Option<UsageBlock>,
    pub weekly: Option<UsageBlock>,
    /// Meridian's `error`, kept only when the profile has no windows.
    pub error: Option<String>,
}

fn block(windows: &[Value], kind: &str) -> Option<UsageBlock> {
    let w = windows
        .iter()
        .find(|w| w.get("type").and_then(Value::as_str) == Some(kind))?;
    // utilization is 0..1 and can pass 1; one decimal avoids 22.000000000000004.
    let used_pct = w
        .get("utilization")
        .and_then(Value::as_f64)
        .map(|u| (u * 1000.0).round() / 10.0);
    let resets_at = w
        .get("resetsAt")
        .and_then(Value::as_i64)
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|d| d.to_rfc3339());
    Some(UsageBlock { used_pct, resets_at, ..Default::default() })
}

/// Claude profiles from a quota body; None when the body is not Meridian's.
pub fn parse_quota_all(body: &Value) -> Option<Vec<Profile>> {
    let profiles = body.get("profiles")?.as_array()?;
    Some(
        profiles
            .iter()
            .filter_map(|p| {
                let id = p.get("id")?.as_str()?.to_string();
                let chatgpt = p.get("type").and_then(Value::as_str) == Some("chatgpt")
                    || p.get("provider").and_then(Value::as_str) == Some("chatgpt");
                if chatgpt {
                    return None;
                }
                let windows = p
                    .get("windows")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let error = if windows.is_empty() {
                    p.get("error").and_then(Value::as_str).map(str::to_string)
                } else {
                    None
                };
                Some(Profile {
                    id,
                    session: block(&windows, "five_hour"),
                    weekly: block(&windows, "seven_day"),
                    error,
                })
            })
            .collect(),
    )
}

/// None when Meridian is unreachable or answered something else.
pub async fn fetch(cfg: &MeridianConfig) -> Option<Vec<Profile>> {
    let url = format!("{}/v1/usage/quota/all", cfg.url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .ok()?;
    let mut req = client.get(url).header("Accept", "application/json");
    if let Some(key) = cfg.api_key.as_deref().filter(|k| !k.is_empty()) {
        req = req.header("x-api-key", key);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    parse_quota_all(&resp.json::<Value>().await.ok()?)
}

/// The usage of one Meridian account. `profiles` is None when Meridian was
/// unreachable.
pub fn result_for(profiles: Option<&[Profile]>, profile_id: &str) -> FetchResult {
    let Some(profiles) = profiles else {
        return FetchResult::err("Meridian offline", "offline");
    };
    let Some(p) = profiles.iter().find(|p| p.id == profile_id) else {
        return FetchResult::err("Not in Meridian", "upstream");
    };
    match p.error.as_deref() {
        Some("rate_limited") => FetchResult::err("Meridian rate limited", "rate-limited"),
        Some("no_token") | Some("not_oauth") => {
            FetchResult::err("Meridian profile not signed in", "signed-out")
        }
        Some(other) => FetchResult::err(format!("Meridian: {other}"), "upstream"),
        None => FetchResult {
            session: p.session.clone(),
            weekly: p.weekly.clone(),
            plan_type: None,
            error: None,
            rotated: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body() -> Value {
        json!({"profiles": [
            {"id": "notarify", "type": "claude-max", "windows": [
                {"type": "five_hour", "utilization": 0.22, "resetsAt": 1793131200000i64},
                {"type": "seven_day", "utilization": 1.04, "resetsAt": 1793563200000i64}
            ], "error": null},
            {"id": "everbright", "type": "claude-max", "windows": [], "error": "no_token"},
            {"id": "gpt", "type": "chatgpt", "windows": [{"type": "five_hour", "utilization": 0.5}]}
        ], "activeProfile": "notarify", "asOf": 1793120000000i64})
    }

    #[test]
    fn parses_claude_profiles_and_skips_chatgpt() {
        let p = parse_quota_all(&body()).unwrap();
        assert_eq!(p.len(), 2);
        let s = p[0].session.as_ref().unwrap();
        assert_eq!(s.used_pct, Some(22.0));
        assert!(s.resets_at.as_deref().unwrap().starts_with("2026-"));
        assert_eq!(p[0].weekly.as_ref().unwrap().used_pct, Some(104.0));
        assert_eq!(p[1].error.as_deref(), Some("no_token"));
    }

    #[test]
    fn rejects_non_meridian_body() {
        assert!(parse_quota_all(&json!({"error": "x"})).is_none());
    }

    #[test]
    fn maps_profile_states() {
        let p = parse_quota_all(&body()).unwrap();
        assert!(result_for(Some(&p), "notarify").error.is_none());
        assert_eq!(result_for(Some(&p), "everbright").error.unwrap().kind, "signed-out");
        assert_eq!(result_for(Some(&p), "gone").error.unwrap().kind, "upstream");
        assert_eq!(result_for(None, "notarify").error.unwrap().kind, "offline");
    }
}
