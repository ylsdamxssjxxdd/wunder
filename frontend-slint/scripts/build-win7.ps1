param(
  [string]$Toolchain = "nightly-2026-03-14-x86_64-pc-windows-gnu",
  [string]$CargoExe = "",
  [string]$MingwBin = $(if ($env:WUNDER_WIN7_MINGW_BIN) { $env:WUNDER_WIN7_MINGW_BIN } else { "C:\mingw32-12.2-winlibs\mingw32\bin" }),
  [string]$HostMingwBin = $(if ($env:WUNDER_WIN7_HOST_MINGW_BIN) { $env:WUNDER_WIN7_HOST_MINGW_BIN } else { "C:\mingw64\bin" }),
  [string]$OutputDirectory = "",
  [ValidateRange(1, 8)][int]$Jobs = 8,
  [switch]$Check
)

$ErrorActionPreference = "Stop"

$frontendRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = Split-Path -Parent $frontendRoot
$manifest = Join-Path $frontendRoot "Cargo.toml"
$target = "i686-win7-windows-gnu"
. (Join-Path $PSScriptRoot 'win7_host_tools.ps1')
Set-Win7HostTools -HostMingwBin $HostMingwBin
$linker = Join-Path $MingwBin "i686-w64-mingw32-gcc.exe"

if (-not (Test-Path -LiteralPath $manifest)) {
  throw "Slint manifest was not found: $manifest"
}

# Cargo owns the package version. Keep the user-facing Win7 release filename
# synchronized with the manifest instead of duplicating version constants.
$manifestText = Get-Content -LiteralPath $manifest -Raw
$packageVersionMatch = [regex]::Match(
  $manifestText,
  '(?ms)^\[package\].*?^version\s*=\s*"(?<version>[^"]+)"\s*$'
)
if (-not $packageVersionMatch.Success) {
  throw "Could not read [package].version from: $manifest"
}
$packageVersion = $packageVersionMatch.Groups['version'].Value
if ($packageVersion -notmatch '^[0-9A-Za-z][0-9A-Za-z.+-]*$') {
  throw "Package version contains characters unsafe for a release filename: $packageVersion"
}
if (-not $OutputDirectory) {
  $OutputDirectory = Join-Path $repoRoot "target\frontend-slint\dist\win7-x86"
} elseif (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) {
  $OutputDirectory = Join-Path $repoRoot $OutputDirectory
}
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)

if (!(Test-Path -LiteralPath $linker)) {
  throw "Missing MinGW linker: $linker"
}

$cargoCommand = if ($CargoExe) { $CargoExe } else { "cargo" }
$rustcCommand = if ($CargoExe) { Join-Path (Split-Path -Parent $CargoExe) "rustc.exe" } else { "rustc" }
if ($CargoExe -and !(Test-Path -LiteralPath $CargoExe)) {
  throw "Offline Cargo executable was not found: $CargoExe"
}
if ($CargoExe -and !(Test-Path -LiteralPath $rustcCommand)) {
  throw "Offline rustc executable was not found: $rustcCommand"
}
if ($CargoExe) {
  $env:RUSTC = $rustcCommand
  $versionDetails = & $rustcCommand -vV
  if ($LASTEXITCODE -ne 0) { throw 'Offline rustc could not run' }
  Assert-Win7RustHost -VersionDetails $versionDetails
  $availableTargets = & $rustcCommand --print target-list
} else {
  $env:RUSTC = (& rustup which --toolchain $Toolchain rustc).Trim()
  if ($LASTEXITCODE -ne 0) { throw "rustc not installed for $Toolchain" }
  $versionDetails = & $env:RUSTC -vV
  if ($LASTEXITCODE -ne 0) { throw 'rustc could not run' }
  Assert-Win7RustHost -VersionDetails $versionDetails
  # Keep the rustup override as an explicit native-command argument. Building
  # it through a conditional array can be serialized as an empty +toolchain
  # argument by PowerShell, causing rustup to report `invalid toolchain name`.
  $availableTargets = & rustc "+$Toolchain" --print target-list
}
if ($LASTEXITCODE -ne 0 -or !($availableTargets -contains $target)) {
  throw "$target is not available in $Toolchain"
}

