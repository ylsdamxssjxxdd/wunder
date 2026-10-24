#Requires -Version 5.1
<#
.SYNOPSIS
  Install the locally built 32-bit Remotion compositor into a Win7 ia32 supplement.

.DESCRIPTION
  Remotion ships @remotion/compositor-win32-x64-msvc only, so the ia32 supplement
  needs two things that npm cannot provide:
    1. @remotion/compositor-win32-ia32-msvc - built from packages/compositor with
       the same Win7 toolchain as the wunder CLI (i686-win7-windows-gnu).
    2. An `ia32` branch in @remotion/renderer's get-executable-path.js, which
       otherwise only knows about x64.

  The script is idempotent and hard-fails when the binary does not look like a
  Win7-compatible 32-bit PE, so a bad artifact can never ship.

.PARAMETER RemotionRoot
  The staged opt\remotion directory (contains node_modules).

.PARAMETER CompositorDir
  Directory produced by scripts/win-ia32-compositor/build.sh, containing
  remotion.exe plus the FFmpeg DLLs.

.PARAMETER Version
  Remotion version recorded in the injected package.json.
#>
param(
  [Parameter(Mandatory = $true)][string]$RemotionRoot,
  [Parameter(Mandatory = $true)][string]$CompositorDir,
  [string]$Version = '4.0.538'
)

$ErrorActionPreference = 'Stop'

function Fail([string]$Message) {
  # Throw instead of exit so a caller (build_win7_desktop_supplement.ps1) can
  # catch the failure and report it in its own error format.
  throw $Message
}

$nodeModules = Join-Path $RemotionRoot 'node_modules'
$rendererDir = Join-Path $nodeModules '@remotion\renderer'
if (-not (Test-Path -LiteralPath $rendererDir -PathType Container)) {
  Fail "not a staged Remotion runtime (missing @remotion/renderer): $RemotionRoot"
}
if (-not (Test-Path -LiteralPath (Join-Path $CompositorDir 'remotion.exe') -PathType Leaf)) {
  Fail "compositor directory has no remotion.exe: $CompositorDir"
}

# --- 1. Validate the binary before touching the payload ----------------------
# Win7 must not see Win10-era imports; a 64-bit PE here would load in a 64-bit
# host but fail in the 32-bit one, so check both.
$peHeader = [System.IO.File]::ReadAllBytes((Join-Path $CompositorDir 'remotion.exe'))
if ($peHeader.Length -lt 0x40) { Fail 'remotion.exe is truncated' }
$peOffset = [BitConverter]::ToInt32($peHeader, 0x3C)
$machine = [BitConverter]::ToUInt16($peHeader, $peOffset + 4)
if ($machine -ne 0x014C) { Fail ('remotion.exe is not a 32-bit i386 PE (machine=0x{0:X4})' -f $machine) }

# DllCharacteristics lived at peOffset + 4 + 18 for PE32.
$characteristics = [BitConverter]::ToUInt16($peHeader, $peOffset + 4 + 18)
$largeAddressAware = ($characteristics -band 0x0020) -ne 0

$dumpbin = Get-Command 'i686-w64-mingw32-objdump.exe' -ErrorAction SilentlyContinue
if ($dumpbin) {
  $imports = (& $dumpbin.Source '-p' (Join-Path $CompositorDir 'remotion.exe')) -join "`n"
  if ($imports -match '(?i)combase\.dll|api-ms-win-|ext-ms-win-|GetDpiForWindow|WaitOnAddress|WakeByAddress|SetThreadDescription|bcryptprimitives') {
    Fail 'remotion.exe imports a Windows 8/10 API and will not start on Win7'
  }
}

# --- 2. Install the package -------------------------------------------------
# Mirror the layout of the official @remotion/compositor-win32-x64-msvc package:
# the renderer resolves it through `require(...).dir`.
$pkgDir = Join-Path $nodeModules '@remotion\compositor-win32-ia32-msvc'
if (Test-Path -LiteralPath $pkgDir) { Remove-Item -LiteralPath $pkgDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $pkgDir | Out-Null
Get-ChildItem -LiteralPath $CompositorDir -File |
  Where-Object { $_.Extension -in @('.exe', '.dll') } |
  ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $pkgDir -Force }
