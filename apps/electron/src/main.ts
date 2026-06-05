import {
  app,
  Tray,
  Menu,
  BrowserWindow,
  ipcMain,
  screen,
  nativeImage,
  safeStorage,
} from 'electron'
import * as path from 'path'
import * as fs from 'fs/promises'
import * as os from 'os'
import { ChildProcess, spawn } from 'child_process'
import { z } from 'zod'

const CLAUDE_CREDS = path.join(os.homedir(), '.claude', '.credentials.json')
const CODEX_CREDS = path.join(os.homedir(), '.codex', 'auth.json')
const APP_ID = 'io.local.ai-plan-usage-electron'
const APP_DATA_DIR_NAME = 'ai-plan-usage'
const LEGACY_SHARED_DIR_NAME = 'claude-usage-tray'

const SHARED_SETTINGS_DIR = path.join(
  process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming'),
  APP_DATA_DIR_NAME
)
const LEGACY_SHARED_SETTINGS_DIR = path.join(
  process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming'),
  LEGACY_SHARED_DIR_NAME
)
const UI_DIST_DIR = path.join(__dirname, 'ui-dist')
const settingsFile = (): string => path.join(SHARED_SETTINGS_DIR, 'settings.json')
const legacySettingsFiles = (): string[] => [
  path.join(LEGACY_SHARED_SETTINGS_DIR, 'settings.json'),
  path.join(
    process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming'),
    'claude-usage-tray-mockup',
    'mockup-settings.json'
  ),
  path.join(
    process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming'),
    'claude-usage-tray-electron',
    'mockup-settings.json'
  ),
]

const SETTINGS_VERSION = 1
const PROVIDER_IDS = ['claude', 'codex', 'copilot'] as const
const ICON_STYLE_IDS = ['solid', 'number', 'ring', 'ring+number', 'bar'] as const

const SettingsSchema = z
  .object({
    settingsVersion: z.number().int().min(1).optional().catch(undefined),
    copilotToken: z.string().optional().catch(undefined),
    devMode: z.boolean().optional().catch(undefined),
    primaryProvider: z.enum(PROVIDER_IDS).optional().catch(undefined),
    enabledProviderIds: z.array(z.enum(PROVIDER_IDS)).optional().catch(undefined),
    iconStyle: z.enum(ICON_STYLE_IDS).optional().catch(undefined),
    pollIntervalMs: z.number().int().positive().optional().catch(undefined),
  })
  .strip()

type PersistShape = z.infer<typeof SettingsSchema>

function normalizePrimaryProvider(): void {
  const primary = state.providers.find((p) => p.id === state.primaryProvider)
  if (primary && primary.enabled && primary.available) return

  const first = state.providers.find((p) => p.enabled && p.available)
  if (first) state.primaryProvider = first.id
}

function applyParsed(parsed: unknown): void {
  const result = SettingsSchema.safeParse(parsed)
  if (!result.success) return

  const settings = result.data
  if (typeof settings.copilotToken === 'string') {
    state.copilotToken = settings.copilotToken.trim() ? settings.copilotToken : null
  }

  const cp = state.providers.find((p) => p.id === 'copilot')
  if (cp) cp.available = !!state.copilotToken

  if (typeof settings.devMode === 'boolean') state.devMode = settings.devMode
  if (settings.primaryProvider) state.primaryProvider = settings.primaryProvider
  if (settings.iconStyle) state.iconStyle = settings.iconStyle
  if (
    typeof settings.pollIntervalMs === 'number' &&
    POLL_INTERVALS.some((x) => x.ms === settings.pollIntervalMs)
  ) {
    state.pollIntervalMs = settings.pollIntervalMs
  }

  if (settings.enabledProviderIds) {
    const enabledProviderIds = new Set(settings.enabledProviderIds)
    for (const provider of state.providers) {
      provider.enabled = provider.available && enabledProviderIds.has(provider.id)
    }
  } else if (state.copilotToken && cp) {
    cp.enabled = cp.available
  }
  normalizePrimaryProvider()
}

function shouldRewriteSettings(parsed: unknown): boolean {
  const result = SettingsSchema.safeParse(parsed)
  if (!result.success) return false

  const settings = result.data
  return (
    settings.settingsVersion !== SETTINGS_VERSION ||
    !settings.enabledProviderIds
  )
}

async function loadPersistedSettings(): Promise<void> {
  // 1. shared file wins if present
  try {
    const raw = await fs.readFile(settingsFile(), 'utf8')
    const parsed = JSON.parse(raw)
    applyParsed(parsed)
    if (shouldRewriteSettings(parsed)) await persistSettings()
    return
  } catch {
    // not present, try legacy
  }
  // 2. legacy migration: old shared settings dir or old mockup file
  for (const legacyPath of legacySettingsFiles()) {
    try {
      const raw = await fs.readFile(legacyPath, 'utf8')
      const parsed = JSON.parse(raw)
      if (parsed.copilotTokenEnc && safeStorage.isEncryptionAvailable()) {
        try {
          state.copilotToken = safeStorage.decryptString(
            Buffer.from(parsed.copilotTokenEnc, 'base64')
          )
        } catch {
          state.copilotToken = null
        }
      }
      applyParsed(parsed)
      await persistSettings()
      return
    } catch {
      // try next legacy location
    }
  }
}

