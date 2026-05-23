pub mod claude;
pub mod codex;
pub mod copilot;

use crate::state::{ProviderSnapshot, SnapshotError, UsageBlock};
use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub session: Option<UsageBlock>,
    pub weekly: Option<UsageBlock>,
    pub plan_type: Option<String>,
    pub error: Option<SnapshotError>,
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
            error: Some(SnapshotError {
                message: message.into(),
                kind: kind.into(),
                detail: detail.map(Into::into),
                occurred_at: Some(chrono::Utc::now().to_rfc3339()),
                retry_at: retry_at.map(Into::into),
            }),
        }
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

pub async fn fetch_all(
    copilot_token: Option<String>,
    enabled_provider_ids: Vec<String>,
) -> Vec<(String, FetchResult)> {
    let enabled = |id: &str| {
        enabled_provider_ids
            .iter()
            .any(|enabled_id| enabled_id == id)
    };
    let mut tasks: FuturesUnordered<BoxFuture<'static, (String, FetchResult)>> =
        FuturesUnordered::new();

    if enabled("claude") {
        tasks.push(Box::pin(async {
            ("claude".to_string(), claude::fetch().await)
        }));
    }
    if enabled("codex") {
        tasks.push(Box::pin(async {
            ("codex".to_string(), codex::fetch().await)
        }));
    }
    if enabled("copilot") {
        tasks.push(Box::pin(async move {
            ("copilot".to_string(), copilot::fetch(copilot_token).await)
        }));
    }

    let mut results = Vec::new();
    while let Some(result) = tasks.next().await {
        results.push(result);
    }
    results
}
