function byId<T extends HTMLElement = HTMLElement>(id: string): T | null {
  return document.getElementById(id) as T | null
}

const NAMES: Record<Provider, string> = { claude: 'Claude', codex: 'Codex', copilot: 'Copilot' }
const LOGO_SRC: Record<Provider, string> = {
  claude: 'assets/claude-symbol.svg',
  codex: 'assets/openai-symbol.svg',
  copilot: 'assets/copilot-symbol.svg',
}
const SOURCE_TAGS: Record<Source, string> = { meridian: 'Meridian', oauth: 'Sign-in', token: 'Token' }

const ICON_GRIP =
  '<svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true">' +
  '<circle cx="6" cy="3.5" r="1.25"/><circle cx="10" cy="3.5" r="1.25"/><circle cx="6" cy="8" r="1.25"/>' +
  '<circle cx="10" cy="8" r="1.25"/><circle cx="6" cy="12.5" r="1.25"/><circle cx="10" cy="12.5" r="1.25"/></svg>'
const STAR_PATH = 'M10 2.2l2.4 4.9 5.4.8-3.9 3.8.9 5.4L10 14.6l-4.8 2.5.9-5.4-3.9-3.8 5.4-.8z'
const ICON_STAR =
  `<svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true"><path d="${STAR_PATH}" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>`
const ICON_STAR_FILLED =
  `<svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true"><path d="${STAR_PATH}" fill="currentColor" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>`
const ICON_REMOVE =
  '<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>'

let cfg: SettingsBundle | null = null
let dragging = false
let addProvider: Provider | null = null
let codexRun = 0
let codexTimer = 0
let meridianDirty = false

function errText(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return String(e)
}

function setAddStatus(text: string, tone: 'ok' | 'err' | 'info' = 'info'): void {
  const el = byId('add-status')
  if (!el) return
  el.textContent = text
  el.className = 'text-[11px] ' + (tone === 'ok' ? 'text-emerald-600' : tone === 'err' ? 'text-red-600' : 'text-zinc-500')
}

// Disables the button while its call runs, so a second click cannot start it twice.
async function busy(btn: HTMLButtonElement | null, fn: () => Promise<void>): Promise<void> {
  if (btn) btn.disabled = true
  try {
    await fn()
  } finally {
    if (btn) btn.disabled = false
  }
}

// Non-add actions: the error goes to the status line of that block, and the UI always reloads to match the backend.
async function runAction(statusId: string, fn: () => Promise<unknown>): Promise<void> {
  const st = byId(statusId)
  if (st) st.textContent = ''
  try {
    await fn()
  } catch (e) {
    if (st) st.textContent = errText(e)
  } finally {
    await reload()
  }
}

async function reload(): Promise<void> {
  cfg = await window.mock.getSettings()
  renderAccounts()
  renderMeridian()
}

function renderAccounts(): void {
  const root = byId('accounts')
  if (!root || !cfg) return
  // A poll landing mid-rename or mid-drag must not throw away the edit.
  // Only the rename text field counts: a toggle keeps focus after a click and must still re-render.
  const active = document.activeElement
  const renaming = active instanceof HTMLInputElement && active.type === 'text' && root.contains(active)
  if (dragging || renaming) return
  // Keyboard focus survives the rebuild: same row, same control.
  const focusRow = active instanceof HTMLElement && root.contains(active) ? active.closest<HTMLElement>('.account-row') : null
  const focusRole = focusRow && active instanceof HTMLElement ? active.dataset.role : undefined
  root.innerHTML = ''
  if (cfg.accounts.length === 0) {
    const empty = document.createElement('div')
    empty.className = 'px-5 py-4 text-xs text-zinc-500'
    empty.textContent = 'No accounts yet. Add one below, or start Meridian.'
    root.appendChild(empty)
    return
  }
  for (const a of cfg.accounts) root.appendChild(accountRow(a))
  if (focusRow && focusRole) {
    root
      .querySelector<HTMLElement>(`.account-row[data-id="${CSS.escape(focusRow.dataset.id ?? '')}"] [data-role="${focusRole}"]`)
      ?.focus()
  }
}