async function persistSettings(): Promise<void> {
  const payload: PersistShape = {
    settingsVersion: SETTINGS_VERSION,
    devMode: state.devMode,
    primaryProvider: state.primaryProvider,
    enabledProviderIds: state.providers.filter((p) => p.enabled).map((p) => p.id),
    iconStyle: state.iconStyle,
    pollIntervalMs: state.pollIntervalMs,
  }
  if (state.copilotToken) payload.copilotToken = state.copilotToken
  const serialized = SettingsSchema.parse(payload)
  await fs.mkdir(SHARED_SETTINGS_DIR, { recursive: true })
  const tmp = settingsFile() + '.tmp'
  await fs.writeFile(tmp, JSON.stringify(serialized, null, 2), 'utf8')
  await fs.rename(tmp, settingsFile())
}

interface FetchResult {
  session?: ProviderSnapshot['session']
  weekly?: ProviderSnapshot['weekly']
  planType?: string
  error: SnapshotError | null
}

let claudeRetryAt: number | null = null
let claudeRefreshInFlight = false
let claudeLastLaunchAt = 0
let codexRefreshInFlight = false
let codexLastLaunchAt = 0

interface ClaudeCredsSnapshot {
  accessToken: string | null
  expiresAt: number | null
}

interface CodexCredsSnapshot {
  accessToken: string | null
  refreshToken: string | null
}

function snapshotError(
  message: string,
  kind: ErrorKind,
  detail?: string,
  retryAt?: string
): SnapshotError {
  return {
    message,
    kind,
    ...(detail ? { detail } : {}),
    occurredAt: new Date().toISOString(),
    ...(retryAt ? { retryAt } : {}),
  }
}

function formatDuration(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const seconds = total % 60
  if (hours > 0) return `${hours}h ${minutes}m`
  if (minutes > 0) return `${minutes}m ${seconds}s`
  return `${seconds}s`
}

function retryAtFromHeaders(headers: Headers): number | null {
  const value = headers.get('retry-after')
  if (!value) return null
  const seconds = Number(value)
  if (Number.isFinite(seconds)) return Date.now() + Math.max(0, seconds) * 1000
  const parsed = Date.parse(value)
  return Number.isNaN(parsed) ? null : parsed
}

function truncate(input: string, maxChars: number): string {
  return input.length > maxChars ? `${input.slice(0, maxChars)}...` : input
}

function anthropicErrorSummary(body: string): string | null {
  try {
    const parsed = JSON.parse(body)
    const err = parsed && parsed.error
    const type = typeof err?.type === 'string' ? err.type : null
    const message = typeof err?.message === 'string' ? err.message : null
    if (type && message) return `${type}: ${message}`
    return type || message
  } catch {
    return null
  }
}

function httpErrorDetail(status: number, body: string, retryAt: number | null): string {
  const parts = [`HTTP ${status}`]
  const summary = anthropicErrorSummary(body)
  if (summary) parts.push(summary)
  else if (body.trim()) parts.push(`Body: ${truncate(body.trim(), 600)}`)
  if (retryAt) {
    parts.push(
      `Retry-After: ${formatDuration(retryAt - Date.now())} (${new Date(retryAt).toISOString()})`
    )
  }
  return parts.join('\n')
}

async function readClaudeCredsSnapshot(): Promise<ClaudeCredsSnapshot | null> {
  try {
    const raw = await fs.readFile(CLAUDE_CREDS, 'utf8')
    const parsed = JSON.parse(raw)
    const oauth = parsed && parsed.claudeAiOauth
    return {
      accessToken:
        oauth && typeof oauth.accessToken === 'string' && oauth.accessToken
          ? oauth.accessToken
          : null,
      expiresAt:
        oauth && typeof oauth.expiresAt === 'number' ? oauth.expiresAt : null,
    }
  } catch {
    return null
  }
}

function freshClaudeCreds(snapshot: ClaudeCredsSnapshot): boolean {
  return !!snapshot.accessToken && !!snapshot.expiresAt && snapshot.expiresAt > Date.now() + 60_000
}

function claudeCredsAdvanced(
  before: ClaudeCredsSnapshot | null,
  after: ClaudeCredsSnapshot
): boolean {
  if (!before) return freshClaudeCreds(after)
  return (
    after.accessToken !== before.accessToken ||
    (after.expiresAt || 0) > (before.expiresAt || 0)
  )
}

async function readCodexCredsSnapshot(): Promise<CodexCredsSnapshot | null> {
  try {
    const raw = await fs.readFile(CODEX_CREDS, 'utf8')
    const parsed = JSON.parse(raw)
    const tokens = parsed && parsed.tokens
    return {
      accessToken:
        tokens && typeof tokens.access_token === 'string' && tokens.access_token
          ? tokens.access_token
          : null,
      refreshToken:
        tokens && typeof tokens.refresh_token === 'string' && tokens.refresh_token
          ? tokens.refresh_token
          : null,
    }
  } catch {
    return null
  }
}

function codexCredsAdvanced(before: CodexCredsSnapshot | null, after: CodexCredsSnapshot): boolean {
  if (!after.accessToken) return false
  if (!before) return true
  return after.accessToken !== before.accessToken
}

function launchHiddenClaude(): ChildProcess {
  let child: ChildProcess
  if (process.platform === 'win32') {
    child = spawn('claude', [], { stdio: 'ignore', windowsHide: true })
  } else if (process.platform === 'darwin') {
    child = spawn('claude', [], { stdio: 'ignore' })
  } else {
    child = spawn('claude', [], { stdio: 'ignore' })
  }
  return child
}

