param(
  [string]$RepoRoot = "",
  [string]$BuilderRoot = "",
  [string]$Image = "rcho-slint-arm64-ubuntu18:latest",
  [string]$AppImageRuntime = "",
  [switch]$Docker,
  [switch]$AppImage
)

$ErrorActionPreference = "Stop"
if (-not $RepoRoot) { $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path }
else { $RepoRoot = (Resolve-Path $RepoRoot).Path }
if (-not $BuilderRoot) { $BuilderRoot = Join-Path (Split-Path $RepoRoot -Parent) "Rust-builder\kylin2" }
$scriptName = if ($AppImage) { "build-linux-amd64-appimage.sh" } else { "build-linux-amd64-offline.sh" }
$containerScript = "/workspace/builders/$scriptName"
if ($AppImage -and -not $AppImageRuntime -and $env:WUNDER_APPIMAGE_RUNTIME) { $AppImageRuntime = $env:WUNDER_APPIMAGE_RUNTIME }

if (-not $Docker) {
  $bash = Get-Command bash.exe -ErrorAction SilentlyContinue
  if (-not $bash) { throw "bash.exe is required for the ARM64 Linux cross build; pass -Docker with Docker Desktop installed." }
  $env:WUNDER_REPO_ROOT = $RepoRoot
  $env:WUNDER_BUILDER_ROOT = $BuilderRoot
  if ($AppImageRuntime) { $env:WUNDER_APPIMAGE_RUNTIME = $AppImageRuntime }
  & bash.exe (Join-Path $RepoRoot "builders/$scriptName")
  if ($LASTEXITCODE -ne 0) { throw "Linux amd64 cross build failed with exit code $LASTEXITCODE" }
  return
}

& docker version --format '{{.Server.Version}}' | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Docker is unavailable. Start Docker Desktop and retry." }
& docker image inspect $Image --format '{{.Id}}' | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Build image is unavailable: $Image" }
$args = @(
  "run", "--rm", "--network", "none", "--platform", "linux/arm64",
  "-e", "WUNDER_REPO_ROOT=/workspace",
  "-e", "WUNDER_BUILDER_ROOT=/builder/kylin2",
  "-e", "WUNDER_OFFLINE_ROOT=/builder/kylin2/offline",
  "-v", "${RepoRoot}:/workspace",
  "-v", "${BuilderRoot}:/builder/kylin2:ro",
  "-w", "/workspace", $Image, "bash", $containerScript
)
if ($AppImage -and $AppImageRuntime) {
  $runtime = (Resolve-Path $AppImageRuntime).Path
  $args = @(
    "run", "--rm", "--network", "none", "--platform", "linux/arm64",
    "-e", "WUNDER_REPO_ROOT=/workspace",
    "-e", "WUNDER_BUILDER_ROOT=/builder/kylin2",
    "-e", "WUNDER_OFFLINE_ROOT=/builder/kylin2/offline",
    "-e", "WUNDER_APPIMAGE_RUNTIME=/builder/appimage-runtime/runtime.AppImage",
    "-v", "${RepoRoot}:/workspace",
    "-v", "${BuilderRoot}:/builder/kylin2:ro",
    "-v", "${runtime}:/builder/appimage-runtime/runtime.AppImage:ro",
    "-w", "/workspace", $Image, "bash", $containerScript
  )
}
& docker @args
if ($LASTEXITCODE -ne 0) { throw "Linux amd64 cross build failed with exit code $LASTEXITCODE" }
