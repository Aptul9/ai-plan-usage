//! Accounts the app polls: a provider plus where its usage comes from.
//! Credentials live here and never reach a webview.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
    Copilot,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Copilot => "copilot",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Meridian,
    Oauth,
    Token,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OauthCreds {
    pub access: String,
    pub refresh: String,
    /// Epoch milliseconds.
    #[serde(rename = "expiresAt")]
    pub expires_at: i64,
    /// ChatGPT account id, Codex only.
    #[serde(rename = "accountId", default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub provider: Provider,
    pub source: Source,
    pub label: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OauthCreds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

pub const MERIDIAN_PREFIX: &str = "meridian:";

impl Account {
    pub fn new(provider: Provider, source: Source, label: &str) -> Self {
        Self {
            id: format!("{}-{}", provider.as_str(), chrono::Utc::now().timestamp_millis()),
            provider,
            source,
            label: label.trim().to_string(),
            enabled: true,
            oauth: None,
            token: None,
        }
    }

    pub fn meridian(profile_id: &str) -> Self {
        Self {
            id: format!("{MERIDIAN_PREFIX}{profile_id}"),
            provider: Provider::Claude,
            source: Source::Meridian,
            label: profile_id.to_string(),
            enabled: true,
            oauth: None,
            token: None,
        }
    }

    pub fn profile_id(&self) -> Option<&str> {
        self.id.strip_prefix(MERIDIAN_PREFIX)
    }

    /// What the webviews may see.
    pub fn public(&self) -> Value {
        json!({
            "id": self.id,
            "provider": self.provider,
            "source": self.source,
            "label": self.label,
            "enabled": self.enabled,
        })
    }
}

/// Appends an enabled account for each Meridian profile not seen before.
/// Returns true when something was added.
pub fn merge_meridian(accounts: &mut Vec<Account>, profile_ids: &[String]) -> bool {
    let mut added = false;
    for pid in profile_ids {
        if !accounts.iter().any(|a| a.profile_id() == Some(pid.as_str())) {
            accounts.push(Account::meridian(pid));
            added = true;
        }
    }
    added
}

/// The primary when it still exists and is enabled, else the first enabled
/// account, else an empty id.
pub fn normalize_primary(accounts: &[Account], primary: &str) -> String {
    if accounts.iter().any(|a| a.id == primary && a.enabled) {
        return primary.to_string();
    }
    accounts
        .iter()
        .find(|a| a.enabled)
        .map(|a| a.id.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_adds_only_new_profiles() {
        let mut list = vec![Account::meridian("notarify")];
        list[0].enabled = false;
        assert!(merge_meridian(&mut list, &["notarify".into(), "everbright".into()]));
        assert_eq!(list.len(), 2);
        assert!(!list[0].enabled, "existing toggle kept");
        assert_eq!(list[1].id, "meridian:everbright");
        assert_eq!(list[1].profile_id(), Some("everbright"));
        assert!(!merge_meridian(&mut list, &["everbright".into()]));
    }

    #[test]
    fn primary_falls_back_to_first_enabled() {
        let mut a = Account::meridian("a");
        a.enabled = false;
        let list = vec![a, Account::meridian("b")];
        assert_eq!(normalize_primary(&list, "meridian:a"), "meridian:b");
        assert_eq!(normalize_primary(&list, "meridian:b"), "meridian:b");
        assert_eq!(normalize_primary(&[], "x"), "");
    }

    #[test]
    fn public_view_has_no_credentials() {
        let mut acc = Account::new(Provider::Codex, Source::Oauth, " Work ");
        acc.oauth = Some(OauthCreds {
            access: "SECRET_A".into(),
            refresh: "SECRET_R".into(),
            expires_at: 1,
            account_id: None,
        });
        acc.token = Some("SECRET_T".into());
        let text = acc.public().to_string();
        assert!(!text.contains("SECRET"));
        assert!(text.contains("\"provider\":\"codex\""));
        assert!(text.contains("\"label\":\"Work\""));
    }

    #[test]
    fn serializes_with_camel_case_keys() {
        let mut acc = Account::new(Provider::Claude, Source::Oauth, "x");
        acc.oauth = Some(OauthCreds { access: "a".into(), refresh: "r".into(), expires_at: 5, account_id: None });
        let text = serde_json::to_string(&acc).unwrap();
        assert!(text.contains("\"expiresAt\":5"));
        assert!(text.contains("\"source\":\"oauth\""));
        assert!(!text.contains("accountId"));
        let back: Account = serde_json::from_str(&text).unwrap();
        assert_eq!(back, acc);
    }
}