function launchHiddenCodexRefresh(): ChildProcess {
  if (process.platform === 'win32') {
    return spawn(process.env.ComSpec || 'cmd.exe', ['/d', '/s', '/c', 'codex debug models'], {
      stdio: 'ignore',
      windowsHide: true,
    })
  }
  return spawn('codex', ['debug', 'models'], { stdio: 'ignore' })
}

function stopClaudeChild(child: ChildProcess | null): void {
  if (!child || child.killed) return
  try {
    child.kill()
  } catch {}
}

function stopCodexChild(child: ChildProcess | null): void {
  if (!child || child.killed) return
  try {
    child.kill()
  } catch {}
}

function setClaudeRefreshError(message: string, detail: string): void {
  const snap = state.snapshots.claude
  snap.error = snapshotError(message, 'token-expired', detail)
  if (state.primaryProvider === 'claude') {
    state.session = snap.session
    state.weekly = snap.weekly
    state.error = snap.error
  }
  broadcast()
}

function setCodexRefreshError(message: string, detail: string): void {
  const snap = state.snapshots.codex
  snap.error = snapshotError(message, 'token-expired', detail)
  if (state.primaryProvider === 'codex') {
    state.session = snap.session
    state.weekly = snap.weekly
    state.error = snap.error
  }
  broadcast()
}

async function refreshClaudeAfterTokenExpired(): Promise<void> {
  if (claudeRefreshInFlight) {
    setClaudeRefreshError(
      'Claude token expired. Waiting for hidden `claude` to refresh credentials.',
      'A hidden Claude refresh is already running. This app is polling ~/.claude/.credentials.json and will refresh usage automatically once the token changes.'
    )
    return
  }

  claudeRefreshInFlight = true
  const before = await readClaudeCredsSnapshot()
  let child: ChildProcess | null = null
  let spawnError: Error | null = null
  try {
    const now = Date.now()
    if (now - claudeLastLaunchAt < 2 * 60_000) {
      setClaudeRefreshError(
        'Claude token expired. Waiting for hidden `claude` to refresh credentials.',
        'A hidden Claude refresh was launched recently. This app is polling ~/.claude/.credentials.json and will refresh usage automatically once the token changes.'
      )
    } else {
      claudeLastLaunchAt = now
      child = launchHiddenClaude()
      child.once('error', (err) => {
        spawnError = err
      })
      setClaudeRefreshError(
        'Claude token expired. Launched hidden `claude`; waiting for refreshed credentials.',
        'This app is polling ~/.claude/.credentials.json and will refresh usage automatically once the token changes.'
      )
    }

    const deadline = Date.now() + 20_000
    while (Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 2000))
      if (spawnError) {
        throw spawnError
      }
      const after = await readClaudeCredsSnapshot()
      if (after && freshClaudeCreds(after) && claudeCredsAdvanced(before, after)) {
        claudeRefreshInFlight = false
        stopClaudeChild(child)
        void realFetchAll()
        return
      }
    }

    setClaudeRefreshError(
      'Claude token expired. Hidden `claude` did not refresh credentials within 20 seconds.',
      'Use Refresh now after Claude finishes updating ~/.claude/.credentials.json, or wait for the next scheduled poll.'
    )
  } catch (e: any) {
    setClaudeRefreshError(
      'Claude token expired. Could not launch hidden `claude`.',
      e && e.message ? e.message : String(e)
    )
  } finally {
    stopClaudeChild(child)
    claudeRefreshInFlight = false
  }
}

async function refreshCodexAfterTokenExpired(): Promise<void> {
  if (codexRefreshInFlight) {
    setCodexRefreshError(
      'Codex token expired. Waiting for hidden `codex debug models` to refresh credentials.',
      'A hidden Codex refresh is already running. This app is polling ~/.codex/auth.json and will refresh usage automatically once the token changes.'
    )
    return
  }

  codexRefreshInFlight = true
  const before = await readCodexCredsSnapshot()
  let child: ChildProcess | null = null
  let spawnError: Error | null = null
  let childExit: { code: number | null; signal: NodeJS.Signals | null } | null = null
  try {
    if (!before?.refreshToken) {
      setCodexRefreshError(
        'Codex token expired. Run `codex` interactively.',
        'No Codex refresh token was found in ~/.codex/auth.json, so the app cannot refresh credentials in the background.'
      )
      return
    }

    const now = Date.now()
    if (now - codexLastLaunchAt < 2 * 60_000) {
      setCodexRefreshError(
        'Codex token expired. Waiting for hidden `codex debug models` to refresh credentials.',
        'A hidden Codex refresh was launched recently. This app is polling ~/.codex/auth.json and will refresh usage automatically once the token changes.'
      )
    } else {
      codexLastLaunchAt = now
      child = launchHiddenCodexRefresh()
      child.once('error', (err) => {
        spawnError = err
      })
      child.once('exit', (code, signal) => {
        childExit = { code, signal }
      })
      setCodexRefreshError(
        'Codex token expired. Launched hidden `codex debug models`; waiting for refreshed credentials.',
        'Bare `codex` requires a terminal, so this app runs `codex debug models` hidden and polls ~/.codex/auth.json for a refreshed access token.'
      )
    }

    const deadline = Date.now() + 20_000
    while (Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 2000))
      if (spawnError) {
        throw spawnError
      }
      const after = await readCodexCredsSnapshot()
      if (after && codexCredsAdvanced(before, after)) {
        codexRefreshInFlight = false
        stopCodexChild(child)
        void realFetchAll()
        return
      }
      if (childExit && childExit.code !== 0) {
        throw new Error(
          `codex debug models exited with ${childExit.code ?? `signal ${childExit.signal}`}`
        )
      }
    }

    setCodexRefreshError(
      'Codex token expired. Hidden `codex debug models` did not refresh credentials within 20 seconds.',
      'Run Codex interactively if it asks for an update or login, then use Refresh now or wait for the next scheduled poll.'
    )
  } catch (e: any) {
    setCodexRefreshError(
      'Codex token expired. Could not refresh with hidden `codex debug models`.',
      e && e.message ? e.message : String(e)
    )
  } finally {
    stopCodexChild(child)
    codexRefreshInFlight = false
  }
}

