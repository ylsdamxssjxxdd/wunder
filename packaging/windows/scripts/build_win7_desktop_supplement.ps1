<#
.SYNOPSIS
Build the Wunder Desktop Windows supplement (bundled Python + Git + ripgrep + ffmpeg).

.DESCRIPTION
Windows counterpart of packaging/linux/scripts/build_linux_supplement.sh.
Reads packaging/windows/scripts/win7-supplement-manifest.json, downloads the
pinned archives, stages the opt/ tree, optionally installs the Playwright
Chromium (manifest python.installBrowser), and produces
wunder-supplement-win7-<arch>[-<profile>].zip.

The archive is imported into WUNDER_HOME (default: the user's .wunder
directory); both local forms auto-detect opt/python, opt/git, opt/rg and
opt/ffmpeg on startup, and probe opt/browser for a bundled browser.

.PARAMETER Arch
Target architecture: ia32 (default, Windows 7) or x64.

.PARAMETER PythonProfile
Python package profile to install. Only 'common' (the pinned Win7-friendly
package set) ships; profiles are defined in win7-supplement-manifest.json.

.PARAMETER BuildRoot
Build root. Defaults to the manifest defaultBuildRoot (temp_dir\win7-gnu-lab\win7-supplement).

.PARAMETER PythonPackageIndexUrl
pip index URL. Defaults to the manifest defaultPackageIndexUrl (Tsinghua Tuna).

.PARAMETER RefreshDownloads
Redownload cached archives instead of reusing them.

.PARAMETER PythonArchivePath / GitArchivePath / RgArchivePath / FfmpegArchivePath / BrowserArchivePath
Use hand-prepared local archives instead of downloading. BrowserArchivePath
also accepts an unpacked browser directory and is staged into opt/browser.

.PARAMETER BestEffort
Install requirements package-by-package and continue past failures (records them).
#>
[CmdletBinding()]
param(
  [ValidateSet('ia32', 'x64')]
  [string]$Arch = 'ia32',

  [ValidateSet('common')]
  [string]$PythonProfile = 'common',

  [string]$BuildRoot = '',

  [string]$PythonPackageIndexUrl = '',

  [switch]$RefreshDownloads,

  [string]$PythonArchivePath = '',

  [string]$GitArchivePath = '',

  [string]$RgArchivePath = '',

  [string]$FfmpegArchivePath = '',

  [string]$R2ArchivePath = '',

  [string]$BrowserArchivePath = '',

  [switch]$SkipFfmpeg,

  [switch]$SkipBrowser,

  [switch]$BestEffort
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptDir = $PSScriptRoot

function Write-Step {
  param([string]$Message)
  Write-Host "[win7-supplement] $Message"
}

function Write-Warn {
  param([string]$Message)
  Write-Warning "[win7-supplement] $Message"
}

function Fail {
  param([string]$Message)
  throw "[win7-supplement] $Message"
}

function Read-Manifest {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { Fail "manifest not found: $Path" }
  return (Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json)
}

function Get-PropertyValue {
  param($Object, [string]$Name, $Default = $null)
  if ($null -eq $Object) { return $Default }
  $prop = $Object.PSObject.Properties[$Name]
  if ($null -eq $prop) { return $Default }
  if ($null -eq $prop.Value) { return $Default }
  return $prop.Value
}

function Ensure-Directory {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { New-Item -ItemType Directory -Path $Path -Force | Out-Null }
}

function Get-DownloadedFile {
  param([string]$Url, [string]$Destination, [switch]$Force)
  Ensure-Directory (Split-Path -Parent $Destination)
  if ((Test-Path -LiteralPath $Destination) -and (-not $Force)) {
    Write-Step "using cached $(Split-Path -Leaf $Destination)"
    return $Destination
  }
  Write-Step "downloading $Url"
  $prevProgress = $ProgressPreference
  $ProgressPreference = 'SilentlyContinue'
  try {
    $attempt = 0
    while ($true) {
      $attempt++
      try {
        Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing -MaximumRedirection 10
        break
      } catch {
        if ($attempt -ge 3) { Write-Warn "Invoke-WebRequest failed after $attempt attempts: $($_.Exception.Message)"; break }
        Write-Warn "download attempt $attempt failed: $($_.Exception.Message)"
        Start-Sleep -Seconds 2
      }
    }
  } finally {
    $ProgressPreference = $prevProgress
  }

  $ok = (Test-Path -LiteralPath $Destination) -and ((Get-Item -LiteralPath $Destination).Length -gt 0)
  if (-not $ok) {
    $candidates = New-Object System.Collections.Generic.List[string]
    $sysCurl = Join-Path $env:SystemRoot 'System32\curl.exe'
    if (Test-Path -LiteralPath $sysCurl) { $candidates.Add($sysCurl) }
    $found = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($found -and ($found.Source)) { $candidates.Add([string]$found.Source) }
    foreach ($candidate in $candidates) {
      Write-Warn "falling back to curl: $candidate"
      $curlProc = Start-Process -FilePath $candidate -ArgumentList @('-L', '--fail', '--retry', '3', '--retry-delay', '2', '-o', $Destination, $Url) -Wait -PassThru -NoNewWindow
      $ok = (Test-Path -LiteralPath $Destination) -and ((Get-Item -LiteralPath $Destination).Length -gt 0)
      if ($ok) { break }
    }
  }
  if (-not $ok) { Fail "download failed: $Url" }
  return $Destination
}

function Get-PeMachine {
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { Fail "PE file not found: $Path" }
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  if ($bytes.Length -lt 0x40 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
    Fail "not a Windows PE image: $Path"
  }
  $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
  if ($peOffset -lt 0 -or $peOffset + 6 -gt $bytes.Length -or
      $bytes[$peOffset] -ne 0x50 -or $bytes[$peOffset + 1] -ne 0x45 -or
      $bytes[$peOffset + 2] -ne 0 -or $bytes[$peOffset + 3] -ne 0) {
    Fail "invalid PE header: $Path"
  }
  return [BitConverter]::ToUInt16($bytes, $peOffset + 4)
}

function Assert-PeMachine {
  param([string]$Path, [ValidateSet('ia32', 'x64')][string]$Arch)
  $machine = Get-PeMachine -Path $Path
  $expected = if ($Arch -eq 'ia32') { 0x014c } else { 0x8664 }
  if ($machine -ne $expected) {
    $actual = ('0x{0:x4}' -f $machine)
    $wanted = if ($Arch -eq 'ia32') { 'i386 (32-bit)' } else { 'x86-64 (64-bit)' }
    Fail "wrong PE architecture for ${Path}: got $actual, expected $wanted"
  }
  Write-Step "validated PE architecture ($Arch): $Path"
}

function Expand-ZipInto {
  param([string]$Archive, [string]$Destination)
  if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Recurse -Force }
  Ensure-Directory $Destination
  Expand-Archive -LiteralPath $Archive -DestinationPath $Destination -Force
}

