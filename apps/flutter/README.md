# Flutter

Flutter build of `ai-plan-usage`, next to the Tauri one for comparison. Same behavior and look; own settings and sign-ins.

## Prereqs

- Flutter 3.38 (Dart 3.10)
- Visual Studio Build Tools with the C++ desktop workload

## Run

```powershell
cd apps/flutter
flutter run -d windows
```

## Build

```powershell
cd apps/flutter
flutter build windows --release
```

Output folder:

```text
apps/flutter/build/windows/x64/runner/Release/
```

## Outside the app

- Settings live in `%APPDATA%\ai-plan-usage-flutter\settings.json`: the Tauri v2 schema, a folder of its own so the two apps never refresh the same tokens.
- Start with Windows writes the value `ai-plan-usage-flutter` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- Assets are copies of `apps/ui/assets`; Inter comes from `google/fonts` (OFL, `assets/fonts/OFL.txt`).

## Module map

- `main.dart` - entrypoint: settings engine or main engine, single instance
- `model.dart` - accounts, usage snapshots, app state and the public view
- `settings_store.dart` - settings loader and saver
- `app.dart` - app state owner: polling, the settings window's commands, Codex callback server, Start with Windows
- `providers.dart` - Claude, Codex, Copilot fetchers
- `meridian.dart` - Claude usage from a local Meridian
- `oauth.dart` - Claude (PKCE, pasted code) and Codex (PKCE, localhost redirect) sign-ins
- `opencode_import.dart` - Copilot logins read from OpenCode's store
- `color.dart` - pace-aware colors
- `icon.dart` - tray icon rendering
- `tray.dart` - tray icon, tooltip and menu
- `popover.dart` - popover window and its widgets
- `settings_page.dart` - settings window (second engine) and its page

## Versions held back

`tray_manager` stays on 0.5.3 (0.6.x retracted, 0.7 and later need Flutter 3.47) and `sqlite3` on 3.5.x (3.6 and later need a newer `meta` than Flutter 3.38 pins).