async function fetchClaudeUsage(): Promise<FetchResult> {
  if (claudeRetryAt && claudeRetryAt > Date.now()) {
    const wait = formatDuration(claudeRetryAt - Date.now())
    return {
      error: snapshotError(
        `Claude usage API is rate limited. Retrying in ${wait}.`,
        'rate-limited',
        'Anthropic previously returned HTTP 429; usage requests are paused until the Retry-After window expires.',
        new Date(claudeRetryAt).toISOString()
      ),
    }
  }
  if (claudeRetryAt && claudeRetryAt <= Date.now()) claudeRetryAt = null

  let creds: any
  try {
    const raw = await fs.readFile(CLAUDE_CREDS, 'utf8')
    creds = JSON.parse(raw)
  } catch {
    return {
      error: snapshotError('Claude not logged in. Run `claude`.', 'not-authenticated'),
    }
  }
  const token = creds.claudeAiOauth && creds.claudeAiOauth.accessToken
  if (!token) {
    return {
      error: snapshotError('Claude credentials missing accessToken', 'bad-credentials'),
    }
  }
  try {
    const resp = await fetch('https://api.anthropic.com/api/oauth/usage', {
      method: 'GET',
      headers: {
        Authorization: `Bearer ${token}`,
        'anthropic-beta': 'oauth-2025-04-20',
        'User-Agent': 'claude-code/2.1.0',
        Accept: 'application/json',
      },
    })
    if (resp.status === 401) {
      claudeRetryAt = null
      return {
        error: snapshotError('Claude token expired. Run `claude`.', 'token-expired'),
      }
    }
    if (!resp.ok) {
      const retryAt =
        resp.status === 429 ? retryAtFromHeaders(resp.headers) ?? Date.now() + 5 * 60_000 : null
      const body = await resp.text().catch((e: any) => `Unable to read response body: ${e.message}`)
      const detail = httpErrorDetail(resp.status, body, retryAt)
      if (resp.status === 429) {
        claudeRetryAt = retryAt
        const retryAtIso = retryAt ? new Date(retryAt).toISOString() : undefined
        return {
          error: snapshotError(
            retryAt
              ? `Claude usage API is rate limited. Try again in ${formatDuration(retryAt - Date.now())}.`
              : 'Claude usage API is rate limited. Try again later.',
            'rate-limited',
            detail,
            retryAtIso
          ),
        }
      }
      return { error: snapshotError(`Claude HTTP ${resp.status}`, 'server-error', detail) }
    }
    const body = await resp.text()
    claudeRetryAt = null
    let data: any
    try {
      data = JSON.parse(body)
    } catch (e: any) {
      return {
        error: snapshotError(
          `Claude parse: ${e.message}`,
          'server-error',
          `Body: ${truncate(body.trim(), 600)}`
        ),
      }
    }
    const fh = data.five_hour
    const sd = data.seven_day
    return {
      session: {
        usedPct: fh && typeof fh.utilization === 'number' ? fh.utilization : null,
        resetsAt: (fh && fh.resets_at) || null,
      },
      weekly: {
        usedPct: sd && typeof sd.utilization === 'number' ? sd.utilization : null,
        resetsAt: (sd && sd.resets_at) || null,
      },
      error: null,
    }
  } catch (e: any) {
    return { error: snapshotError(`Claude network: ${e.message}`, 'network-error') }
  }
}

