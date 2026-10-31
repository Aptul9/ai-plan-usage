pub mod claude;
pub mod codex;
pub mod copilot;

use crate::accounts::{Account, OauthCreds, Provider, Source};
use crate::meridian::{self, MeridianConfig, Profile};
use crate::state::{ProviderSnapshot, SnapshotError, UsageBlock};
use futures::future::join_all;

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

/// A failed token refresh: offline when no answer came back, otherwise the
/// sign-in is gone.
pub fn refresh_failed(message: &str, detail: String) -> FetchResult {
    let kind = if detail.starts_with("network:") { "offline" } else { "signed-out" };
    FetchResult::err_with_detail(message, kind, Some(detail), None::<String>)
}

pub struct FetchOutput {
    /// One entry per enabled account that is not a Meridian one.
    pub results: Vec<(String, FetchResult)>,
    /// Meridian's Claude profiles; None when it is off or unreachable.
    pub meridian: Option<Vec<Profile>>,
}

pub async fn fetch_all(accounts: Vec<Account>, meridian_cfg: MeridianConfig) -> FetchOutput {
    let own = accounts
        .into_iter()
        .filter(|a| a.enabled && a.source != Source::Meridian)
        .map(|a| async move {
            let res = match (a.provider, a.oauth.as_ref(), a.token.as_deref()) {
                (Provider::Claude, Some(c), _) => claude::fetch(&a.id, c).await,
                (Provider::Codex, Some(c), _) => codex::fetch(c).await,
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
