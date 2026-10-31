use crate::accounts::{Account, Provider};
use crate::meridian::MeridianConfig;
use crate::oauth::{DeviceCode, Pkce};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderSnapshot {
    pub session: UsageBlock,
    pub weekly: UsageBlock,
    #[serde(rename = "planType", skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    pub error: Option<SnapshotError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollIntervalOption {
    pub label: String,
    pub ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MeridianStatus {
    pub reachable: bool,
    pub profiles: usize,
    #[serde(rename = "checkedAt")]
    pub checked_at: Option<String>,
}

/// Sign-ins started in settings and not finished yet.
#[derive(Debug, Clone, Default)]
pub struct PendingLogins {
    pub claude_pkce: Option<Pkce>,
    pub codex: Option<DeviceCode>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub fetched_at: String,
    pub icon_style: String,
    /// Mirror of the primary account, read by the tray icon and tooltip.
    pub session: UsageBlock,
    pub weekly: UsageBlock,
    pub error: Option<SnapshotError>,
    pub accounts: Vec<Account>,
    pub primary: String,
    pub meridian: MeridianConfig,
    pub meridian_status: MeridianStatus,
    pub poll_interval_ms: u64,
    pub dev_mode: bool,
    pub snapshots: BTreeMap<String, ProviderSnapshot>,
    pub pending: PendingLogins,
}

impl AppState {
    pub fn defaults() -> Self {
        Self {
            fetched_at: chrono::Utc::now().to_rfc3339(),
            icon_style: "ring+number".to_string(),
            session: UsageBlock::default(),
            weekly: UsageBlock::default(),
            error: None,
            accounts: Vec::new(),
            primary: String::new(),
            meridian: MeridianConfig::default(),
            meridian_status: MeridianStatus::default(),
            poll_interval_ms: 5 * 60_000,
            dev_mode: false,
            snapshots: BTreeMap::new(),
            pending: PendingLogins::default(),
        }
    }

    /// Everything the webviews get. Credentials and the Meridian key stay here.
    pub fn public_state(&self) -> serde_json::Value {
        json!({
            "fetchedAt": self.fetched_at,
            "iconStyle": self.icon_style,
            "session": self.session,
            "weekly": self.weekly,
            "error": self.error,
            "accounts": self.accounts.iter().map(Account::public).collect::<Vec<_>>(),
            "primary": self.primary,
            "pollIntervalMs": self.poll_interval_ms,
            "devMode": self.dev_mode,
            "snapshots": self.snapshots,
            "meridian": {
                "url": self.meridian.url,
                "hasApiKey": self.meridian.api_key.as_deref().is_some_and(|k| !k.is_empty()),
            },
            "meridianStatus": self.meridian_status,
        })
    }

    pub fn mirror_primary(&mut self) {
        match self.snapshots.get(&self.primary) {
            Some(p) => {
                self.session = p.session.clone();
                self.weekly = p.weekly.clone();
                self.error = p.error.clone();
            }
            None => {
                self.session = UsageBlock::default();
                self.weekly = UsageBlock::default();
                self.error = None;
            }
        }
    }

    pub fn primary_provider(&self) -> Option<Provider> {
        self.accounts
            .iter()
            .find(|a| a.id == self.primary)
            .map(|a| a.provider)
    }

    /// One snapshot per account, none for removed ones.
    pub fn ensure_snapshots(&mut self) {
        let ids: BTreeSet<String> = self.accounts.iter().map(|a| a.id.clone()).collect();
        self.snapshots.retain(|id, _| ids.contains(id));
        for id in ids {
            self.snapshots.entry(id).or_default();
        }
    }

    pub fn poll_intervals() -> Vec<PollIntervalOption> {
        vec![
            PollIntervalOption { label: "30 seconds (dev)".into(), ms: 30_000 },
            PollIntervalOption { label: "1 minute".into(), ms: 60_000 },
            PollIntervalOption { label: "5 minutes".into(), ms: 5 * 60_000 },
            PollIntervalOption { label: "15 minutes".into(), ms: 15 * 60_000 },
            PollIntervalOption { label: "30 minutes".into(), ms: 30 * 60_000 },
        ]
    }

    pub fn icon_styles() -> Vec<&'static str> {
        vec!["solid", "number", "ring", "ring+number", "bar"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{OauthCreds, Source};

    #[test]
    fn public_state_hides_secrets() {
        let mut s = AppState::defaults();
        let mut a = Account::new(Provider::Claude, Source::Oauth, "Notarify");
        a.oauth = Some(OauthCreds {
            access: "SECRET1".into(),
            refresh: "SECRET2".into(),
            expires_at: 0,
            account_id: None,
        });
        s.accounts.push(a);
        s.meridian.api_key = Some("SECRET3".into());
        let text = s.public_state().to_string();
        assert!(!text.contains("SECRET"));
        assert!(text.contains("\"hasApiKey\":true"));
    }

    #[test]
    fn snapshots_follow_accounts() {
        let mut s = AppState::defaults();
        s.snapshots.insert("gone".into(), ProviderSnapshot::default());
        s.accounts.push(Account::meridian("a"));
        s.ensure_snapshots();
        assert_eq!(s.snapshots.keys().collect::<Vec<_>>(), vec!["meridian:a"]);
    }
}
