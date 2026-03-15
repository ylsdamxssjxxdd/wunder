Set-StrictMode -Version Latest

function Write-Win7GnuStep {
  param([string]$Message)
  Write-Host "[win7-electron-gnu] $Message"
}

function Ensure-Win7GnuDirectory {
  param([string]$Path)
  New-Item -ItemType Directory -Path $Path -Force | Out-Null
}

function Resolve-Win7GnuRepoRoot {
  param([string]$ScriptPath)
  (Resolve-Path (Join-Path (Split-Path -Parent $ScriptPath) '..\..\..')).Path
}

function Read-Win7GnuToolchainProfile {
  param([string]$RepoRoot)

  $profilePath = Join-Path $RepoRoot 'desktop\electron\scripts\win7-gnu-toolchain.json'
  if (-not (Test-Path $profilePath)) {
    throw "missing Win7 GNU toolchain profile: $profilePath"
  }

  return [pscustomobject]@{
    Path = $profilePath
    Data = (Get-Content -Path $profilePath -Raw | ConvertFrom-Json)
  }
}

function New-Win7GnuBuildContext {
  param(
    [string]$RepoRoot,
    [ValidateSet('x64', 'ia32')]
    [string]$Arch,
    [string]$LabRoot = '',
    [string]$RustToolchain = ''
  )

  $profile = Read-Win7GnuToolchainProfile -RepoRoot $RepoRoot
  $data = $profile.Data
  $archProfile = if ($Arch -eq 'ia32') { $data.architectures.ia32 } else { $data.architectures.x64 }
  $resolvedLabRoot = if ($LabRoot) { $LabRoot } else { Join-Path $RepoRoot $data.labRoot }
  $resolvedToolchain = if ($RustToolchain) { $RustToolchain } else { $data.rustToolchain }
  $bridgeTargetDirName = $data.paths.bridgeTargetDirPattern.Replace('{arch}', $Arch)

  return @{
    RepoRoot = $RepoRoot
    ProfilePath = $profile.Path
    Arch = $Arch
    LabRoot = $resolvedLabRoot
    RustToolchain = $resolvedToolchain
    Target = $archProfile.target
    MinGwBin = $archProfile.mingwBin
    Gcc = $archProfile.gcc
    Gxx = $archProfile.gxx
    Ar = $archProfile.ar
    Ranlib = $archProfile.ranlib
    ElectronVersion = $data.electronVersion
    ElectronBuilderVersion = $data.electronBuilderVersion
    CargoHome = Join-Path $resolvedLabRoot $data.paths.cargoHome
    CargoPatchConfigPath = Join-Path $resolvedLabRoot $data.paths.cargoPatchConfig
    ToolchainManifestPath = Join-Path $resolvedLabRoot $data.paths.toolchainManifest
    BridgeTargetDir = Join-Path $resolvedLabRoot $bridgeTargetDirName
    TokioRustlsPatchDir = Join-Path $RepoRoot $data.paths.tokioRustlsPatchDir
  }
}

function Assert-Win7GnuCommand {
  param([string]$Name)

  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    throw "required command not found: $Name"
  }
}

function Assert-Win7GnuPath {
  param(
    [string]$Path,
    [string]$Label
  )

  if (-not (Test-Path $Path)) {
    throw "$Label is missing: $Path"
  }
}

function Resolve-WindowsTargetsCompatLibDir {
  param(
    [string]$CargoHome,
    [string]$Arch
  )

  $crateDirName = if ($Arch -eq 'ia32') { 'windows_i686_gnu-0.48.5' } else { 'windows_x86_64_gnu-0.48.5' }
  $registrySrcRoot = Join-Path $CargoHome 'registry\src'
  if (-not (Test-Path $registrySrcRoot)) {
    return $null
  }

  Get-ChildItem -Path $registrySrcRoot -Directory -ErrorAction SilentlyContinue |
    ForEach-Object { Join-Path $_.FullName (Join-Path $crateDirName 'lib') } |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1
}

function Set-Win7GnuBaseEnvironment {
  param([hashtable]$Context)

  $env:RUSTUP_TOOLCHAIN = $Context.RustToolchain
  $env:CARGO_HOME = $Context.CargoHome
  $env:CARGO_TARGET_DIR = $Context.BridgeTargetDir
  $env:CARGO_INCREMENTAL = '0'
  $env:PKG_CONFIG_ALLOW_CROSS = '1'
  if ($env:PATH -notlike "$($Context.MinGwBin)*") {
    $env:PATH = "$($Context.MinGwBin);$env:PATH"
  }

  $targetIdUpper = $Context.Target.Replace('-', '_').ToUpperInvariant()
  $targetIdLower = $Context.Target.Replace('-', '_').ToLowerInvariant()
  Set-Item -Path ("Env:CARGO_TARGET_{0}_LINKER" -f $targetIdUpper) -Value $Context.Gcc
  Set-Item -Path ("Env:CC_{0}" -f $targetIdLower) -Value $Context.Gcc
  Set-Item -Path ("Env:CXX_{0}" -f $targetIdLower) -Value $Context.Gxx
  Set-Item -Path ("Env:AR_{0}" -f $targetIdLower) -Value $Context.Ar
  Set-Item -Path ("Env:RANLIB_{0}" -f $targetIdLower) -Value $Context.Ranlib
}

