use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistShape {
    #[serde(rename = "settingsVersion", skip_serializing_if = "Option::is_none")]
    settings_version: Option<u32>,
    #[serde(rename = "copilotToken", skip_serializing_if = "Option::is_none")]
    copilot_token: Option<String>,
    #[serde(rename = "devMode", skip_serializing_if = "Option::is_none")]
    dev_mode: Option<bool>,
    #[serde(rename = "primaryProvider", skip_serializing_if = "Option::is_none")]
    primary_provider: Option<String>,
    #[serde(rename = "enabledProviderIds", skip_serializing_if = "Option::is_none")]
    enabled_provider_ids: Option<Vec<String>>,
    #[serde(rename = "iconStyle", skip_serializing_if = "Option::is_none")]
    icon_style: Option<String>,
    #[serde(rename = "pollIntervalMs", skip_serializing_if = "Option::is_none")]
    poll_interval_ms: Option<u64>,
}

fn shared_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")));
    base.join("ai-plan-usage")
}

fn legacy_shared_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")));
    base.join("claude-usage-tray")
}

fn settings_path() -> PathBuf {
    shared_dir().join("settings.json")
}

fn legacy_settings_path() -> PathBuf {
    legacy_shared_dir().join("settings.json")
}

fn apply_enabled_provider_ids(state: &mut AppState, enabled_ids: &[String]) {
    for provider in &mut state.providers {
        provider.enabled = provider.available && enabled_ids.iter().any(|id| id == &provider.id);
    }
}

fn normalize_primary_provider(state: &mut AppState) {
    let primary_ok = state
        .providers
        .iter()
        .any(|p| p.id == state.primary_provider && p.enabled && p.available);
    if primary_ok {
        return;
    }
    if let Some(first) = state.providers.iter().find(|p| p.enabled && p.available) {
        state.primary_provider = first.id.clone();
    }
}

pub fn load_or_default() -> AppState {
    let mut state = AppState::defaults();
    let path = settings_path();
    let (raw, migrated) = match std::fs::read_to_string(&path) {
        Ok(raw) => (raw, false),
        Err(_) => match std::fs::read_to_string(legacy_settings_path()) {
            Ok(raw) => (raw, true),
            Err(_) => return state,
        },
    };
    let Ok(parsed) = serde_json::from_str::<PersistShape>(&raw) else {
        return state;
    };
    let should_rewrite = migrated
        || parsed.settings_version != Some(SETTINGS_VERSION)
        || parsed.enabled_provider_ids.is_none();
    if let Some(t) = parsed.copilot_token {
        if !t.is_empty() {
            state.copilot_token = Some(t);
            if let Some(p) = state.providers.iter_mut().find(|p| p.id == "copilot") {
                p.available = true;
            }
        }
    }
    if let Some(d) = parsed.dev_mode {
        state.dev_mode = d;
    }
    if let Some(p) = parsed.primary_provider {
        if state.providers.iter().any(|x| x.id == p) {
            state.primary_provider = p;
        }
    }
    if let Some(enabled_provider_ids) = parsed.enabled_provider_ids {
        apply_enabled_provider_ids(&mut state, &enabled_provider_ids);
    } else if state.copilot_token.is_some() {
        if let Some(p) = state.providers.iter_mut().find(|p| p.id == "copilot") {
            p.enabled = p.available;
        }
    }
    if let Some(s) = parsed.icon_style {
        if AppState::icon_styles().contains(&s.as_str()) {
            state.icon_style = s;
        }
    }
    if let Some(ms) = parsed.poll_interval_ms {
        if AppState::poll_intervals().iter().any(|x| x.ms == ms) {
            state.poll_interval_ms = ms;
        }
    }
    normalize_primary_provider(&mut state);
    if should_rewrite {
        persist(&state);
    }
    state
}

pub fn persist(state: &AppState) {
    let payload = PersistShape {
        settings_version: Some(SETTINGS_VERSION),
        copilot_token: state.copilot_token.clone(),
        dev_mode: Some(state.dev_mode),
        primary_provider: Some(state.primary_provider.clone()),
        enabled_provider_ids: Some(
            state
                .providers
                .iter()
                .filter(|p| p.enabled)
                .map(|p| p.id.clone())
                .collect(),
        ),
        icon_style: Some(state.icon_style.clone()),
        poll_interval_ms: Some(state.poll_interval_ms),
    };
    let dir = shared_dir();
    if std::fs::create_dir_all(&dir).is_err() {
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
