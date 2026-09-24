param(
  [string]$Image = "wunder-slint-arm64-ubuntu18:latest",
  [string]$RepoRoot = "",
  [string]$BuilderRoot = "",
  [string]$AppImageRuntime = "",
  [string]$OutputDirectory = ""
)

$ErrorActionPreference = "Stop"
if (-not $RepoRoot) { $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path }
else { $RepoRoot = (Resolve-Path $RepoRoot).Path }
if (-not $BuilderRoot) { $BuilderRoot = Join-Path (Split-Path $RepoRoot -Parent) "Rust-builder\kylin-arm" }
if (-not $AppImageRuntime -and $env:WUNDER_APPIMAGE_RUNTIME) { $AppImageRuntime = $env:WUNDER_APPIMAGE_RUNTIME }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $RepoRoot "target\slint\dist\linux-arm64" }
elseif (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory = Join-Path $RepoRoot $OutputDirectory }
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)

if (-not (Test-Path -LiteralPath (Join-Path $RepoRoot "frontend-slint\Cargo.toml"))) { throw "Slint manifest was not found under $RepoRoot" }
if (-not (Test-Path -LiteralPath $BuilderRoot)) { throw "ARM64 Slint offline SDK was not found: $BuilderRoot" }
if (-not $AppImageRuntime -or -not (Test-Path -LiteralPath $AppImageRuntime)) {
  throw "An ARM64 AppImage runtime is required. Pass -AppImageRuntime <existing ARM64 AppImage> or set WUNDER_APPIMAGE_RUNTIME."
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
  "-e", "WUNDER_BUILDER_ROOT=/builder/kylin-arm",
  "-e", "WUNDER_OFFLINE_ROOT=/builder/kylin-arm/offline",
  "-e", "WUNDER_APPIMAGE_RUNTIME=/builder/appimage-runtime/rcho-arm64.AppImage",
  "-e", "WUNDER_OUTPUT_DIR=/workspace/target/slint/dist/linux-arm64",
  "-e", "CARGO_HOME=/workspace/target/linux-arm64-ubuntu18-slint/cargo-home",
  "-e", "CARGO_TARGET_DIR=/workspace/target/linux-arm64-ubuntu18-slint/cargo",
  "-e", "WUNDER_SLINT_LINUX_MAX_GLIBC=2.27",
  "-v", "${RepoRoot}:/workspace",
  "-v", "${BuilderRoot}:/builder/kylin-arm:ro",
  "-v", "${AppImageRuntime}:/builder/appimage-runtime/rcho-arm64.AppImage:ro",
  "-w", "/workspace",
  $Image,
  "bash", "/workspace/builders/build-linux-arm64-appimage.sh"
)
Write-Host "[wunder-slint-appimage] Docker image: $Image"
Write-Host "[wunder-slint-appimage] Offline SDK: $BuilderRoot"
Write-Host "[wunder-slint-appimage] AppImage runtime: $AppImageRuntime"
$previousErrorActionPreference = $ErrorActionPreference
$ErrorActionPreference = "Continue"
try { & docker @dockerArgs 2>&1 | Tee-Object -FilePath $logPath; $dockerExitCode = $LASTEXITCODE }
finally { $ErrorActionPreference = $previousErrorActionPreference }
if ($dockerExitCode -ne 0) { throw "ARM64 Slint AppImage build failed. See $logPath" }
$versionMatch = [regex]::Match((Get-Content -Raw (Join-Path $RepoRoot "frontend-slint\Cargo.toml")), '(?m)^version\s*=\s*"(?<version>[0-9A-Za-z][0-9A-Za-z.+-]*)"\s*$')
if (-not $versionMatch.Success) { throw "Could not read a safe package version from frontend-slint\Cargo.toml" }
$artifact = Join-Path $OutputDirectory "wunder-slint-$($versionMatch.Groups['version'].Value)-linux-arm64.AppImage"
if (-not (Test-Path -LiteralPath $artifact)) { throw "Container reported success but did not produce: $artifact" }
Write-Host "[wunder-slint-appimage] Produced: $artifact ($((Get-Item -LiteralPath $artifact).Length) bytes)"
