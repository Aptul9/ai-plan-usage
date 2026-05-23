use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

fn build(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    if app.get_webview_window("settings").is_some() {
        return Ok(());
    }
    let _win = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("ai-plan-usage settings")
        .inner_size(520.0, 640.0)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .visible(visible)
        .skip_taskbar(!visible)
        .build()?;
    Ok(())
}

/// Build the settings webview hidden at startup so the WebView2 + Tailwind
/// CDN load happens before the user clicks Settings. Cuts open latency from
/// ~1s cold to <100ms warm.
pub fn prewarm(app: &AppHandle) -> tauri::Result<()> {
    build(app, false)
}

pub fn open(app: &AppHandle) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window("settings") {
        // Pre-warmed window was skip_taskbar; flip back when showing for real.
        let _ = win.set_skip_taskbar(false);
        win.show()?;
        win.unminimize()?;
        win.set_focus()?;
        return Ok(());
    }
    build(app, true)
}
