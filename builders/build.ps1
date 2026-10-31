param(
  [Alias("t")][ValidateSet("desktop", "cli")][string]$Target = "",
  [Alias("a")][ValidateSet("linux-arm64", "linux-amd64", "win7-x86")][string]$Arch = "",
  [switch]$Egui,
  [switch]$All,
  [string]$KylinBuilderRoot = "",
  [string]$Win7BuilderRoot = "",
  [string]$Image = "rcho-slint-arm64-ubuntu18:latest",
  [ValidateRange(1, 8)][int]$Jobs = 8,
  [switch]$Check
)

# Public Windows build dispatcher. Linux cross work runs in the kylin-arm
# Docker image; the Win7 x86 target stays on the local win7 offline SDK, which
# covers the full 32-bit Windows range (a Win7-compatible binary also runs on
# Win10/11 x86), so no separate Win32 Docker cross is needed.
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $KylinBuilderRoot) { $KylinBuilderRoot = if ($env:WUNDER_KYLIN_BUILDER_ROOT) { $env:WUNDER_KYLIN_BUILDER_ROOT } else { Join-Path (Split-Path $repoRoot -Parent) "Rust-builder\kylin-arm" } }
if (-not $Win7BuilderRoot) { $Win7BuilderRoot = if ($env:WUNDER_WIN7_BUILDER_ROOT) { $env:WUNDER_WIN7_BUILDER_ROOT } else { Join-Path (Split-Path $repoRoot -Parent) "Rust-builder\win7" } }
$KylinBuilderRoot = [IO.Path]::GetFullPath($KylinBuilderRoot)
$Win7BuilderRoot = [IO.Path]::GetFullPath($Win7BuilderRoot)

if ($All) {
  if ($Target -or $Arch -or $Egui) { throw "-All selects every distribution; do not combine it with -Target, -Arch or -Egui." }
} elseif ($Egui) {
  if ($Target) { throw "-Egui builds the egui desktop; do not combine it with -Target." }
  if ($Arch -and $Arch -ne "win7-x86") { throw "-Egui supports the native host build or -a win7-x86." }
} elseif (-not $Target -or -not $Arch) {
  throw "Specify both -t desktop|cli and -a linux-arm64|linux-amd64|win7-x86, or use -All."
}

function Invoke-LinuxBuild {
  param(
    [ValidateSet("desktop", "cli")][string]$SelectedTarget,
    [ValidateSet("linux-arm64", "linux-amd64")][string]$SelectedArch
  )

  if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "docker.exe is required for the Linux/Win32 Docker build dispatcher."
  }
  # Linux desktop builds produce an AppDir directory archive; CLI stays a plain ELF.
  $dockerArgs = @(
    "run", "--rm", "--network", "none", "--platform", "linux/arm64",
    "-e", "WUNDER_IN_DOCKER=1",
    "-e", "WUNDER_REPO_ROOT=/workspace",
    "-e", "WUNDER_BUILDER_ROOT=/builder/kylin-arm",
    "-e", "WUNDER_OFFLINE_ROOT=/builder/kylin-arm/offline",
    "-v", "${repoRoot}:/workspace",
    "-v", "${KylinBuilderRoot}:/builder/kylin-arm:ro"
  )
  $dockerArgs += @(
    "-w", "/workspace",
    $Image,
    "bash", "/workspace/build.sh", "-t", $SelectedTarget, "-a", $SelectedArch, "--native"
  )
  & docker @dockerArgs
  if ($LASTEXITCODE -ne 0) { throw "Docker build failed with exit code $LASTEXITCODE" }
}

function Invoke-Win7Build {
  param([ValidateSet("desktop", "cli", "egui")][string]$SelectedTarget)

  $scriptName = switch ($SelectedTarget) {
    "desktop" { "build-win7-offline.ps1"; break }
    "cli" { "build-win7-cli.ps1"; break }
    "egui" { "build-win7-egui.ps1"; break }
  }
  & (Join-Path $PSScriptRoot $scriptName) -BuilderRoot $Win7BuilderRoot -Jobs $Jobs -Check:$Check
  if ($LASTEXITCODE -ne 0) { throw "Win7 $SelectedTarget build failed with exit code $LASTEXITCODE" }
}

function Invoke-EguiBuild {
  $targetDirectory = Join-Path $repoRoot "target\wunder-egui"
  New-Item -ItemType Directory -Force -Path $targetDirectory | Out-Null
  $previousTargetDirectory = $env:CARGO_TARGET_DIR
  try {
    $env:CARGO_TARGET_DIR = $targetDirectory
    $cargoArgs = @("build", "--locked", "--release", "-p", "wunder-egui", "--features", "native")
    if ($Check) { $cargoArgs[0] = "check" }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "egui native build failed with exit code $LASTEXITCODE" }
    if ($Check) {
      Write-Host "egui native check complete"
    } else {
      $runningOnWindows = ($env:OS -eq "Windows_NT") -or ($IsWindows -eq $true)
      $suffix = if ($runningOnWindows) { ".exe" } else { "" }
      $artifact = Join-Path $targetDirectory "release\wunder-egui$suffix"
      if (-not (Test-Path -LiteralPath $artifact -PathType Leaf)) { throw "egui native artifact was not produced: $artifact" }
      Write-Host "egui native build complete: $artifact"
    }
  } finally {
    $env:CARGO_TARGET_DIR = $previousTargetDirectory
  }
}

if ($All) {
  foreach ($selectedArch in @("linux-arm64", "linux-amd64")) {
    Invoke-LinuxBuild -SelectedTarget desktop -SelectedArch $selectedArch
    Invoke-LinuxBuild -SelectedTarget cli -SelectedArch $selectedArch
  }
  Invoke-Win7Build -SelectedTarget desktop
  Invoke-Win7Build -SelectedTarget cli
  return
}

if ($Egui) {
  if ($Arch -eq "win7-x86") {
    Invoke-Win7Build -SelectedTarget egui
  } else {
    Invoke-EguiBuild
  }
  return
}

if ($Arch -eq "win7-x86") {
  Invoke-Win7Build -SelectedTarget $Target
} else {
  Invoke-LinuxBuild -SelectedTarget $Target -SelectedArch $Arch
}
