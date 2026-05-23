use once_cell::sync::Lazy;
use std::sync::Mutex;
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

// Last known tray icon rect, captured from click events.
static LAST_TRAY_RECT: Lazy<Mutex<Option<Rect>>> = Lazy::new(|| Mutex::new(None));

pub fn remember_tray_rect(rect: Rect) {
    if let Ok(mut g) = LAST_TRAY_RECT.lock() {
        *g = Some(rect);
    }
}

fn last_tray_rect() -> Option<Rect> {
    LAST_TRAY_RECT.lock().ok().and_then(|g| g.clone())
}

/// True when the system cursor is currently over (or within a small margin of)
/// the tray icon. Used by the popover's blur handler to differentiate a
/// tray-click-induced focus loss (cursor on tray, ignore — the tray click
/// handler owns visibility) from a user clicking anywhere else (hide popup).
pub fn cursor_over_tray(app: &AppHandle) -> bool {
    let Ok(cursor) = app.cursor_position() else {
        return false;
    };
    let Some(rect) = last_tray_rect() else {
        return false;
    };
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let tp = rect.position.to_physical::<f64>(scale);
    let ts = rect.size.to_physical::<f64>(scale);
    let margin = 4.0;
    cursor.x >= tp.x - margin
        && cursor.x <= tp.x + ts.width + margin
        && cursor.y >= tp.y - margin
        && cursor.y <= tp.y + ts.height + margin
}

pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(win) = app.get_webview_window("popover") {
        return Ok(win);
    }
    let win = WebviewWindowBuilder::new(app, "popover", WebviewUrl::App("popover.html".into()))
        .title("Claude Usage")
        .inner_size(280.0, 180.0)
        .visible(false)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .build()?;
    Ok(win)
}

pub fn toggle(win: &WebviewWindow, _app: &AppHandle) -> tauri::Result<()> {
    if win.is_visible().unwrap_or(false) {
        win.hide()?;
        return Ok(());
    }
    if let Err(e) = position_anchored(win) {
        eprintln!("position_anchored: {e}");
    }
    win.show()?;
    win.set_focus()?;
    Ok(())
}

fn position_anchored(win: &WebviewWindow) -> tauri::Result<()> {
    let Some(rect) = last_tray_rect() else {
        // No tray rect captured yet (e.g. single-instance ping before any click).
        return Ok(());
    };
    let monitor = win
        .current_monitor()?
        .or(win.primary_monitor()?)
        .ok_or_else(|| tauri::Error::from(anyhow::anyhow!("no monitor")))?;

    let mon_size = monitor.size();
    let mon_pos = monitor.position();
    let win_size = win.outer_size()?;
    let scale = monitor.scale_factor();

    let tp = rect.position.to_physical::<f64>(scale);
    let ts = rect.size.to_physical::<f64>(scale);
    let tray_x = tp.x;
    let tray_y = tp.y;
    let tray_w = ts.width;
    let tray_h = ts.height;
    let win_w = win_size.width as f64;
    let win_h = win_size.height as f64;
    let mon_x = mon_pos.x as f64;
    let mon_y = mon_pos.y as f64;
    let mon_w = mon_size.width as f64;
    let mon_h = mon_size.height as f64;

    let taskbar_on_bottom = tray_y > mon_y + mon_h / 2.0;
    let gap = 8.0;

    let x_centered = tray_x + tray_w / 2.0 - win_w / 2.0;
    let x = x_centered.max(mon_x + 8.0).min(mon_x + mon_w - win_w - 8.0);
    let y = if taskbar_on_bottom {
        tray_y - win_h - gap
    } else {
        tray_y + tray_h + gap
    };

    win.set_position(PhysicalPosition::new(x as i32, y as i32))?;
    Ok(())
}

pub fn resize(app: &AppHandle, w: u32, h: u32) -> tauri::Result<()> {
    let Some(win) = app.get_webview_window("popover") else {
        return Ok(());
    };
    let width = w.clamp(180, 640) as f64;
    let height = h.clamp(60, 600) as f64;
    win.set_size(LogicalSize::new(width, height))?;
    if win.is_visible().unwrap_or(false) {
        let _ = position_anchored(&win);
    }
    Ok(())
}
