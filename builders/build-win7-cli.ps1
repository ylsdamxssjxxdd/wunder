param(
  [string]$BuilderRoot = "",
  [string]$CargoCacheRoot = "",
  [ValidateRange(1, 8)][int]$Jobs = 8,
  [switch]$Check
)

# Build the plain Win7-compatible CLI with the same offline Rust/MinGW toolchain
# as the Slint Desktop release. This stays separate from the desktop builder
# because the CLI does not need a UI manifest, fonts, or an AppImage stage.
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $BuilderRoot) {
  $BuilderRoot = if ($env:WUNDER_BUILDER_ROOT) { $env:WUNDER_BUILDER_ROOT } else { Join-Path (Split-Path $repoRoot -Parent) "Rust-builder\win7" }
}
$offline = Join-Path $BuilderRoot "offline"
$toolchainBin = Join-Path $offline "rust\toolchains\nightly-2026-03-14-x86_64-pc-windows-gnu\bin"
$vendor = Join-Path $offline "cargo-vendor-slint"
$target = "i686-win7-windows-gnu"

function Require-Path([string]$Path, [string]$Label) {
  if (-not (Test-Path -LiteralPath $Path)) { throw "$Label is missing: $Path" }
}

foreach ($required in @(
  @{ Path = (Join-Path $repoRoot "Cargo.toml"); Label = "Workspace manifest" },
  @{ Path = (Join-Path $toolchainBin "cargo.exe"); Label = "Offline Cargo" },
  @{ Path = (Join-Path $toolchainBin "rustc.exe"); Label = "Offline rustc" },
  @{ Path = (Join-Path $offline "mingw64\bin\gcc.exe"); Label = "Win7 host GCC" },
  @{ Path = (Join-Path $offline "mingw32\bin\i686-w64-mingw32-gcc.exe"); Label = "Win7 target GCC" },
  @{ Path = (Join-Path $offline "mingw32\bin\objdump.exe"); Label = "Win7 PE inspection tool" },
  @{ Path = (Join-Path $offline "mingw32\bin\strip.exe"); Label = "Win7 PE strip tool" },
  @{ Path = $vendor; Label = "Offline Cargo vendor" }
)) {
  Require-Path $required.Path $required.Label
}

# The shared vendor is maintained as the Desktop/CLI dependency union. Check
# the runtime crates before invoking Cargo so an incomplete offline SDK fails
# with a direct, actionable error instead of a long resolver trace.
foreach ($crate in @("axum-*", "reqwest", "tokio-rustls", "syntect")) {
  if (-not (Get-ChildItem -LiteralPath $vendor -Directory -Filter $crate -ErrorAction SilentlyContinue | Select-Object -First 1)) {
    throw "Offline Cargo vendor is missing the CLI runtime dependency pattern: $crate"
  }
}