function accountRow(a: Account): HTMLDivElement {
  const row = document.createElement('div')
  row.className = 'account-row flex items-center gap-2 pl-1 pr-2 min-h-[56px] py-2 transition-colors'
  row.dataset.id = a.id

  const handle = document.createElement('span')
  handle.className = 'flex items-center justify-center w-6 h-8 shrink-0 rounded cursor-grab select-none touch-none text-fg-3 hover:text-fg'
  handle.innerHTML = ICON_GRIP
  handle.title = 'Drag to reorder'
  handle.addEventListener('pointerdown', (e) => startDrag(e, row))
  row.appendChild(handle)

  const logo = document.createElement('img')
  logo.src = LOGO_SRC[a.provider]
  logo.alt = NAMES[a.provider]
  // The OpenAI and Copilot marks are black.
  logo.className = 'w-5 h-5 mx-1 shrink-0' + (a.provider === 'claude' ? '' : ' dark:invert') + (a.enabled ? '' : ' opacity-40')
  row.appendChild(logo)

  const text = document.createElement('div')
  text.className = 'flex-1 min-w-0 flex flex-col'
  const name = document.createElement('button')
  name.type = 'button'
  name.className = 'name-btn self-start max-w-full h-6 rounded text-left text-sm font-medium truncate hover:underline underline-offset-2' + (a.enabled ? '' : ' text-fg-3')
  name.textContent = a.label
  name.title = 'Rename'
  name.dataset.role = 'name'
  name.addEventListener('click', () => startRename(row, a))
  text.appendChild(name)
  const caption = document.createElement('span')
  caption.className = 'text-xs text-fg-2 truncate'
  caption.textContent = `${NAMES[a.provider]} · ${SOURCE_TAGS[a.source]}` + (a.enabled ? '' : ' · off')
  text.appendChild(caption)
  row.appendChild(text)

  const isPrimary = cfg?.primary === a.id
  const star = document.createElement('button')
  star.type = 'button'
  star.className = 'flex items-center justify-center w-8 h-8 rounded-md shrink-0 hover:bg-subtle disabled:opacity-30 disabled:hover:bg-transparent disabled:cursor-not-allowed ' + (isPrimary ? 'text-star' : 'text-fg-3 hover:text-fg')
  star.innerHTML = isPrimary ? ICON_STAR_FILLED : ICON_STAR
  star.title = isPrimary ? 'Behind the tray icon' : 'Use for the tray icon'
  star.setAttribute('aria-label', 'Use for tray icon')
  star.setAttribute('aria-pressed', String(isPrimary))
  star.dataset.role = 'star'
  star.disabled = !a.enabled
  star.addEventListener('click', () => runAction('accounts-status', () => window.mock.setPrimary(a.id)))
  row.appendChild(star)

  const sw = document.createElement('label')
  sw.className = 'switch'
  sw.innerHTML = `
    <input type="checkbox" ${a.enabled ? 'checked' : ''}>
    <span class="track"></span>
    <span class="knob"></span>
  `
  const cb = sw.querySelector('input') as HTMLInputElement
  cb.setAttribute('aria-label', `Show ${a.label} in the popover`)
  cb.dataset.role = 'toggle'
  cb.addEventListener('change', () => runAction('accounts-status', () => window.mock.setAccountEnabled(a.id, cb.checked)))
  row.appendChild(sw)

  const remove = document.createElement('button')
  remove.type = 'button'
  remove.className = 'flex items-center justify-center h-8 min-w-8 px-2 rounded-md text-xs font-medium shrink-0 text-fg-3 hover:text-err hover:bg-err/10'
  remove.innerHTML = ICON_REMOVE
  remove.setAttribute('aria-label', `Remove ${a.label}`)
  remove.dataset.role = 'remove'
  // Meridian rows come back on the next poll, so they are turned off instead.
  if (a.source === 'meridian') remove.classList.add('invisible')
  remove.addEventListener('click', async () => {
    if (remove.dataset.armed !== '1') {
      remove.dataset.armed = '1'
      remove.textContent = 'Remove?'
      remove.setAttribute('aria-label', `Confirm remove ${a.label}`)
      remove.classList.add('text-err', 'bg-err/10')
      setTimeout(() => {
        remove.dataset.armed = ''
        remove.innerHTML = ICON_REMOVE
        remove.setAttribute('aria-label', `Remove ${a.label}`)
        remove.classList.remove('text-err', 'bg-err/10')
      }, 3000)
      return
    }
    await runAction('accounts-status', () => window.mock.removeAccount(a.id))
  })
  row.appendChild(remove)
  return row
}

