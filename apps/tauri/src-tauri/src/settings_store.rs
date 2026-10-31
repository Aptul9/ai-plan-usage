use crate::accounts::{self, Account, Provider, Source};
use crate::meridian::MeridianConfig;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const SETTINGS_VERSION: u32 = 2;

/// Set when settings.json exists, could not be used, and copying it aside
/// failed: saving would overwrite the only copy of its tokens.
static PERSIST_BLOCKED: AtomicBool = AtomicBool::new(false);

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
/// than this build gives None: the caller copies the file aside and loads
/// defaults.
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

/// Copies a settings file this build cannot use next to it, so the first save
/// of defaults does not destroy the only copy of its tokens. When the copy
/// fails, saving stays off for this run.
fn keep_unreadable(path: &Path, why: &str) {
    let backup = path.with_file_name(format!(
        "settings.unreadable-{}.json",
        chrono::Utc::now().timestamp_millis()
    ));
    match std::fs::copy(path, &backup) {
        Ok(_) => eprintln!("settings: {why}; kept a copy at {}", backup.display()),
        Err(e) => {
            PERSIST_BLOCKED.store(true, Ordering::Relaxed);
            eprintln!("settings: {why}; copying it aside failed: {e}; changes will not be saved");
        }
    }
}

/// Keeps the v1 file before it is rewritten as v2. An existing backup is
/// never overwritten; a missing file (legacy migration) has nothing to keep.
fn backup_v1(path: &Path) {
    let bak = path.with_file_name("settings.v1.bak.json");
    if !path.exists() || bak.exists() {
        return;
    }
    if let Err(e) = std::fs::copy(path, &bak) {
        eprintln!("settings: copying {} to {} failed: {e}", path.display(), bak.display());
    }
}

/// The settings to load and whether the file must be rewritten. Only a
/// missing file falls back to the legacy one.
fn read_settings(path: &Path, legacy: &Path) -> Option<(Persist, bool)> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let raw = std::fs::read_to_string(legacy).ok()?;
            return parse(&raw).map(|(p, _)| (p, true));
        }
        Err(e) => {
            keep_unreadable(path, &format!("cannot read {}: {e}", path.display()));
            return None;
        }
    };
    let parsed = parse(&raw);
    if parsed.is_none() {
        keep_unreadable(path, &format!("{} is not usable by this build", path.display()));
    }
    parsed
}

pub fn load_or_default() -> AppState {
    let mut state = AppState::defaults();
    let Some((persisted, rewrite)) = read_settings(&settings_path(), &legacy_settings_path()) else {
        return state;
    };
    apply(&mut state, persisted);
    state.ensure_snapshots();
    if rewrite {
        backup_v1(&settings_path());
        persist(&state);
    }
    state
}

pub fn persist(state: &AppState) {
    if PERSIST_BLOCKED.load(Ordering::Relaxed) {
        eprintln!("settings: not saving, the unusable settings file could not be copied aside");
        return;
    }
    let payload = Persist {
        settings_version: SETTINGS_VERSION,
        accounts: state.accounts.clone(),
        primary: state.primary.clone(),
        meridian: state.meridian.clone(),
        dev_mode: state.dev_mode,
        icon_style: Some(state.icon_style.clone()),
        poll_interval_ms: Some(state.poll_interval_ms),
    };
    let path = settings_path();
    let tmp = path.with_extension("json.tmp");
    let saved = std::fs::create_dir_all(shared_dir())
        .and_then(|_| serde_json::to_string_pretty(&payload).map_err(std::io::Error::other))
        .and_then(|text| {
            // Flushed to disk before the rename, so a crash cannot leave an empty settings.json.
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()
        })
        .and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = saved {
        eprintln!("settings: saving {} failed: {e}", path.display());
    }
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
    fn newer_version_is_not_parsed() {
        assert!(parse(r#"{"settingsVersion":3,"accounts":[]}"#).is_none());
    }

    #[test]
    fn unreadable_file_that_cannot_be_copied_blocks_saving() {
        let dir = std::env::temp_dir().join(format!("ai-plan-usage-test-blocked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A directory named settings.json can be neither read nor copied.
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&path).unwrap();
        assert!(read_settings(&path, &dir.join("legacy.json")).is_none());
        assert!(PERSIST_BLOCKED.load(Ordering::Relaxed));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn v1_backup_is_made_once_and_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("ai-plan-usage-test-v1bak-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let bak = dir.join("settings.v1.bak.json");
        backup_v1(&path);
        assert!(!bak.exists(), "no source, no backup");
        std::fs::write(&path, "first").unwrap();
        backup_v1(&path);
        std::fs::write(&path, "second").unwrap();
        backup_v1(&path);
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "first");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unusable_file_is_copied_aside_and_legacy_ignored() {
        let dir = std::env::temp_dir().join(format!("ai-plan-usage-test-{}", std::process::id()));
        let path = dir.join("settings.json");
        let legacy = dir.join("legacy.json");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read_settings(&path, &legacy).is_none(), "nothing to load");
        for raw in ["{not json", r#"{"settingsVersion":3,"accounts":[]}"#] {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, raw).unwrap();
            std::fs::write(&legacy, r#"{"settingsVersion":1}"#).unwrap();
            assert!(read_settings(&path, &legacy).is_none(), "legacy must not replace {raw}");
            let copies: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().starts_with("settings.unreadable-"))
                .collect();
            assert_eq!(copies.len(), 1);
            assert_eq!(std::fs::read_to_string(copies[0].path()).unwrap(), raw);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
