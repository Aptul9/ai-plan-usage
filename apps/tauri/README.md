# Tauri

Tauri implementation of `ai-plan-usage`.

## Prereqs

- Rust 1.77+
- Tauri CLI 2.x
- Windows 11 WebView2 runtime

## Run

One-time shared UI setup:

```powershell
cd apps/ui
npm install
```

```powershell
cd apps/tauri
npm run dev
```

## Build

```powershell
scripts\build-tauri.ps1
```

Output binary:

```text
apps/tauri/src-tauri/target/release/ai-plan-usage.exe
```

## Shared pieces

- `%APPDATA%\ai-plan-usage\settings.json` is shared with Electron.
- UI source lives in `apps/ui/` and is copied into `apps/tauri/ui-dist/` by `sync-ui.cjs`.
- App identity icons are generated from `apps/ui/icons/` into `src-tauri/icons/`.

## Layout

- `src-tauri/` - Rust backend
- `package.json` - thin wrapper around `cargo tauri ...`
- `sync-ui.cjs` - syncs built UI files from `apps/ui/` into `ui-dist/`

## Module map

- `main.rs` - entrypoint
- `lib.rs` - Tauri setup
- `state.rs` - app state and serde types
- `settings_store.rs` - shared settings loader/saver
- `providers/` - Claude, Codex, Copilot fetchers
- `icon.rs` - tray icon rendering
- `tray.rs` - tray/menu behavior
- `popover.rs` - popover window behavior
- `settings_win.rs` - settings window
- `scheduler.rs` - background polling
- `commands.rs` - Tauri IPC commands
