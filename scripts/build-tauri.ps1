#Requires -Version 5.1
<#
.SYNOPSIS
  Build the Tauri app and copy the release .exe to artifacts/.
.DESCRIPTION
  Workflow:
    1. (optional) cargo clean if -Clean
    2. npm run build in apps/tauri (runs sync-ui.cjs via beforeBuildCommand)
    3. Copy apps/tauri/src-tauri/target/release/*.exe to ../artifacts/

  tauri.conf.json has bundle.active = false, so no MSI/NSIS installer
  is produced. Only the standalone .exe.

.PARAMETER Clean
  Run cargo clean before building. Wipes target/, full recompile follows.

.EXAMPLE
  .\build-tauri.ps1
.EXAMPLE
  .\build-tauri.ps1 -Clean
#>

[CmdletBinding()]
param(
    [switch]$Clean
)

$ErrorActionPreference = 'Stop'

$scriptRoot   = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot     = Split-Path -Parent $scriptRoot
$tauriDir     = Join-Path $repoRoot 'apps\tauri'
$uiDir        = Join-Path $repoRoot 'apps\ui'
$srcTauriDir  = Join-Path $tauriDir 'src-tauri'
$releaseDir   = Join-Path $srcTauriDir 'target\release'
$artifactsDir = Join-Path $repoRoot 'artifacts'
$iconScript   = Join-Path $scriptRoot 'sync-icons.ps1'

if (-not (Test-Path $tauriDir)) {
    throw "Tauri directory not found: $tauriDir"
}
if (-not (Test-Path $uiDir)) {
    throw "Shared UI directory not found: $uiDir"
}
if (-not (Test-Path $srcTauriDir)) {
    throw "src-tauri directory not found: $srcTauriDir"
}

New-Item -ItemType Directory -Force -Path $artifactsDir | Out-Null

Write-Host '[tauri] sync app icons'
& $iconScript

if (-not (Test-Path (Join-Path $uiDir 'node_modules\typescript')) ) {
    Write-Host '[tauri] npm install (shared UI)'
    Push-Location $uiDir
    try {
        & npm install
        if ($LASTEXITCODE -ne 0) {
            throw "ui npm install failed (exit $LASTEXITCODE)"
        }
    }
    finally {
        Pop-Location
    }
}

# Ensure cargo is reachable. Some shells (non-interactive, freshly spawned)
# do not inherit ~/.cargo/bin from the user PATH registry. Prepend it
# transiently when needed so Rust builds launched by the Tauri CLI resolve.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
        $env:PATH = "$cargoBin;$env:PATH"
        Write-Host "[tauri] PATH: prepended $cargoBin"
    } else {
        throw "cargo not found in PATH and $cargoBin\cargo.exe does not exist. Install Rust via rustup."
    }
}
if (-not (Test-Path (Join-Path $tauriDir 'node_modules\@tauri-apps\cli'))) {
    Write-Host '[tauri] npm install (Tauri CLI)'
    Push-Location $tauriDir
    try {
        & npm install
        if ($LASTEXITCODE -ne 0) {
            throw "tauri npm install failed (exit $LASTEXITCODE)"
        }
    }
    finally {
        Pop-Location
    }
}

Push-Location $tauriDir
try {
    if ($Clean) {
        Write-Host '[tauri] cargo clean'
        & cargo clean --manifest-path (Join-Path $srcTauriDir 'Cargo.toml')
        if ($LASTEXITCODE -ne 0) {
            throw "cargo clean failed (exit $LASTEXITCODE)"
        }
    }

    Write-Host '[tauri] npm run build'
    & npm run build
    if ($LASTEXITCODE -ne 0) {
        throw "tauri build failed (exit $LASTEXITCODE)"
    }

    if (-not (Test-Path $releaseDir)) {
        throw "Release directory not found: $releaseDir"
    }

    # Pick the binary matching the crate name; fallback to first .exe.
    $exe = Get-ChildItem -Path $releaseDir -Filter 'ai-plan-usage.exe' -File -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $exe) {
        $exe = Get-ChildItem -Path $releaseDir -Filter '*.exe' -File |
            Where-Object { $_.Name -notmatch '-(build|deps)\.' } |
            Select-Object -First 1
    }
    if (-not $exe) {
        throw "No .exe produced under $releaseDir"
    }

    $dest = Join-Path $artifactsDir $exe.Name
    try {
        Copy-Item -Path $exe.FullName -Destination $dest -Force
    } catch {
        $fallbackName = '{0}-{1}{2}' -f $exe.BaseName, (Get-Date -Format 'yyyyMMdd-HHmmss'), $exe.Extension
        $dest = Join-Path $artifactsDir $fallbackName
        Copy-Item -Path $exe.FullName -Destination $dest -Force
        Write-Warning "Primary artifact path was locked. Mirrored to $dest instead."
    }
    Write-Host "[tauri] OK"
    Write-Host "[tauri]   source : $($exe.FullName)"
    Write-Host "[tauri]   mirror : $dest"
}
finally {
    Pop-Location
}
