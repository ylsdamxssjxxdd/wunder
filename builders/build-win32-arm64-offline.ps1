param(
  [string]$RepoRoot = "",
  [string]$BuilderRoot = "",
  [string]$Image = "rcho-slint-arm64-ubuntu18:latest",
  [switch]$Docker
)

$ErrorActionPreference = "Stop"
if (-not $RepoRoot) { $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path }
else { $RepoRoot = (Resolve-Path $RepoRoot).Path }
if (-not $BuilderRoot) { $BuilderRoot = Join-Path (Split-Path $RepoRoot -Parent) "Rust-builder\kylin2" }
$scriptPath = "/workspace/builders/build-win32-arm64-offline.sh"

if (-not $Docker) {
  $bash = Get-Command bash.exe -ErrorAction SilentlyContinue
  if (-not $bash) { throw "bash.exe is required for the ARM64 Linux cross build; pass -Docker with Docker Desktop installed." }
  $env:WUNDER_REPO_ROOT = $RepoRoot
  $env:WUNDER_BUILDER_ROOT = $BuilderRoot
  & bash.exe (Join-Path $RepoRoot "builders/build-win32-arm64-offline.sh")
  if ($LASTEXITCODE -ne 0) { throw "Win32 ARM64 cross build failed with exit code $LASTEXITCODE" }
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
  "-w", "/workspace", $Image, "bash", $scriptPath
)
& docker @args
if ($LASTEXITCODE -ne 0) { throw "Win32 ARM64 cross build failed with exit code $LASTEXITCODE" }
