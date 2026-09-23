param(
    [string]$BuilderRoot = "",
    [ValidateRange(1, 8)][int]$Jobs = 8,
    [switch]$NativeRuntime,
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
foreach ($required in @("$toolchainBin\cargo.exe", "$offline\mingw64\bin\gcc.exe", "$offline\mingw32\bin\i686-w64-mingw32-gcc.exe", $vendor)) {
    if (!(Test-Path -LiteralPath $required)) { throw "Offline SDK component missing: $required" }
}
# Keep the shared SDK read-only; use a project-local Cargo configuration.
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
$env:CARGO_NET_OFFLINE = "true"
$env:RUSTC = Join-Path $toolchainBin "rustc.exe"
$env:PATH = "$toolchainBin;$env:PATH"
& (Join-Path $PSScriptRoot "build-win7.ps1") -CargoExe (Join-Path $toolchainBin "cargo.exe") -MingwBin (Join-Path $offline "mingw32\bin") -HostMingwBin (Join-Path $offline "mingw64\bin") -Jobs $Jobs -Check:$Check -NativeRuntime:$NativeRuntime
