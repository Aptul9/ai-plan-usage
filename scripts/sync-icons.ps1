#Requires -Version 5.1
<#
.SYNOPSIS
  Regenerate the Tauri desktop icon set from the shared app icon.
.DESCRIPTION
  apps/ui/icons is the canonical source. Uses the Tauri CLI from apps/tauri
  (npm devDependency), so no separate cargo-tauri install is needed.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptRoot
$sourceDir = Join-Path $repoRoot 'apps\ui\icons'
$tauriDir = Join-Path $repoRoot 'apps\tauri'
$tauriIconsDir = Join-Path $tauriDir 'src-tauri\icons'
$source1024 = Join-Path $sourceDir 'ai-plan-usage-icon-1024.png'

if (-not (Test-Path $source1024)) {
    throw "Missing canonical app icon: $source1024"
}
if (-not (Test-Path (Join-Path $tauriDir 'node_modules\@tauri-apps\cli'))) {
    throw "Tauri CLI not installed. Run: npm --prefix apps/tauri install"
}

New-Item -ItemType Directory -Force -Path $tauriIconsDir | Out-Null
& npm --prefix $tauriDir exec -- tauri icon $source1024 --output $tauriIconsDir
if ($LASTEXITCODE -ne 0) {
    throw "tauri icon failed (exit $LASTEXITCODE)"
}

foreach ($dir in @((Join-Path $tauriIconsDir 'android'), (Join-Path $tauriIconsDir 'ios'))) {
    if (Test-Path $dir) {
        Remove-Item -LiteralPath $dir -Recurse -Force
    }
}
Get-ChildItem -Path $tauriIconsDir -File -Filter 'Square*Logo.png' -ErrorAction SilentlyContinue | Remove-Item -Force
$storeLogo = Join-Path $tauriIconsDir 'StoreLogo.png'
if (Test-Path $storeLogo) {
    Remove-Item -LiteralPath $storeLogo -Force
}

Write-Host "[icons] tauri: regenerated desktop icons from apps\ui\icons"
