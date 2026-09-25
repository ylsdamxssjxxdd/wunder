param(
  [Alias("t")][ValidateSet("desktop", "cli")][string]$Target = "",
  [Alias("a")][ValidateSet("linux-arm64", "linux-amd64", "win32-x86", "win7-x86")][string]$Arch = "",
  [switch]$All,
  [switch]$AppImage,
  [string]$KylinBuilderRoot = "",
  [string]$Win7BuilderRoot = "",
  [string]$Image = "rcho-slint-arm64-ubuntu18:latest",
  [string]$AppImageRuntimeArm64 = "",
  [string]$AppImageRuntimeAmd64 = "",
  [ValidateRange(1, 8)][int]$Jobs = 8,
  [switch]$Check
)

# Public Windows build dispatcher. Linux/Win32 cross work runs in the kylin-arm
# Docker image; the Win7 x86 target stays on the local win7 offline SDK.
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $KylinBuilderRoot) { $KylinBuilderRoot = if ($env:WUNDER_KYLIN_BUILDER_ROOT) { $env:WUNDER_KYLIN_BUILDER_ROOT } else { Join-Path (Split-Path $repoRoot -Parent) "Rust-builder\kylin-arm" } }
if (-not $Win7BuilderRoot) { $Win7BuilderRoot = if ($env:WUNDER_WIN7_BUILDER_ROOT) { $env:WUNDER_WIN7_BUILDER_ROOT } else { Join-Path (Split-Path $repoRoot -Parent) "Rust-builder\win7" } }
$KylinBuilderRoot = [IO.Path]::GetFullPath($KylinBuilderRoot)
$Win7BuilderRoot = [IO.Path]::GetFullPath($Win7BuilderRoot)

if ($All) {
  if ($Target -or $Arch -or $AppImage) { throw "-All selects every distribution; do not combine it with -Target, -Arch, or -AppImage." }
} elseif (-not $Target -or -not $Arch) {
  throw "Specify both -Target desktop|cli and -Arch linux-arm64|linux-amd64|win32-x86|win7-x86, or use -All."
}
if ($AppImage -and ($Target -ne "desktop" -or $Arch -ne "linux-amd64")) {
  throw "-AppImage is only valid for -Target desktop -Arch linux-amd64. CLI never uses AppImage."
}

function Invoke-LinuxBuild {
  param(
    [ValidateSet("desktop", "cli")][string]$SelectedTarget,
    [ValidateSet("linux-arm64", "linux-amd64", "win32-x86")][string]$SelectedArch,
    [switch]$Package
  )

  if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "docker.exe is required for the Linux/Win32 Docker build dispatcher."
  }
  $needsRuntime = $SelectedTarget -eq "desktop" -and ($SelectedArch -eq "linux-arm64" -or $Package)
  $runtime = if ($SelectedArch -eq "linux-arm64") { $AppImageRuntimeArm64 } else { $AppImageRuntimeAmd64 }
  if ($needsRuntime -and -not $runtime) {
    throw "An AppImage runtime is required for $SelectedArch Desktop packaging."
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
  if ($Package) { $dockerArgs += "--appimage" }
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
  if (-not $AppImageRuntimeArm64 -or -not $AppImageRuntimeAmd64) {
    throw "-All packages both Linux Desktop AppImages. Provide -AppImageRuntimeArm64 and -AppImageRuntimeAmd64."
  }
  foreach ($selectedArch in @("linux-arm64", "linux-amd64", "win32-x86")) {
    Invoke-LinuxBuild -SelectedTarget desktop -SelectedArch $selectedArch -Package:($selectedArch -eq "linux-amd64")
    Invoke-LinuxBuild -SelectedTarget cli -SelectedArch $selectedArch
  }
  Invoke-Win7Build -SelectedTarget desktop
  Invoke-Win7Build -SelectedTarget cli
  return
}

if ($Arch -eq "win7-x86") {
  Invoke-Win7Build -SelectedTarget $Target
} else {
  Invoke-LinuxBuild -SelectedTarget $Target -SelectedArch $Arch -Package:$AppImage
}
