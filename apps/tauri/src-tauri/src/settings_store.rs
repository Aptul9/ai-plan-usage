use crate::accounts::{self, Account, Provider, Source};
use crate::meridian::MeridianConfig;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SETTINGS_VERSION: u32 = 2;

/// Version 1: fixed providers, one Copilot token. Read only, for migration.
#[derive(Debug, Default, Deserialize)]
struct PersistV1 {
    #[serde(rename = "copilotToken")]
    copilot_token: Option<String>,
    #[serde(rename = "devMode")]
    dev_mode: Option<bool>,
    #[serde(rename = "primaryProvider")]
    primary_provider: Option<String>,
    #[serde(rename = "iconStyle")]
    icon_style: Option<String>,
    #[serde(rename = "pollIntervalMs")]
    poll_interval_ms: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
struct Persist {
    #[serde(rename = "settingsVersion")]
    settings_version: u32,
    #[serde(default)]
    accounts: Vec<Account>,
    #[serde(default)]
    primary: String,
    #[serde(default)]
    meridian: MeridianConfig,
    #[serde(rename = "devMode", default)]
    dev_mode: bool,
    #[serde(rename = "iconStyle", default)]
    icon_style: Option<String>,
    #[serde(rename = "pollIntervalMs", default)]
    poll_interval_ms: Option<u64>,
}

fn appdata() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")))
}

fn shared_dir() -> PathBuf {
    appdata().join("ai-plan-usage")
}

fn settings_path() -> PathBuf {
    shared_dir().join("settings.json")
}

fn legacy_settings_path() -> PathBuf {
    appdata().join("claude-usage-tray").join("settings.json")
}

fn migrate_v1(v1: PersistV1) -> Persist {
    let mut list = Vec::new();
    if let Some(token) = v1.copilot_token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        let mut acc = Account::new(Provider::Copilot, Source::Token, "Copilot");
        acc.token = Some(token);
        list.push(acc);
    }
    let primary = list
        .iter()
        .find(|a| Some(a.provider.as_str()) == v1.primary_provider.as_deref())
        .or(list.first())
        .map(|a| a.id.clone())
        .unwrap_or_default();
    Persist {
        settings_version: SETTINGS_VERSION,
        accounts: list,
        primary,
        meridian: MeridianConfig::default(),
        dev_mode: v1.dev_mode.unwrap_or(false),
        icon_style: v1.icon_style,
        poll_interval_ms: v1.poll_interval_ms,
    }
}

/// Either layout; the bool says the file must be rewritten. A version newer
/// than this build is left alone.
fn parse(raw: &str) -> Option<(Persist, bool)> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    match value.get("settingsVersion").and_then(|v| v.as_u64()) {
        Some(v) if v == SETTINGS_VERSION as u64 => serde_json::from_value(value).ok().map(|p| (p, false)),
        None | Some(1) => serde_json::from_value::<PersistV1>(value)
            .ok()
            .map(|v1| (migrate_v1(v1), true)),
        Some(_) => None,
    }
}

fn apply(state: &mut AppState, p: Persist) {
    state.accounts = p.accounts;
    state.meridian = p.meridian;
    state.dev_mode = p.dev_mode;
    if let Some(s) = p.icon_style.filter(|s| AppState::icon_styles().contains(&s.as_str())) {
        state.icon_style = s;
    }
    if let Some(ms) = p
        .poll_interval_ms
        .filter(|ms| AppState::poll_intervals().iter().any(|x| x.ms == *ms))
    {
        state.poll_interval_ms = ms;
    }
    state.primary = accounts::normalize_primary(&state.accounts, &p.primary);
}

pub fn load_or_default() -> AppState {
    let mut state = AppState::defaults();
    let (raw, legacy) = match std::fs::read_to_string(settings_path()) {
        Ok(raw) => (raw, false),
        Err(_) => match std::fs::read_to_string(legacy_settings_path()) {
            Ok(raw) => (raw, true),
            Err(_) => return state,
        },
    };
    let Some((persisted, rewrite)) = parse(&raw) else {
        return state;
    };
    apply(&mut state, persisted);
    state.ensure_snapshots();
    if rewrite || legacy {
        persist(&state);
    }
    state
}

pub fn persist(state: &AppState) {
    let payload = Persist {
        settings_version: SETTINGS_VERSION,
        accounts: state.accounts.clone(),
        primary: state.primary.clone(),
        meridian: state.meridian.clone(),
        dev_mode: state.dev_mode,
        icon_style: Some(state.icon_style.clone()),
        poll_interval_ms: Some(state.poll_interval_ms),
    };
    if std::fs::create_dir_all(shared_dir()).is_err() {
        return;
    }
    let path = settings_path();
    let tmp = path.with_extension("json.tmp");
    let Ok(serialized) = serde_json::to_string_pretty(&payload) else {
        return;
    };
    if std::fs::write(&tmp, serialized).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp, &path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_v1_copilot_token_and_keeps_prefs() {
        let raw = r#"{"settingsVersion":1,"copilotToken":" ghp_x ","devMode":true,"primaryProvider":"copilot","enabledProviderIds":["claude","copilot"],"iconStyle":"bar","pollIntervalMs":900000}"#;
        let (p, rewrite) = parse(raw).unwrap();
        assert!(rewrite);
        assert_eq!(p.settings_version, 2);
        assert_eq!(p.accounts.len(), 1);
        assert_eq!(p.accounts[0].provider, Provider::Copilot);
        assert_eq!(p.accounts[0].source, Source::Token);
        assert_eq!(p.accounts[0].token.as_deref(), Some("ghp_x"));
        assert_eq!(p.primary, p.accounts[0].id);
        assert!(p.dev_mode);
        assert_eq!(p.icon_style.as_deref(), Some("bar"));
        assert_eq!(p.poll_interval_ms, Some(900_000));
        assert_eq!(p.meridian.url, "http://127.0.0.1:3456");
    }

    #[test]
    fn migrates_v1_without_token_to_no_accounts() {
        let (p, rewrite) = parse(r#"{"settingsVersion":1,"primaryProvider":"claude"}"#).unwrap();
        assert!(rewrite);
        assert!(p.accounts.is_empty());
        assert_eq!(p.primary, "");
    }

    #[test]
    fn v2_round_trips_without_rewrite() {
        let mut acc = Account::meridian("notarify");
        acc.label = "Notarify".into();
        let p = Persist {
            settings_version: 2,
            accounts: vec![acc],
            primary: "meridian:notarify".into(),
            ..Default::default()
        };
        let raw = serde_json::to_string(&p).unwrap();
        assert_eq!(parse(&raw), Some((p, false)));
    }

    #[test]
    fn newer_version_is_left_alone() {
        assert!(parse(r#"{"settingsVersion":3,"accounts":[]}"#).is_none());
    }
}
