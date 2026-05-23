// Loaded by popover.html + settings.html before the page scripts.
// Under Electron, window.__TAURI__ is undefined → no-op (Electron preload.js sets window.mock).
// Under Tauri (withGlobalTauri = true), defines window.mock backed by Tauri invoke + events.
(function () {
  if (typeof window === 'undefined') return
  if (!window.__TAURI__ || !window.__TAURI__.core || !window.__TAURI__.event) return
  if (window.mock) return // shouldn't happen but be defensive

  const invoke = window.__TAURI__.core.invoke
  const listen = window.__TAURI__.event.listen

  window.mock = {
    getSnapshot: () => invoke('snapshot_get'),
    quit: () => invoke('app_quit'),
    getAutostart: () => invoke('settings_get_autostart'),
    setAutostart: (on) => invoke('settings_set_autostart', { on: !!on }),
    getSettings: () => invoke('settings_get_all'),
    setProviderEnabled: (id, on) =>
      invoke('settings_set_provider_enabled', { id, on: !!on }),
    setPrimary: (id) => invoke('settings_set_primary', { id }),
    setInterval: (ms) => invoke('settings_set_interval', { ms }),
    setIconStyle: (style) => invoke('settings_set_icon_style', { style }),
    getCopilotStatus: () => invoke('settings_get_copilot_status'),
    setCopilotToken: (token) => invoke('settings_set_copilot_token', { token }),
    setDevMode: (on) => invoke('settings_set_dev_mode', { on: !!on }),
    resize: (w, h) =>
      invoke('popover_resize', { w: Math.ceil(w), h: Math.ceil(h) }),
    onUpdate: (cb) => {
      listen('snapshot-updated', (event) => cb(event.payload))
    },
  }
})()
