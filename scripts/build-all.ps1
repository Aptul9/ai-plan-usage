#Requires -Version 5.1
<#
.SYNOPSIS
  Build both Electron and Tauri apps sequentially.
.DESCRIPTION
  Calls build-electron.ps1 then build-tauri.ps1. Final artifacts mirrored
  into ../artifacts/. Each step inherits the same $ErrorActionPreference,
  so a failure in one stops the run.

.PARAMETER Clean
  Forwarded to both child scripts. Wipes their respective build outputs first.

.PARAMETER SkipElectronInstall
  Forwarded to build-electron.ps1 as -SkipInstall.

.PARAMETER ElectronOnly
  Only run the Electron build.

.PARAMETER TauriOnly
  Only run the Tauri build.

.EXAMPLE
  .\build-all.ps1
.EXAMPLE
  .\build-all.ps1 -Clean
.EXAMPLE
  .\build-all.ps1 -ElectronOnly -SkipElectronInstall
#>

[CmdletBinding()]
param(
    [switch]$Clean,
    [switch]$SkipElectronInstall,
    [switch]$ElectronOnly,
    [switch]$TauriOnly
)

$ErrorActionPreference = 'Stop'

if ($ElectronOnly -and $TauriOnly) {
    throw '-ElectronOnly and -TauriOnly are mutually exclusive.'
}

$scriptRoot      = Split-Path -Parent $MyInvocation.MyCommand.Path
$electronScript  = Join-Path $scriptRoot 'build-electron.ps1'
$tauriScript     = Join-Path $scriptRoot 'build-tauri.ps1'

if (-not $TauriOnly) {
    Write-Host ''
    Write-Host '=== build-all: electron ==='
    & $electronScript -Clean:$Clean -SkipInstall:$SkipElectronInstall
}

if (-not $ElectronOnly) {
    Write-Host ''
    Write-Host '=== build-all: tauri ==='
    & $tauriScript -Clean:$Clean
}

$repoRoot     = Split-Path -Parent $scriptRoot
$artifactsDir = Join-Path $repoRoot 'artifacts'

Write-Host ''
Write-Host '=== artifacts ==='
if (Test-Path $artifactsDir) {
    Get-ChildItem -Path $artifactsDir -Filter '*.exe' -File |
        Sort-Object Name |
        ForEach-Object { Write-Host ("  {0}  ({1:N1} MB)" -f $_.FullName, ($_.Length / 1MB)) }
} else {
    Write-Host '  (artifacts/ not found)'
}
Write-Host ''
Write-Host 'done.'
