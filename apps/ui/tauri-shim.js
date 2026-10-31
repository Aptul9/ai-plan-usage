// Loaded by popover.html + settings.html before the page scripts. Defines
// window.mock on top of Tauri invoke + events (withGlobalTauri = true).
(function () {
  if (typeof window === 'undefined') return
  if (!window.__TAURI__ || !window.__TAURI__.core || !window.__TAURI__.event) return
  if (window.mock) return

  const invoke = window.__TAURI__.core.invoke
  const listen = window.__TAURI__.event.listen

  window.mock = {
    getSnapshot: () => invoke('snapshot_get'),
    quit: () => invoke('app_quit'),
    openSettings: () => invoke('settings_open'),
    getAutostart: () => invoke('settings_get_autostart'),
    setAutostart: (on) => invoke('settings_set_autostart', { on: !!on }),
    getSettings: () => invoke('settings_get_all'),
    setPrimary: (id) => invoke('settings_set_primary', { id }),
    setInterval: (ms) => invoke('settings_set_interval', { ms }),
    setIconStyle: (style) => invoke('settings_set_icon_style', { style }),
    setDevMode: (on) => invoke('settings_set_dev_mode', { on: !!on }),
    setAccountEnabled: (id, on) => invoke('account_set_enabled', { id, on: !!on }),
    renameAccount: (id, label) => invoke('account_rename', { id, label }),
    moveAccount: (id, to) => invoke('account_move', { id, to }),
    removeAccount: (id) => invoke('account_remove', { id }),
    setMeridian: (url, apiKey) => invoke('meridian_set', { url, apiKey }),
    setMeridianEnabled: (on) => invoke('meridian_set_enabled', { on }),
    claudeLoginStart: () => invoke('claude_login_start'),
    claudeLoginFinish: (code, label) => invoke('claude_login_finish', { code, label }),
    codexLoginStart: () => invoke('codex_login_start'),
    codexLoginFinish: (label) => invoke('codex_login_finish', { label }),
    copilotAddToken: (token, label) => invoke('copilot_add_token', { token, label }),
    copilotImportList: () => invoke('copilot_import_list'),
    copilotImport: (id, label) => invoke('copilot_import', { id, label }),
    codexImportList: () => invoke('codex_import_list'),
    codexImport: (id, label) => invoke('codex_import', { id, label }),
    resize: (w, h) => invoke('popover_resize', { w: Math.ceil(w), h: Math.ceil(h) }),
    onUpdate: (cb) => {
      listen('snapshot-updated', (event) => cb(event.payload))
    },
  }
})()