function startRename(row: HTMLDivElement, a: Account): void {
  const btn = row.querySelector('.name-btn') as HTMLButtonElement | null
  if (!btn) return
  const input = document.createElement('input')
  input.value = a.label
  input.className = 'flex-1 min-w-0 h-8 rounded-md border border-zinc-300 px-2 text-sm'
  input.setAttribute('aria-label', 'Account name')
  btn.replaceWith(input)
  input.focus()
  input.select()
  let done = false
  const finish = async (save: boolean): Promise<void> => {
    if (done) return
    done = true
    const next = input.value.trim()
    input.blur()
    if (save && next && next !== a.label) {
      try {
        await window.mock.renameAccount(a.id, next)
      } catch (e) {
        setAddStatus(errText(e), 'err')
      }
    }
    await reload()
  }
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') void finish(true)
    if (e.key === 'Escape') void finish(false)
  })
  input.addEventListener('blur', () => void finish(true))
}

function startDrag(e: PointerEvent, row: HTMLDivElement): void {
  const root = byId('accounts')
  if (!root || !cfg || e.button !== 0) return
  e.preventDefault()
  dragging = true
  const id = row.dataset.id
  const from = cfg.accounts.findIndex((a) => a.id === id)
  row.classList.add('opacity-60', 'bg-zinc-50')
  const move = (ev: PointerEvent): void => {
    const others = Array.from(root.querySelectorAll<HTMLDivElement>('.account-row')).filter((r) => r !== row)
    const before = others.find((r) => {
      const b = r.getBoundingClientRect()
      return ev.clientY < b.top + b.height / 2
    })
    if (before) root.insertBefore(row, before)
    else root.appendChild(row)
  }
  // pointerup, pointercancel and window blur all end the drag, so `dragging` cannot stay true.
  const up = async (): Promise<void> => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', up)
    window.removeEventListener('pointercancel', up)
    window.removeEventListener('blur', up)
    if (!dragging) return
    row.classList.remove('opacity-60', 'bg-zinc-50')
    const to = Array.from(root.querySelectorAll<HTMLDivElement>('.account-row')).findIndex((r) => r === row)
    dragging = false
    await runAction('accounts-status', async () => {
      if (to >= 0 && to !== from) await window.mock.moveAccount(id, to)
    })
  }
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
  window.addEventListener('pointercancel', up)
  window.addEventListener('blur', up)
}

function renderMeridian(): void {
  if (!cfg) return
  const url = byId<HTMLInputElement>('meridian-url')
  if (url && !meridianDirty && document.activeElement !== url) url.value = cfg.meridian.url
  const key = byId<HTMLInputElement>('meridian-key')
  if (key) {
    key.placeholder = cfg.meridian.hasApiKey
      ? 'API key saved, type to replace'
      : 'API key, only if MERIDIAN_API_KEY is set'
  }
  byId('meridian-clear-key')?.classList.toggle('hidden', !cfg.meridian.hasApiKey)
  const st = byId('meridian-status')
  if (!st) return
  const m = cfg.meridianStatus
  const at = m.checkedAt ? new Date(m.checkedAt) : null
  const checked = at && !Number.isNaN(at.getTime())
    ? `, checked ${at.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })}`
    : ''
  let text = `Not reachable${checked}`
  if (!cfg.meridian.url) text = 'Off'
  else if (!m.checkedAt) text = 'Not checked yet'
  else if (m.reachable) text = `Reachable, ${m.profiles} Claude profile${m.profiles === 1 ? '' : 's'}${checked}`
  st.textContent = text
  st.className = 'text-xs ' + (m.reachable && cfg.meridian.url ? 'text-emerald-600' : 'text-zinc-500')
}