function Get-Sha256Hex {
  param([string]$Path)
  # Get-FileHash needs PowerShell 4+ plus a resolvable module path; the .NET
  # provider works on every host these build scripts run on.
  $stream = [IO.File]::OpenRead($Path)
  try {
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
      ($sha.ComputeHash($stream) | ForEach-Object { $_.ToString('X2') }) -join ''
    } finally {
      $sha.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

function Assert-FileSha256 {
  param([string]$Path, [string]$Expected)
  if ([string]::IsNullOrWhiteSpace($Expected)) { return }
  $actual = Get-Sha256Hex -Path $Path
  if ($actual.ToUpperInvariant() -ne $Expected.ToUpperInvariant()) {
    Fail "SHA256 mismatch for ${Path}: got $actual, expected $Expected"
  }
  Write-Step "validated SHA256: $Path"
}

function Expand-ZipEntry {
  param([string]$Archive, [string]$Entry, [string]$Destination)
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $zip = [System.IO.Compression.ZipFile]::OpenRead($Archive)
  try {
    $item = $zip.Entries | Where-Object { $_.FullName -eq $Entry } | Select-Object -First 1
    if ($null -eq $item) { Fail "archive entry not found: $Entry in $Archive" }
    Ensure-Directory (Split-Path -Parent $Destination)
    [System.IO.Compression.ZipFileExtensions]::ExtractToFile($item, $Destination, $true)
  } finally {
    $zip.Dispose()
  }
}

function Expand-ZipFlatten {
  param([string]$Archive, [string]$Destination)
  $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ('wunder-supp-' + [guid]::NewGuid().ToString('N'))
  Expand-ZipInto -Archive $Archive -Destination $tmp
  $entries = @(Get-ChildItem -LiteralPath $tmp -Force)
  if (($entries.Count -eq 1) -and ($entries[0].PSIsContainer)) { $source = $entries[0].FullName } else { $source = $tmp }
  if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Recurse -Force }
  Ensure-Directory $Destination
  Get-ChildItem -LiteralPath $source -Force | ForEach-Object { Move-Item -LiteralPath $_.FullName -Destination $Destination -Force }
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

function Expand-GzipFile {
  param([string]$Archive, [string]$DestinationFile)
  Ensure-Directory (Split-Path -Parent $DestinationFile)
  $in = [System.IO.File]::OpenRead($Archive)
  try {
    $gz = New-Object System.IO.Compression.GZipStream($in, [System.IO.Compression.CompressionMode]::Decompress)
    try {
      $out = [System.IO.File]::Create($DestinationFile)
      try { $gz.CopyTo($out) } finally { $out.Dispose() }
    } finally { $gz.Dispose() }
  } finally { $in.Dispose() }
}

function Expand-SevenZipSelfExtractor {
  param([string]$Archive, [string]$Destination)
  if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Recurse -Force }
  Ensure-Directory $Destination
  $arg = '-o"' + $Destination + '"'
  $proc = Start-Process -FilePath $Archive -ArgumentList @($arg, '-y') -Wait -PassThru -NoNewWindow
  if ($proc.ExitCode -ne 0) { Fail "7z self-extractor failed ($($proc.ExitCode)) for $Archive" }
}

function Install-BundledBrowser {
  # Stages a hand-prepared browser payload (.zip or directory) into the given
  # destination (opt/browser) and normalizes the layout so $ExpectedExe (by
  # default chrome-win/chrome.exe) exists. The runtime probes
  # opt/browser/<expectedExe> first and opt/browser/chrome.exe as fallback.
  param(
    [string]$Source,
    [string]$Destination,
    [string]$ExpectedExe,
    [ValidateSet('ia32', 'x64')][string]$Arch
  )
  $expectedRel = $ExpectedExe -replace '/', '\'
  if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Recurse -Force }
  Ensure-Directory $Destination
  $item = Get-Item -LiteralPath $Source
  if ($item.PSIsContainer) {
    if (Test-Path -LiteralPath (Join-Path $item.FullName 'chrome.exe')) {
      # The directory itself is the browser dir (e.g. chrome-win); keep its name.
      Copy-Item -LiteralPath $item.FullName -Destination $Destination -Recurse -Force
    } else {
      Copy-Item -Path (Join-Path $item.FullName '*') -Destination $Destination -Recurse -Force
    }
  } elseif ($item.Extension -ieq '.zip') {
    Expand-Archive -LiteralPath $item.FullName -DestinationPath $Destination -Force
  } else {
    Fail "browser source must be a .zip archive or a directory: $Source"
  }

  $expectedPath = Join-Path $Destination $expectedRel
  if (-not (Test-Path -LiteralPath $expectedPath)) {
    $found = @(Get-ChildItem -LiteralPath $Destination -Recurse -File -Filter 'chrome.exe')
    if ($found.Count -eq 0) { Fail "chrome.exe not found in browser payload: $Source" }
    $chromeExe = $found[0].FullName
    if ($chromeExe -eq (Join-Path $Destination 'chrome.exe')) {
      # Top-level chrome.exe matches the runtime fallback probe; leave as is.
      $expectedPath = $chromeExe
      $expectedRel = 'chrome.exe'
    } else {
      $expectedDir = Split-Path -Parent $expectedRel
      if ([string]::IsNullOrWhiteSpace($expectedDir)) { Fail "cannot rearrange browser payload for expectedExe '$ExpectedExe'" }
      $targetDir = Join-Path $Destination $expectedDir
      $parentDir = Split-Path -Parent $chromeExe
      if ((Test-Path -LiteralPath $targetDir) -and ($parentDir.StartsWith($targetDir, [System.StringComparison]::OrdinalIgnoreCase))) {
        Fail "browser payload has a partial $expectedDir tree without chrome.exe at its root: $Source"
      }
      if (Test-Path -LiteralPath $targetDir) { Remove-Item -LiteralPath $targetDir -Recurse -Force }
      Ensure-Directory (Split-Path -Parent $targetDir)
      Move-Item -LiteralPath $parentDir -Destination $targetDir
      $expectedPath = Join-Path $Destination $expectedRel
      if (-not (Test-Path -LiteralPath $expectedPath)) { Fail "failed to rearrange browser payload so $ExpectedExe exists" }
    }
  }

  if ($Arch -eq 'ia32') {
    Assert-PeMachine -Path $expectedPath -Arch $Arch
  } else {
    # A 32-bit Chrome also runs under WoW64 on x64 Windows, so only report.
    $machine = Get-PeMachine -Path $expectedPath
    Write-Step ('browser PE machine: 0x{0:x4} (x64 pack accepts both 32/64-bit)' -f $machine)
  }
  Write-Step "bundled browser staged at opt/browser/$($expectedRel -replace '\\', '/')"
}