async function fetchCopilotUsage(): Promise<FetchResult> {
  if (!state.copilotToken) {
    return {
      error: {
        message: 'No GitHub token. Open Settings → Copilot.',
        kind: 'not-authenticated',
      },
    }
  }
  try {
    const resp = await fetch('https://api.github.com/copilot_internal/user', {
      method: 'GET',
      headers: {
        Authorization: `token ${state.copilotToken}`,
        'Editor-Version': 'vscode/1.96.2',
        'Editor-Plugin-Version': 'copilot-chat/0.26.7',
        'User-Agent': 'GitHubCopilotChat/0.26.7',
        'X-Github-Api-Version': '2025-04-01',
        Accept: 'application/json',
      },
    })
    if (resp.status === 401 || resp.status === 403) {
      return {
        error: {
          message: 'GitHub token rejected. Needs Copilot access.',
          kind: 'token-expired',
        },
      }
    }
    if (!resp.ok) {
      return { error: { message: `Copilot HTTP ${resp.status}`, kind: 'server-error' } }
    }
    const data: any = await resp.json()
    const snaps = data.quota_snapshots || data.quotaSnapshots || {}
    const premium = snaps.premium_interactions || snaps.premiumInteractions
    const readPct = (q: any): number | null => {
      if (!q) return null
      const pr =
        typeof q.percent_remaining === 'number'
          ? q.percent_remaining
          : typeof q.percentRemaining === 'number'
            ? q.percentRemaining
            : null
      if (pr != null) return Math.max(0, Math.min(100, 100 - pr))
      if (typeof q.used_percent === 'number') return q.used_percent
      if (typeof q.usedPercent === 'number') return q.usedPercent
      return null
    }
    const monthlyReset =
      data.quota_reset_date_utc ||
      (data.quota_reset_date ? `${data.quota_reset_date}T00:00:00Z` : null)
    const entitlement =
      premium && typeof premium.entitlement === 'number' ? premium.entitlement : null
    const overage =
      premium && typeof premium.overage_count === 'number' ? premium.overage_count : 0
    const remaining =
      premium && typeof premium.remaining === 'number'
        ? premium.remaining
        : premium && typeof premium.quota_remaining === 'number'
          ? Math.round(premium.quota_remaining)
          : premium && typeof premium.quotaRemaining === 'number'
            ? Math.round(premium.quotaRemaining)
            : null
    // Consumed = (entitlement - remaining) + overage. Falls back to the old
    // entitlement + overage when `remaining` is absent (pre-2026-06 payloads).
    const usedAbs =
      entitlement != null
        ? (remaining != null ? Math.max(0, entitlement - remaining) : entitlement) + overage
        : null
    return {
      session: { usedPct: null, resetsAt: null },
      weekly: {
        usedPct: readPct(premium),
        resetsAt: monthlyReset,
        usedAbs,
        entitlement,
        overage,
      },
      planType: data.copilot_plan || data.copilotPlan,
      error: null,
    }
  } catch (e: any) {
    return { error: { message: `Copilot network: ${e.message}`, kind: 'network-error' } }
  }
}

async function fetchCodexUsage(): Promise<FetchResult> {
  let creds: any
  try {
    const raw = await fs.readFile(CODEX_CREDS, 'utf8')
    creds = JSON.parse(raw)
  } catch {
    return {
      error: { message: 'Codex not logged in. Run `codex`.', kind: 'not-authenticated' },
    }
  }
  const token = (creds.tokens && creds.tokens.access_token) || creds.OPENAI_API_KEY
  if (!token) {
    return { error: { message: 'Codex credentials missing token', kind: 'bad-credentials' } }
  }
  const accountId = creds.tokens && creds.tokens.account_id
  try {
    const headers: Record<string, string> = {
      Authorization: `Bearer ${token}`,
        'User-Agent': 'ai-plan-usage-electron',
      Accept: 'application/json',
    }
    if (accountId) headers['ChatGPT-Account-Id'] = accountId

    const resp = await fetch('https://chatgpt.com/backend-api/wham/usage', {
      method: 'GET',
      headers,
    })
    if (resp.status === 401) {
      return {
        error: { message: 'Codex token expired. Run `codex`.', kind: 'token-expired' },
      }
    }
    if (!resp.ok) {
      return { error: { message: `Codex HTTP ${resp.status}`, kind: 'server-error' } }
    }
    const data: any = await resp.json()
    const p = data.rate_limit && data.rate_limit.primary_window
    const s = data.rate_limit && data.rate_limit.secondary_window
    return {
      session: {
        usedPct: p && typeof p.used_percent === 'number' ? p.used_percent : null,
        resetsAt:
          p && typeof p.reset_at === 'number'
            ? new Date(p.reset_at * 1000).toISOString()
            : null,
      },
      weekly: {
        usedPct: s && typeof s.used_percent === 'number' ? s.used_percent : null,
        resetsAt:
          s && typeof s.reset_at === 'number'
            ? new Date(s.reset_at * 1000).toISOString()
            : null,
      },
      error: null,
    }
  } catch (e: any) {
    return { error: { message: `Codex network: ${e.message}`, kind: 'network-error' } }
  }
}

async function realFetchAll(): Promise<void> {
  if (fetchInFlight) return
  fetchInFlight = true
  try {
    const jobs: Array<[ProviderId, Promise<FetchResult>]> = []
    const enabled = (id: ProviderId): boolean =>
      state.providers.some((p) => p.id === id && p.enabled && p.available)

    if (enabled('claude')) jobs.push(['claude', fetchClaudeUsage()])
    if (enabled('codex')) jobs.push(['codex', fetchCodexUsage()])
    if (enabled('copilot')) jobs.push(['copilot', fetchCopilotUsage()])

    const results = await Promise.all(
      jobs.map(async ([id, promise]) => {
        try {
          return [id, await promise] as const
        } catch (reason) {
          return [
            id,
            { error: { message: String(reason), kind: 'unknown' } },
          ] as const
        }
      })
    )

    const claudeTokenExpired = results.some(
      ([id, result]) => id === 'claude' && result.error?.kind === 'token-expired'
    )
    const codexTokenExpired = results.some(
      ([id, result]) => id === 'codex' && result.error?.kind === 'token-expired'
    )

    for (const [id, result] of results) {
      applyResult(id, result)
    }

    const primary = state.snapshots[state.primaryProvider]
    if (primary) {
      state.session = primary.session
      state.weekly = primary.weekly
      state.error = primary.error
    }
    state.fetchedAt = new Date().toISOString()
    broadcast()

    if (claudeTokenExpired) void refreshClaudeAfterTokenExpired()
    if (codexTokenExpired) void refreshCodexAfterTokenExpired()
  } finally {
    fetchInFlight = false
  }
}

function applyResult(id: ProviderId, result: FetchResult): void {
  const snap = state.snapshots[id]
  if (!snap) return
  if (result.error) {
    snap.error = result.error
  } else {
    if (result.session) snap.session = result.session
    if (result.weekly) snap.weekly = result.weekly
    if (result.planType) snap.planType = result.planType
    snap.error = null
  }
}