$rustSrc = if ($CargoExe) {
  Join-Path (Split-Path -Parent $CargoExe) '..\lib\rustlib\src\rust\library\Cargo.toml'
} else {
  $components = & rustup component list --installed --toolchain $Toolchain
  if ($LASTEXITCODE -ne 0 -or !($components -match '^rust-src')) {
    throw "rust-src is not installed for $Toolchain"
  }
  $null
}
if ($CargoExe -and !(Test-Path -LiteralPath $rustSrc)) {
  throw "Offline rust-src is not installed for ${Toolchain}: $rustSrc"
}

$env:PATH = "$HostMingwBin;$env:PATH;$MingwBin"
$env:WUNDER_WIN7_MINGW_BIN = $MingwBin
$env:CARGO_TARGET_DIR = Join-Path $repoRoot "target\frontend-slint"
$env:CARGO_TARGET_I686_WIN7_WINDOWS_GNU_LINKER = $linker
$env:CC_i686_win7_windows_gnu = $linker
$env:CXX_i686_win7_windows_gnu = Join-Path $MingwBin 'i686-w64-mingw32-g++.exe'
$env:AR_i686_win7_windows_gnu = Join-Path $MingwBin 'i686-w64-mingw32-gcc-ar.exe'
$env:RANLIB_i686_win7_windows_gnu = Join-Path $MingwBin 'i686-w64-mingw32-gcc-ranlib.exe'

# windows-targets 0.48 only recognizes target_vendor=pc/uwp in its build
# script. Resolve its prebuilt GNU import library explicitly for the Win7
# vendor target used by this repository.
if ($CargoExe) {
  $metadataJson = & $cargoCommand metadata --locked --manifest-path $manifest --format-version 1
} else {
  $metadataJson = & cargo "+$Toolchain" metadata --locked --manifest-path $manifest --format-version 1
}
if ($LASTEXITCODE -ne 0) {
  throw "cargo metadata failed with exit code $LASTEXITCODE"
}
$metadata = $metadataJson | ConvertFrom-Json
$windowsImportPackages = @($metadata.packages |
  Where-Object { $_.name -eq "windows_i686_gnu" })
if ($windowsImportPackages.Count -eq 0) {
  throw "windows_i686_gnu is missing from the Slint dependency graph"
}
$windowsImportDirs = @()
foreach ($windowsImportPackage in $windowsImportPackages) {
  $windowsImportDir = Join-Path (Split-Path -Parent $windowsImportPackage.manifest_path) "lib"
  $windowsImportLibraries = @(Get-ChildItem -LiteralPath $windowsImportDir -Filter "libwindows.*.a" -File -ErrorAction SilentlyContinue)
  if ($windowsImportLibraries.Count -eq 0) {
    throw "missing Windows import library in: $windowsImportDir"
  }
  $windowsImportDirs += $windowsImportDir
}
$existingRustFlags = $env:CARGO_TARGET_I686_WIN7_WINDOWS_GNU_RUSTFLAGS
$nativeLinkFlag = (($windowsImportDirs | Select-Object -Unique) |
  ForEach-Object { "-Lnative=$_" }) -join " "
$env:CARGO_TARGET_I686_WIN7_WINDOWS_GNU_RUSTFLAGS =
  if ($existingRustFlags) { "$existingRustFlags $nativeLinkFlag" } else { $nativeLinkFlag }

$cargoArgs = @()
$cargoArgs += @(
  "-Z", "build-std=std,panic_abort",
  "build",
  "--locked",
  "--bin", "wunder-frontend-slint",
  "--jobs", "$Jobs",
  "--manifest-path", $manifest,
  "--target", $target
)