function Write-Utf8NoBom {
  param([string]$Path, [string]$Content)
  Ensure-Directory (Split-Path -Parent $Path)
  $enc = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Content, $enc)
}

function Enable-EmbeddedSite {
  param([string]$PythonRoot, [string]$PyTag)
  $pth = Join-Path $PythonRoot ($PyTag + '._pth')
  $lib = Join-Path $PythonRoot 'Lib\site-packages'
  Ensure-Directory $lib
  if (Test-Path -LiteralPath $pth) {
    $lines = @(Get-Content -LiteralPath $pth -Encoding UTF8)
    $out = New-Object System.Collections.Generic.List[string]
    $hasSite = $false
    foreach ($line in $lines) {
      if ($line -match '^\s*#\s*import\s+site\s*$') { $out.Add('import site'); $hasSite = $true; continue }
      if ($line -match '^\s*import\s+site\s*$') { $hasSite = $true }
      $out.Add($line)
    }
    if (-not ($out -contains 'Lib\site-packages')) { $out.Add('Lib\site-packages') }
    if (-not $hasSite) { $out.Add('import site') }
    Write-Utf8NoBom -Path $pth -Content (($out -join "`r`n") + "`r`n")
  }
}

function ConvertTo-ProcessArgument {
  # Start-Process joins -ArgumentList with spaces and does not add quotes, so an
  # argument such as 'import requests' (python -c) would be split into two.
  param([string]$Value)
  if ($Value -match '[\s"]') { return '"' + ($Value -replace '"', '\"') + '"' }
  return $Value
}

function Invoke-PythonProcess {
  # Runs python and returns its exit code. Start-Process/ExitCode is used because
  # some hosts do not populate $LASTEXITCODE for native commands.
  param([string]$PythonExe, [string[]]$Arguments, [switch]$SuppressOutput)
  $stdout = Join-Path $env:TEMP ('wunder-py-' + [guid]::NewGuid().ToString('N') + '.out')
  $stderr = Join-Path $env:TEMP ('wunder-py-' + [guid]::NewGuid().ToString('N') + '.err')
  try {
    $argList = @($Arguments | ForEach-Object { ConvertTo-ProcessArgument -Value ([string]$_) })
    $proc = Start-Process -FilePath $PythonExe -ArgumentList $argList -Wait -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $code = $proc.ExitCode
    if ((-not $SuppressOutput) -and (Test-Path -LiteralPath $stdout)) { Get-Content -LiteralPath $stdout | Write-Host }
    if (($code -ne 0) -and (Test-Path -LiteralPath $stderr)) { Get-Content -LiteralPath $stderr | Write-Host }
    return $code
  } finally {
    Remove-Item -LiteralPath $stdout, $stderr -Force -ErrorAction SilentlyContinue
  }
}

function Invoke-Python {
  param([string]$PythonExe, [string[]]$Arguments)
  $rc = Invoke-PythonProcess -PythonExe $PythonExe -Arguments $Arguments
  if ($rc -ne 0) { Fail "python failed ($rc): $PythonExe $($Arguments -join ' ')" }
}

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
$repoRoot = (Resolve-Path -LiteralPath $ScriptDir).Path
for ($i = 0; $i -lt 3; $i++) { $repoRoot = Split-Path -Parent $repoRoot }
$manifestPath = Join-Path $ScriptDir 'win7-supplement-manifest.json'
$manifest = Read-Manifest -Path $manifestPath

