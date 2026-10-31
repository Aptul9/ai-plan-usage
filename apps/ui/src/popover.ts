const CIRCUMFERENCE = 2 * Math.PI * 42

const PROVIDER_NAMES: Record<Provider, string> = {
  claude: 'Claude',
  codex: 'Codex',
  copilot: 'Copilot',
}

const LOGOS: Record<Provider, string> = {
  claude: 'assets/claude-symbol.svg',
  codex: 'assets/openai-symbol.svg',
  copilot: 'assets/copilot-symbol.svg',
}

interface WidgetSlot {
  label: string
  key: 'session' | 'weekly'
  kind: 'ring' | 'overage'
}

const ROW_LAYOUTS: Record<Provider, WidgetSlot[]> = {
  claude: [
    { label: 'SESSION', key: 'session', kind: 'ring' },
    { label: 'WEEK', key: 'weekly', kind: 'ring' },
  ],
  codex: [
    { label: 'SESSION', key: 'session', kind: 'ring' },
    { label: 'WEEK', key: 'weekly', kind: 'ring' },
  ],
  copilot: [
    { label: 'MONTH', key: 'weekly', kind: 'ring' },
    { label: 'OVERAGE', key: 'weekly', kind: 'overage' },
  ],
}

const $ = (id: string): HTMLElement | null => document.getElementById(id)

interface ErrorView {
  id: string
  label: string
  error: SnapshotError
}

let errorPanelOpen = false
let selectedErrorProvider: string | null = null
let latestErrors: ErrorView[] = []

function fmtDuration(ms: number): string {
  if (ms <= 0) return '0m'
  const d = Math.floor(ms / 86_400_000)
  const h = Math.floor((ms % 86_400_000) / 3_600_000)
  const m = Math.floor((ms % 3_600_000) / 60_000)
  if (d > 0) return `${d}d${h}h`
  if (h > 0) return `${h}h${m}m`
  return `${m}m`
}

const HEX_GREEN = '#22a06b'
const HEX_AMBER = '#d97706'
const HEX_RED = '#dc2626'

const SESSION_5H_MS = 5 * 3600 * 1000
const WEEKLY_7D_MS = 168 * 3600 * 1000

interface ColorParams {
  aStart: number
  aEnd: number
  rStart: number
  rEnd: number
  redFloor: number | null
}

const PARAMS_SESSION: ColorParams = { aStart: 25, aEnd: 3, rStart: 30, rEnd: 5, redFloor: 95 }
const PARAMS_WEEKLY: ColorParams = { aStart: 20, aEnd: 2, rStart: 28, rEnd: 3, redFloor: null }

function threshold(pace: number, start: number, end: number): number {
  return start + (end - start) * (pace / 100)
}

function colorForPctStatic(pct: number): string {
  if (pct >= 95) return HEX_RED
  if (pct >= 80) return HEX_AMBER
  return HEX_GREEN
}

function colorForWindow(
  usedPct: number,
  resetsAtIso: string | null,
  totalWindowMs: number,
  params: ColorParams
): string {
  if (params.redFloor != null && usedPct >= params.redFloor) return HEX_RED
  if (!resetsAtIso || totalWindowMs <= 0) return colorForPctStatic(usedPct)
  const resetsMs = new Date(resetsAtIso).getTime()
  if (!Number.isFinite(resetsMs)) return colorForPctStatic(usedPct)
  const nowMs = Date.now()
  const remaining = Math.max(0, Math.min(totalWindowMs, resetsMs - nowMs))
  const elapsed = totalWindowMs - remaining
  const pace = (elapsed / totalWindowMs) * 100
  const delta = usedPct - pace
  const amberThr = threshold(pace, params.aStart, params.aEnd)
  const redThr = threshold(pace, params.rStart, params.rEnd)
  if (delta >= redThr) return HEX_RED
  if (delta >= amberThr) return HEX_AMBER
  return HEX_GREEN
}

