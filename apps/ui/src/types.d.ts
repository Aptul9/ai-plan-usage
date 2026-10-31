// Global ambient types for the Tauri webviews.

declare global {
  type Provider = 'claude' | 'codex' | 'copilot'

  type Source = 'meridian' | 'oauth' | 'token' | 'opencode'

  type IconStyle = 'solid' | 'number' | 'ring' | 'ring+number' | 'bar'

  type ErrorKind = 'signed-out' | 'offline' | 'rate-limited' | 'upstream' | 'not-authenticated'

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

  interface Account {
    id: string
    provider: Provider
    source: Source
    label: string
    enabled: boolean
  }

  interface PollIntervalOption {
    label: string
    ms: number
  }

  interface MeridianStatus {
    reachable: boolean
    profiles: number
    checkedAt: string | null
  }

  interface PublicState {
    fetchedAt: string
    iconStyle: IconStyle
    session: UsageBlock
    weekly: UsageBlock
    error: SnapshotError | null
    accounts: Account[]
    primary: string
    pollIntervalMs: number
    devMode: boolean
    snapshots: Record<string, ProviderSnapshot>
    meridian: { url: string; hasApiKey: boolean }
    meridianStatus: MeridianStatus
  }

  interface SettingsBundle extends PublicState {
    intervals: PollIntervalOption[]
    iconStyles: IconStyle[]
  }

  interface OpenCodeLogin {
    id: string
    label: string
  }

  interface MockAPI {
    getSnapshot(): Promise<PublicState>
    quit(): Promise<void>
    openSettings(): Promise<void>
    getAutostart(): Promise<boolean>
    setAutostart(on: boolean): Promise<boolean>
    getSettings(): Promise<SettingsBundle>
    setPrimary(id: string): Promise<string>
    setInterval(ms: number): Promise<number>
    setIconStyle(style: IconStyle): Promise<IconStyle>
    setDevMode(on: boolean): Promise<boolean>
    setAccountEnabled(id: string, on: boolean): Promise<void>
    renameAccount(id: string, label: string): Promise<void>
    moveAccount(id: string, to: number): Promise<void>
    removeAccount(id: string): Promise<void>
    setMeridian(url: string, apiKey: string | null): Promise<void>
    claudeLoginStart(): Promise<string>
    claudeLoginFinish(code: string, label: string): Promise<string>
    codexLoginStart(): Promise<{ userCode: string; url: string; expiresInSecs: number }>
    codexLoginFinish(label: string): Promise<string>
    copilotAddToken(token: string, label: string): Promise<string>
    copilotImportList(): Promise<OpenCodeLogin[]>
    copilotImport(id: string, label: string): Promise<string>
    codexImportList(): Promise<OpenCodeLogin[]>
    codexImport(id: string, label: string): Promise<string>
    resize(w: number, h: number): Promise<void>
    onUpdate(cb: (s: PublicState) => void): void
  }

  interface Window {
    mock: MockAPI
  }
}

export {}