$buildRootEffective = $BuildRoot
if ([string]::IsNullOrWhiteSpace($buildRootEffective)) {
  $rel = [string](Get-PropertyValue -Object $manifest -Name 'defaultBuildRoot' -Default 'temp_dir\win7-gnu-lab\win7-supplement')
  $buildRootEffective = Join-Path $repoRoot $rel
} elseif (-not [System.IO.Path]::IsPathRooted($buildRootEffective)) {
  $buildRootEffective = Join-Path $repoRoot $buildRootEffective
}

$downDir  = Join-Path $buildRootEffective ([string](Get-PropertyValue -Object $manifest -Name 'downloadsDir' -Default 'downloads'))
$stageDir = Join-Path $buildRootEffective ([string](Get-PropertyValue -Object $manifest -Name 'stageDir' -Default 'stage'))
$distDir  = Join-Path $buildRootEffective ([string](Get-PropertyValue -Object $manifest -Name 'distDir' -Default 'dist'))
$packRoot = Join-Path $stageDir 'package-root'

$layout = Get-PropertyValue -Object $manifest -Name 'layout'
$pythonRoot = Join-Path $packRoot ([string](Get-PropertyValue -Object $layout -Name 'pythonRoot' -Default 'opt\python'))
$gitRoot    = Join-Path $packRoot ([string](Get-PropertyValue -Object $layout -Name 'gitRoot' -Default 'opt\git'))
$rgRoot     = Join-Path $packRoot ([string](Get-PropertyValue -Object $layout -Name 'rgRoot' -Default 'opt\rg'))
$ffmpegRoot = Join-Path $packRoot ([string](Get-PropertyValue -Object $layout -Name 'ffmpegRoot' -Default 'opt\ffmpeg'))
$r2Root     = Join-Path $packRoot ([string](Get-PropertyValue -Object $layout -Name 'r2Root' -Default 'opt\r2'))

Write-Step "repo root : $repoRoot"
Write-Step "build root: $buildRootEffective"
Write-Step "arch      : $Arch"
Write-Step "profile   : $PythonProfile"

if (Test-Path -LiteralPath $stageDir) { Remove-Item -LiteralPath $stageDir -Recurse -Force }
Ensure-Directory $packRoot
Ensure-Directory $downDir
Ensure-Directory $distDir

# ---------------------------------------------------------------------------
# Python (embeddable) + pip + requirements
# ---------------------------------------------------------------------------
$pythonMeta = Get-PropertyValue -Object $manifest -Name 'python'
$pyVer = [string](Get-PropertyValue -Object $pythonMeta -Name 'version' -Default '3.8.10')
$pyArchiveMeta = Get-PropertyValue -Object (Get-PropertyValue -Object $pythonMeta -Name 'archives') -Name $Arch
if ($null -eq $pyArchiveMeta) { Fail "manifest has no python archive for arch $Arch" }

$pyArchive = $PythonArchivePath
if ([string]::IsNullOrWhiteSpace($pyArchive)) {
  $pyArchive = Join-Path $downDir ([string](Get-PropertyValue -Object $pyArchiveMeta -Name 'fileName' -Default 'python-embed.zip'))
  Get-DownloadedFile -Url ([string](Get-PropertyValue -Object $pyArchiveMeta -Name 'url')) -Destination $pyArchive -Force:$RefreshDownloads | Out-Null
} elseif (-not (Test-Path -LiteralPath $pyArchive)) { Fail "python archive not found: $pyArchive" }

Write-Step "staging Python $pyVer into $pythonRoot"
Expand-ZipInto -Archive $pyArchive -Destination $pythonRoot
$pyParts = $pyVer.Split('.')
$pyTag = 'python' + $pyParts[0] + $pyParts[1]
Enable-EmbeddedSite -PythonRoot $pythonRoot -PyTag $pyTag
$pythonExe = Join-Path $pythonRoot 'python.exe'
if (-not (Test-Path -LiteralPath $pythonExe)) { Fail "python.exe missing after extraction: $pythonExe" }

$indexUrl = $PythonPackageIndexUrl
if ([string]::IsNullOrWhiteSpace($indexUrl)) {
  $indexUrl = [string](Get-PropertyValue -Object $pythonMeta -Name 'defaultPackageIndexUrl' -Default 'https://pypi.tuna.tsinghua.edu.cn/simple')
}
$bootstrap = Get-PropertyValue -Object $pythonMeta -Name 'bootstrap'
$getPip = Join-Path $downDir 'get-pip.py'
Get-DownloadedFile -Url ([string](Get-PropertyValue -Object $bootstrap -Name 'getPipUrl' -Default 'https://bootstrap.pypa.io/pip/3.8/get-pip.py')) -Destination $getPip -Force:$RefreshDownloads | Out-Null

Write-Step 'bootstrapping pip'
Invoke-Python -PythonExe $pythonExe -Arguments @($getPip, '--no-warn-script-location', '--no-cache-dir', '-i', $indexUrl)
Invoke-Python -PythonExe $pythonExe -Arguments @(
  '-m', 'pip', 'install', '--no-cache-dir', '-i', $indexUrl,
  [string](Get-PropertyValue -Object $bootstrap -Name 'pipSpec' -Default 'pip'),
  [string](Get-PropertyValue -Object $bootstrap -Name 'setuptoolsSpec' -Default 'setuptools'),
  [string](Get-PropertyValue -Object $bootstrap -Name 'wheelSpec' -Default 'wheel')
)

$profileNode = Get-PropertyValue -Object (Get-PropertyValue -Object $pythonMeta -Name 'profiles') -Name $PythonProfile
if ($null -eq $profileNode) { Fail "manifest has no python profile '$PythonProfile'" }
$requirementsRel = [string](Get-PropertyValue -Object $profileNode -Name 'requirementsPath' -Default '')

$installedPackages = New-Object System.Collections.Generic.List[string]
$failedPackages = New-Object System.Collections.Generic.List[string]