if (-not (Test-Path -LiteralPath (Join-Path $pkgDir 'remotion.exe') -PathType Leaf)) {
  Fail "compositor copy failed: $pkgDir"
}

$packageJson = [ordered]@{
  name = '@remotion/compositor-win32-ia32-msvc'
  version = $Version
  os = @('win32')
  cpu = @('ia32')
  exports = [ordered]@{
    '.' = [ordered]@{ types = './index.d.ts'; require = './index.js' }
    './package.json' = './package.json'
  }
}
$packageJsonPath = Join-Path $pkgDir 'package.json'
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($packageJsonPath, (ConvertTo-Json -InputObject $packageJson -Depth 5), $utf8NoBom)
[System.IO.File]::WriteAllText((Join-Path $pkgDir 'index.js'), "exports.dir = __dirname;`n", $utf8NoBom)
[System.IO.File]::WriteAllText((Join-Path $pkgDir 'index.d.ts'), "export const dir: string;`n", $utf8NoBom)
[System.IO.File]::WriteAllText(
  (Join-Path $pkgDir 'README.md'),
  "# @remotion/compositor-win32-ia32-msvc`n`nInternal package. Locally built 32-bit (i686) compositor for Windows 7 / ia32.`n",
  $utf8NoBom)

# --- 3. Teach the renderer about ia32 ---------------------------------------
$execPath = Join-Path $rendererDir 'dist\compositor\get-executable-path.js'
if (-not (Test-Path -LiteralPath $execPath -PathType Leaf)) {
  Fail "renderer layout changed (missing get-executable-path.js): $execPath"
}
$execSource = [System.IO.File]::ReadAllText($execPath)
if ($execSource -notmatch 'compositor-win32-ia32-msvc') {
  $needle = "                case 'x64':`n                    return require('@remotion/compositor-win32-x64-msvc').dir;`n"
  if ($execSource -notmatch [regex]::Escape("case 'x64':")) {
    Fail 'renderer layout changed (x64 case not found); refusing to patch blindly'
  }
  $replacement = $needle + "                case 'ia32':`n                    return require('@remotion/compositor-win32-ia32-msvc').dir;`n"
  $patched = $execSource.Replace($needle, $replacement)
  if ($patched -eq $execSource) {
    Fail 'renderer patch did not apply (line endings or indentation changed)'
  }
  [System.IO.File]::WriteAllText($execPath, $patched, $utf8NoBom)
}

# --- 4. The ia32 bundler package must be present ----------------------------
$esbuildExe = Join-Path $nodeModules '@esbuild\win32-ia32\esbuild.exe'
if (-not (Test-Path -LiteralPath $esbuildExe -PathType Leaf)) {
  Fail "missing 32-bit esbuild: $esbuildExe"
}

# --- 5. Cap x264 threads when the bundled encoder is 32-bit -----------------
# x264 sizes its thread pool from the CPU count (48 threads on a 32-core host).
# Each thread keeps full-resolution frame buffers, which for UHD means several
# gigabytes; measured peaks with libx264 preset medium on a 32-core machine:
#   4K auto (48 threads) -> 3716 MB -> encoder dies with OOM (EPIPE upstream)
#   4K 8 threads        -> 2596 MB
#   1440p auto          -> 2998 MB
#   1080p auto          -> 1470 MB
#   720p auto           ->  532 MB
# A 32-bit process only has 4GB of address space (2GB without LARGE_ADDRESS_AWARE),
# so 1440p and UHD need an explicit cap while 1080p and below are fine as-is.
$ffmpegExe = Join-Path $pkgDir 'ffmpeg.exe'
$encoderIs32Bit = $false
if (Test-Path -LiteralPath $ffmpegExe -PathType Leaf) {
  $ffBytes = [System.IO.File]::ReadAllBytes($ffmpegExe)
  $ffOffset = [BitConverter]::ToInt32($ffBytes, 0x3C)
  $encoderIs32Bit = ([BitConverter]::ToUInt16($ffBytes, $ffOffset + 4) -eq 0x014C)
}

