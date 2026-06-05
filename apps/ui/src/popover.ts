const CIRCUMFERENCE = 2 * Math.PI * 42

const LOGOS: Record<ProviderId, string> = {
  claude: 'assets/claude-symbol.svg',
  codex: 'assets/openai-symbol.svg',
  copilot: 'assets/copilot-symbol.svg',
}

interface WidgetSlot {
  label: string
  key: 'session' | 'weekly'
  kind: 'ring' | 'overage'
}

const ROW_LAYOUTS: Record<ProviderId, WidgetSlot[]> = {
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
  id: ProviderId
  label: string
  error: SnapshotError
}

let errorPanelOpen = false
let selectedErrorProvider: ProviderId | null = null
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
const PARAMS_WEEKLY: ColorParams = { aStart: 35, aEnd: 2, rStart: 55, rEnd: 3, redFloor: null }

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
    arc.style.stroke = '#bbb'
    num.textContent = errored ? '!' : '—'
    num.style.color = '#888'
    reset.textContent = ''
    return
  }
  const clamped = Math.max(0, Math.min(100, pct))
  const { params, totalMs } = paramsForSlot(slotKey)
  const color = colorForWindow(clamped, resetIso, totalMs, params)
  arc.style.strokeDashoffset = String(CIRCUMFERENCE * (1 - clamped / 100))
  arc.style.stroke = color
  num.textContent = String(Math.round(clamped))
  num.style.color = color
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
  if (errored) {
    used.textContent = '!'
    of.textContent = ''
    main.style.color = '#888'
    reset.textContent = ''
    return
  }
  if (block && block.usedAbs != null && block.entitlement != null) {
    used.textContent = String(block.usedAbs)
    of.textContent = `of ${block.entitlement}`
    const color = (block.overage ?? 0) > 0 ? '#dc2626' : '#222'
    used.style.color = color
    of.style.color = (block.overage ?? 0) > 0 ? '#dc2626' : '#888'
    reset.textContent = block.resetsAt
      ? fmtDuration(new Date(block.resetsAt).getTime() - Date.now())
      : ''
  } else {
    used.textContent = '—'
    of.textContent = ''
    main.style.color = '#888'
    reset.textContent = ''
  }
}

function buildProviderRow(id: ProviderId, label: string): HTMLDivElement {
  const row = document.createElement('div')
  row.className = 'provider-row'
  row.dataset.provider = id

  const img = document.createElement('img')
  img.className = 'logo'
  img.src = LOGOS[id] || ''
  img.alt = label
  row.appendChild(img)

  const layout = ROW_LAYOUTS[id] || ROW_LAYOUTS.claude
  for (const slot of layout) {
    if (slot.kind === 'overage') row.appendChild(buildOverageWidget(slot.label))
    else row.appendChild(buildWidget(slot.label))
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
  return (s.providers || [])
    .filter((p) => p.enabled)
    .map((p) => {
      const snap = s.snapshots && s.snapshots[p.id]
      return snap && snap.error ? { id: p.id, label: p.label, error: snap.error } : null
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
  for (const p of s.providers) {
    if (!p.enabled) continue
    root.appendChild(buildProviderRow(p.id, p.label))
  }
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
    const id = row.dataset.provider as ProviderId
    const snap = s.snapshots && s.snapshots[id]
    if (!snap) continue
    const errored = !!snap.error
    row.classList.toggle('err-state', errored)
    row.title = errored ? snap.error!.message : ''
    const widgets = row.querySelectorAll('.widget')
    const layout = ROW_LAYOUTS[id] || ROW_LAYOUTS.claude
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
let lastProvIds = ''

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
  lastProvIds = (s.providers || [])
    .filter((p) => p.enabled)
    .map((p) => p.id)
    .join(',')
  rebuildAndResize(s)
})

window.mock.onUpdate((s) => {
  current = s
  const ids = (s.providers || [])
    .filter((p) => p.enabled)
    .map((p) => p.id)
    .join(',')
  if (ids !== lastProvIds) {
    lastProvIds = ids
    rebuildAndResize(s)
  } else {
    render(s)
  }
})

setInterval(() => {
  if (current) render(current)
}, 1000)