if (-not [string]::IsNullOrWhiteSpace($requirementsRel)) {
  $requirementsPath = Join-Path $repoRoot $requirementsRel
  if (-not (Test-Path -LiteralPath $requirementsPath)) { Fail "requirements not found: $requirementsPath" }
  Write-Step "installing requirements ($requirementsRel) from $indexUrl"
  if ($BestEffort) {
    $specs = @(Get-Content -LiteralPath $requirementsPath |
      ForEach-Object { $_.Trim() } |
      Where-Object { $_ -ne '' -and $_ -notmatch '^#' })
    foreach ($spec in $specs) {
      try {
        Invoke-Python -PythonExe $pythonExe -Arguments @('-m', 'pip', 'install', '--no-cache-dir', '--only-binary=:all:', '-i', $indexUrl, $spec)
        $installedPackages.Add($spec)
      } catch {
        Write-Warn "package failed: $spec -> $($_.Exception.Message)"
        $failedPackages.Add($spec)
      }
    }
  } else {
    # Requirements files may carry non-ASCII comments; without UTF-8 mode pip
    # decodes them with the runner locale (cp1252) and aborts the whole install.
    Invoke-Python -PythonExe $pythonExe -Arguments @('-X', 'utf8', '-m', 'pip', 'install', '--no-cache-dir', '--only-binary=:all:', '-r', $requirementsPath, '-i', $indexUrl)
  }
}

# Wheels ship each package's test suite too (pandas alone is ~26 MB of tests).
# The frozen supplement never runs them, so prune them before packaging; the
# dist-info RECORD may then reference deleted paths, which is fine for runtime.
Write-Step 'pruning package test suites'
$sitePackages = Join-Path $pythonRoot 'Lib\site-packages'
$prunedTests = 0
foreach ($pkgDir in @(Get-ChildItem -LiteralPath $sitePackages -Directory -ErrorAction SilentlyContinue)) {
  foreach ($name in @('tests', 'testdata')) {
    $target = Join-Path $pkgDir.FullName $name
    if (Test-Path -LiteralPath $target -PathType Container) {
      Remove-Item -LiteralPath $target -Recurse -Force
      $prunedTests++
    }
  }
}
Write-Step ("pruned {0} test directories" -f $prunedTests)

# Playwright 1.40 supports Node >=16, but its win32 wheel still carries an x64
# driver/node.exe. The ia32 supplement replaces that file with a pinned 32-bit
# Node runtime; the PE check below remains a release gate so a future package
# update cannot silently make the Win7 package unloadable on 32-bit Windows.
$playwrightDriverNode = Join-Path $pythonRoot 'Lib\site-packages\playwright\driver\node.exe'
if ($Arch -eq 'ia32') {
  $playwrightNodeMeta = Get-PropertyValue -Object $pythonMeta -Name 'playwrightNode'
  if ($null -eq $playwrightNodeMeta) {
    Fail "manifest python.playwrightNode is required for ia32"
  }
  $nodeVersion = [string](Get-PropertyValue -Object $playwrightNodeMeta -Name 'version' -Default '16.20.2')
  $nodeFileName = [string](Get-PropertyValue -Object $playwrightNodeMeta -Name 'fileName' -Default "node-v$nodeVersion-win-x86.zip")
  $nodeUrl = [string](Get-PropertyValue -Object $playwrightNodeMeta -Name 'url')
  $nodeSha = [string](Get-PropertyValue -Object $playwrightNodeMeta -Name 'sha256')
  $nodeEntry = [string](Get-PropertyValue -Object $playwrightNodeMeta -Name 'archiveEntry' -Default "node-v$nodeVersion-win-x86/node.exe")
  if ([string]::IsNullOrWhiteSpace($nodeUrl)) { Fail "manifest python.playwrightNode.url is empty" }
  $nodeArchive = Join-Path $downDir $nodeFileName
  Get-DownloadedFile -Url $nodeUrl -Destination $nodeArchive -Force:$RefreshDownloads | Out-Null
  Assert-FileSha256 -Path $nodeArchive -Expected $nodeSha
  Expand-ZipEntry -Archive $nodeArchive -Entry $nodeEntry -Destination $playwrightDriverNode
  Write-Step "replaced Playwright driver node.exe with Node $nodeVersion x86"
}
if (-not (Test-Path -LiteralPath $playwrightDriverNode)) {
  Fail "Playwright driver node.exe missing after package installation: $playwrightDriverNode"
}
Assert-PeMachine -Path $playwrightDriverNode -Arch $Arch

# ---------------------------------------------------------------------------
# Playwright browsers (manifest-driven; off for the Win7 pack)
# ---------------------------------------------------------------------------
$installBrowser = [bool](Get-PropertyValue -Object $pythonMeta -Name 'installBrowser' -Default $false)
$browserInstalled = $false
if ($installBrowser -and (-not $SkipBrowser)) {
  $browsersRel = [string](Get-PropertyValue -Object $pythonMeta -Name 'browsersPath' -Default '')
  if ([string]::IsNullOrWhiteSpace($browsersRel)) { $browsersPath = Join-Path $pythonRoot 'ms-playwright' } else { $browsersPath = Join-Path $packRoot $browsersRel }
  Ensure-Directory $browsersPath
  Write-Step "installing Playwright Chromium into $browsersPath"
  $env:PLAYWRIGHT_BROWSERS_PATH = $browsersPath
  try {
    Invoke-Python -PythonExe $pythonExe -Arguments @('-m', 'playwright', 'install', 'chromium')
    $browserInstalled = $true
  } catch {
    Write-Warn "Playwright Chromium install failed (driver still bundled): $($_.Exception.Message)"
  }
} elseif ($installBrowser) {
  Write-Step 'skipping Playwright browsers (-SkipBrowser)'
}

