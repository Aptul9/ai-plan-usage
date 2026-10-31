pub mod claude;
pub mod codex;
pub mod copilot;

use crate::accounts::{Account, OauthCreds, Provider, Source};
use crate::meridian::{self, MeridianConfig, Profile};
use crate::settings_store;
use crate::state::{AppState, ProviderSnapshot, SnapshotError, UsageBlock};
use futures::future::join_all;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type SharedState = Arc<Mutex<AppState>>;

/// Saves rotated tokens before the new access token is used: the old refresh
/// token is spent, so a quit later in the round must not lose the new pair.
pub async fn save_rotated(state: &SharedState, id: &str, creds: &OauthCreds) {
    let mut s = state.lock().await;
    if let Some(acc) = s.accounts.iter_mut().find(|a| a.id == id) {
        acc.oauth = Some(creds.clone());
        settings_store::persist(&s);
    }
}

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub session: Option<UsageBlock>,
    pub weekly: Option<UsageBlock>,
    pub plan_type: Option<String>,
    pub error: Option<SnapshotError>,
    /// Tokens that replaced the account's own after a refresh. Saved even when
    /// the usage call afterwards failed: refresh tokens are single use.
    pub rotated: Option<OauthCreds>,
}

impl FetchResult {
    pub fn err(message: impl Into<String>, kind: impl Into<String>) -> Self {
        Self::err_with_detail(message, kind, None::<String>, None::<String>)
    }

    pub fn err_with_detail(
        message: impl Into<String>,
        kind: impl Into<String>,
        detail: Option<impl Into<String>>,
        retry_at: Option<impl Into<String>>,
    ) -> Self {
        Self {
            session: None,
            weekly: None,
            plan_type: None,
            rotated: None,
            error: Some(SnapshotError {
                message: message.into(),
                kind: kind.into(),
                detail: detail.map(Into::into),
                occurred_at: Some(chrono::Utc::now().to_rfc3339()),
                retry_at: retry_at.map(Into::into),
            }),
        }
    }

    pub fn with_rotated(mut self, rotated: Option<OauthCreds>) -> Self {
        self.rotated = rotated;
        self
    }

    pub fn apply_to(&self, snap: &mut ProviderSnapshot) {
        if let Some(e) = &self.error {
            snap.error = Some(e.clone());
            return;
        }
        if let Some(s) = &self.session {
            snap.session = s.clone();
        }
        if let Some(w) = &self.weekly {
            snap.weekly = w.clone();
        }
        if let Some(p) = &self.plan_type {
            snap.plan_type = Some(p.clone());
        }
        snap.error = None;
    }
}

/// A failed token refresh: offline when no answer came back, rate-limited on
/// 429, upstream on 5xx, otherwise the sign-in is gone. `detail` is
/// `network: ...`, `HTTP <code>: ...` (oauth.rs) or `refresh HTTP <code>: ...`
/// (claude.rs). Only `signed-out` uses the caller's sign-in-again text.
pub fn refresh_failed(provider: &str, sign_in_again: &str, detail: String) -> FetchResult {
    let status = detail
        .trim_start_matches("refresh ")
        .strip_prefix("HTTP ")
        .and_then(|rest| rest.get(..3))
        .and_then(|code| code.parse::<u16>().ok());
    let (kind, message) = match status {
        _ if detail.starts_with("network:") => {
            ("offline", format!("{provider} unreachable. Retrying next poll."))
        }
        Some(429) => (
            "rate-limited",
            format!("{provider} token refresh is rate limited. Retrying later."),
        ),
        Some(500..=599) => (
            "upstream",
            format!("{provider} sign-in service error. Retrying next poll."),
        ),
        _ => ("signed-out", sign_in_again.to_string()),
    };
    FetchResult::err_with_detail(message, kind, Some(detail), None::<String>)
}

pub struct FetchOutput {
    /// One entry per enabled account that is not a Meridian one.
    pub results: Vec<(String, FetchResult)>,
    /// Meridian's Claude profiles; None when it is off or unreachable.
    pub meridian: Option<Vec<Profile>>,
}

pub async fn fetch_all(
    accounts: Vec<Account>,
    meridian_cfg: MeridianConfig,
    state: &SharedState,
) -> FetchOutput {
    let own = accounts
        .into_iter()
        .filter(|a| a.enabled && a.source != Source::Meridian)
        .map(|a| async move {
            let res = match (a.provider, a.oauth.as_ref(), a.token.as_deref()) {
                (Provider::Claude, Some(c), _) => claude::fetch(&a.id, c, state).await,
                (Provider::Codex, Some(c), _) => codex::fetch(&a.id, c, state).await,
                (Provider::Copilot, _, Some(t)) => copilot::fetch(t).await,
                _ => FetchResult::err("Not signed in. Open Settings.", "signed-out"),
            };
            (a.id, res)
        });
    let meridian_task = async {
        if meridian_cfg.url.trim().is_empty() {
            None
        } else {
            meridian::fetch(&meridian_cfg).await
        }
    };
    let (results, meridian) = futures::join!(join_all(own), meridian_task);
    FetchOutput { results, meridian }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_failure_kind() {
        let cases = [
            ("network: timed out", "offline", "Codex unreachable. Retrying next poll."),
            ("HTTP 400: invalid_grant", "signed-out", "Sign in again."),
            ("HTTP 429: slow down", "rate-limited", "Codex token refresh is rate limited. Retrying later."),
            ("HTTP 503: unavailable", "upstream", "Codex sign-in service error. Retrying next poll."),
            ("refresh HTTP 502: bad gateway", "upstream", "Codex sign-in service error. Retrying next poll."),
            ("refresh HTTP 401: x", "signed-out", "Sign in again."),
        ];
        for (detail, kind, message) in cases {
            let e = refresh_failed("Codex", "Sign in again.", detail.into())
                .error
                .unwrap();
            assert_eq!((e.kind.as_str(), e.message.as_str()), (kind, message), "{detail}");
            assert_eq!(e.detail.as_deref(), Some(detail));
        }
    }
}