let tray: Tray | null = null
let popover: BrowserWindow | null = null
let settingsWin: BrowserWindow | null = null
let iconRenderer: BrowserWindow | null = null
let iconRendererReady: Promise<void> | null = null

const APP_ASSET_DIR = path.join(__dirname, 'assets')
const STATUS_ICON_DIR = path.join(UI_DIST_DIR, 'assets')
const APP_ICON_PATH = path.join(APP_ASSET_DIR, 'app-icon.png')
const ICON_STYLES: IconStyle[] = [...ICON_STYLE_IDS]
const POLL_INTERVALS: PollIntervalOption[] = [
  { label: '30 seconds (dev)', ms: 30_000 },
  { label: '1 minute', ms: 60_000 },
  { label: '5 minutes', ms: 5 * 60_000 },
  { label: '15 minutes', ms: 15 * 60_000 },
  { label: '30 minutes', ms: 30 * 60_000 },
]

const state: State = {
  fetchedAt: new Date().toISOString(),
  iconStyle: 'ring+number',
  session: {
    usedPct: 47,
    resetsAt: new Date(Date.now() + 3 * 3600_000 + 53 * 60_000).toISOString(),
  },
  weekly: {
    usedPct: 12,
    resetsAt: new Date(Date.now() + 3 * 86_400_000 + 20 * 3600_000).toISOString(),
  },
  error: null,
  providers: [
    { id: 'claude', label: 'Claude', enabled: true, available: true },
    { id: 'codex', label: 'Codex', enabled: true, available: true },
    { id: 'copilot', label: 'Copilot', enabled: false, available: false },
  ],
  primaryProvider: 'claude',
  pollIntervalMs: 5 * 60_000,
  dataSource: 'oauth-api',
  copilotToken: null,
  devMode: false,
  snapshots: {
    claude: {
      session: { usedPct: null, resetsAt: null },
      weekly: { usedPct: null, resetsAt: null },
      error: null,
    },
    codex: {
      session: { usedPct: null, resetsAt: null },
      weekly: { usedPct: null, resetsAt: null },
      error: null,
    },
    copilot: {
      session: { usedPct: null, resetsAt: null },
      weekly: { usedPct: null, resetsAt: null },
      error: null,
    },
  },
}

function publicState(): PublicState {
  const { copilotToken, ...rest } = state
  return rest
}

const HEX_GREEN = '#22a06b'
const HEX_AMBER = '#d97706'
const HEX_RED = '#dc2626'
const HEX_GRAY = '#888888'

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
  usedPct: number | null,
  resetsAtIso: string | null | undefined,
  totalWindowMs: number,
  params: ColorParams
): string {
  if (usedPct == null) return HEX_GRAY
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

function iconPayload(): IconPayload {
  // If primary has no session window (e.g. Copilot is monthly-only), mirror weekly into session
  // so number + ring both display the single available metric.
  let sess = state.session
  const week = state.weekly
  if ((sess?.usedPct ?? null) == null && (week?.usedPct ?? null) != null) {
    sess = week
  }
  // Window lengths per provider. Codex assumed same as Claude (API does not expose length).
  // Copilot uses overage signal, not these thresholds; defaults are safe fallback.
  const sessionTotalMs = SESSION_5H_MS
  const weeklyTotalMs = WEEKLY_7D_MS
  return {
    error: state.error,
    session: {
      pct: sess?.usedPct ?? null,
      color: colorForWindow(sess?.usedPct ?? null, sess?.resetsAt, sessionTotalMs, PARAMS_SESSION),
    },
    weekly: {
      pct: week?.usedPct ?? null,
      color: colorForWindow(week?.usedPct ?? null, week?.resetsAt, weeklyTotalMs, PARAMS_WEEKLY),
    },
  }
}

function fallbackIconPath(): string {
  if (state.error) return path.join(STATUS_ICON_DIR, 'icon-error.png')
  const sess = state.session.usedPct ?? 0
  const week = state.weekly.usedPct ?? 0
  const max = Math.max(sess, week)
  if (max >= 95) return path.join(STATUS_ICON_DIR, 'icon-red.png')
  if (max >= 80) return path.join(STATUS_ICON_DIR, 'icon-orange.png')
  return path.join(STATUS_ICON_DIR, 'icon-green.png')
}

function createIconRenderer(): void {
  iconRenderer = new BrowserWindow({
    width: 64,
    height: 64,
    show: false,
    frame: false,
    transparent: true,
    skipTaskbar: true,
    focusable: false,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      backgroundThrottling: false,
    },
  })
  iconRendererReady = new Promise<void>((resolve) => {
    iconRenderer!.webContents.once('did-finish-load', () => resolve())
  })
  iconRenderer.loadFile(path.join(UI_DIST_DIR, 'icon-renderer.html'))
}

async function renderDynamicIcon(): Promise<Electron.NativeImage | null> {
  if (!iconRenderer || !iconRendererReady) return null
  await iconRendererReady
  const style = state.iconStyle
  const payload = iconPayload()
  const escapedStyle = JSON.stringify(style)
  const escapedPayload = JSON.stringify(payload)
  try {
    const dataUrl: string = await iconRenderer.webContents.executeJavaScript(
      `renderIcon(${escapedStyle}, ${escapedPayload})`
    )
    if (typeof dataUrl !== 'string' || !dataUrl.startsWith('data:image/png')) return null
    return nativeImage.createFromDataURL(dataUrl)
  } catch (err) {
    console.error('icon render failed:', err)
    return null
  }
}