# ---------------------------------------------------------------------------
# Git
# ---------------------------------------------------------------------------
$gitMeta = Get-PropertyValue -Object $manifest -Name 'git'
$gitVersion = [string](Get-PropertyValue -Object $gitMeta -Name 'version' -Default '')
$gitArchiveMeta = Get-PropertyValue -Object (Get-PropertyValue -Object $gitMeta -Name 'archives') -Name $Arch
if ($null -ne $gitArchiveMeta) {
  $gitArchive = $GitArchivePath
  if ([string]::IsNullOrWhiteSpace($gitArchive)) {
    $gitArchive = Join-Path $downDir ([string](Get-PropertyValue -Object $gitArchiveMeta -Name 'fileName' -Default 'PortableGit.7z.exe'))
    Get-DownloadedFile -Url ([string](Get-PropertyValue -Object $gitArchiveMeta -Name 'url')) -Destination $gitArchive -Force:$RefreshDownloads | Out-Null
  } elseif (-not (Test-Path -LiteralPath $gitArchive)) { Fail "git archive not found: $gitArchive" }
  Write-Step "staging Git $gitVersion into $gitRoot"
  Expand-SevenZipSelfExtractor -Archive $gitArchive -Destination $gitRoot

  # Slim the payload to what agents and Git Bash need: Git itself, Git Bash,
  # git-lfs and Git Credential Manager stay; vim, the Perl runtime (only used
  # by interactive helpers like `git add -p`), the Tcl/Tk GUI stack (gitk and
  # git gui), scalar and all documentation trees go. Pruning here keeps the
  # supplement and every installer built from it small; the same tree is what
  # `opt/git` ships to users.
  Write-Step 'pruning Git payload (vim/perl/tcl-tk/scalar/docs)'
  $gitPruneDirs = @(
    'usr\share\vim',
    'usr\share\doc',
    'usr\share\man',
    'usr\share\gtk-doc',
    'usr\lib\perl5',
    'usr\share\perl5',
    'mingw32\share\doc',
    'mingw32\share\perl5',
    'mingw32\share\git-gui',
    'mingw32\lib\tcl8.6',
    'mingw32\lib\tk8.6'
  )
  foreach ($rel in $gitPruneDirs) {
    $target = Join-Path $gitRoot $rel
    if (Test-Path -LiteralPath $target -PathType Container) {
      Remove-Item -LiteralPath $target -Recurse -Force
    }
  }
  $gitPruneFiles = @(
    'usr\bin\vim.exe',
    'usr\bin\vimdiff.exe',
    'usr\bin\view.exe',
    'usr\bin\rvim.exe',
    'usr\bin\rview.exe',
    'usr\bin\ex.exe',
    'usr\bin\perl.exe',
    'usr\bin\perl5.38.2.exe',
    'usr\bin\msys-perl5_38.dll',
    'usr\bin\msys-svn_swig_perl-1-0.dll',
    'mingw32\bin\scalar.exe',
    'mingw32\bin\gitk',
    'mingw32\libexec\git-core\scalar.exe',
    'mingw32\libexec\git-core\git-gui.exe',
    'mingw32\libexec\git-core\git-gui--askpass.exe',
    'mingw32\libexec\git-core\gitk.exe',
    'mingw32\libexec\git-core\tcl86.dll',
    'mingw32\libexec\git-core\tk86.dll'
  )
  $prunedGitFiles = 0
  foreach ($rel in $gitPruneFiles) {
    $target = Join-Path $gitRoot $rel
    if (Test-Path -LiteralPath $target -PathType Leaf) {
      Remove-Item -LiteralPath $target -Force
      $prunedGitFiles++
    }
  }
  $gitSmoke = & (Join-Path $gitRoot 'cmd\git.exe') '--version' 2>&1
  if (-not ("$gitSmoke" -match '^git version')) { Fail "git smoke failed after prune: $gitSmoke" }
  Write-Step ("git smoke ok: $gitSmoke (pruned {0} files)" -f $prunedGitFiles)
}

# ---------------------------------------------------------------------------
# ripgrep
# ---------------------------------------------------------------------------
$rgMeta = Get-PropertyValue -Object $manifest -Name 'rg'
$rgVersion = [string](Get-PropertyValue -Object $rgMeta -Name 'version' -Default '')
$rgArchiveMeta = Get-PropertyValue -Object (Get-PropertyValue -Object $rgMeta -Name 'archives') -Name $Arch
if ($null -ne $rgArchiveMeta) {
  $rgArchive = $RgArchivePath
  if ([string]::IsNullOrWhiteSpace($rgArchive)) {
    $rgArchive = Join-Path $downDir ([string](Get-PropertyValue -Object $rgArchiveMeta -Name 'fileName' -Default 'ripgrep.zip'))
    Get-DownloadedFile -Url ([string](Get-PropertyValue -Object $rgArchiveMeta -Name 'url')) -Destination $rgArchive -Force:$RefreshDownloads | Out-Null
  } elseif (-not (Test-Path -LiteralPath $rgArchive)) { Fail "ripgrep archive not found: $rgArchive" }
  Write-Step "staging ripgrep $rgVersion into $rgRoot"
  Expand-ZipFlatten -Archive $rgArchive -Destination $rgRoot
}

