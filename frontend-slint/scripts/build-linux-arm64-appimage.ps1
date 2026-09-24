param(
  [string]$Image = "wunder-slint-arm64-ubuntu18:latest",
  [string]$RepoRoot = "",
  [string]$BuilderRoot = "",
  [string]$AppImageRuntime = "",
  [string]$OutputDirectory = ""
)

$ErrorActionPreference = "Stop"
if (-not $RepoRoot) { $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path }
else { $RepoRoot = (Resolve-Path $RepoRoot).Path }
if (-not $BuilderRoot) { $BuilderRoot = Join-Path (Split-Path $RepoRoot -Parent) "Rust-builder\kylin2" }
if (-not $AppImageRuntime) {
  $candidate = Join-Path (Split-Path $RepoRoot -Parent) "rcho\target\electron\release\rcho-0.1.5-linux-arm64.AppImage"
  if (Test-Path -LiteralPath $candidate) { $AppImageRuntime = $candidate }
}
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $RepoRoot "target\slint\dist\linux-arm64" }
elseif (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory = Join-Path $RepoRoot $OutputDirectory }
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)

if (-not (Test-Path -LiteralPath (Join-Path $RepoRoot "frontend-slint\Cargo.toml"))) { throw "Slint manifest was not found under $RepoRoot" }
if (-not (Test-Path -LiteralPath $BuilderRoot)) { throw "ARM64 Slint offline SDK was not found: $BuilderRoot" }
if (-not $AppImageRuntime -or -not (Test-Path -LiteralPath $AppImageRuntime)) {
  throw "An ARM64 AppImage runtime is required. Pass -AppImageRuntime <existing ARM64 AppImage>; an existing rcho runtime is auto-detected when available."
}
docker version --format '{{.Server.Version}}' | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Docker is unavailable. Start Docker Desktop and retry." }
docker image inspect $Image --format '{{.Id}}' | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Build image is unavailable: $Image. Build packaging/docker/Dockerfile.ubuntu18-arm64-slint first." }

$targetRoot = Join-Path $RepoRoot "target\linux-arm64-ubuntu18-slint"
New-Item -ItemType Directory -Force -Path $targetRoot, $OutputDirectory | Out-Null
$logPath = Join-Path $targetRoot "appimage-build.log"
$dockerArgs = @(
  "run", "--rm", "--platform", "linux/arm64",
  "-e", "WUNDER_REPO_ROOT=/workspace",
  "-e", "WUNDER_BUILDER_ROOT=/builder/kylin2",
  "-e", "WUNDER_OFFLINE_ROOT=/builder/kylin2/offline",
  "-e", "WUNDER_APPIMAGE_RUNTIME=/builder/appimage-runtime/rcho-arm64.AppImage",
  "-e", "WUNDER_OUTPUT_DIR=/workspace/target/slint/dist/linux-arm64",
  "-e", "CARGO_HOME=/workspace/target/linux-arm64-ubuntu18-slint/cargo-home",
  "-e", "CARGO_TARGET_DIR=/workspace/target/linux-arm64-ubuntu18-slint/cargo",
  "-e", "WUNDER_SLINT_LINUX_MAX_GLIBC=2.27",
  "-v", "${RepoRoot}:/workspace",
  "-v", "${BuilderRoot}:/builder/kylin2:ro",
  "-v", "${AppImageRuntime}:/builder/appimage-runtime/rcho-arm64.AppImage:ro",
  "-w", "/workspace",
  $Image,
  "bash", "/workspace/frontend-slint/scripts/build-linux-arm64-appimage.sh"
)
Write-Host "[wunder-slint-appimage] Docker image: $Image"
Write-Host "[wunder-slint-appimage] Offline SDK: $BuilderRoot"
Write-Host "[wunder-slint-appimage] AppImage runtime: $AppImageRuntime"
$previousErrorActionPreference = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try { & docker @dockerArgs 2>&1 | Tee-Object -FilePath $logPath; $dockerExitCode = $LASTEXITCODE }
finally { $ErrorActionPreference = $previousErrorActionPreference }
if ($dockerExitCode -ne 0) { throw "ARM64 Slint AppImage build failed. See $logPath" }
$artifacts = @(Get-ChildItem -LiteralPath $OutputDirectory -Filter "*.AppImage" -File)
if ($artifacts.Count -ne 1) { throw "Expected exactly one AppImage in $OutputDirectory, found $($artifacts.Count)" }
Write-Host "[wunder-slint-appimage] Produced: $($artifacts[0].FullName) ($($artifacts[0].Length) bytes)"
