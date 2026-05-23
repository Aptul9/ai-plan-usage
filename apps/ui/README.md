# UI

Shared UI source for both `ai-plan-usage` desktop shells.

- source HTML, CSS, assets, and browser-side TypeScript live here
- app identity icon sources live in `icons/`
- Electron syncs built UI into `apps/electron/ui-dist/`
- Tauri syncs built UI into `apps/tauri/ui-dist/`

Build once after changes:

```powershell
cd apps/ui
npm install
npm run build
```
