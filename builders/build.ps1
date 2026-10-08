param(
  [Alias("t")][ValidateSet("desktop", "cli")][string]$Target = "",
  [Alias("a")][ValidateSet("linux-arm64", "linux-amd64", "win7-x86")][string]$Arch = "",
  [switch]$All,
  [string]$KylinBuilderRoot = "",
  [string]$Win7BuilderRoot = "",
  [string]$Image = "rcho-slint-arm64-ubuntu18:latest",
  [string]$AppImageRuntimeArm64 = "",
  [string]$AppImageRuntimeAmd64 = "",
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

# AppImage runtimes default to the blobs shipped in the kylin-arm offline SDK,
# so plain `-t desktop -a linux-arm64` works without extra parameters; the
# explicit parameters (or the WUNDER_APPIMAGE_RUNTIME_* env vars) only override.
if (-not $AppImageRuntimeArm64) { $AppImageRuntimeArm64 = $env:WUNDER_APPIMAGE_RUNTIME_ARM64 }
if (-not $AppImageRuntimeAmd64) { $AppImageRuntimeAmd64 = $env:WUNDER_APPIMAGE_RUNTIME_AMD64 }
if (-not $AppImageRuntimeArm64) { $AppImageRuntimeArm64 = Join-Path $KylinBuilderRoot "offline\appimage-runtime\runtime-aarch64" }
if (-not $AppImageRuntimeAmd64) { $AppImageRuntimeAmd64 = Join-Path $KylinBuilderRoot "offline\appimage-runtime\runtime-x86_64" }

if ($All) {
  if ($Target -or $Arch) { throw "-All selects every distribution; do not combine it with -Target or -Arch." }
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
  # Both Linux Desktop distributions are AppImages; CLI stays a plain ELF.
  $needsRuntime = $SelectedTarget -eq "desktop"
  $runtime = if ($SelectedArch -eq "linux-arm64") { $AppImageRuntimeArm64 } else { $AppImageRuntimeAmd64 }
  if ($needsRuntime -and -not $runtime) {
    throw "AppImage runtime for $SelectedArch was not resolved; pass -AppImageRuntimeArm64/-AppImageRuntimeAmd64 or place the blob under $KylinBuilderRoot\offline\appimage-runtime."
  }
  $dockerArgs = @(
    "run", "--rm", "--network", "none", "--platform", "linux/arm64",
    "-e", "WUNDER_IN_DOCKER=1",
    "-e", "WUNDER_REPO_ROOT=/workspace",
    "-e", "WUNDER_BUILDER_ROOT=/builder/kylin-arm",
    "-e", "WUNDER_OFFLINE_ROOT=/builder/kylin-arm/offline",
    "-v", "${repoRoot}:/workspace",
    "-v", "${KylinBuilderRoot}:/builder/kylin-arm:ro"
  )
  if ($needsRuntime) {
    $runtimePath = [IO.Path]::GetFullPath($runtime)
    if (-not (Test-Path -LiteralPath $runtimePath -PathType Leaf)) {
      throw "AppImage runtime does not exist: $runtimePath"
    }
    $runtimeName = if ($SelectedArch -eq "linux-arm64") { "linux-arm64.AppImage" } else { "linux-amd64.AppImage" }
    $runtimeVariable = if ($SelectedArch -eq "linux-arm64") { "WUNDER_APPIMAGE_RUNTIME_ARM64" } else { "WUNDER_APPIMAGE_RUNTIME_AMD64" }
    $dockerArgs += @(
      "-e", ("{0}=/builder/appimage-runtime/{1}" -f $runtimeVariable, $runtimeName),
      "-v", "${runtimePath}:/builder/appimage-runtime/${runtimeName}:ro"
    )
  }
  $dockerArgs += @(
    "-w", "/workspace",
    $Image,
    "bash", "/workspace/build.sh", "-t", $SelectedTarget, "-a", $SelectedArch, "--native"
  )
  & docker @dockerArgs
  if ($LASTEXITCODE -ne 0) { throw "Docker build failed with exit code $LASTEXITCODE" }
}

function Invoke-Win7Build {
  param([ValidateSet("desktop", "cli")][string]$SelectedTarget)

  $scriptName = if ($SelectedTarget -eq "desktop") { "build-win7-offline.ps1" } else { "build-win7-cli.ps1" }
  & (Join-Path $PSScriptRoot $scriptName) -BuilderRoot $Win7BuilderRoot -Jobs $Jobs -Check:$Check
  if ($LASTEXITCODE -ne 0) { throw "Win7 $SelectedTarget build failed with exit code $LASTEXITCODE" }
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

if ($Arch -eq "win7-x86") {
  Invoke-Win7Build -SelectedTarget $Target
} else {
  Invoke-LinuxBuild -SelectedTarget $Target -SelectedArch $Arch
}
