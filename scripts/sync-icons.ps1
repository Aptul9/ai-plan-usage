#Requires -Version 5.1
<#
.SYNOPSIS
  Sync the shared app identity icon into the Electron and Tauri shells.
.DESCRIPTION
  apps/ui/icons is the canonical source. Electron receives a PNG for runtime
  windows plus a Windows .ico for packaging. Tauri receives the generated
  desktop icon set through `cargo tauri icon` unless -ElectronOnly is set.

.PARAMETER ElectronOnly
  Only update Electron assets. This keeps the Electron build independent from
  the Rust/Tauri toolchain.
#>

[CmdletBinding()]
param(
    [switch]$ElectronOnly
)

$ErrorActionPreference = 'Stop'

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptRoot
$sourceDir = Join-Path $repoRoot 'apps\ui\icons'
$electronAssetsDir = Join-Path $repoRoot 'apps\electron\assets'
$tauriIconsDir = Join-Path $repoRoot 'apps\tauri\src-tauri\icons'

$source1024 = Join-Path $sourceDir 'ai-plan-usage-icon-1024.png'
$source512 = Join-Path $sourceDir 'ai-plan-usage-icon-512.png'

if (-not (Test-Path $source1024)) {
    throw "Missing canonical app icon: $source1024"
}
if (-not (Test-Path $source512)) {
    throw "Missing Electron app icon source: $source512"
}

function Write-IcoFromPngSet {
    param(
        [Parameter(Mandatory = $true)][string]$SourceDirectory,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    $sizes = @(16, 32, 64, 128, 256)
    $images = foreach ($size in $sizes) {
        $png = Join-Path $SourceDirectory "ai-plan-usage-icon-$size.png"
        if (-not (Test-Path $png)) {
            throw "Missing PNG for ICO generation: $png"
        }
        [pscustomobject]@{
            Size = $size
            Bytes = [System.IO.File]::ReadAllBytes($png)
        }
    }

    $destDir = Split-Path -Parent $Destination
    New-Item -ItemType Directory -Force -Path $destDir | Out-Null

    $stream = [System.IO.File]::Open($Destination, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write)
    try {
        $writer = [System.IO.BinaryWriter]::new($stream)
        try {
            $writer.Write([uint16]0)
            $writer.Write([uint16]1)
            $writer.Write([uint16]$images.Count)

            $offset = 6 + (16 * $images.Count)
            foreach ($image in $images) {
                $dimensionByte = if ($image.Size -ge 256) { 0 } else { $image.Size }
                $writer.Write([byte]$dimensionByte)
                $writer.Write([byte]$dimensionByte)
                $writer.Write([byte]0)
                $writer.Write([byte]0)
                $writer.Write([uint16]1)
                $writer.Write([uint16]32)
                $writer.Write([uint32]$image.Bytes.Length)
                $writer.Write([uint32]$offset)
                $offset += $image.Bytes.Length
            }

            foreach ($image in $images) {
                $writer.Write($image.Bytes)
            }
        }
        finally {
            $writer.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

New-Item -ItemType Directory -Force -Path $electronAssetsDir | Out-Null
Copy-Item -Path $source512 -Destination (Join-Path $electronAssetsDir 'app-icon.png') -Force
Write-IcoFromPngSet -SourceDirectory $sourceDir -Destination (Join-Path $electronAssetsDir 'app-icon.ico')
Write-Host "[icons] electron: assets\app-icon.png + assets\app-icon.ico"

if ($ElectronOnly) {
    return
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
        $env:PATH = "$cargoBin;$env:PATH"
        Write-Host "[icons] PATH: prepended $cargoBin"
    } else {
        throw "cargo not found in PATH and $cargoBin\cargo.exe does not exist. Install Rust via rustup."
    }
}
if (-not (Get-Command cargo-tauri -ErrorAction SilentlyContinue)) {
    throw "cargo-tauri subcommand not found. Install with: cargo install tauri-cli --version `"^2`""
}

New-Item -ItemType Directory -Force -Path $tauriIconsDir | Out-Null
& cargo tauri icon $source1024 --output $tauriIconsDir
if ($LASTEXITCODE -ne 0) {
    throw "cargo tauri icon failed (exit $LASTEXITCODE)"
}

$mobileAndStoreDirs = @(
    (Join-Path $tauriIconsDir 'android'),
    (Join-Path $tauriIconsDir 'ios')
)
foreach ($dir in $mobileAndStoreDirs) {
    if (Test-Path $dir) {
        Remove-Item -LiteralPath $dir -Recurse -Force
    }
}

Get-ChildItem -Path $tauriIconsDir -File -Filter 'Square*Logo.png' -ErrorAction SilentlyContinue |
    Remove-Item -Force
$storeLogo = Join-Path $tauriIconsDir 'StoreLogo.png'
if (Test-Path $storeLogo) {
    Remove-Item -LiteralPath $storeLogo -Force
}

Write-Host "[icons] tauri: regenerated desktop icons from apps\ui\icons"