function wireMeridian(): void {
  // Unsaved edits in the address survive polls, focused or not.
  byId('meridian-url')?.addEventListener('input', () => {
    meridianDirty = true
  })
  byId('meridian-save')?.addEventListener('click', () =>
    runAction('meridian-error', async () => {
      const keyInput = byId<HTMLInputElement>('meridian-key')
      const key = keyInput ? keyInput.value.trim() : ''
      await window.mock.setMeridian(byId<HTMLInputElement>('meridian-url')?.value ?? '', key ? key : null)
      if (keyInput) keyInput.value = ''
      meridianDirty = false
    })
  )
  byId('meridian-clear-key')?.addEventListener('click', () =>
    runAction('meridian-error', async () => {
      await window.mock.setMeridian(byId<HTMLInputElement>('meridian-url')?.value ?? '', '')
      meridianDirty = false
    })
  )
}

function stopCodexTimer(): void {
  window.clearInterval(codexTimer)
  codexTimer = 0
}

// Ticks "Code expires in m:ss" under the device code until 0, a newer run, or the end of the flow.
function startCodexTimer(secs: number, run: number): void {
  stopCodexTimer()
  const el = byId('codex-expiry')
  const end = Date.now() + secs * 1000
  const tick = (): void => {
    if (run !== codexRun) return stopCodexTimer()
    const left = Math.max(0, Math.round((end - Date.now()) / 1000))
    if (el) {
      el.textContent = left > 0
        ? `Code expires in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}`
        : 'Code expired. Get a new one.'
    }
    if (left === 0) stopCodexTimer()
  }
  tick()
  codexTimer = window.setInterval(tick, 1000)
}

function showAdd(p: Provider | null): void {
  addProvider = p
  for (const id of ['claude', 'codex', 'copilot'] as Provider[]) {
    byId(`add-${id}`)?.classList.toggle('hidden', p !== id)
    document
      .querySelector<HTMLButtonElement>(`#add-provider [data-provider="${id}"]`)
      ?.setAttribute('aria-pressed', String(p === id))
  }
  setAddStatus('')
}

function value(id: string): string {
  return byId<HTMLInputElement>(id)?.value ?? ''
}