# ---------------------------------------------------------------------------
# ffmpeg (Windows-only)
# ---------------------------------------------------------------------------
$ffMeta = Get-PropertyValue -Object $manifest -Name 'ffmpeg'
$ffVersion = [string](Get-PropertyValue -Object $ffMeta -Name 'version' -Default '')
$ffmpegBundled = $false
if (-not $SkipFfmpeg) {
  $ffArchiveMeta = Get-PropertyValue -Object (Get-PropertyValue -Object $ffMeta -Name 'archives') -Name $Arch
  if ($null -ne $ffArchiveMeta) {
    $ffArchive = $FfmpegArchivePath
    if ([string]::IsNullOrWhiteSpace($ffArchive)) {
      $ffArchive = Join-Path $downDir ([string](Get-PropertyValue -Object $ffArchiveMeta -Name 'fileName' -Default 'ffmpeg.gz'))
      Get-DownloadedFile -Url ([string](Get-PropertyValue -Object $ffArchiveMeta -Name 'url')) -Destination $ffArchive -Force:$RefreshDownloads | Out-Null
    } elseif (-not (Test-Path -LiteralPath $ffArchive)) { Fail "ffmpeg archive not found: $ffArchive" }
    Write-Step "staging ffmpeg $ffVersion into $ffmpegRoot\bin"
    Expand-GzipFile -Archive $ffArchive -DestinationFile (Join-Path $ffmpegRoot 'bin\ffmpeg.exe')
    $ffmpegBundled = $true
  }
}

