param(
  [string]$BuilderRoot = "",
  [string]$CargoCacheRoot = "",
  [ValidateRange(1, 8)][int]$Jobs = 8,
  [switch]$Check
)

# Produce a standalone 32-bit Win7 Egui release with the same offline
# toolchain and PE gate used by the Slint desktop release.  Do not replace this
# target with i686-pc-windows-gnu: the repository's Win7 target and rebuilt
# standard library deliberately avoid imports unavailable on Windows 7.
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$manifest = Join-Path $repoRoot "crates\wunder-egui\Cargo.toml"
$target = "i686-win7-windows-gnu"

if (-not $BuilderRoot) {
  $BuilderRoot = if ($env:WUNDER_BUILDER_ROOT) {
    $env:WUNDER_BUILDER_ROOT
  } else {
    Join-Path (Split-Path $repoRoot -Parent) "Rust-builder\win7"
  }
}
$offline = Join-Path $BuilderRoot "offline"
$toolchainBin = Join-Path $offline "rust\toolchains\nightly-2026-03-14-x86_64-pc-windows-gnu\bin"
# Egui has a larger locked closure than the Slint frontend.  Keep its vendor
# snapshot separate so updating one frontend cannot silently change the other.
$vendor = if ($env:WUNDER_EGUI_CARGO_VENDOR) {
  [IO.Path]::GetFullPath($env:WUNDER_EGUI_CARGO_VENDOR)
} else {
  Join-Path $offline "cargo-vendor-egui"
}
$hostMingwBin = Join-Path $offline "mingw64\bin"
$targetMingwBin = Join-Path $offline "mingw32\bin"
$targetDir = Join-Path $repoRoot "target\wunder-egui-win7-x86\cargo"
$outputDir = Join-Path $repoRoot "target\wunder-egui\dist\win7-x86"

function Require-Path([string]$Path, [string]$Label) {
  if (-not (Test-Path -LiteralPath $Path)) {
    throw "$Label is missing: $Path"
  }
}

foreach ($required in @(
  @{ Path = (Join-Path $repoRoot "Cargo.toml"); Label = "Workspace manifest" },
  @{ Path = $manifest; Label = "Egui manifest" },
  @{ Path = (Join-Path $toolchainBin "cargo.exe"); Label = "Offline Cargo" },
  @{ Path = (Join-Path $toolchainBin "rustc.exe"); Label = "Offline rustc" },
  @{ Path = (Join-Path $hostMingwBin "gcc.exe"); Label = "Win7 host GCC" },
  @{ Path = (Join-Path $targetMingwBin "i686-w64-mingw32-gcc.exe"); Label = "Win7 target GCC" },
  @{ Path = (Join-Path $targetMingwBin "objdump.exe"); Label = "Win7 PE inspection tool" },
  @{ Path = (Join-Path $targetMingwBin "strip.exe"); Label = "Win7 PE strip tool" }
)) {
  Require-Path $required.Path $required.Label
}

