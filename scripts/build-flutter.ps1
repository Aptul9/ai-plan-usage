#Requires -Version 5.1
<#
.SYNOPSIS
  Build the Flutter app and its per-user installer into artifacts/.
.DESCRIPTION
  Workflow:
    1. flutter build windows --release in apps/flutter
    2. Mirror build/windows/x64/runner/Release/ to ../artifacts/flutter/
    3. ISCC apps/flutter/installer/ai-plan-usage.iss with the pubspec version
       -> artifacts/ai-plan-usage-<version>-windows-x64-setup.exe

  Needs Inno Setup 6 (winget install JRSoftware.InnoSetup).

.EXAMPLE
  .\build-flutter.ps1
#>

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$scriptRoot   = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot     = Split-Path -Parent $scriptRoot
$flutterDir   = Join-Path $repoRoot 'apps\flutter'
$releaseDir   = Join-Path $flutterDir 'build\windows\x64\runner\Release'
$installer    = Join-Path $flutterDir 'installer\ai-plan-usage.iss'
$artifactsDir = Join-Path $repoRoot 'artifacts'

$iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    $iscc = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe')
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) {
    throw 'ISCC.exe not found. Install Inno Setup 6: winget install JRSoftware.InnoSetup'
}

$version = (Select-String -Path (Join-Path $flutterDir 'pubspec.yaml') -Pattern '^version:\s*([0-9.]+)').Matches[0].Groups[1].Value
if (-not $version) {
    throw 'version not found in apps/flutter/pubspec.yaml'
}

Push-Location $flutterDir
try {
    Write-Host "[flutter] flutter build windows --release ($version)"
    & flutter build windows --release
    if ($LASTEXITCODE -ne 0) {
        throw "flutter build failed (exit $LASTEXITCODE)"
    }
}
finally {
    Pop-Location
}

New-Item -ItemType Directory -Force -Path $artifactsDir | Out-Null
$mirror = Join-Path $artifactsDir 'flutter'
try {
    New-Item -ItemType Directory -Force -Path $mirror | Out-Null
    Copy-Item -Path (Join-Path $releaseDir '*') -Destination $mirror -Recurse -Force
} catch {
    Write-Warning "Mirror to $mirror failed (app running from there?): $($_.Exception.Message)"
}

Write-Host '[flutter] ISCC'
& $iscc /Q "/DAppVersion=$version" "/DOutputDir=$artifactsDir" $installer
if ($LASTEXITCODE -ne 0) {
    throw "ISCC failed (exit $LASTEXITCODE)"
}

$setup = Join-Path $artifactsDir "ai-plan-usage-$version-windows-x64-setup.exe"
Write-Host '[flutter] OK'
Write-Host "[flutter]   folder    : $mirror"
Write-Host "[flutter]   installer : $setup"
