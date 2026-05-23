# Electron

Electron implementation of `ai-plan-usage`.

This variant stays in repo for two reasons:
- UX reference for tray and popover behavior
- packaging comparison against Tauri

## Run

One-time shared UI setup:

```powershell
cd apps/ui
npm install
```

```powershell
cd apps/electron
npm install
npm start
```

## Build

```powershell
scripts\build-electron.ps1
```

`npm run package` also works inside this folder.

## Notes

- Supports Claude, Codex, and GitHub Copilot usage data.
- Shared settings live at `%APPDATA%\ai-plan-usage\settings.json`.
- Shared UI source lives in `apps/ui/` and gets staged into `apps/electron/ui-dist/`.
- App identity icons are generated from `apps/ui/icons/` into `apps/electron/assets/`.

## Layout

```text
apps/electron/
├── package.json
├── tsconfig.json
├── sync-ui.cjs
├── src/
│   ├── main.ts
│   ├── preload.ts
├── ui-dist/
├── assets/
│   ├── app-icon.ico
│   └── app-icon.png
└── electron-builder.yml
```