# A caller can use a fully populated offline Cargo cache while bringing a new
# Egui dependency closure into the SDK.  The normal path stays self-contained
# and reads only the repository's Win7 vendor tree.
$cargoHome = if ($CargoCacheRoot) {
  [IO.Path]::GetFullPath($CargoCacheRoot)
} else {
  Join-Path $repoRoot "target\wunder-egui-win7-x86\cargo-home"
}
if ($CargoCacheRoot) {
  Require-Path (Join-Path $cargoHome "registry") "Offline Cargo registry cache"
} else {
  Require-Path $vendor "Offline Cargo vendor"
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

New-Item -ItemType Directory -Force -Path $targetDir, $outputDir | Out-Null

$cargo = Join-Path $toolchainBin "cargo.exe"
$rustc = Join-Path $toolchainBin "rustc.exe"
$linker = Join-Path $targetMingwBin "i686-w64-mingw32-gcc.exe"
$env:CARGO_HOME = $cargoHome
$env:CARGO_TARGET_DIR = $targetDir
$env:CARGO_NET_OFFLINE = "true"
$env:RUSTC = $rustc
$env:PATH = "$toolchainBin;$hostMingwBin;$targetMingwBin;$env:PATH"
$env:CARGO_TARGET_I686_WIN7_WINDOWS_GNU_LINKER = $linker
$env:CC_i686_win7_windows_gnu = $linker
$env:CXX_i686_win7_windows_gnu = Join-Path $targetMingwBin "i686-w64-mingw32-g++.exe"
$env:AR_i686_win7_windows_gnu = Join-Path $targetMingwBin "i686-w64-mingw32-gcc-ar.exe"
$env:RANLIB_i686_win7_windows_gnu = Join-Path $targetMingwBin "i686-w64-mingw32-gcc-ranlib.exe"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $hostMingwBin "gcc.exe"
$env:CC_x86_64_pc_windows_gnu = $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER
$env:CXX_x86_64_pc_windows_gnu = Join-Path $hostMingwBin "g++.exe"
$env:AR_x86_64_pc_windows_gnu = Join-Path $hostMingwBin "gcc-ar.exe"
$env:RANLIB_x86_64_pc_windows_gnu = Join-Path $hostMingwBin "gcc-ranlib.exe"

$versionDetails = & $rustc -vV
if ($LASTEXITCODE -ne 0) { throw "Offline rustc could not run" }
if (-not ($versionDetails -contains "host: x86_64-pc-windows-gnu")) {
  throw "Win7 release builds require x86_64-pc-windows-gnu host Rust to avoid the 32-bit LLVM address-space limit."
}
$availableTargets = & $rustc --print target-list
if ($LASTEXITCODE -ne 0 -or -not ($availableTargets -contains $target)) {
  throw "$target is not available in the offline Win7 toolchain"
}
$rustSrc = Join-Path $toolchainBin "..\lib\rustlib\src\rust\library\Cargo.toml"
Require-Path $rustSrc "Offline rust-src"

# windows-targets does not discover GNU import libraries for the repository's
# custom target vendor.  Obtain their paths from Cargo metadata so this works
# with either the vendored SDK or a supplied offline registry cache.
$metadataArgs = @(
  "metadata", "--locked", "--offline", "--manifest-path", $manifest,
  "--format-version", "1", "--filter-platform", $target
)
$metadataJson = & $cargo @metadataArgs
if ($LASTEXITCODE -ne 0) {
  throw "Cargo metadata failed with exit code $LASTEXITCODE"
}
$metadata = $metadataJson | ConvertFrom-Json
$windowsImportPackages = @($metadata.packages | Where-Object { $_.name -eq "windows_i686_gnu" })
if ($windowsImportPackages.Count -eq 0) {
  throw "windows_i686_gnu is missing from the Egui dependency graph"
}
$windowsImportDirs = @()
foreach ($windowsImportPackage in $windowsImportPackages) {
  $windowsImportDir = Join-Path (Split-Path -Parent $windowsImportPackage.manifest_path) "lib"
  $windowsImportLibraries = @(Get-ChildItem -LiteralPath $windowsImportDir -Filter "libwindows.*.a" -File -ErrorAction SilentlyContinue)
  if ($windowsImportLibraries.Count -eq 0) {
    throw "Missing Windows import library in: $windowsImportDir"
  }
  $windowsImportDirs += $windowsImportDir
}
$existingRustFlags = $env:CARGO_TARGET_I686_WIN7_WINDOWS_GNU_RUSTFLAGS
$nativeLinkFlag = (($windowsImportDirs | Select-Object -Unique) |
  ForEach-Object { "-Lnative=$_" }) -join " "
$env:CARGO_TARGET_I686_WIN7_WINDOWS_GNU_RUSTFLAGS =
  if ($existingRustFlags) { "$existingRustFlags $nativeLinkFlag" } else { $nativeLinkFlag }

$cargoArgs = @(
  "-Z", "build-std=std,panic_abort",
  "build", "--locked", "--offline", "--release",
  "-p", "wunder-egui", "--bin", "wunder-egui", "--target", $target,
  "-j", "$Jobs"
)
if ($Check) { $cargoArgs[$cargoArgs.IndexOf("build")] = "check" }

$previousRayonThreads = $env:RAYON_NUM_THREADS
try {
  $env:RAYON_NUM_THREADS = "$Jobs"
  Write-Host "Egui Win7 build: host=x86_64-pc-windows-gnu, bin=wunder-egui, target=$target, jobs=$Jobs, Rayon threads=$Jobs"
  & $cargo @cargoArgs
  if ($LASTEXITCODE -ne 0) {
    throw "Win7 Egui build failed with exit code $LASTEXITCODE"
  }
} finally {
  $env:RAYON_NUM_THREADS = $previousRayonThreads
}

if ($Check) { return }

$executable = Join-Path $targetDir "$target\release\wunder-egui.exe"
Require-Path $executable "Win7 Egui executable"
$objdump = Join-Path $targetMingwBin "objdump.exe"
$strip = Join-Path $targetMingwBin "strip.exe"
$peDetails = (& $objdump -p $executable) -join [Environment]::NewLine
if ($LASTEXITCODE -ne 0) {
  throw "PE import inspection failed with exit code $LASTEXITCODE"
}
if ($peDetails -notmatch "file format pei-i386") {
  throw "Win7 Egui output must be a 32-bit i386 PE executable: $executable"
}

# The import gate is intentionally strict: Win7-compatible Rust codegen alone
# does not prevent a graphics/windowing dependency from linking a Win8+/WinRT
# DLL or symbol.  bcrypt.dll itself is supported by Win7; bcryptprimitives.dll
# is not shipped there and must never enter a release binary.
$importedDlls = @(
  [regex]::Matches($peDetails, "(?im)^\s*DLL Name:\s*(.+?)\s*$") |
    ForEach-Object { $_.Groups[1].Value.ToLowerInvariant() }
)
$blockedDllPatterns = @(
  "^bcryptprimitives\.dll$",
  "^combase\.dll$",
  "^api-ms-win-",
  "^ext-ms-win-"
)
$blockedDlls = @(
  $importedDlls | Where-Object {
    $candidate = $_
    $blockedDllPatterns | Where-Object { $candidate -match $_ } | Select-Object -First 1
  }
)
$blockedImports = @(
  "SystemParametersInfoForDpi",
  "GetDpiForWindow",
  "GetSystemTimePreciseAsFileTime",
  "WaitOnAddress",
  "WakeByAddressSingle",
  "WakeByAddressAll",
  "SetThreadDescription",
  "ProcessPrng",
  "PathCchStripPrefix"
) | Where-Object {
  $peDetails -match ("(?m)^\s*(?:[0-9a-fA-F]+\s+[0-9]+\s+)?" + [regex]::Escape($_) + "(?:A|W)?\s*$")
}
if ($blockedDlls.Count -gt 0 -or $blockedImports.Count -gt 0) {
  $detail = @()
  if ($blockedDlls.Count -gt 0) { $detail += "DLL: $($blockedDlls -join ', ')" }
  if ($blockedImports.Count -gt 0) { $detail += "symbol: $($blockedImports -join ', ')" }
  throw "Win7 PE import gate failed ($($detail -join '; '))"
}
Write-Host "Win7 PE import gate passed (no bcryptprimitives, WinRT/api-ms, or known Win8+ strong imports)."

& $strip --strip-all $executable
if ($LASTEXITCODE -ne 0) {
  throw "Win7 Egui strip failed with exit code $LASTEXITCODE"
}

$workspaceManifestText = Get-Content -LiteralPath (Join-Path $repoRoot "Cargo.toml") -Raw
$versionMatch = [regex]::Match(
  $workspaceManifestText,
  '(?ms)^\[workspace\.package\].*?^version\s*=\s*"(?<version>[0-9A-Za-z][0-9A-Za-z.+-]*)"\s*$'
)
if (-not $versionMatch.Success) {
  throw "Could not read a safe workspace package version"
}
$release = Join-Path $outputDir "wunder-egui-$($versionMatch.Groups['version'].Value)-win7-x86.exe"
Copy-Item -LiteralPath $executable -Destination $release -Force
$peDetails | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $outputDir "imports.txt")

Write-Host "Win7 Egui release executable: $release"
Write-Host "Win7 Egui PE imports: $(Join-Path $outputDir 'imports.txt')"