function Set-Win7GnuRustFlags {
  param(
    [hashtable]$Context,
    [switch]$StaticRuntime
  )

  $previousRustFlags = $env:RUSTFLAGS
  $rustFlagParts = @()
  if (-not [string]::IsNullOrWhiteSpace($previousRustFlags)) {
    $rustFlagParts += $previousRustFlags
  }

  $compatLibDir = Resolve-WindowsTargetsCompatLibDir -CargoHome $Context.CargoHome -Arch $Context.Arch
  if ($compatLibDir) {
    Write-Win7GnuStep "using extra windows-targets import lib: $compatLibDir"
    $rustFlagParts += "-L native=$compatLibDir"
  }

  if ($StaticRuntime) {
    $rustFlagParts += '-C target-feature=+crt-static'
  }

  $env:RUSTFLAGS = ($rustFlagParts -join ' ').Trim()
}

function Write-Win7GnuPatchConfig {
  param([hashtable]$Context)

  $patchPath = $Context.TokioRustlsPatchDir.Replace('\', '/')
  @"
[patch.crates-io]
tokio-rustls = { path = "$patchPath" }
"@ | Set-Content -Path $Context.CargoPatchConfigPath -Encoding UTF8
}

function Write-Win7GnuToolchainManifest {
  param(
    [hashtable]$Context,
    [switch]$StaticRuntime
  )

  $manifest = [ordered]@{
    generatedAt = (Get-Date).ToString('o')
    arch = $Context.Arch
    target = $Context.Target
    rustToolchain = $Context.RustToolchain
    electronVersion = $Context.ElectronVersion
    electronBuilderVersion = $Context.ElectronBuilderVersion
    staticRuntime = [bool]$StaticRuntime
    repoRoot = $Context.RepoRoot
    profilePath = $Context.ProfilePath
    labRoot = $Context.LabRoot
    cargoHome = $Context.CargoHome
    cargoPatchConfig = $Context.CargoPatchConfigPath
    bridgeTargetDir = $Context.BridgeTargetDir
    mingwBin = $Context.MinGwBin
    tokioRustlsPatchDir = $Context.TokioRustlsPatchDir
    quickStart = [ordered]@{
      setup = "powershell -ExecutionPolicy Bypass -File desktop/electron/scripts/setup-win7-gnu-toolchain.ps1 -Arch $($Context.Arch)"
      build = "powershell -ExecutionPolicy Bypass -File desktop/electron/scripts/build-win7-gnu.ps1 -Arch $($Context.Arch)"
      fastBuild = "powershell -ExecutionPolicy Bypass -File desktop/electron/scripts/build-win7-gnu.ps1 -Arch $($Context.Arch) -SkipBootstrap"
    }
  }

  $manifest | ConvertTo-Json -Depth 6 | Set-Content -Path $Context.ToolchainManifestPath -Encoding UTF8
}

function Test-Win7GnuPrerequisites {
  param([hashtable]$Context)

  Assert-Win7GnuCommand -Name 'cargo'
  Assert-Win7GnuCommand -Name 'rustup'
  Assert-Win7GnuCommand -Name 'node'
  Assert-Win7GnuCommand -Name 'npm.cmd'
  Assert-Win7GnuPath -Path $Context.MinGwBin -Label 'MinGW bin directory'
  Assert-Win7GnuPath -Path $Context.TokioRustlsPatchDir -Label 'Win7 tokio-rustls patch directory'
  foreach ($tool in @($Context.Gcc, $Context.Gxx, $Context.Ar, $Context.Ranlib)) {
    if (-not (Test-Path (Join-Path $Context.MinGwBin "$tool.exe"))) {
      throw "required MinGW tool is missing: $(Join-Path $Context.MinGwBin "$tool.exe")"
    }
  }
}

function Initialize-Win7GnuToolchain {
  param(
    [hashtable]$Context,
    [switch]$SkipRustup,
    [switch]$SkipFetch,
    [switch]$StaticRuntime
  )

  Ensure-Win7GnuDirectory -Path $Context.LabRoot
  Ensure-Win7GnuDirectory -Path $Context.CargoHome
  Ensure-Win7GnuDirectory -Path $Context.BridgeTargetDir

  Test-Win7GnuPrerequisites -Context $Context
  Write-Win7GnuPatchConfig -Context $Context

  if (-not $SkipRustup) {
    Write-Win7GnuStep "ensuring Rust toolchain $($Context.RustToolchain)"
    rustup toolchain install $Context.RustToolchain --profile minimal
    if ($LASTEXITCODE -ne 0) {
      throw "rustup toolchain install failed with exit code $LASTEXITCODE"
    }
    rustup component add rust-src --toolchain $Context.RustToolchain
    if ($LASTEXITCODE -ne 0) {
      throw "rustup component add rust-src failed with exit code $LASTEXITCODE"
    }
  }

  Set-Win7GnuBaseEnvironment -Context $Context

  if (-not $SkipFetch) {
    Write-Win7GnuStep "prefetching crates for $($Context.Target)"
    cargo --config $Context.CargoPatchConfigPath fetch --target $Context.Target
    if ($LASTEXITCODE -ne 0) {
      throw "cargo fetch failed with exit code $LASTEXITCODE"
    }
  } elseif (-not (Test-Path (Join-Path $Context.CargoHome 'registry\src'))) {
    throw "Win7 GNU cargo cache is not ready. Run setup-win7-gnu-toolchain.ps1 first."
  }

  Set-Win7GnuRustFlags -Context $Context -StaticRuntime:$StaticRuntime
  Write-Win7GnuToolchainManifest -Context $Context -StaticRuntime:$StaticRuntime
}
