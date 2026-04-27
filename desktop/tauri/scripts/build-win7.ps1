param(
  [ValidateSet('auto', 'fixed', 'skip')]
  [string]$WebviewMode = 'auto',
  [string]$LabRoot = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step {
  param([string]$Message)
  Write-Host "[win7-tauri] $Message"
}

function Ensure-Directory {
  param([string]$Path)
  New-Item -ItemType Directory -Path $Path -Force | Out-Null
}

function Ensure-Nasm {
  param([string]$ToolsRoot)

  $nasmExe = Get-ChildItem -Path $ToolsRoot -Recurse -Filter 'nasm.exe' -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty FullName
  if ($nasmExe) {
    return Split-Path -Parent $nasmExe
  }

  $zipPath = Join-Path $ToolsRoot 'nasm-2.16.03-win64.zip'
  $extractRoot = Join-Path $ToolsRoot 'nasm'
  Ensure-Directory $ToolsRoot
  if (-not (Test-Path $zipPath)) {
    Write-Step 'downloading portable NASM'
    curl.exe -L 'https://www.nasm.us/pub/nasm/releasebuilds/2.16.03/win64/nasm-2.16.03-win64.zip' -o $zipPath | Out-Null
  }
  if (Test-Path $extractRoot) {
    Remove-Item $extractRoot -Recurse -Force
  }
  Expand-Archive -Path $zipPath -DestinationPath $extractRoot -Force
  $nasmExe = Get-ChildItem -Path $extractRoot -Recurse -Filter 'nasm.exe' | Select-Object -First 1 -ExpandProperty FullName
  if (-not $nasmExe) {
    throw 'portable NASM download succeeded but nasm.exe was not found'
  }
  return Split-Path -Parent $nasmExe
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$tauriDir = (Resolve-Path (Join-Path $scriptDir '..')).Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '..\..\..')).Path
if (-not $LabRoot) {
  $LabRoot = Join-Path $repoRoot 'temp_dir\win7-lab'
}

$frontendDist = Join-Path $repoRoot 'frontend\dist'
if (-not (Test-Path $frontendDist)) {
  throw "frontend/dist is missing. Build it first: npm run build --workspace wunder-frontend"
}

$nsisDir = 'C:\Program Files (x86)\NSIS'
if (-not (Test-Path (Join-Path $nsisDir 'makensis.exe'))) {
  throw 'NSIS is missing. Install it first so cargo tauri can generate the NSIS bundle.'
}

$fixedRuntimeDir = Join-Path $tauriDir 'webview2\win7-x86'
$configFile = switch ($WebviewMode) {
  'fixed' { Join-Path $tauriDir 'tauri.bundle.win7-x86.json' }
  'skip' { Join-Path $tauriDir 'tauri.bundle.win7-skip-webview.json' }
  default {
    if (Test-Path $fixedRuntimeDir) {
      Join-Path $tauriDir 'tauri.bundle.win7-x86.json'
    } else {
      Write-Step 'fixed WebView2 runtime 109 is missing, falling back to skip mode'
      Join-Path $tauriDir 'tauri.bundle.win7-skip-webview.json'
    }
  }
}

$cargoHome = Join-Path $LabRoot 'cargo-home'
$targetDir = Join-Path $LabRoot 'tauri-build-target'
$toolsRoot = Join-Path $LabRoot 'tools'
$nasmDir = Ensure-Nasm -ToolsRoot $toolsRoot

Ensure-Directory $cargoHome
Ensure-Directory $targetDir

$env:CARGO_HOME = $cargoHome
$env:CARGO_TARGET_DIR = $targetDir
$env:CARGO_INCREMENTAL = '0'
$env:PATH = "$nsisDir;$nasmDir;$env:PATH"

Write-Step "using config: $configFile"
Push-Location $tauriDir
try {
  cargo tauri build -f desktop -c $configFile --bundles nsis --no-sign --target i686-pc-windows-msvc -- --manifest-path ../../Cargo.toml
}
finally {
  Pop-Location
}

$output = Join-Path $targetDir 'i686-pc-windows-msvc\release\bundle\nsis\wunder-desktop_0.2.0_x86-setup.exe'
Write-Step "output installer: $output"
