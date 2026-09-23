param(
    [string]$BuilderRoot = "",
    [string]$CargoCacheRoot = "",
    [ValidateRange(1, 8)][int]$Jobs = 8,
    [switch]$Check
)
$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (!$BuilderRoot) {
    $BuilderRoot = if ($env:WUNDER_BUILDER_ROOT) { $env:WUNDER_BUILDER_ROOT } else { Join-Path (Split-Path -Parent $repoRoot) "Rust-builder\win7" }
}
$offline = Join-Path $BuilderRoot "offline"
$toolchainBin = Join-Path $offline "rust\toolchains\nightly-2026-03-14-x86_64-pc-windows-gnu\bin"
$vendor = Join-Path $offline "cargo-vendor-slint"
foreach ($required in @("$toolchainBin\cargo.exe", "$offline\mingw64\bin\gcc.exe", "$offline\mingw32\bin\i686-w64-mingw32-gcc.exe")) {
    if (!(Test-Path -LiteralPath $required)) { throw "Offline SDK component missing: $required" }
}
if ($CargoCacheRoot) {
    # A fully populated Cargo registry cache is an offline source too. This
    # supports the native backend closure without modifying the shared SDK.
    $env:CARGO_HOME = [IO.Path]::GetFullPath($CargoCacheRoot)
    if (!(Test-Path -LiteralPath (Join-Path $env:CARGO_HOME "registry"))) {
        throw "Cargo registry cache missing: $CargoCacheRoot"
    }
} else {
    if (!(Get-ChildItem -LiteralPath $vendor -Directory -Filter "axum-*" -ErrorAction SilentlyContinue)) {
        throw "The frontend-only SDK lacks native runtime dependencies. Supply -CargoCacheRoot with a populated Cargo registry cache, or update cargo-vendor-slint from frontend-slint/Cargo.lock."
    }
    $cargoCache = Join-Path $repoRoot "target\frontend-slint\cargo-home"
    New-Item -ItemType Directory -Force -Path $cargoCache | Out-Null
    $vendorPath = [IO.Path]::GetFullPath($vendor).Replace('\', '/')
@"
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$vendorPath"
[net]
offline = true
"@ | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $cargoCache "config.toml")
    $env:CARGO_HOME = $cargoCache
}
# AWS-LC's Win7 GNU build uses the same offline C toolchain as the backend.
foreach ($nativeTool in @("cmake64\bin", "nasm64")) {
    $toolPath = Join-Path $offline $nativeTool
    if (Test-Path -LiteralPath $toolPath) { $env:PATH = "$toolPath;$env:PATH" }
}
$env:CARGO_NET_OFFLINE = "true"
$env:RUSTC = Join-Path $toolchainBin "rustc.exe"
$env:PATH = "$toolchainBin;$env:PATH"
& (Join-Path $PSScriptRoot "build-win7.ps1") -CargoExe (Join-Path $toolchainBin "cargo.exe") -MingwBin (Join-Path $offline "mingw32\bin") -HostMingwBin (Join-Path $offline "mingw64\bin") -Jobs $Jobs -Check:$Check