function wireAdd(): void {
  document.querySelectorAll<HTMLButtonElement>('#add-provider [data-provider]').forEach((b) => {
    b.addEventListener('click', () => {
      const p = b.dataset.provider as Provider
      showAdd(addProvider === p ? null : p)
    })
  })

  byId('claude-start')?.addEventListener('click', (ev) => busy(ev.currentTarget as HTMLButtonElement, async () => {
    try {
      const url = await window.mock.claudeLoginStart()
      const urlField = byId<HTMLInputElement>('claude-url')
      if (urlField) urlField.value = url
      byId('claude-step2')?.classList.remove('hidden')
      byId<HTMLInputElement>('claude-code')?.focus()
      setAddStatus('Sign in in the browser, then paste the code from the last page.')
    } catch (e) {
      setAddStatus(errText(e), 'err')
    }
  }))

  byId('claude-finish')?.addEventListener('click', (ev) => busy(ev.currentTarget as HTMLButtonElement, async () => {
    try {
      await window.mock.claudeLoginFinish(value('claude-code'), value('claude-label'))
      const code = byId<HTMLInputElement>('claude-code')
      if (code) code.value = ''
      byId('claude-step2')?.classList.add('hidden')
      setAddStatus('Claude account added.', 'ok')
      await reload()
    } catch (e) {
      setAddStatus(errText(e), 'err')
    }
  }))

  byId('codex-start')?.addEventListener('click', async (ev) => {
    const run = ++codexRun
    stopCodexTimer()
    try {
      // Disabled only while the code is fetched: a new code may be requested while one waits.
      const btn = ev.currentTarget as HTMLButtonElement
      btn.disabled = true
      const d = await window.mock.codexLoginStart().finally(() => {
        btn.disabled = false
      })
      if (run !== codexRun) return
      const code = byId('codex-code')
      if (code) code.textContent = d.userCode
      const url = byId('codex-url')
      if (url) url.textContent = d.url
      byId('codex-step2')?.classList.remove('hidden')
      startCodexTimer(d.expiresInSecs, run)
      setAddStatus('Waiting for approval in the browser…')
      await window.mock.codexLoginFinish(value('codex-label'))
      if (run !== codexRun) return
      stopCodexTimer()
      byId('codex-step2')?.classList.add('hidden')
      setAddStatus('Codex account added.', 'ok')
      await reload()
    } catch (e) {
      // A newer "Get sign-in code" click owns the panel now.
      if (run !== codexRun) return
      stopCodexTimer()
      byId('codex-step2')?.classList.add('hidden')
      setAddStatus(errText(e), 'err')
    }
  })

  byId('copilot-save')?.addEventListener('click', (ev) => busy(ev.currentTarget as HTMLButtonElement, async () => {
    try {
      await window.mock.copilotAddToken(value('copilot-token'), value('copilot-label'))
      const token = byId<HTMLInputElement>('copilot-token')
      if (token) token.value = ''
      setAddStatus('Copilot account added.', 'ok')
      await reload()
    } catch (e) {
      setAddStatus(errText(e), 'err')
    }
  }))

  byId('copilot-import-load')?.addEventListener('click', async () => {
    const list = byId('copilot-import-list')
    if (!list) return
    list.innerHTML = ''
    try {
      const logins = await window.mock.copilotImportList()
      if (logins.length === 0) {
        setAddStatus('No Copilot logins in OpenCode.')
        return
      }
      setAddStatus('')
      for (const l of logins) {
        const row = document.createElement('div')
        row.className = 'flex items-center gap-3 rounded-lg border border-stroke bg-subtle pl-3 pr-1.5 py-1.5 text-sm'
        const logo = document.createElement('img')
        logo.src = LOGO_SRC.copilot
        logo.alt = ''
        logo.className = 'w-4 h-4 shrink-0 dark:invert'
        const name = document.createElement('span')
        name.className = 'flex-1 min-w-0 truncate'
        name.textContent = l.label
        const add = document.createElement('button')
        add.type = 'button'
        add.className = 'btn-quiet'
        add.textContent = 'Add'
        add.addEventListener('click', () => busy(add, async () => {
          try {
            await window.mock.copilotImport(l.id, l.label)
            setAddStatus(`Copilot · ${l.label} added.`, 'ok')
            row.remove()
            await reload()
          } catch (e) {
            setAddStatus(errText(e), 'err')
          }
        }))
        row.append(logo, name, add)
        list.appendChild(row)
      }
    } catch (e) {
      setAddStatus(errText(e), 'err')
    }
  })
}

function renderInterval(): void {
  const sel = byId<HTMLSelectElement>('interval')
  if (!sel || !cfg) return
  sel.innerHTML = ''
  for (const it of cfg.intervals) {
    const opt = document.createElement('option')
    opt.value = String(it.ms)
    opt.textContent = it.label
    opt.selected = it.ms === cfg.pollIntervalMs
    sel.appendChild(opt)
  }
  sel.addEventListener('change', async () => {
    const ms = await window.mock.setInterval(Number(sel.value))
    if (cfg) cfg.pollIntervalMs = ms
  })
}

function renderIconStyle(): void {
  const sel = byId<HTMLSelectElement>('icon-style')
  if (!sel || !cfg) return
  sel.innerHTML = ''
  for (const s of cfg.iconStyles) {
    const opt = document.createElement('option')
    opt.value = s
    opt.textContent = s
    opt.selected = s === cfg.iconStyle
    sel.appendChild(opt)
  }
  sel.addEventListener('change', async () => {
    const next = await window.mock.setIconStyle(sel.value as IconStyle)
    if (cfg) cfg.iconStyle = next
  })
}

async function init(): Promise<void> {
  await reload()
  renderInterval()
  renderIconStyle()
  wireAdd()
  wireMeridian()
  window.mock.onUpdate(() => void reload())

  const autostart = byId<HTMLInputElement>('autostart')
  if (autostart) {
    autostart.checked = !!(await window.mock.getAutostart())
    autostart.addEventListener('change', async () => {
      autostart.checked = !!(await window.mock.setAutostart(autostart.checked))
    })
  }

  const devMode = byId<HTMLInputElement>('dev-mode')
  if (devMode && cfg) {
    devMode.checked = !!cfg.devMode
    devMode.addEventListener('change', async () => {
      devMode.checked = !!(await window.mock.setDevMode(devMode.checked))
    })
  }
}

void init()