function paramsForSlot(slotKey: 'session' | 'weekly'): { params: ColorParams; totalMs: number } {
  if (slotKey === 'weekly') return { params: PARAMS_WEEKLY, totalMs: WEEKLY_7D_MS }
  return { params: PARAMS_SESSION, totalMs: SESSION_5H_MS }
}

function buildWidget(label: string): HTMLDivElement {
  const w = document.createElement('div')
  w.className = 'widget'
  w.innerHTML = `
    <span class="wlabel">${label}</span>
    <div class="ring-wrap">
      <svg class="ring" viewBox="0 0 100 100">
        <circle cx="50" cy="50" r="42" class="bg"/>
        <circle cx="50" cy="50" r="42" class="fg"/>
      </svg>
      <div class="ring-num">--</div>
    </div>
    <span class="reset">--</span>
  `
  return w
}

function buildOverageWidget(label: string): HTMLDivElement {
  const w = document.createElement('div')
  w.className = 'widget overage'
  w.innerHTML = `
    <span class="wlabel">${label}</span>
    <div class="overage-main">
      <div class="overage-used">--</div>
      <div class="overage-of">--</div>
    </div>
    <span class="reset">--</span>
  `
  return w
}

function updateWidget(
  widgetEl: Element,
  pct: number | null,
  resetIso: string | null,
  errored: boolean,
  slotKey: 'session' | 'weekly'
): void {
  const arc = widgetEl.querySelector('.ring .fg') as SVGCircleElement
  const num = widgetEl.querySelector('.ring-num') as HTMLElement
  const reset = widgetEl.querySelector('.reset') as HTMLElement
  if (errored || pct == null) {
    arc.style.strokeDashoffset = String(CIRCUMFERENCE)
    arc.style.stroke = 'var(--ring-off)'
    num.textContent = errored ? '!' : '--'
    num.style.color = errored ? 'var(--danger)' : 'var(--text-3)'
    reset.textContent = ''
    return
  }
  const clamped = Math.max(0, Math.min(100, pct))
  const { params, totalMs } = paramsForSlot(slotKey)
  const color = colorForWindow(clamped, resetIso, totalMs, params)
  arc.style.strokeDashoffset = String(CIRCUMFERENCE * (1 - clamped / 100))
  arc.style.stroke = color
  num.textContent = String(Math.round(clamped))
  // The arc carries the pace color; the number stays in the text color for contrast in both themes.
  num.style.color = ''
  reset.textContent = resetIso
    ? fmtDuration(new Date(resetIso).getTime() - Date.now())
    : ''
}

function updateOverageWidget(
  widgetEl: Element,
  block: UsageBlock,
  errored: boolean
): void {
  const main = widgetEl.querySelector('.overage-main') as HTMLElement
  const used = widgetEl.querySelector('.overage-used') as HTMLElement
  const of = widgetEl.querySelector('.overage-of') as HTMLElement
  const reset = widgetEl.querySelector('.reset') as HTMLElement
  used.style.color = ''
  of.style.color = ''
  if (errored) {
    used.textContent = '!'
    of.textContent = ''
    main.style.color = 'var(--danger)'
    reset.textContent = ''
    return
  }
  if (block && block.usedAbs != null && block.entitlement != null) {
    used.textContent = String(block.usedAbs)
    of.textContent = `of ${block.entitlement}`
    main.style.color = ''
    if ((block.overage ?? 0) > 0) {
      used.style.color = HEX_RED
      of.style.color = HEX_RED
    }
    reset.textContent = block.resetsAt
      ? fmtDuration(new Date(block.resetsAt).getTime() - Date.now())
      : ''
  } else {
    used.textContent = '--'
    of.textContent = ''
    main.style.color = 'var(--text-3)'
    reset.textContent = ''
  }
}

function accountName(a: Account): string {
  return `${PROVIDER_NAMES[a.provider]} · ${a.label}`
}

