param(
  [ValidateSet('ia32', 'x64')]
  [string]$Arch = 'ia32',
  [string]$LabRoot = '',
  [string]$ElectronVersion = '22.3.27',
  [string]$ElectronBuilderVersion = '24.13.3'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step {
  param([string]$Message)
  Write-Host "[win7-electron] $Message"
}

function Ensure-Directory {
  param([string]$Path)
  New-Item -ItemType Directory -Path $Path -Force | Out-Null
}

function Write-Utf8NoBomFile {
  param(
    [string]$Path,
    [string]$Content
  )

  $encoding = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Content, $encoding)
}

function Resolve-ExistingPath {
  param([string[]]$Candidates)

  foreach ($candidate in $Candidates) {
    if ($candidate -and (Test-Path $candidate)) {
      return (Resolve-Path $candidate).Path
    }
  }
  return $null
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '..\..\..')).Path
if (-not $LabRoot) {
  $LabRoot = Join-Path $repoRoot 'temp_dir\win7-lab'
}

$frontendDist = Join-Path $repoRoot 'frontend\dist'
if (-not (Test-Path $frontendDist)) {
  throw "frontend/dist is missing. Build it first: npm run build --workspace wunder-frontend"
}

$bridgeArchDir = if ($Arch -eq 'ia32') { 'i686-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$bridgeBinary = if ($env:WUNDER_BRIDGE_BIN -and (Test-Path $env:WUNDER_BRIDGE_BIN)) {
  (Resolve-Path $env:WUNDER_BRIDGE_BIN).Path
} elseif ($Arch -eq 'ia32') {
  Resolve-ExistingPath @(
    (Join-Path $LabRoot 'bridge-build-target\i686-pc-windows-msvc\release\wunder-desktop-bridge.exe'),
    (Join-Path $LabRoot 'bridge-build-target-ia32\i686-pc-windows-msvc\release\wunder-desktop-bridge.exe'),
    (Join-Path $repoRoot 'target\i686-pc-windows-msvc\release\wunder-desktop-bridge.exe')
  )
} else {
  Resolve-ExistingPath @(
    (Join-Path $repoRoot 'target\release\wunder-desktop-bridge.exe'),
    (Join-Path $repoRoot 'target\x86_64-pc-windows-msvc\release\wunder-desktop-bridge.exe'),
    (Join-Path $LabRoot 'bridge-build-target-x64\x86_64-pc-windows-msvc\release\wunder-desktop-bridge.exe')
  )
}
if (-not (Test-Path $bridgeBinary)) {
  throw "bridge binary is missing for $Arch.`nBuild it first, for example: cargo build --release --bin wunder-desktop-bridge --target $bridgeArchDir"
}

$stageRoot = Join-Path $LabRoot ("electron-win7-{0}" -f $Arch)
$stageApp = Join-Path $stageRoot 'app'
$outputRoot = Join-Path $stageRoot 'dist'
$npmCache = Join-Path $LabRoot 'npm-cache'
$electronCache = Join-Path $LabRoot 'electron-cache'
$builderCache = Join-Path $LabRoot 'electron-builder-cache'

Ensure-Directory $LabRoot
if (Test-Path $stageRoot) {
  Remove-Item $stageRoot -Recurse -Force
}
Ensure-Directory $stageApp
Ensure-Directory $outputRoot
Ensure-Directory $npmCache
Ensure-Directory $electronCache
Ensure-Directory $builderCache

# Keep the stage minimal so Win7 experiments do not contaminate workspace node_modules.
$itemsToCopy = @(
  'src',
  'scripts',
  'build',
  'assets',
  'electron-builder.win7.yml'
)
foreach ($item in $itemsToCopy) {
  Copy-Item -Path (Join-Path (Join-Path $repoRoot 'desktop\electron') $item) -Destination $stageApp -Recurse -Force
}

$stagePackage = @{
  name = 'wunder-desktop-electron-win7'
  version = '0.1.0'
  private = $true
  description = 'Wunder Desktop Electron Win7 experiment'
  main = 'src/main.js'
  author = 'wunder'
} | ConvertTo-Json -Depth 3
Write-Utf8NoBomFile -Path (Join-Path $stageApp 'package.json') -Content $stagePackage

$env:WUNDER_REPO_ROOT = $repoRoot
$env:WUNDER_FRONTEND_DIST = $frontendDist
$env:WUNDER_BRIDGE_BIN = $bridgeBinary
$env:WUNDER_SKIP_RUNTIME_DEPS_COPY = '1'
$env:npm_config_cache = $npmCache
$env:ELECTRON_CACHE = $electronCache
$env:ELECTRON_BUILDER_CACHE = $builderCache

Write-Step "preparing resources for $Arch"
Push-Location $stageApp
try {
  & node .\scripts\prepare-resources.js
  if ($LASTEXITCODE -ne 0) {
    throw "prepare-resources.js failed with exit code $LASTEXITCODE"
  }

  Write-Step "building Electron $ElectronVersion NSIS package"
  $builderCommand = @(
    'exec',
    '--yes',
    "--package=electron-builder@$ElectronBuilderVersion",
    '--',
    'electron-builder',
    '--win',
    'nsis',
    "--$Arch",
    '--publish',
    'never',
    '--config',
    'electron-builder.win7.yml',
    '--config.npmRebuild=false',
    '--config.buildDependenciesFromSource=false',
    "--config.directories.output=$outputRoot",
    "--config.electronVersion=$ElectronVersion"
  )
  & npm.cmd @builderCommand
  if ($LASTEXITCODE -ne 0) {
    throw "electron-builder failed with exit code $LASTEXITCODE"
  }
}
finally {
  Pop-Location
}

Write-Step "output directory: $outputRoot"