async function refreshTray(): Promise<void> {
  if (!tray) return
  const dynImg = await renderDynamicIcon()
  if (dynImg && !dynImg.isEmpty()) {
    tray.setImage(dynImg)
  } else {
    tray.setImage(fallbackIconPath())
  }
  const sess = state.session && state.session.usedPct
  const week = state.weekly && state.weekly.usedPct
  tray.setToolTip(
    `Session ${sess == null ? '—' : Math.round(sess) + '%'} · Weekly ${
      week == null ? '—' : Math.round(week) + '%'
    }`
  )
}

function broadcast(): void {
  void refreshTray()
  if (popover && !popover.isDestroyed()) {
    popover.webContents.send('snapshot:updated', publicState())
  }
}

function setIconStyle(style: IconStyle): void {
  if (!ICON_STYLES.includes(style)) return
  state.iconStyle = style
  void refreshTray()
}

let tickerHandle: NodeJS.Timeout | null = null
let fetchInFlight = false
function restartTicker(): void {
  if (tickerHandle) clearInterval(tickerHandle)
  tickerHandle = setInterval(() => {
    void realFetchAll()
  }, state.pollIntervalMs)
}

function positionPopover(): void {
  if (!tray || !popover) return
  const trayBounds = tray.getBounds()
  const winBounds = popover.getBounds()
  const display = screen.getDisplayMatching(trayBounds)
  const taskbarOnBottom =
    trayBounds.y > display.workArea.y + display.workArea.height / 2

  const xCentered = Math.round(
    trayBounds.x + trayBounds.width / 2 - winBounds.width / 2
  )
  const x = Math.min(
    Math.max(xCentered, display.workArea.x + 8),
    display.workArea.x + display.workArea.width - winBounds.width - 8
  )
  const y = taskbarOnBottom
    ? trayBounds.y - winBounds.height - 8
    : trayBounds.y + trayBounds.height + 8

  popover.setBounds({ x, y, width: winBounds.width, height: winBounds.height })
}

function createPopover(): void {
  popover = new BrowserWindow({
    width: 280,
    height: 180,
    useContentSize: true,
    show: false,
    frame: false,
    resizable: false,
    movable: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    focusable: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  })
  popover.loadFile(path.join(UI_DIST_DIR, 'popover.html'))
  popover.on('blur', () => {
    if (settingsWin && settingsWin.isFocused()) return
    popover!.hide()
  })
}

function togglePopover(): void {
  if (!popover || popover.isDestroyed()) createPopover()
  if (!popover) return
  if (popover.isVisible()) {
    popover.hide()
    return
  }
  positionPopover()
  popover.show()
  popover.focus()
}

function openSettings(): void {
  if (settingsWin && !settingsWin.isDestroyed()) {
    settingsWin.focus()
    return
  }
  settingsWin = new BrowserWindow({
    width: 520,
    height: 640,
    icon: APP_ICON_PATH,
    resizable: false,
    minimizable: false,
    maximizable: false,
    title: 'ai-plan-usage settings',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  })
  settingsWin.setMenuBarVisibility(false)
  settingsWin.loadFile(path.join(UI_DIST_DIR, 'settings.html'))
}

type DevProfile =
  | 'low'
  | 'mid'
  | 'high'
  | 'reset'
  | 'stale'
  | 'error'
  | 'clear-error'

function setDevState(profile: DevProfile): void {
  const all = Object.values(state.snapshots)
  switch (profile) {
    case 'low':
      all.forEach((s) => {
        s.session.usedPct = 45
        s.weekly.usedPct = 12
      })
      break
    case 'mid':
      all.forEach((s) => {
        s.session.usedPct = 85
        s.weekly.usedPct = 50
      })
      break
    case 'high':
      all.forEach((s) => {
        s.session.usedPct = 97
        s.weekly.usedPct = 78
      })
      break
    case 'reset':
      all.forEach((s) => {
        s.session.usedPct = 2
        s.weekly.usedPct = 3
      })
      break
    case 'stale':
      state.fetchedAt = new Date(Date.now() - 22 * 60_000).toISOString()
      break
    case 'error':
      all.forEach((s) => {
        s.error = snapshotError(
          'Not authenticated',
          'not-authenticated',
          'This is the simulated dev error from the tray menu.'
        )
      })
      state.error = all[0].error
      break
    case 'clear-error':
      all.forEach((s) => {
        s.error = null
      })
      state.error = null
      break
  }
  const primary = state.snapshots[state.primaryProvider]
  if (primary) {
    state.session = primary.session
    state.weekly = primary.weekly
    state.error = primary.error
  }
  if (profile !== 'stale') state.fetchedAt = new Date().toISOString()
  broadcast()
}

