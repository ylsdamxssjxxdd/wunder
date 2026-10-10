<#
.SYNOPSIS
Build the Wunder Desktop Windows supplement (bundled Python + Git + ripgrep + ffmpeg).

.DESCRIPTION
Windows counterpart of packaging/linux/scripts/build_linux_supplement.sh.
Reads packaging/windows/scripts/win7-supplement-manifest.json, downloads the
pinned archives, stages the opt/ tree, optionally installs the Playwright
Chromium (manifest python.installBrowser), and produces
wunder-supplement-win7-<arch>[-<profile>].zip.

The archive is meant to be extracted directly into the Wunder Desktop install
directory; the native runtime auto-detects opt/python, opt/git, opt/rg and
opt/ffmpeg on startup.

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

.PARAMETER PythonArchivePath / GitArchivePath / RgArchivePath / FfmpegArchivePath
Use hand-prepared local archives instead of downloading.

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

function Expand-ZipInto {
  param([string]$Archive, [string]$Destination)
  if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Recurse -Force }
  Ensure-Directory $Destination
  Expand-Archive -LiteralPath $Archive -DestinationPath $Destination -Force
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
    Invoke-Python -PythonExe $pythonExe -Arguments @('-m', 'pip', 'install', '--no-cache-dir', '--only-binary=:all:', '-r', $requirementsPath, '-i', $indexUrl)
  }
}

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

$browserNote = if ($browserInstalled) { 'Playwright Chromium in opt/python/ms-playwright' } else { 'not bundled; the browser_* tools reuse a locally installed browser' }
$readmeText = @'
Wunder Desktop Windows supplement (__ARCH__, profile: __PROFILE__)

Contents:
  opt/python   Python __PYVER__ (embeddable) + pinned packages
  opt/git      PortableGit __GITVER__
  opt/rg       ripgrep __RGVER__
  opt/ffmpeg   ffmpeg __FFVER__ (when bundled)

Unpack this archive into the Wunder Desktop install directory (next to the
executable). The native runtime auto-detects opt/python, opt/git, opt/rg and
opt/ffmpeg on startup, and prepends their bin directories to PATH.

Browsers: __BROWSER__
'@
$readmeText = $readmeText.Replace('__ARCH__', $archLabel).Replace('__PROFILE__', $PythonProfile).Replace('__PYVER__', $pyVer).Replace('__GITVER__', $gitVersion).Replace('__RGVER__', $rgVersion).Replace('__FFVER__', $ffVersion).Replace('__BROWSER__', $browserNote)
Write-Utf8NoBom -Path (Join-Path $packRoot 'README-win7-supplement.txt') -Content $readmeText

$metaObj = [ordered]@{
  packageName   = [string](Get-PropertyValue -Object $manifest -Name 'packageName' -Default 'wunder-supplement')
  arch          = $Arch
  profile       = $PythonProfile
  generatedAt   = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
  python        = [ordered]@{ version = $pyVer; packages = [string[]]$installedPackages }
  git           = [ordered]@{ version = $gitVersion }
  rg            = [ordered]@{ version = $rgVersion }
  ffmpeg        = [ordered]@{ version = $ffVersion; bundled = $ffmpegBundled }
  browser       = [ordered]@{ installBrowser = $installBrowser; installed = $browserInstalled }
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