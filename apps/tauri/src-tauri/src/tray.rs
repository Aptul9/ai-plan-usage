use crate::icon;
use crate::popover;
use crate::scheduler;
use crate::settings_store;
use crate::state::AppState;
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tokio::sync::Mutex;

type AppStateMutex = Arc<Mutex<AppState>>;

fn build_menu(app: &AppHandle, state: &AppState) -> tauri::Result<Menu<tauri::Wry>> {
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings\u{2026}", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::new(app)?;
    menu.append(&refresh)?;
    menu.append(&settings)?;

    if state.dev_mode {
        let dev_sep = PredefinedMenuItem::separator(app)?;
        menu.append(&dev_sep)?;

        let dev_low = MenuItem::with_id(app, "dev_low", "Low (45 / 12)", true, None::<&str>)?;
        let dev_mid = MenuItem::with_id(app, "dev_mid", "Mid (85 / 50)", true, None::<&str>)?;
        let dev_high = MenuItem::with_id(app, "dev_high", "High (97 / 78)", true, None::<&str>)?;
        let dev_reset = MenuItem::with_id(app, "dev_reset", "Reset (2 / 3)", true, None::<&str>)?;
        let dev_inner_sep = PredefinedMenuItem::separator(app)?;
        let dev_stale = MenuItem::with_id(
            app,
            "dev_stale",
            "Force stale (22m old)",
            true,
            None::<&str>,
        )?;
        let dev_error = MenuItem::with_id(app, "dev_error", "Force error", true, None::<&str>)?;
        let dev_clear =
            MenuItem::with_id(app, "dev_clear_error", "Clear error", true, None::<&str>)?;
        let dev_simulate = Submenu::with_id_and_items(
            app,
            "dev_simulate",
            "Dev: simulate state",
            true,
            &[
                &dev_low,
                &dev_mid,
                &dev_high,
                &dev_reset,
                &dev_inner_sep,
                &dev_stale,
                &dev_error,
                &dev_clear,
            ],
        )?;
        menu.append(&dev_simulate)?;

        let styles = AppState::icon_styles();
        let mut style_items: Vec<CheckMenuItem<tauri::Wry>> = Vec::new();
        for s in &styles {
            let id = format!("icon_style:{s}");
            let checked = *s == state.icon_style.as_str();
            let item = CheckMenuItem::with_id(app, &id, *s, true, checked, None::<&str>)?;
            style_items.push(item);
        }
        let style_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = style_items
            .iter()
            .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
            .collect();
        let dev_icon = Submenu::with_id_and_items(
            app,
            "dev_icon",
            &format!("Dev: icon style (now: {})", state.icon_style),
            true,
            &style_refs,
        )?;
        menu.append(&dev_icon)?;
    }

    menu.append(&sep1)?;
    menu.append(&quit)?;
    Ok(menu)
}

