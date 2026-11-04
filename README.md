<p align="center">
  <img src="apps/ui/icons/ai-plan-usage-icon-128.png" width="96" height="96" alt="ai-plan-usage icon">
</p>

<h1 align="center">ai-plan-usage</h1>

<p align="center">
  A small Windows tray app for Claude, Codex, and GitHub Copilot plan usage.
</p>

<p align="center">
  Minimal interface. Local data. No dashboards. No terminal workflow for checking usage.
</p>

## Why

`ai-plan-usage` is for people who use AI plans every day and just want to monitor how much of their plan they got left.

It stays out of the way, lives in the tray, and shows only the usage signals that matter:

- Claude session and weekly usage
- Codex session and weekly usage
- GitHub Copilot monthly usage and overage
- reset countdowns
- a dynamic tray icon for the account you care about most

Daily use is handled through the UI. Click the tray icon, check the popover, adjust settings when needed, and get back to work.

## Easy Tray Readout

The tray icon shows the selected primary account at a glance. The number and ring are designed for quick checking, and the tooltip gives the current session and weekly percentages without opening a full window.

<p align="center">
  <img src="screenshots/tooltip.png" width="235" alt="Tray tooltip showing session and weekly usage">
</p>

## Compact Usage Popover

The popover shows each enabled account in one compact view. Claude and Codex show session and weekly usage; Copilot shows monthly usage and overage. Reset countdowns are visible where they matter.

<p align="center">
  <img src="screenshots/popup.png" width="322" alt="ai-plan-usage tray popover showing Claude, Codex, and Copilot usage">
</p>

## Simple Settings

Settings are plain and practical: add and order accounts, pick the one behind the tray icon, point the app at Meridian, set the polling interval and change the icon style.

<p align="center">
  <img src="screenshots/settings.png" width="520" alt="ai-plan-usage settings window with account toggles and icon options">
</p>

## Features

- **Claude, Codex, and GitHub Copilot** usage in one tray app.
- **Account toggles** so the popover only shows what you use.
- **Primary account selection** for the tray icon number and ring.
- **Configurable refresh interval** with a 5 minute default.
- **Several accounts per provider**, each with its own name, order and toggle.
- **Claude from Meridian or a direct sign-in**, Codex from a browser sign-in, Copilot from a pasted token or a login copied from OpenCode. The Claude Code and Codex CLIs are not needed.
- **Local settings** under `%APPDATA%\ai-plan-usage-flutter\`, tokens included, in plain text.
- **Pace-aware coloring** for the tray icon and popover (see `docs/coloring.md`).
- **No hosted backend** for your usage data.
- **No analytics dashboard** or historical reporting layer.

## Install

Download `ai-plan-usage-<version>-windows-x64-setup.exe` from [Releases](https://github.com/Aptul9/ai-plan-usage/releases) and run it. It installs for the current user under `%LOCALAPPDATA%\Programs\ai-plan-usage`, no admin rights, and adds `ai-plan-usage` to the Start menu. Uninstall from Settings > Apps; settings and sign-ins stay in `%APPDATA%\ai-plan-usage-flutter\`.

The installer is not signed, so SmartScreen may warn on first run: More info, then Run anyway.

## Run

```powershell
cd apps/flutter
flutter run -d windows
```

The Tauri build in `apps/tauri` is kept for comparison: see `apps/tauri/README.md`.

## Build

```powershell
scripts\build-flutter.ps1
```

Needs Flutter 3.38 and Inno Setup 6 (`winget install JRSoftware.InnoSetup`). The app folder is mirrored into `artifacts/flutter/` and the installer is written to `artifacts/`.

## GitHub Actions

The desktop build workflow runs on `windows-latest`.

- Every push, pull request, and manual run uploads a `windows-desktop-exes` artifact: the Flutter installer and the Tauri `.exe`.
- Tags that start with `v` also create or update a GitHub Release and upload the installer.
