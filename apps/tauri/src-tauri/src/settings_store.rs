use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistShape {
    #[serde(rename = "copilotToken", skip_serializing_if = "Option::is_none")]
    copilot_token: Option<String>,
    #[serde(rename = "devMode", skip_serializing_if = "Option::is_none")]
    dev_mode: Option<bool>,
    #[serde(rename = "primaryProvider", skip_serializing_if = "Option::is_none")]
    primary_provider: Option<String>,
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
    if migrated {
        persist(&state);
    }
    state
}

pub fn persist(state: &AppState) {
    let payload = PersistShape {
        copilot_token: state.copilot_token.clone(),
        dev_mode: Some(state.dev_mode),
        primary_provider: Some(state.primary_provider.clone()),
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