$prespawnPath = Join-Path $rendererDir 'dist\prespawn-ffmpeg.js'
if ($encoderIs32Bit -and (Test-Path -LiteralPath $prespawnPath -PathType Leaf)) {
  $prespawn = [System.IO.File]::ReadAllText($prespawnPath)
  if ($prespawn -notmatch 'wunder x264 thread cap') {
    $anchor = "        ...encodingArgsWithOwnedPixels,`n"
    if ($prespawn -notmatch [regex]::Escape('...encodingArgsWithOwnedPixels,')) {
      Fail 'prespawn-ffmpeg.js layout changed; refusing to patch blindly'
    }
    $injection = $anchor + @"
        // wunder x264 thread cap: the bundled encoder is 32-bit, so its address
        // space cannot hold x264's default thread pool at 1440p and above.
        ...(function () {
            var pixels = options.width * options.height;
            if (pixels >= 3840 * 2160) { return [['-threads:v', '6']]; }
            if (pixels >= 2560 * 1440) { return [['-threads:v', '8']]; }
            return [];
        })(),
"@
    [System.IO.File]::WriteAllText($prespawnPath, $prespawn.Replace($anchor, $injection), $utf8NoBom)
    $prespawnPatched = $true
  } else {
    $prespawnPatched = $true
  }
} else {
  $prespawnPatched = $false
}

# --- 6. Remove the 4096MB GPU memory budget --------------------------------
# open-browser.js always announces --force-gpu-mem-available-mb=4096. A 32-bit
# renderer owns only 4GB of address space in total, so the compositor cannot
# allocate its raster buffers and every captured frame comes back as an empty
# page: the body background colour with no content painted on top.
# Measured with the bundled Chrome 109 ia32 at 1920x1080:
#   with the flag     -> uniform #111 frame, 36 KB png, no text drawn
#   without the flag  -> correct frame, 1.7 MB png
# 64-bit hosts are unaffected, so the override is dropped for ia32 only.
$openBrowserPath = Join-Path $rendererDir 'dist\open-browser.js'
$gpuMemPatched = $false
if (Test-Path -LiteralPath $openBrowserPath -PathType Leaf) {
  $openBrowser = [System.IO.File]::ReadAllText($openBrowserPath)
  if ($openBrowser -notmatch 'wunder ia32: a 32-bit renderer') {
    $gpuAnchor = "            '--force-gpu-mem-available-mb=4096',"
    if ($openBrowser -notmatch [regex]::Escape($gpuAnchor)) {
      Fail 'open-browser.js layout changed (gpu memory flag not found); refusing to patch blindly'
    }
    $gpuInjection = @"
            // wunder ia32: a 32-bit renderer has a 4GB address space in total.
            // Announcing a 4096MB GPU memory budget makes the compositor fail to
            // allocate its raster buffers, and every captured frame comes back as
            // an empty, background-only image. Fall back to no override.
            process.arch === 'ia32' ? null : '--force-gpu-mem-available-mb=4096',
"@
    [System.IO.File]::WriteAllText($openBrowserPath, $openBrowser.Replace($gpuAnchor, $gpuInjection), $utf8NoBom)
    $gpuMemPatched = $true
  } else {
    $gpuMemPatched = $true
  }
} else {
  Fail "renderer layout changed (missing open-browser.js): $openBrowserPath"
}

$summary = [ordered]@{
  compositor = $pkgDir
  peMachine = ('0x{0:X4}' -f $machine)
  largeAddressAware = $largeAddressAware
  addressSpaceGb = if ($largeAddressAware) { 4 } else { 2 }
  encoderIs32Bit = $encoderIs32Bit
  x264ThreadCap = $prespawnPatched
  gpuMemoryBudgetOverride = if ($gpuMemPatched) { 'removed-on-ia32' } else { 'unpatched' }
  files = (Get-ChildItem -LiteralPath $pkgDir -File).Count
}
$summary | ConvertTo-Json -Compress
