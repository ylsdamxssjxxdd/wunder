param(
  [ValidateSet('ia32', 'x64')]
  [string]$Arch = 'ia32',
  [string]$LabRoot = '',
  [string]$SupplementRoot = '',
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

function Remove-DirectoryTree {
  param([string]$Path)

  if (-not (Test-Path $Path)) {
    return
  }

  try {
    Remove-Item $Path -Recurse -Force -ErrorAction Stop
    return
  }
  catch {
    & cmd.exe /d /c "rmdir /s /q `"$Path`"" | Out-Null
    if (Test-Path $Path) {
      throw
    }
  }
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

function Resolve-FullPath {
  param(
    [string]$Path,
    [string]$BasePath = ''
  )

  if ([System.IO.Path]::IsPathRooted($Path)) {
    return [System.IO.Path]::GetFullPath($Path)
  }
  if ($BasePath) {
    return [System.IO.Path]::GetFullPath((Join-Path $BasePath $Path))
  }
  return [System.IO.Path]::GetFullPath($Path)
}

function Resolve-AppVersion {
  param([string]$RepoRoot)

  $manualVersion = [string]$env:WUNDER_APP_VERSION
  if (-not [string]::IsNullOrWhiteSpace($manualVersion)) {
    return $manualVersion.Trim()
  }

  $appVersionPath = Join-Path $RepoRoot 'config\app_version.json'
  if (-not (Test-Path $appVersionPath)) {
    throw "missing app version file: $appVersionPath"
  }

  try {
    $payload = Get-Content -Path $appVersionPath -Raw | ConvertFrom-Json
  }
  catch {
    throw "failed to parse app version file: $appVersionPath"
  }

  $resolvedVersion = [string]$payload.version
  if ([string]::IsNullOrWhiteSpace($resolvedVersion)) {
    throw "missing version in app version file: $appVersionPath"
  }

  return $resolvedVersion.Trim()
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '..\..\..')).Path
$appVersion = Resolve-AppVersion -RepoRoot $repoRoot
if (-not $LabRoot) {
  $LabRoot = Join-Path $repoRoot 'temp_dir\win7-lab'
}
$LabRoot = Resolve-FullPath -Path $LabRoot -BasePath $repoRoot

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
  Remove-DirectoryTree -Path $stageRoot
}
Ensure-Directory $stageApp
Ensure-Directory $outputRoot
Ensure-Directory $npmCache
Ensure-Directory $electronCache
Ensure-Directory $builderCache

# Keep the stage minimal so Win7 experiments do not contaminate workspace node_modules.
$itemsToCopy = @(
  @{ Name = 'src'; Required = $true },
  @{ Name = 'scripts'; Required = $true },
  @{ Name = 'build'; Required = $false },
  @{ Name = 'assets'; Required = $true },
  @{ Name = 'electron-builder.win7.yml'; Required = $true }
)
foreach ($item in $itemsToCopy) {
  $sourcePath = Join-Path (Join-Path $repoRoot 'desktop\electron') $item.Name
  if (-not (Test-Path $sourcePath)) {
    if ($item.Required) {
      throw "missing required electron staging item: $sourcePath"
    }
    Write-Step "skip optional missing staging item: $sourcePath"
    continue
  }
  Copy-Item -Path $sourcePath -Destination $stageApp -Recurse -Force
}

$stageBuildDir = Join-Path $stageApp 'build'
Ensure-Directory $stageBuildDir
$stageIconIco = Join-Path $stageBuildDir 'icon.ico'
$stageIconPng = Join-Path $stageBuildDir 'icon.png'
$fallbackIconIco = Resolve-ExistingPath @(
  (Join-Path $stageApp 'assets\icon.ico'),
  (Join-Path $repoRoot 'desktop\electron\assets\icon.ico')
)
$fallbackIconPng = Resolve-ExistingPath @(
  (Join-Path $stageApp 'assets\icon.png'),
  (Join-Path $repoRoot 'desktop\electron\assets\icon.png')
)

if (-not (Test-Path $stageIconIco)) {
  if ($fallbackIconIco) {
    Copy-Item -Path $fallbackIconIco -Destination $stageIconIco -Force
    Write-Step "injected fallback icon.ico into staged build resources"
  } else {
    throw "missing icon resource: build/icon.ico and assets/icon.ico were both not found"
  }
}

if (-not (Test-Path $stageIconPng) -and $fallbackIconPng) {
  Copy-Item -Path $fallbackIconPng -Destination $stageIconPng -Force
  Write-Step "injected fallback icon.png into staged build resources"
}

$stageBuilderConfig = Join-Path $stageApp 'electron-builder.win7.yml'
if ((Test-Path $stageBuilderConfig) -and -not (Test-Path $stageIconPng)) {
  $configContent = Get-Content -Raw -Path $stageBuilderConfig
  $nextContent = $configContent -replace '(?m)^\s*icon:\s*build/icon\.png\s*$', 'icon: build/icon.ico'
  if ($nextContent -ne $configContent) {
    Write-Utf8NoBomFile -Path $stageBuilderConfig -Content $nextContent
    Write-Step "build/icon.png missing; patched win7 builder icon path to build/icon.ico"
  }
}

$stagePackage = @{
  name = 'wunder-desktop-electron-win7'
  version = $appVersion
  private = $true
  description = 'Wunder Desktop Electron Win7 experiment'
  main = 'src/main.js'
  author = 'wunder'
} | ConvertTo-Json -Depth 3
Write-Utf8NoBomFile -Path (Join-Path $stageApp 'package.json') -Content $stagePackage

$env:WUNDER_REPO_ROOT = $repoRoot
$env:WUNDER_FRONTEND_DIST = $frontendDist
$env:WUNDER_BRIDGE_BIN = $bridgeBinary
$env:WUNDER_INCLUDE_CLI = '0'
$env:WUNDER_CLI_BIN = ''
$env:WUNDER_SKIP_RUNTIME_DEPS_COPY = '1'
$env:WUNDER_EXTRA_RUNTIME_ROOTS = ''
$win7DisableUpdaterFlag = Resolve-ExistingPath @(
  (Join-Path $repoRoot 'desktop\electron\assets\win7-disable-updater.flag')
)
$extraRuntimeFiles = @()
if ($win7DisableUpdaterFlag) {
  $extraRuntimeFiles += $win7DisableUpdaterFlag
}
$env:WUNDER_EXTRA_RUNTIME_FILES = if ($extraRuntimeFiles.Count -gt 0) {
  [string]::Join([System.IO.Path]::PathSeparator, $extraRuntimeFiles)
} else {
  ''
}
$resolvedSupplementRoot = if ($SupplementRoot -and (Test-Path $SupplementRoot)) {
  (Resolve-Path $SupplementRoot).Path
} elseif ($env:WUNDER_SUPPLEMENT_ROOT -and (Test-Path $env:WUNDER_SUPPLEMENT_ROOT)) {
  (Resolve-Path $env:WUNDER_SUPPLEMENT_ROOT).Path
} else {
  $null
}
if ($resolvedSupplementRoot) {
  Write-Step "supplement root detected: $resolvedSupplementRoot"
  Write-Step "Win7 setup.exe no longer embeds supplement runtime; ship the supplement zip separately and extract it into the install directory when Python/Git are needed"
}
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

  $stageResourcesDir = Join-Path $stageApp 'resources'
  Ensure-Directory $stageResourcesDir
  foreach ($obsoleteName in @('README-win7-supplement.txt', 'wunder-win7-supplement.json')) {
    $obsoletePath = Join-Path $stageResourcesDir $obsoleteName
    if (Test-Path $obsoletePath) {
      Remove-Item -Force $obsoletePath
      Write-Step "removed obsolete win7 resource artifact: $obsoletePath"
    }
  }
  Write-Utf8NoBomFile -Path (Join-Path $stageResourcesDir 'disable-updater.flag') -Content 'disable updater for win7 desktop package'
  Write-Utf8NoBomFile -Path (Join-Path $stageResourcesDir 'win7-disable-updater.flag') -Content 'disable updater for win7 desktop package'

  $stageUpdaterModuleDir = Join-Path $stageApp 'node_modules\electron-updater'
  if (Test-Path $stageUpdaterModuleDir) {
    Remove-DirectoryTree -Path $stageUpdaterModuleDir
    Write-Step "removed staged electron-updater module to keep win7 package updater-free"
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