$cargoHome = if ($CargoCacheRoot) {
  [IO.Path]::GetFullPath($CargoCacheRoot)
} else {
  Join-Path $repoRoot "target\cli-win7-x86\cargo-home"
}
if ($CargoCacheRoot) {
  Require-Path (Join-Path $cargoHome "registry") "Offline Cargo registry cache"
} else {
  New-Item -ItemType Directory -Force -Path $cargoHome | Out-Null
  $vendorPath = [IO.Path]::GetFullPath($vendor).Replace('\', '/')
@"
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$vendorPath"
[net]
offline = true
"@ | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $cargoHome "config.toml")
}

$hostMingwBin = Join-Path $offline "mingw64\bin"
$targetMingwBin = Join-Path $offline "mingw32\bin"
$targetDir = Join-Path $repoRoot "target\cli-win7-x86\cargo"
$outputDir = Join-Path $repoRoot "target\cli\dist\win7-x86"
New-Item -ItemType Directory -Force -Path $targetDir, $outputDir | Out-Null

$env:CARGO_HOME = $cargoHome
$env:CARGO_TARGET_DIR = $targetDir
$env:CARGO_NET_OFFLINE = "true"
$env:RUSTC = Join-Path $toolchainBin "rustc.exe"
$env:PATH = "$toolchainBin;$hostMingwBin;$targetMingwBin;$env:PATH"
$env:CARGO_TARGET_I686_WIN7_WINDOWS_GNU_LINKER = Join-Path $targetMingwBin "i686-w64-mingw32-gcc.exe"
$env:CC_i686_win7_windows_gnu = $env:CARGO_TARGET_I686_WIN7_WINDOWS_GNU_LINKER
$env:CXX_i686_win7_windows_gnu = Join-Path $targetMingwBin "i686-w64-mingw32-g++.exe"
$env:AR_i686_win7_windows_gnu = Join-Path $targetMingwBin "i686-w64-mingw32-gcc-ar.exe"
$env:RANLIB_i686_win7_windows_gnu = Join-Path $targetMingwBin "i686-w64-mingw32-gcc-ranlib.exe"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $hostMingwBin "gcc.exe"
$env:CC_x86_64_pc_windows_gnu = $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER
$env:CXX_x86_64_pc_windows_gnu = Join-Path $hostMingwBin "g++.exe"
$env:AR_x86_64_pc_windows_gnu = Join-Path $hostMingwBin "gcc-ar.exe"
$env:RANLIB_x86_64_pc_windows_gnu = Join-Path $hostMingwBin "gcc-ranlib.exe"
$env:RAYON_NUM_THREADS = "$Jobs"

# windows-targets does not infer the custom Win7 target vendor. Make the
# prebuilt GNU import library discoverable, as the Desktop Win7 builder does.
$windowsImportDirs = @(
  Get-ChildItem -LiteralPath $vendor -Directory -Filter "windows_i686_gnu-*" -ErrorAction SilentlyContinue |
    ForEach-Object { Join-Path $_.FullName "lib" } |
    Where-Object { Test-Path -LiteralPath $_ }
)
if ($windowsImportDirs.Count -gt 0) {
  $env:CARGO_TARGET_I686_WIN7_WINDOWS_GNU_RUSTFLAGS = (
    $windowsImportDirs | Select-Object -Unique | ForEach-Object { "-Lnative=$_" }
  ) -join " "
}

$cargo = Join-Path $toolchainBin "cargo.exe"
$cargoArgs = @("-Z", "build-std=std,panic_abort", "build", "--locked", "--offline", "--release", "-p", "wunder-cli", "--bin", "wunder-cli", "--target", $target, "-j", "$Jobs")
if ($Check) { $cargoArgs[$cargoArgs.IndexOf("build")] = "check" }
& $cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { throw "Win7 CLI build failed with exit code $LASTEXITCODE" }
if ($Check) { return }

$exe = Join-Path $targetDir "$target\release\wunder-cli.exe"
Require-Path $exe "Win7 CLI executable"
$objdump = Join-Path $targetMingwBin "objdump.exe"
$strip = Join-Path $targetMingwBin "strip.exe"
Require-Path $objdump "PE inspection tool"
Require-Path $strip "PE strip tool"
$peDetails = (& $objdump -p $exe) -join [Environment]::NewLine
if ($LASTEXITCODE -ne 0) { throw "PE import inspection failed with exit code $LASTEXITCODE" }
if ($peDetails -notmatch 'file format pei-i386') { throw "Win7 CLI must be a 32-bit i386 PE: $exe" }
if ($peDetails -match '(?i)combase\.dll|api-ms-win-|ext-ms-win-|GetDpiForWindow|WaitOnAddress|WakeByAddress|SetThreadDescription') {
  throw "Win7 CLI imports an unsupported Windows API"
}
& $strip --strip-all $exe
if ($LASTEXITCODE -ne 0) { throw "Win7 CLI strip failed with exit code $LASTEXITCODE" }

$versionText = Get-Content -LiteralPath (Join-Path $repoRoot "Cargo.toml") -Raw
$versionMatch = [regex]::Match($versionText, '(?m)^version\s*=\s*"(?<version>[0-9A-Za-z][0-9A-Za-z.+-]*)"\s*$')
if (-not $versionMatch.Success) { throw "Could not read a safe workspace version" }
$release = Join-Path $outputDir "wunder-cli-$($versionMatch.Groups['version'].Value)-win7-x86.exe"
Copy-Item -LiteralPath $exe -Destination $release -Force
$peDetails | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $outputDir "imports.txt")
Write-Host "Win7 CLI release executable: $release"
