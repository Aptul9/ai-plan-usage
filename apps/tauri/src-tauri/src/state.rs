use serde::{Deserialize, Serialize};

pub type ProviderId = String;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageBlock {
    #[serde(rename = "usedPct")]
    pub used_pct: Option<f64>,
    #[serde(rename = "resetsAt")]
    pub resets_at: Option<String>,
    #[serde(rename = "usedAbs", skip_serializing_if = "Option::is_none")]
    pub used_abs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entitlement: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overage: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotError {
    pub message: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(rename = "occurredAt", skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
    #[serde(rename = "retryAt", skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSnapshot {
    pub session: UsageBlock,
    pub weekly: UsageBlock,
    #[serde(rename = "planType", skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    pub error: Option<SnapshotError>,
}

impl Default for ProviderSnapshot {
    fn default() -> Self {
        Self {
            session: UsageBlock::default(),
            weekly: UsageBlock::default(),
            plan_type: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub id: ProviderId,
    pub label: String,
    pub enabled: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollIntervalOption {
    pub label: String,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppState {
    #[serde(rename = "fetchedAt")]
    pub fetched_at: String,
    #[serde(rename = "iconStyle")]
    pub icon_style: String,
    pub session: UsageBlock,
    pub weekly: UsageBlock,
    pub error: Option<SnapshotError>,
    pub providers: Vec<ProviderEntry>,
    #[serde(rename = "primaryProvider")]
    pub primary_provider: ProviderId,
    #[serde(rename = "pollIntervalMs")]
    pub poll_interval_ms: u64,
    #[serde(rename = "dataSource")]
    pub data_source: String,
    #[serde(skip_serializing)]
    pub copilot_token: Option<String>,
    #[serde(rename = "devMode")]
    pub dev_mode: bool,
    pub snapshots: std::collections::BTreeMap<ProviderId, ProviderSnapshot>,
}

impl AppState {
    pub fn defaults() -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        let mut snapshots = std::collections::BTreeMap::new();
        for id in ["claude", "codex", "copilot"] {
            snapshots.insert(id.to_string(), ProviderSnapshot::default());
        }
        Self {
            fetched_at: now,
            icon_style: "ring+number".to_string(),
            session: UsageBlock::default(),
            weekly: UsageBlock::default(),
            error: None,
            providers: vec![
                ProviderEntry {
                    id: "claude".into(),
                    label: "Claude".into(),
                    enabled: true,
                    available: true,
                },
                ProviderEntry {
                    id: "codex".into(),
                    label: "Codex".into(),
                    enabled: true,
                    available: true,
                },
                ProviderEntry {
                    id: "copilot".into(),
                    label: "Copilot".into(),
                    enabled: false,
                    available: false,
                },
            ],
            primary_provider: "claude".into(),
            poll_interval_ms: 5 * 60_000,
            data_source: "oauth-api".into(),
            copilot_token: None,
            dev_mode: false,
            snapshots,
        }
    }

    pub fn public_state(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    pub fn mirror_primary(&mut self) {
        if let Some(p) = self.snapshots.get(&self.primary_provider) {
            self.session = p.session.clone();
            self.weekly = p.weekly.clone();
            self.error = p.error.clone();
        }
    }

    pub fn poll_intervals() -> Vec<PollIntervalOption> {
        vec![
            PollIntervalOption {
                label: "30 seconds (dev)".into(),
                ms: 30_000,
            },
            PollIntervalOption {
                label: "1 minute".into(),
                ms: 60_000,
            },
            PollIntervalOption {
                label: "5 minutes".into(),
                ms: 5 * 60_000,
            },
            PollIntervalOption {
                label: "15 minutes".into(),
                ms: 15 * 60_000,
            },
            PollIntervalOption {
                label: "30 minutes".into(),
                ms: 30 * 60_000,
            },
        ]
    }

    pub fn icon_styles() -> Vec<&'static str> {
        vec!["solid", "number", "ring", "ring+number", "bar"]
    }
}