$cargoArgs += "--release"
if ($Check) { $cargoArgs[$cargoArgs.IndexOf("build")] = "check" }
# Release packaging needs only the application, not the large benchmark bins.
# Override the repository's 16-job/16-thread defaults for this build only;
# x64 host Rust removes the per-process 32-bit limit. Keep optimizations intact.
$previousRayonThreads = $env:RAYON_NUM_THREADS
try {
  $env:RAYON_NUM_THREADS = "$Jobs"
  Write-Host "Slint Win7 build: host=x86_64-pc-windows-gnu, bin=wunder-frontend-slint, target=$target, jobs=$Jobs, Rayon threads=$Jobs"
  if ($CargoExe) {
    & $cargoCommand @cargoArgs
  } else {
    & cargo "+$Toolchain" @cargoArgs
  }
  if ($LASTEXITCODE -ne 0) {
    throw "Slint Win7 build failed with exit code $LASTEXITCODE"
  }
} finally {
  $env:RAYON_NUM_THREADS = $previousRayonThreads
}

if ($Check) { return }
$profile = "release"
$executable = Join-Path $env:CARGO_TARGET_DIR "$target\$profile\wunder-frontend-slint.exe"
if (!(Test-Path -LiteralPath $executable)) {
  throw "Slint Win7 executable was not produced: $executable"
}

# A successful custom-target build does not prove that a dependency has not
# gained a Win8+/WinRT import. Inspect the PE import table in the same build
# command so a regression cannot silently become a release candidate.
$objdump = Join-Path $MingwBin "objdump.exe"
if (!(Test-Path -LiteralPath $objdump)) {
  throw "Missing MinGW PE inspector: $objdump"
}
$peDetails = (& $objdump -p $executable) -join [Environment]::NewLine
if ($LASTEXITCODE -ne 0) {
  throw "PE import inspection failed with exit code $LASTEXITCODE"
}
if ($peDetails -notmatch 'file format pei-i386') {
  throw "Win7 output must be a 32-bit i386 PE executable: $executable"
}
$importedDlls = @(
  [regex]::Matches($peDetails, '(?im)^\s*DLL Name:\s*(.+?)\s*$') |
    ForEach-Object { $_.Groups[1].Value.ToLowerInvariant() }
)
$blockedDllPatterns = @(
  '^combase\.dll$',
  '^api-ms-win-',
  '^ext-ms-win-'
)
$blockedDlls = @(
  $importedDlls | Where-Object {
    $candidate = $_
    $blockedDllPatterns | Where-Object { $candidate -match $_ } | Select-Object -First 1
  }
)
$blockedImports = @(
  'SystemParametersInfoForDpi',
  'GetDpiForWindow'
) | Where-Object { $peDetails -match ("(?m)^\s*(?:[0-9a-fA-F]+\s+[0-9]+\s+)?" + [regex]::Escape($_) + "(?:A|W)?\s*$") }
# objdump prefixes imported names with an address and ordinal. Match that
# form as well as a plain name. CreateWaitableTimerEx is available since
# Vista; only its optional HIGH_RESOLUTION flag needs newer Windows, so
# importing the function is valid for the Win7 Rust standard library.
if ($blockedDlls.Count -gt 0 -or $blockedImports.Count -gt 0) {
  $detail = @()
  if ($blockedDlls.Count -gt 0) { $detail += "DLL: $($blockedDlls -join ', ')" }
  if ($blockedImports.Count -gt 0) { $detail += "symbol: $($blockedImports -join ', ')" }
  throw "Win7 PE import gate failed ($($detail -join '; '))"
}

Write-Host "Win7 PE import gate passed (no WinRT/api-ms or known Win8+ strong imports)."

# ld's -Wl,--strip-all silently keeps the COFF symbol table for this link
# (84 MB exe instead of 37 MB). Run the toolchain strip explicitly so the
# release executable never ships symbol data.
$strip = Join-Path $MingwBin "strip.exe"
if (!(Test-Path -LiteralPath $strip)) {
  throw "Missing MinGW strip tool: $strip"
}
& $strip --strip-all $executable
if ($LASTEXITCODE -ne 0) {
  throw "strip failed with exit code $LASTEXITCODE"
}

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$releaseFileName = "wunder-frontend-slint-$packageVersion-win7-x86.exe"
$releaseExecutable = Join-Path $OutputDirectory $releaseFileName
Copy-Item -LiteralPath $executable -Destination $releaseExecutable -Force
Write-Host "Slint Win7 release executable: $releaseExecutable"
