// Global ambient types shared by Electron shell and browser-side UI.

declare global {
  type ProviderId = 'claude' | 'codex' | 'copilot'

  type IconStyle = 'solid' | 'number' | 'ring' | 'ring+number' | 'bar'

  type ErrorKind =
    | 'not-authenticated'
    | 'token-expired'
    | 'refresh-failed'
    | 'server-error'
    | 'network-error'
    | 'bad-credentials'
    | 'rate-limited'
    | 'unknown'

  interface UsageBlock {
    usedPct: number | null
    resetsAt: string | null
    usedAbs?: number | null
    entitlement?: number | null
    overage?: number
  }

  interface SnapshotError {
    message: string
    kind: ErrorKind
    detail?: string
    occurredAt?: string
    retryAt?: string
  }

  interface ProviderSnapshot {
    session: UsageBlock
    weekly: UsageBlock
    planType?: string
    error: SnapshotError | null
  }

  interface ProviderEntry {
    id: ProviderId
    label: string
    enabled: boolean
    available: boolean
  }

  interface PollIntervalOption {
    label: string
    ms: number
  }

  interface State {
    fetchedAt: string
    iconStyle: IconStyle
    session: UsageBlock
    weekly: UsageBlock
    error: SnapshotError | null
    providers: ProviderEntry[]
    primaryProvider: ProviderId
    pollIntervalMs: number
    dataSource: string
    copilotToken: string | null
    devMode: boolean
    snapshots: Record<ProviderId, ProviderSnapshot>
  }

  type PublicState = Omit<State, 'copilotToken'>

  interface IconPayload {
    error: SnapshotError | null
    session: { pct: number | null; color: string }
    weekly: { pct: number | null; color: string }
  }

  interface SettingsBundle {
    providers: ProviderEntry[]
    primaryProvider: ProviderId
    pollIntervalMs: number
    iconStyle: IconStyle
    dataSource: string
    intervals: PollIntervalOption[]
    iconStyles: IconStyle[]
    devMode: boolean
  }

  interface CopilotStatus {
    hasToken: boolean
    encryptionAvailable: boolean
  }

  interface MockAPI {
    getSnapshot(): Promise<PublicState>
    quit(): Promise<void>
    getAutostart(): Promise<boolean>
    setAutostart(on: boolean): Promise<boolean>
    getSettings(): Promise<SettingsBundle>
    setProviderEnabled(
      id: ProviderId,
      on: boolean
    ): Promise<{ providers: ProviderEntry[]; primaryProvider: ProviderId }>
    setPrimary(id: ProviderId): Promise<ProviderId>
    setInterval(ms: number): Promise<number>
    setIconStyle(style: IconStyle): Promise<IconStyle>
    getCopilotStatus(): Promise<CopilotStatus>
    setCopilotToken(token: string): Promise<boolean>
    setDevMode(on: boolean): Promise<boolean>
    resize(w: number, h: number): Promise<void>
    onUpdate(cb: (s: PublicState) => void): void
  }

  interface Window {
    mock: MockAPI
    renderIcon?: (style: IconStyle, payload: IconPayload) => string
  }
}

export {}