function buildContextMenu(): Menu {
  const items: Electron.MenuItemConstructorOptions[] = [
    { label: 'Refresh now', click: () => { void realFetchAll() } },
    { label: 'Settings…', click: openSettings },
  ]
  if (state.devMode) {
    items.push(
      { type: 'separator' },
      {
        label: 'Dev: simulate state',
        submenu: [
          { label: 'Low (45 / 12)', click: () => setDevState('low') },
          { label: 'Mid (85 / 50)', click: () => setDevState('mid') },
          { label: 'High (97 / 78)', click: () => setDevState('high') },
          { label: 'Reset (2 / 3)', click: () => setDevState('reset') },
          { type: 'separator' },
          { label: 'Force stale (22m old)', click: () => setDevState('stale') },
          { label: 'Force error', click: () => setDevState('error') },
          { label: 'Clear error', click: () => setDevState('clear-error') },
        ],
      },
      {
        label: `Dev: icon style (now: ${state.iconStyle})`,
        submenu: ICON_STYLES.map((s) => ({
          label: s,
          type: 'radio' as const,
          checked: state.iconStyle === s,
          click: () => setIconStyle(s),
        })),
      }
    )
  }
  items.push({ type: 'separator' }, { label: 'Quit', click: () => app.quit() })
  return Menu.buildFromTemplate(items)
}

const gotLock = app.requestSingleInstanceLock()
if (!gotLock) {
  app.quit()
} else {
  if (process.platform === 'win32') app.setAppUserModelId(APP_ID)

  app.on('second-instance', () => togglePopover())

  app.whenReady().then(async () => {
    await loadPersistedSettings()
    createIconRenderer()
    tray = new Tray(fallbackIconPath())
    tray.setToolTip('ai-plan-usage')
    tray.on('click', togglePopover)
    tray.on('right-click', () => tray!.popUpContextMenu(buildContextMenu()))

    createPopover()

    await refreshTray()
    void realFetchAll()
    restartTicker()
  })

  app.on('window-all-closed', () => {
    // keep app alive in tray; do nothing
  })

  ipcMain.handle('snapshot:get', () => publicState())
  ipcMain.handle('app:quit', () => app.quit())
  ipcMain.handle(
    'settings:get-autostart',
    () => app.getLoginItemSettings().openAtLogin
  )
  ipcMain.handle('settings:set-autostart', (_e, on: boolean) => {
    app.setLoginItemSettings({
      openAtLogin: !!on,
      args: ['--hidden'],
    })
    return app.getLoginItemSettings().openAtLogin
  })

  ipcMain.handle('settings:get-all', () => ({
    providers: state.providers,
    primaryProvider: state.primaryProvider,
    pollIntervalMs: state.pollIntervalMs,
    iconStyle: state.iconStyle,
    dataSource: state.dataSource,
    intervals: POLL_INTERVALS,
    iconStyles: ICON_STYLES,
    devMode: state.devMode,
  }))

  function mirrorPrimary(): void {
    const primary = state.snapshots[state.primaryProvider]
    if (primary) {
      state.session = primary.session
      state.weekly = primary.weekly
      state.error = primary.error
    }
  }

  ipcMain.handle('settings:set-provider-enabled', (_e, id: ProviderId, enabled: boolean) => {
    const p = state.providers.find((x) => x.id === id)
    if (p && p.available) p.enabled = !!enabled
    const primary = state.providers.find((x) => x.id === state.primaryProvider)
    if (!primary || !primary.enabled) {
      const first = state.providers.find((x) => x.enabled && x.available)
      if (first) state.primaryProvider = first.id
    }
    mirrorPrimary()
    void persistSettings()
    broadcast()
    return { primaryProvider: state.primaryProvider, providers: state.providers }
  })

  ipcMain.handle('settings:set-primary', (_e, id: ProviderId) => {
    const p = state.providers.find((x) => x.id === id)
    if (p && p.enabled && p.available) {
      state.primaryProvider = id
      mirrorPrimary()
      void persistSettings()
      broadcast()
    }
    return state.primaryProvider
  })

  ipcMain.handle('settings:set-interval', (_e, ms: number) => {
    if (POLL_INTERVALS.some((x) => x.ms === ms)) {
      state.pollIntervalMs = ms
      restartTicker()
      void persistSettings()
    }
    return state.pollIntervalMs
  })

  ipcMain.handle('settings:set-icon-style', (_e, style: IconStyle) => {
    if (ICON_STYLES.includes(style)) {
      state.iconStyle = style
      void persistSettings()
      broadcast()
    }
    return state.iconStyle
  })

  ipcMain.handle('settings:set-dev-mode', (_e, on: boolean) => {
    state.devMode = !!on
    void persistSettings()
    return state.devMode
  })

  ipcMain.handle('settings:get-copilot-status', () => ({
    hasToken: !!state.copilotToken,
    encryptionAvailable: safeStorage.isEncryptionAvailable(),
  }))

  ipcMain.handle('settings:set-copilot-token', async (_e, raw: string) => {
    const token = typeof raw === 'string' ? raw.trim() : ''
    state.copilotToken = token || null
    const cp = state.providers.find((p) => p.id === 'copilot')
    if (cp) {
      cp.available = !!state.copilotToken
      if (!cp.available) cp.enabled = false
    }
    const primary = state.providers.find((x) => x.id === state.primaryProvider)
    if (!primary || !primary.enabled || !primary.available) {
      const first = state.providers.find((x) => x.enabled && x.available)
      if (first) state.primaryProvider = first.id
    }
    mirrorPrimary()
    await persistSettings()
    broadcast()
    void realFetchAll()
    return !!state.copilotToken
  })

  ipcMain.handle('popover:resize', (_e, w: number, h: number) => {
    if (!popover || popover.isDestroyed()) return
    const width = Math.max(180, Math.min(640, Math.ceil(w)))
    const height = Math.max(60, Math.min(600, Math.ceil(h)))
    const current = popover.getContentBounds()
    if (current.width === width && current.height === height) return
    popover.setContentSize(width, height)
    if (popover.isVisible()) positionPopover()
  })
}