# ---------------------------------------------------------------------------
# r2 (radare2): console disassembler toolkit staged from the pinned release
# zip. The zip keeps one top-level directory, which is flattened into opt
# r2, so opt/r2/bin/radare2.exe matches the runtime PATH layout.
# ---------------------------------------------------------------------------
$r2Meta = Get-PropertyValue -Object $manifest -Name 'r2'
$r2Version = [string](Get-PropertyValue -Object $r2Meta -Name 'version' -Default '')
$r2Bundled = $false
if ($null -ne $r2Meta) {
  $r2ArchiveMeta = Get-PropertyValue -Object (Get-PropertyValue -Object $r2Meta -Name 'archives') -Name $Arch
  if ($null -ne $r2ArchiveMeta) {
    $r2Archive = $R2ArchivePath
    if ([string]::IsNullOrWhiteSpace($r2Archive)) {
      $r2Archive = Join-Path $downDir ([string](Get-PropertyValue -Object $r2ArchiveMeta -Name 'fileName' -Default 'radare2.zip'))
      Get-DownloadedFile -Url ([string](Get-PropertyValue -Object $r2ArchiveMeta -Name 'url')) -Destination $r2Archive -Force:$RefreshDownloads | Out-Null
    } elseif (-not (Test-Path -LiteralPath $r2Archive)) { Fail "r2 archive not found: $r2Archive" }
    $r2Sha = [string](Get-PropertyValue -Object $r2ArchiveMeta -Name 'sha256' -Default '')
    if (-not [string]::IsNullOrWhiteSpace($r2Sha)) { Assert-FileSha256 -Path $r2Archive -Expected $r2Sha }
    $r2Layout = Get-PropertyValue -Object $r2Meta -Name 'layout'
    $r2DestRel = [string](Get-PropertyValue -Object $r2Layout -Name 'dest' -Default 'opt\r2')
    $r2ExpectedExe = [string](Get-PropertyValue -Object $r2Layout -Name 'expectedExe' -Default 'bin/radare2.exe')
    $r2Target = Join-Path $packRoot $r2DestRel
    New-Item -ItemType Directory -Force -Path $r2Target | Out-Null
    Write-Step "staging r2 $r2Version into $r2Target"
    $r2Tmp = Join-Path $stageDir ('r2-unpack-' + [IO.Path]::GetRandomFileName())
    Expand-ZipInto -Archive $r2Archive -Destination $r2Tmp
    $r2Inner = @(Get-ChildItem -LiteralPath $r2Tmp -Directory | Select-Object -First 1)
    if ($r2Inner.Count -eq 1) {
      Copy-Item -Path (Join-Path $r2Inner[0].FullName '*') -Destination $r2Target -Recurse -Force
    } else {
      Copy-Item -Path (Join-Path $r2Tmp '*') -Destination $r2Target -Recurse -Force
    }
    Remove-Item -LiteralPath $r2Tmp -Recurse -Force -ErrorAction SilentlyContinue
    $r2Exe = Join-Path $r2Target ($r2ExpectedExe.Replace('/', '\'))
    if (-not (Test-Path -LiteralPath $r2Exe -PathType Leaf)) { Fail "radare2 executable missing after staging: $r2Exe" }
    Assert-PeMachine -Path $r2Exe -Arch $Arch
    $r2Smoke = & $r2Exe '-v' 2>&1
    if (-not ("$r2Smoke" -match 'radare2')) { Fail "r2 smoke failed: $r2Smoke" }
    Write-Step "r2 smoke ok: $r2Smoke"
    $r2Bundled = $true
  } else {
    Write-Step "r2 archive not defined for arch $Arch; supplement ships without r2"
  }
}

# ---------------------------------------------------------------------------
# Bundled browser (optional; hand-fed Chrome 109 win32, manifest browser section)
# ---------------------------------------------------------------------------
$browserMeta = Get-PropertyValue -Object $manifest -Name 'browser'
$browserEnabled = [bool](Get-PropertyValue -Object $browserMeta -Name 'enabled' -Default $false)
$browserVersion = [string](Get-PropertyValue -Object $browserMeta -Name 'version' -Default '')
$browserExeRel = [string](Get-PropertyValue -Object $browserMeta -Name 'expectedExe' -Default 'chrome-win/chrome.exe')
$browserDestRel = [string](Get-PropertyValue -Object $browserMeta -Name 'dest' -Default 'opt\browser')
$browserBundled = $false
$browserSourceLeaf = ''
if ($browserEnabled) {
  $browserSrc = $BrowserArchivePath
  if ([string]::IsNullOrWhiteSpace($browserSrc)) { $browserSrc = [string](Get-PropertyValue -Object $browserMeta -Name 'archivePath' -Default '') }
  if (-not [string]::IsNullOrWhiteSpace($browserSrc)) {
    if (-not (Test-Path -LiteralPath $browserSrc)) { Fail "browser archive not found: $browserSrc" }
    $browserRoot = Join-Path $packRoot $browserDestRel
    Write-Step "staging browser $browserVersion into $browserRoot"
    Install-BundledBrowser -Source $browserSrc -Destination $browserRoot -ExpectedExe $browserExeRel -Arch $Arch
    $browserBundled = $true
    $browserSourceLeaf = Split-Path -Leaf $browserSrc
  } else {
    Write-Step 'browser archivePath empty; supplement ships without a bundled browser'
  }
} else {
  Write-Step 'bundled browser disabled in manifest (browser.enabled=false)'
}

# ---------------------------------------------------------------------------
# Validate imports
# ---------------------------------------------------------------------------
$validate = @(Get-PropertyValue -Object $profileNode -Name 'validateImports' -Default @())
$missingImports = New-Object System.Collections.Generic.List[string]
foreach ($mod in $validate) {
  $rc = Invoke-PythonProcess -PythonExe $pythonExe -Arguments @('-c', ('import ' + $mod)) -SuppressOutput
  if ($rc -ne 0) { $missingImports.Add([string]$mod) }
}

# ---------------------------------------------------------------------------
# Metadata + archive
# ---------------------------------------------------------------------------
$archLabel = 'win7-' + $Arch
$zipName = 'wunder-supplement-' + $archLabel
if ($PythonProfile -eq 'common') { $zipName = $zipName + '-common' }
$zipName = $zipName + '.zip'
$zipPath = Join-Path $distDir $zipName

$browserNote = if ($browserBundled) { "bundled Chrome $browserVersion at opt/browser/$browserExeRel (win32); the browser_* tools detect it automatically" } elseif ($browserInstalled) { 'Playwright Chromium in opt/python/ms-playwright' } else { 'not bundled; the browser_* tools reuse a locally installed browser' }
$readmeText = @'
Wunder Desktop Windows supplement (__ARCH__, profile: __PROFILE__)

Contents:
  opt/python   Python __PYVER__ (embeddable) + pinned packages
  opt/git      PortableGit __GITVER__
  opt/rg       ripgrep __RGVER__
  opt/ffmpeg   ffmpeg __FFVER__ (when bundled)
  opt/r2       radare2 __R2VER__ (when bundled)
  opt/browser  Chrome __BROWSERVER__ win32 at __BROWSEREXE__ (when bundled)

Use Import supplement in the Pupa settings to install into WUNDER_HOME.
For manual installation, close processes using this runtime and unpack into
WUNDER_HOME (default: .wunder in your user home). The Pupa and Helm share
opt/python, opt/git, opt/rg and opt/ffmpeg, and prepend their bins to PATH.

Browsers: __BROWSER__
'@
$readmeText = $readmeText.Replace('__ARCH__', $archLabel).Replace('__PROFILE__', $PythonProfile).Replace('__PYVER__', $pyVer).Replace('__GITVER__', $gitVersion).Replace('__RGVER__', $rgVersion).Replace('__FFVER__', $ffVersion).Replace('__R2VER__', $r2Version).Replace('__BROWSERVER__', $browserVersion).Replace('__BROWSEREXE__', $browserExeRel).Replace('__BROWSER__', $browserNote)
Write-Utf8NoBom -Path (Join-Path $packRoot 'README-win7-supplement.txt') -Content $readmeText

$metaObj = [ordered]@{
  packageName   = [string](Get-PropertyValue -Object $manifest -Name 'packageName' -Default 'wunder-supplement')
  arch          = $Arch
  profile       = $PythonProfile
  generatedAt   = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  python        = [ordered]@{
    version = $pyVer
    packages = [string[]]$installedPackages
    playwrightDriverNode = [ordered]@{
      path = 'opt/python/Lib/site-packages/playwright/driver/node.exe'
      machine = if ($Arch -eq 'ia32') { 'i386' } else { 'x86_64' }
    }
  }
  git           = [ordered]@{ version = $gitVersion }
  rg            = [ordered]@{ version = $rgVersion }
  ffmpeg        = [ordered]@{ version = $ffVersion; bundled = $ffmpegBundled }
  r2            = [ordered]@{ version = $r2Version; bundled = $r2Bundled }
  browser       = [ordered]@{
    installBrowser = $installBrowser
    installed = $browserInstalled
    bundled = $browserBundled
    version = $browserVersion
    expectedExe = ('opt/browser/' + $browserExeRel)
    source = if ($browserBundled) { $browserSourceLeaf } else { '' }
  }
  failedPackages = [string[]]$failedPackages
  missingImports = [string[]]$missingImports
}
Write-Utf8NoBom -Path (Join-Path $packRoot 'wunder-win7-supplement.json') -Content (ConvertTo-Json -InputObject $metaObj -Depth 6)

Write-Step "packaging -> $zipPath"
if (Test-Path -LiteralPath $zipPath) { Remove-Item -LiteralPath $zipPath -Force }
Compress-Archive -Path (Join-Path $packRoot '*') -DestinationPath $zipPath -CompressionLevel Optimal -Force

$zipInfo = Get-Item -LiteralPath $zipPath
Write-Step ('done: {0} ({1:N1} MB)' -f $zipInfo.FullName, ($zipInfo.Length / 1MB))
if ($failedPackages.Count -gt 0) { Write-Warn "failed packages: $($failedPackages -join ', ')" }
if ($missingImports.Count -gt 0) { Write-Warn "missing imports: $($missingImports -join ', ')" }

$summaryObj = [ordered]@{
  zip            = $zipInfo.FullName
  sizeMB         = [math]::Round($zipInfo.Length / 1MB, 1)
  failedPackages = [string[]]$failedPackages
  missingImports = [string[]]$missingImports
}
Write-Host (ConvertTo-Json -InputObject $summaryObj -Depth 4)
