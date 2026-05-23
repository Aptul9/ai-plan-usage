import { contextBridge, ipcRenderer } from 'electron'

const api: MockAPI = {
  getSnapshot: () => ipcRenderer.invoke('snapshot:get'),
  quit: () => ipcRenderer.invoke('app:quit'),
  getAutostart: () => ipcRenderer.invoke('settings:get-autostart'),
  setAutostart: (on) => ipcRenderer.invoke('settings:set-autostart', on),
  getSettings: () => ipcRenderer.invoke('settings:get-all'),
  setProviderEnabled: (id, on) =>
    ipcRenderer.invoke('settings:set-provider-enabled', id, on),
  setPrimary: (id) => ipcRenderer.invoke('settings:set-primary', id),
  setInterval: (ms) => ipcRenderer.invoke('settings:set-interval', ms),
  setIconStyle: (style) => ipcRenderer.invoke('settings:set-icon-style', style),
  getCopilotStatus: () => ipcRenderer.invoke('settings:get-copilot-status'),
  setCopilotToken: (token) => ipcRenderer.invoke('settings:set-copilot-token', token),
  setDevMode: (on) => ipcRenderer.invoke('settings:set-dev-mode', on),
  resize: (w, h) => ipcRenderer.invoke('popover:resize', w, h),
  onUpdate: (cb) => {
    ipcRenderer.on('snapshot:updated', (_e, s: PublicState) => cb(s))
  },
}

contextBridge.exposeInMainWorld('mock', api)
