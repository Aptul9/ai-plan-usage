# Tauri

Tauri implementation of `ai-plan-usage`.

## Prereqs

- Rust 1.77+
- Node.js 20+
- Windows 11 WebView2 runtime

## Run

One-time UI setup:

```powershell
cd apps/ui
npm install
```

```powershell
cd apps/tauri
npm install
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

## Outside the crate

- Settings live in `%APPDATA%\ai-plan-usage\settings.json`.
- UI source lives in `apps/ui/` and is copied into `apps/tauri/ui-dist/` by `sync-ui.cjs`.
- App identity icons are generated from `apps/ui/icons/` into `src-tauri/icons/`.

## Layout

- `src-tauri/` - Rust backend
- `package.json` - thin wrapper around the npm Tauri CLI
- `sync-ui.cjs` - syncs built UI files from `apps/ui/` into `ui-dist/`

## Module map

- `main.rs` - entrypoint
- `lib.rs` - Tauri setup
- `state.rs` - app state and serde types
- `accounts.rs` - accounts, their credentials, and the public view sent to the webviews
- `settings_store.rs` - settings loader/saver, v1 to v2 migration
- `providers/` - Claude, Codex, Copilot fetchers
- `meridian.rs` - Claude usage from a local Meridian
- `oauth.rs` - Claude (PKCE) and Codex (device code) sign-ins
- `opencode_import.rs` - Copilot logins read from OpenCode's store
- `color.rs` - pace-aware colors for the tray icon
- `icon.rs` - tray icon rendering
- `tray.rs` - tray/menu behavior
- `popover.rs` - popover window behavior
- `settings_win.rs` - settings window
- `scheduler.rs` - background polling
- `commands.rs` - Tauri IPC commands