pub fn create(app: &AppHandle, state: AppStateMutex) -> tauri::Result<()> {
    let initial = {
        let guard = tauri::async_runtime::block_on(state.lock());
        build_menu(app, &guard)?
    };

    let state_clone = state.clone();
    let _tray = TrayIconBuilder::with_id("main")
        .icon(load_default_icon())
        .icon_as_template(false)
        .menu(&initial)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let app = app.clone();
            let state_inner = state_clone.clone();
            let id = event.id.as_ref().to_string();
            handle_menu_click(app, state_inner, id);
        })
        .on_tray_icon_event(|tray, event| {
            // Capture latest tray icon rect from every event so popover anchors correctly
            // even on the first click and after monitor / taskbar moves.
            if let TrayIconEvent::Click { rect, .. } | TrayIconEvent::Enter { rect, .. } =
                event.clone()
            {
                popover::remember_tray_rect(rect);
            }
            // Toggle on Down (single click = single toggle). Up was the
            // standard choice but the Win11 focus loss between Down and Up
            // makes Down-only the only race-free option.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Down,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("popover") {
                    let _ = popover::toggle(&win, app);
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn handle_menu_click(app: AppHandle, state: AppStateMutex, id: String) {
    match id.as_str() {
        "refresh" => {
            tauri::async_runtime::spawn(async move {
                scheduler::run_once(app, state).await;
            });
        }
        "settings" => {
            let _ = crate::settings_win::open(&app);
        }
        "quit" => {
            app.exit(0);
        }
        "dev_low" => apply_dev_profile(app, state, DevProfile::Low),
        "dev_mid" => apply_dev_profile(app, state, DevProfile::Mid),
        "dev_high" => apply_dev_profile(app, state, DevProfile::High),
        "dev_reset" => apply_dev_profile(app, state, DevProfile::Reset),
        "dev_stale" => apply_dev_profile(app, state, DevProfile::Stale),
        "dev_error" => apply_dev_profile(app, state, DevProfile::Error),
        "dev_clear_error" => apply_dev_profile(app, state, DevProfile::ClearError),
        other if other.starts_with("icon_style:") => {
            let style = other.trim_start_matches("icon_style:").to_string();
            tauri::async_runtime::spawn(async move {
                {
                    let mut s = state.lock().await;
                    if AppState::icon_styles().contains(&style.as_str()) {
                        s.icon_style = style;
                        settings_store::persist(&s);
                        let public = s.public_state();
                        let _ = app.emit("snapshot-updated", public);
                        refresh_icon(&app, &s);
                    }
                }
                rebuild_menu(&app, &state).await;
            });
        }
        _ => {}
    }
}

#[derive(Copy, Clone)]
enum DevProfile {
    Low,
    Mid,
    High,
    Reset,
    Stale,
    Error,
    ClearError,
}

fn apply_dev_profile(app: AppHandle, state: AppStateMutex, profile: DevProfile) {
    tauri::async_runtime::spawn(async move {
        {
            let mut s = state.lock().await;
            match profile {
                DevProfile::Low => set_all(&mut s, Some(45.0), Some(12.0)),
                DevProfile::Mid => set_all(&mut s, Some(85.0), Some(50.0)),
                DevProfile::High => set_all(&mut s, Some(97.0), Some(78.0)),
                DevProfile::Reset => set_all(&mut s, Some(2.0), Some(3.0)),
                DevProfile::Stale => {
                    let stale = chrono::Utc::now() - chrono::Duration::minutes(22);
                    s.fetched_at = stale.to_rfc3339();
                }
                DevProfile::Error => {
                    let err = crate::state::SnapshotError {
                        message: "Not authenticated (dev)".into(),
                        kind: "not-authenticated".into(),
                        detail: Some("This is the simulated dev error from the tray menu.".into()),
                        occurred_at: Some(chrono::Utc::now().to_rfc3339()),
                        retry_at: None,
                    };
                    for snap in s.snapshots.values_mut() {
                        snap.error = Some(err.clone());
                    }
                    s.error = Some(err);
                }
                DevProfile::ClearError => {
                    for snap in s.snapshots.values_mut() {
                        snap.error = None;
                    }
                    s.error = None;
                }
            }
            if !matches!(profile, DevProfile::Stale) {
                s.fetched_at = chrono::Utc::now().to_rfc3339();
            }
            s.mirror_primary();
            let public = s.public_state();
            let _ = app.emit("snapshot-updated", public);
            refresh_icon(&app, &s);
        }
    });
}

fn set_all(s: &mut AppState, sess: Option<f64>, week: Option<f64>) {
    for snap in s.snapshots.values_mut() {
        snap.session.used_pct = sess;
        snap.weekly.used_pct = week;
    }
}

pub async fn rebuild_menu(app: &AppHandle, state: &AppStateMutex) {
    let guard = state.lock().await;
    let new_menu = match build_menu(app, &guard) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("rebuild_menu: build_menu failed: {e}");
            return;
        }
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_menu(Some(new_menu));
    }
}

fn load_default_icon() -> Image<'static> {
    let png: &'static [u8] = include_bytes!("../icons/icon-green.png");
    Image::from_bytes(png).unwrap_or_else(|_| {
        static EMPTY: [u8; 4] = [0; 4];
        Image::new(&EMPTY, 1, 1)
    })
}

pub fn refresh_icon(app: &AppHandle, state: &AppState) {
    let payload = icon::payload_from_state(state);
    let png = icon::render(&state.icon_style, &payload);
    let tray = match app.tray_by_id("main") {
        Some(t) => t,
        None => return,
    };
    if !png.is_empty() {
        if let Ok(img) = Image::from_bytes(&png) {
            let _ = tray.set_icon(Some(img));
        }
    }
    let sess = state
        .session
        .used_pct
        .map(|p| format!("{}%", p.round() as i32))
        .unwrap_or_else(|| "\u{2014}".into());
    let week = state
        .weekly
        .used_pct
        .map(|p| format!("{}%", p.round() as i32))
        .unwrap_or_else(|| "\u{2014}".into());
    let _ = tray.set_tooltip(Some(format!("Session {sess} \u{00b7} Weekly {week}")));
}