function buildAccountRow(a: Account): HTMLDivElement {
  const row = document.createElement('div')
  row.className = 'provider-row'
  row.dataset.account = a.id
  row.dataset.provider = a.provider

  const ident = document.createElement('div')
  ident.className = 'ident'
  const img = document.createElement('img')
  img.className = 'logo'
  img.src = LOGOS[a.provider] || ''
  img.alt = PROVIDER_NAMES[a.provider]
  ident.appendChild(img)
  const name = document.createElement('span')
  name.className = 'acct'
  name.textContent = a.label
  ident.title = accountName(a)
  ident.appendChild(name)
  row.appendChild(ident)

  for (const slot of ROW_LAYOUTS[a.provider] || ROW_LAYOUTS.claude) {
    row.appendChild(slot.kind === 'overage' ? buildOverageWidget(slot.label) : buildWidget(slot.label))
  }
  return row
}

function formatTimestamp(iso: string | undefined): string | null {
  if (!iso) return null
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return iso
  return date.toLocaleString([], {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function collectErrors(s: PublicState): ErrorView[] {
  return (s.accounts || [])
    .filter((a) => a.enabled)
    .map((a) => {
      const snap = s.snapshots && s.snapshots[a.id]
      return snap && snap.error ? { id: a.id, label: accountName(a), error: snap.error } : null
    })
    .filter(Boolean) as ErrorView[]
}

function appendText(parent: HTMLElement, className: string, text: string): HTMLElement {
  const el = document.createElement('div')
  el.className = className
  el.textContent = text
  parent.appendChild(el)
  return el
}

function renderErrorPanel(): void {
  const panel = $('error-panel')
  const chip = $('chip') as HTMLButtonElement | null
  if (!panel) return

  panel.innerHTML = ''
  if (!errorPanelOpen || latestErrors.length === 0) {
    panel.className = 'error-panel hidden'
    chip?.setAttribute('aria-expanded', 'false')
    requestAnimationFrame(reportSize)
    return
  }

  if (!selectedErrorProvider || !latestErrors.some((e) => e.id === selectedErrorProvider)) {
    selectedErrorProvider = latestErrors[0].id
  }

  const ordered = [...latestErrors].sort((a, b) => {
    if (a.id === selectedErrorProvider) return -1
    if (b.id === selectedErrorProvider) return 1
    return 0
  })

  panel.className = 'error-panel'
  chip?.setAttribute('aria-expanded', 'true')

  for (const item of ordered) {
    const wrap = document.createElement('section')
    wrap.className = 'error-item'

    const head = document.createElement('div')
    head.className = 'error-head'
    appendText(head, 'error-provider', item.label)
    appendText(head, 'error-kind', item.error.kind)
    wrap.appendChild(head)

    appendText(wrap, 'error-message', item.error.message)
    if (item.error.detail) {
      appendText(wrap, 'error-detail', item.error.detail)
    }

    const meta = [
      formatTimestamp(item.error.occurredAt),
      item.error.retryAt ? `Retry at ${formatTimestamp(item.error.retryAt)}` : null,
    ].filter(Boolean)
    if (meta.length > 0) appendText(wrap, 'error-meta', meta.join(' · '))

    panel.appendChild(wrap)
  }

  requestAnimationFrame(reportSize)
}

function rebuild(s: PublicState): void {
  const root = $('providers')
  if (!root) return
  root.innerHTML = ''
  const shown = (s.accounts || []).filter((a) => a.enabled)
  if (shown.length === 0) {
    const empty = document.createElement('div')
    empty.className = 'empty'
    empty.textContent = (s.accounts || []).length === 0
      ? 'No accounts yet. Right-click the tray icon, then Settings, to add one.'
      : 'All accounts are turned off. Turn one on in Settings.'
    root.appendChild(empty)
  }
  for (const a of shown) root.appendChild(buildAccountRow(a))
  render(s)
}

function render(s: PublicState): void {
  latestErrors = collectErrors(s)
  if (latestErrors.length === 0) {
    errorPanelOpen = false
    selectedErrorProvider = null
  }
  let firstErrMsg = latestErrors[0]?.error.message || null
  for (const row of Array.from(document.querySelectorAll<HTMLDivElement>('.provider-row'))) {
    const id = row.dataset.account
    const provider = row.dataset.provider as Provider
    const snap = s.snapshots && s.snapshots[id]
    if (!snap) continue
    const errored = !!snap.error
    row.classList.toggle('err-state', errored)
    row.title = errored ? snap.error!.message : ''
    const widgets = row.querySelectorAll('.widget')
    const layout = ROW_LAYOUTS[provider] || ROW_LAYOUTS.claude
    layout.forEach((slot, i) => {
      const block: UsageBlock = (snap as any)[slot.key] || { usedPct: null, resetsAt: null }
      const widget = widgets[i]
      if (!widget) return
      if (slot.kind === 'overage') {
        updateOverageWidget(widget, block, errored)
      } else {
        updateWidget(widget, block.usedPct, block.resetsAt, errored, slot.key)
      }
    })
  }

  const chip = $('chip') as HTMLElement | null
  if (!chip) return
  const ageMs = Date.now() - new Date(s.fetchedAt).getTime()
  if (firstErrMsg) {
    chip.textContent = latestErrors.length > 1 ? `${latestErrors.length} errors` : 'error'
    chip.title = firstErrMsg
    chip.className = 'chip err'
    chip.removeAttribute('disabled')
  } else if (ageMs > 15 * 60_000) {
    chip.textContent = `stale ${fmtDuration(ageMs)}`
    chip.title = ''
    chip.className = 'chip stale'
    chip.setAttribute('disabled', 'true')
  } else {
    chip.className = 'chip hidden'
    chip.textContent = ''
    chip.title = ''
    chip.setAttribute('disabled', 'true')
  }
  renderErrorPanel()
}

function reportSize(): void {
  const b = document.body
  const r = b.getBoundingClientRect()
  const w = Math.max(b.offsetWidth, b.scrollWidth, Math.ceil(r.width))
  const h = Math.max(b.offsetHeight, b.scrollHeight, Math.ceil(r.height))
  if (w > 0 && h > 0) window.mock.resize(w, h)
}

const sizeObserver = new ResizeObserver(() => reportSize())
sizeObserver.observe(document.body)

function rebuildAndResize(s: PublicState): void {
  rebuild(s)
  requestAnimationFrame(() => requestAnimationFrame(reportSize))
  for (const img of Array.from(document.querySelectorAll<HTMLImageElement>('.logo'))) {
    if (img.complete) continue
    img.addEventListener('load', reportSize, { once: true })
  }
}

let current: PublicState | null = null
let lastRowsKey = ''

// Rows are rebuilt when the shown accounts, their order or their names change,
// and when the last account goes away (the empty hint changes wording).
function rowsKey(s: PublicState): string {
  const all = s.accounts || []
  if (all.length === 0) return '#none'
  return all
    .filter((a) => a.enabled)
    .map((a) => `${a.id}=${a.label}`)
    .join('|')
}

const chipEl = $('chip') as HTMLButtonElement | null
if (chipEl) {
  chipEl.addEventListener('click', () => {
    if (latestErrors.length === 0) return
    errorPanelOpen = !errorPanelOpen
    if (errorPanelOpen && !selectedErrorProvider) selectedErrorProvider = latestErrors[0].id
    renderErrorPanel()
  })
}

document.addEventListener('keydown', (event) => {
  if (event.key !== 'Escape' || !errorPanelOpen) return
  errorPanelOpen = false
  renderErrorPanel()
})

window.mock.getSnapshot().then((s) => {
  current = s
  lastRowsKey = rowsKey(s)
  rebuildAndResize(s)
})

window.mock.onUpdate((s) => {
  current = s
  const key = rowsKey(s)
  if (key !== lastRowsKey) {
    lastRowsKey = key
    rebuildAndResize(s)
  } else {
    render(s)
  }
})

setInterval(() => {
  if (current) render(current)
}, 1000)
