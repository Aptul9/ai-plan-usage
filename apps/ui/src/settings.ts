function byId(id: string): HTMLElement | null {
  return document.getElementById(id)
}

let cfg: SettingsBundle | null = null

async function init(): Promise<void> {
  cfg = await window.mock.getSettings()
  renderProviders()
  renderPrimary()
  renderInterval()
  renderIconStyle()

  window.mock.onUpdate(async () => {
    cfg = await window.mock.getSettings()
    renderProviders()
    renderPrimary()
    void refreshCopilotStatus()
  })

  const auto = await window.mock.getAutostart()
  const autostart = byId('autostart') as HTMLInputElement | null
  if (autostart) {
    autostart.checked = !!auto
    autostart.addEventListener('change', async () => {
      const actual = await window.mock.setAutostart(autostart.checked)
      autostart.checked = !!actual
    })
  }

  const devMode = byId('dev-mode') as HTMLInputElement | null
  if (devMode && cfg) {
    devMode.checked = !!cfg.devMode
    devMode.addEventListener('change', async () => {
      const actual = await window.mock.setDevMode(devMode.checked)
      devMode.checked = !!actual
    })
  }

  await refreshCopilotStatus()
  const save = byId('save-copilot')
  if (save) {
    save.addEventListener('click', async () => {
      const input = byId('copilot-token') as HTMLInputElement
      const value = input.value
      const ok = await window.mock.setCopilotToken(value)
      input.value = ''
      setStatus(ok ? 'set' : 'cleared')
      cfg = await window.mock.getSettings()
      renderProviders()
      renderPrimary()
      setTimeout(refreshCopilotStatus, 1500)
    })
  }
}

async function refreshCopilotStatus(): Promise<void> {
  const st = await window.mock.getCopilotStatus()
  setStatus(st.hasToken ? 'set' : 'unset', st.encryptionAvailable)
}

function setStatus(state: 'set' | 'cleared' | 'unset', encAvail = true): void {
  const el = byId('copilot-status')
  const dot = byId('copilot-dot')
  if (!el || !dot) return
  if (state === 'set') {
    el.textContent = 'set' + (encAvail ? '' : ' (warning: OS encryption unavailable)')
    el.className = 'font-medium text-emerald-600'
    dot.className = 'w-1.5 h-1.5 rounded-full bg-emerald-500'
  } else if (state === 'cleared') {
    el.textContent = 'cleared'
    el.className = 'font-medium text-amber-600'
    dot.className = 'w-1.5 h-1.5 rounded-full bg-amber-500'
  } else {
    el.textContent = 'not set'
    el.className = 'font-medium text-amber-600'
    dot.className = 'w-1.5 h-1.5 rounded-full bg-amber-500'
  }
}

function renderProviders(): void {
  const root = byId('providers')
  if (!root || !cfg) return
  root.innerHTML = ''
  for (const p of cfg.providers) {
    const disabled = !p.available
    const row = document.createElement('div')
    row.className =
      'flex items-center justify-between gap-3 px-5 py-3.5 ' +
      (disabled ? 'opacity-50' : 'hover:bg-zinc-50/50')
    const left = document.createElement('div')
    left.className = 'flex items-center gap-3'
    const labelText = document.createElement('div')
    labelText.className = 'text-sm font-medium'
    labelText.textContent = p.label
    left.appendChild(labelText)
    if (!p.available) {
      const tag = document.createElement('span')
      tag.className = 'text-[10px] uppercase tracking-wider font-medium px-2 py-0.5 rounded-full bg-zinc-100 text-zinc-500'
      tag.textContent = p.id === 'copilot' ? 'needs token' : 'soon'
      left.appendChild(tag)
    }
    row.appendChild(left)
    // Switch toggle
    const sw = document.createElement('label')
    sw.className = 'relative inline-flex h-6 w-11 cursor-pointer items-center shrink-0'
    if (disabled) sw.classList.add('opacity-50', 'cursor-not-allowed')
    sw.innerHTML = `
      <input type="checkbox" class="peer sr-only" ${p.enabled ? 'checked' : ''} ${disabled ? 'disabled' : ''}>
      <span class="absolute h-full w-full rounded-full bg-zinc-200 peer-checked:bg-emerald-500 transition-colors"></span>
      <span class="absolute h-5 w-5 translate-x-0.5 rounded-full bg-white shadow ring-1 ring-zinc-200 transition-transform peer-checked:translate-x-[22px]"></span>
    `
    const cb = sw.querySelector('input') as HTMLInputElement
    cb.addEventListener('change', async () => {
      const result = await window.mock.setProviderEnabled(p.id, cb.checked)
      if (cfg) {
        cfg.providers = result.providers
        cfg.primaryProvider = result.primaryProvider
      }
      renderPrimary()
    })
    row.appendChild(sw)
    root.appendChild(row)
  }
}

function renderPrimary(): void {
  const root = byId('primary')
  if (!root || !cfg) return
  root.innerHTML = ''
  for (const p of cfg.providers) {
    const usable = p.available && p.enabled
    const selected = cfg.primaryProvider === p.id
    const chip = document.createElement('button')
    chip.type = 'button'
    chip.disabled = !usable
    chip.className = [
      'inline-flex items-center gap-2 h-9 px-3 rounded-lg text-sm font-medium transition',
      selected
        ? 'bg-zinc-900 text-white shadow-sm'
        : 'bg-white text-zinc-700 border border-zinc-200 hover:border-zinc-300',
      !usable && 'opacity-40 cursor-not-allowed',
    ].filter(Boolean).join(' ')
    chip.textContent = p.label
    if (usable) {
      chip.addEventListener('click', async () => {
        const newPrimary = await window.mock.setPrimary(p.id)
        if (cfg) cfg.primaryProvider = newPrimary
        renderPrimary()
      })
    }
    root.appendChild(chip)
  }
}

function renderInterval(): void {
  const sel = byId('interval') as HTMLSelectElement | null
  if (!sel || !cfg) return
  sel.innerHTML = ''
  for (const it of cfg.intervals) {
    const opt = document.createElement('option')
    opt.value = String(it.ms)
    opt.textContent = it.label
    if (it.ms === cfg.pollIntervalMs) opt.selected = true
    sel.appendChild(opt)
  }
  sel.addEventListener('change', async () => {
    const ms = await window.mock.setInterval(Number(sel.value))
    if (cfg) cfg.pollIntervalMs = ms
  })
}

function renderIconStyle(): void {
  const sel = byId('icon-style') as HTMLSelectElement | null
  if (!sel || !cfg) return
  sel.innerHTML = ''
  for (const s of cfg.iconStyles) {
    const opt = document.createElement('option')
    opt.value = s
    opt.textContent = s
    if (s === cfg.iconStyle) opt.selected = true
    sel.appendChild(opt)
  }
  sel.addEventListener('change', async () => {
    const next = await window.mock.setIconStyle(sel.value as IconStyle)
    if (cfg) cfg.iconStyle = next
  })
}

void init()
