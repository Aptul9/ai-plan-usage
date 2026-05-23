#Requires -Version 5.1
<#
.SYNOPSIS
  Build the Electron app as a portable Windows .exe.
.DESCRIPTION
  Workflow:
    1. npm install (skippable via -SkipInstall) inside apps/electron/
    2. tsc compile (npm run build)
    3. electron-builder --win portable, config from electron-builder.yml
    4. Copy the resulting .exe to ../artifacts/

  Output:
    apps/electron/dist-packaged/<artifactName>
    artifacts/<artifactName> (mirror)

.PARAMETER SkipInstall
  Skip "npm install" step. Use when node_modules is already up to date.

.PARAMETER Clean
  Delete dist-packaged/, out/, and previously emitted .js/.js.map next to the HTML files before building.

.EXAMPLE
  .\build-electron.ps1
.EXAMPLE
  .\build-electron.ps1 -Clean
.EXAMPLE
  .\build-electron.ps1 -SkipInstall
#>

[CmdletBinding()]
param(
    [switch]$SkipInstall,
    [switch]$Clean
)

$ErrorActionPreference = 'Stop'

$scriptRoot   = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot     = Split-Path -Parent $scriptRoot
$electronDir  = Join-Path $repoRoot 'apps\electron'
$uiDir        = Join-Path $repoRoot 'apps\ui'
$artifactsDir = Join-Path $repoRoot 'artifacts'
$outDir       = Join-Path $electronDir 'dist-packaged'
$iconScript   = Join-Path $scriptRoot 'sync-icons.ps1'

if (-not (Test-Path $electronDir)) {
    throw "Electron directory not found: $electronDir"
}
if (-not (Test-Path $uiDir)) {
    throw "Shared UI directory not found: $uiDir"
}

New-Item -ItemType Directory -Force -Path $artifactsDir | Out-Null

Write-Host '[electron] sync app icons'
& $iconScript -ElectronOnly

Push-Location $electronDir
try {
    if ($Clean) {
        Write-Host '[electron] clean: dist-packaged/, out/, compiled JS in project root'
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue 'dist-packaged', 'out', 'dist', 'ui-dist'
        # Remove compiled JS that sits next to HTML (tsc outDir = ".")
        $tsBaseNames = Get-ChildItem -Path (Join-Path $electronDir 'src') -Filter '*.ts' -File |
            Where-Object { $_.Name -notmatch '\.d\.ts$' } |
            ForEach-Object { [System.IO.Path]::GetFileNameWithoutExtension($_.Name) }
        foreach ($base in $tsBaseNames) {
            Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $electronDir "$base.js")
            Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $electronDir "$base.js.map")
        }
        $uiBaseNames = Get-ChildItem -Path (Join-Path $uiDir 'src') -Filter '*.ts' -File |
            Where-Object { $_.Name -notmatch '\.d\.ts$' } |
            ForEach-Object { [System.IO.Path]::GetFileNameWithoutExtension($_.Name) }
        foreach ($base in $uiBaseNames) {
            Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $uiDir "$base.js")
            Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $uiDir "$base.js.map")
        }
    }

    if (-not $SkipInstall) {
        Write-Host '[electron] npm install (shared UI)'
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

        Write-Host '[electron] npm install'
        & npm install
        if ($LASTEXITCODE -ne 0) {
            throw "npm install failed (exit $LASTEXITCODE)"
        }
    }

    Write-Host '[electron] tsc compile (npm run build)'
    & npm run build
    if ($LASTEXITCODE -ne 0) {
        throw "tsc compile failed (exit $LASTEXITCODE)"
    }

    Write-Host '[electron] electron-builder --win portable'
    & npx --no-install electron-builder --win portable
    if ($LASTEXITCODE -ne 0) {
        throw "electron-builder failed (exit $LASTEXITCODE)"
    }

    if (-not (Test-Path $outDir)) {
        throw "electron-builder output directory not found: $outDir"
    }

    $exe = Get-ChildItem -Path $outDir -Filter '*.exe' -File -Recurse | Select-Object -First 1
    if (-not $exe) {
        throw "No .exe produced under $outDir"
    }

    $dest = Join-Path $artifactsDir $exe.Name
    Copy-Item -Path $exe.FullName -Destination $dest -Force
    Write-Host "[electron] OK"
    Write-Host "[electron]   source : $($exe.FullName)"
    Write-Host "[electron]   mirror : $dest"
}
finally {
    Pop-Location
}
