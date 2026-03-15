param(
  [ValidateSet('x64', 'ia32')]
  [string]$Arch = 'x64',
  [ValidateSet('auto', 'fixed', 'skip')]
  [string]$WebviewMode = 'auto',
  [string]$LabRoot = '',
  [switch]$StaticRuntime
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step {
  param([string]$Message)
  Write-Host "[win7-tauri-gnu] $Message"
}

function Ensure-Directory {
  param([string]$Path)
  New-Item -ItemType Directory -Path $Path -Force | Out-Null
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$tauriDir = (Resolve-Path (Join-Path $scriptDir '..')).Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '..\..\..')).Path
if (-not $LabRoot) {
  $LabRoot = Join-Path $repoRoot 'temp_dir\win7-gnu-lab'
}

$frontendDist = Join-Path $repoRoot 'frontend\dist'
if (-not (Test-Path $frontendDist)) {
  throw "frontend/dist is missing. Build it first: npm run build --workspace wunder-frontend"
}

$nsisDir = 'C:\Program Files (x86)\NSIS'
if (-not (Test-Path (Join-Path $nsisDir 'makensis.exe'))) {
  throw 'NSIS is missing. Install it first so cargo tauri can generate the NSIS bundle.'
}

$target = if ($Arch -eq 'ia32') { 'i686-pc-windows-gnu' } else { 'x86_64-pc-windows-gnu' }
$mingwBin = if ($Arch -eq 'ia32') { 'C:\mingw32\bin' } else { 'C:\mingw64\bin' }
$gcc = if ($Arch -eq 'ia32') { 'i686-w64-mingw32-gcc' } else { 'x86_64-w64-mingw32-gcc' }
$gxx = if ($Arch -eq 'ia32') { 'i686-w64-mingw32-g++' } else { 'x86_64-w64-mingw32-g++' }
$ar = 'ar'

$fixedRuntimeDir = Join-Path $tauriDir 'webview2\win7-x86'
$configFile = switch ($WebviewMode) {
  'fixed' { Join-Path $tauriDir 'tauri.bundle.win7-x86.json' }
  'skip' { Join-Path $tauriDir 'tauri.bundle.win7-skip-webview.json' }
  default {
    if (($Arch -eq 'ia32') -and (Test-Path $fixedRuntimeDir)) {
      Join-Path $tauriDir 'tauri.bundle.win7-x86.json'
    } else {
      Write-Step 'using skip-webview mode for GNU experiment'
      Join-Path $tauriDir 'tauri.bundle.win7-skip-webview.json'
    }
  }
}

$cargoHome = Join-Path $LabRoot 'cargo-home'
$targetDir = Join-Path $LabRoot ("tauri-build-target-{0}-gnu" -f $Arch)
Ensure-Directory $cargoHome
Ensure-Directory $targetDir

$env:CARGO_HOME = $cargoHome
$env:CARGO_TARGET_DIR = $targetDir
$env:CARGO_INCREMENTAL = '0'
$env:PKG_CONFIG_ALLOW_CROSS = '1'
$env:PATH = "$nsisDir;$mingwBin;$env:PATH"

if ($Arch -eq 'ia32') {
  $env:CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER = $gcc
  $env:CC_i686_pc_windows_gnu = $gcc
  $env:CXX_i686_pc_windows_gnu = $gxx
  $env:AR_i686_pc_windows_gnu = $ar
  $env:RC_i686_pc_windows_gnu = 'windres'
} else {
  $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $gcc
  $env:CC_x86_64_pc_windows_gnu = $gcc
  $env:CXX_x86_64_pc_windows_gnu = $gxx
  $env:AR_x86_64_pc_windows_gnu = $ar
  $env:RC_x86_64_pc_windows_gnu = 'windres'
}

$previousRustFlags = $env:RUSTFLAGS
if ($StaticRuntime) {
  $extraFlags = '-C target-feature=+crt-static'
  $env:RUSTFLAGS = if ([string]::IsNullOrWhiteSpace($previousRustFlags)) {
    $extraFlags
  } else {
    "$previousRustFlags $extraFlags"
  }
}

Write-Step "using target $target"
Write-Step "using config: $configFile"
Push-Location $tauriDir
try {
  cargo tauri build -f desktop -c $configFile --bundles nsis --no-sign --target $target -- --manifest-path ../../Cargo.toml
  if ($LASTEXITCODE -ne 0) {
    throw "cargo tauri build failed with exit code $LASTEXITCODE"
  }
}
finally {
  Pop-Location
}

$output = Join-Path $targetDir (Join-Path $target 'release\bundle\nsis')
Write-Step "output directory: $output"
