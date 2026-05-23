use crate::providers::FetchResult;
use crate::state::UsageBlock;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Deserialize)]
struct CopilotResp {
    quota_snapshots: Option<Value>,
    #[serde(rename = "quotaSnapshots")]
    quota_snapshots_camel: Option<Value>,
    quota_reset_date_utc: Option<String>,
    quota_reset_date: Option<String>,
    copilot_plan: Option<String>,
    #[serde(rename = "copilotPlan")]
    copilot_plan_camel: Option<String>,
}

fn read_pct(q: &Value) -> Option<f64> {
    if !q.is_object() {
        return None;
    }
    if let Some(pr) = q
        .get("percent_remaining")
        .or_else(|| q.get("percentRemaining"))
        .and_then(|v| v.as_f64())
    {
        return Some((100.0 - pr).clamp(0.0, 100.0));
    }
    q.get("used_percent")
        .or_else(|| q.get("usedPercent"))
        .and_then(|v| v.as_f64())
}

pub async fn fetch(token: Option<String>) -> FetchResult {
    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => {
            return FetchResult::err(
                "No GitHub token. Open Settings → Copilot.",
                "not-authenticated",
            )
        }
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => return FetchResult::err(format!("Copilot client: {e}"), "unknown"),
    };
    let resp = match client
        .get("https://api.github.com/copilot_internal/user")
        .header("Authorization", format!("token {token}"))
        .header("Editor-Version", "vscode/1.96.2")
        .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
        .header("User-Agent", "GitHubCopilotChat/0.26.7")
        .header("X-Github-Api-Version", "2025-04-01")
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return FetchResult::err(format!("Copilot network: {e}"), "network-error"),
    };
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return FetchResult::err(
            "GitHub token rejected. Needs Copilot access.",
            "token-expired",
        );
    }
    if !status.is_success() {
        return FetchResult::err(format!("Copilot HTTP {}", status.as_u16()), "server-error");
    }
    let data: CopilotResp = match resp.json().await {
        Ok(d) => d,
        Err(e) => return FetchResult::err(format!("Copilot parse: {e}"), "server-error"),
    };
    let snaps = data
        .quota_snapshots
        .or(data.quota_snapshots_camel)
        .unwrap_or(Value::Null);
    let premium = snaps
        .get("premium_interactions")
        .or_else(|| snaps.get("premiumInteractions"))
        .cloned()
        .unwrap_or(Value::Null);

    let pct = read_pct(&premium);
    let monthly_reset = data
        .quota_reset_date_utc
        .or_else(|| data.quota_reset_date.map(|d| format!("{d}T00:00:00Z")));
    let entitlement = premium.get("entitlement").and_then(|v| v.as_i64());
    let overage = premium
        .get("overage_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let used_abs = entitlement.map(|e| e + overage);

    FetchResult {
        session: Some(UsageBlock::default()),
        weekly: Some(UsageBlock {
            used_pct: pct,
            resets_at: monthly_reset,
            used_abs,
            entitlement,
            overage: Some(overage),
        }),
        plan_type: data.copilot_plan.or(data.copilot_plan_camel),
        error: None,
    }
}
