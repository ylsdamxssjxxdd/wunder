param(
  [ValidateSet('x64', 'ia32')]
  [string]$Arch = 'x64',
  [string]$BuildRoot = '',
  [ValidateSet('minimal', 'common')]
  [string]$PythonProfile = 'minimal',
  [string]$PythonRequirementsPath = '',
  [string]$PythonPackageIndexUrl = '',
  [string]$PythonArchivePath = '',
  [string]$GitArchivePath = '',
  [switch]$RefreshDownloads,
  [switch]$KeepStage
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step {
  param([string]$Message)
  Write-Host "[win7-supplement] $Message"
}

function Ensure-Directory {
  param([string]$Path)
  New-Item -ItemType Directory -Path $Path -Force | Out-Null
}

function Remove-DirectoryIfExists {
  param([string]$Path)
  if (Test-Path $Path) {
    Remove-Item -Path $Path -Recurse -Force
  }
}

function Invoke-DownloadIfNeeded {
  param(
    [string]$Url,
    [string]$Destination,
    [switch]$Refresh,
    [switch]$ValidateZip
  )

  if ($Refresh -and (Test-Path $Destination)) {
    Remove-Item -Path $Destination -Force
  }
  if (Test-Path $Destination) {
    if ($ValidateZip -and -not (Test-ZipArchiveReadable -Path $Destination)) {
      Write-Step "discarding broken zip: $Destination"
      Remove-Item -Path $Destination -Force
    } else {
      Write-Step "reusing download: $Destination"
      return
    }
  }
  Write-Step "downloading: $Url"
  if ($Url -like 'https://api.github.com/repos/*/releases/assets/*') {
    $previousProgressPreference = $global:ProgressPreference
    try {
      $global:ProgressPreference = 'SilentlyContinue'
      Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing -Headers @{ 'User-Agent' = 'wunder-build'; 'Accept' = 'application/octet-stream' }
    } finally {
      $global:ProgressPreference = $previousProgressPreference
    }
  } elseif ($Url -like 'https://github.com/*') {
    $finalUrl = Resolve-DownloadUrl -Url $Url
    try {
      $client = New-Object System.Net.WebClient
      $client.Headers.Add('User-Agent', 'wunder-build')
      $client.DownloadFile($finalUrl, $Destination)
    } finally {
      if ($client) {
        $client.Dispose()
      }
    }
  } else {
    $previousProgressPreference = $global:ProgressPreference
    try {
      $global:ProgressPreference = 'SilentlyContinue'
      Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing -Headers @{ 'User-Agent' = 'wunder-build' }
    } catch {
      if ($Url -notlike 'https://www.python.org/*') {
        throw
      }
      Invoke-PythonOrgFallbackDownload -Url $Url -Destination $Destination
    } finally {
      $global:ProgressPreference = $previousProgressPreference
    }
  }
  if ($ValidateZip -and -not (Test-ZipArchiveReadable -Path $Destination)) {
    throw "downloaded zip is unreadable: $Destination"
  }
}

function Resolve-DownloadUrl {
  param([string]$Url)

  if ($Url -notlike 'https://github.com/*') {
    return $Url
  }

  $request = [System.Net.HttpWebRequest]::Create($Url)
  $request.UserAgent = 'wunder-build'
  $request.AllowAutoRedirect = $false
  $response = $request.GetResponse()
  try {
    $redirect = $response.Headers['Location']
    if ($redirect) {
      return $redirect
    }
  } finally {
    $response.Close()
  }
  return $Url
}

function Invoke-PythonOrgFallbackDownload {
  param(
    [string]$Url,
    [string]$Destination
  )

  $resolvedIp = Resolve-PythonOrgIpv4Address
  if (-not $resolvedIp) {
    throw "python.org download failed and IPv4 fallback could not resolve www.python.org"
  }
  if (-not (Get-Command 'curl.exe' -ErrorAction SilentlyContinue)) {
    throw "python.org download failed and curl.exe is unavailable for IPv4 fallback"
  }
  Write-Step "python.org IPv4 fallback via $resolvedIp"
  & curl.exe --fail --location --output $Destination --resolve ("www.python.org:443:{0}" -f $resolvedIp) $Url
  if ($LASTEXITCODE -ne 0) {
    throw "python.org fallback download failed with exit code $LASTEXITCODE"
  }
}

function Resolve-PythonOrgIpv4Address {
  $lines = & nslookup www.python.org 8.8.8.8 2>$null
  foreach ($line in $lines) {
    if ($line -match '(?<ip>(?:\d{1,3}\.){3}\d{1,3})') {
      return $matches.ip
    }
  }
  return $null
}

function Test-ZipArchiveReadable {
  param([string]$Path)

  try {
    $archive = [System.IO.Compression.ZipFile]::OpenRead($Path)
    $archive.Dispose()
    return $true
  } catch {
    return $false
  }
}

function Expand-ZipArchive {
  param(
    [string]$Archive,
    [string]$Destination
  )

  Remove-DirectoryIfExists -Path $Destination
  Ensure-Directory -Path $Destination
  Expand-Archive -Path $Archive -DestinationPath $Destination -Force
}

function Resolve-ArchiveInput {
  param(
    [string]$ProvidedPath,
    [string]$CachedPath
  )

  if ($ProvidedPath) {
    $resolved = (Resolve-Path $ProvidedPath).Path
    if ($resolved -ne $CachedPath) {
      Copy-Item -Path $resolved -Destination $CachedPath -Force
    }
    return $CachedPath
  }
  return $CachedPath
}

function Resolve-RepoRelativePath {
  param(
    [string]$Path,
    [string]$RepoRoot
  )

  if ([string]::IsNullOrWhiteSpace($Path)) {
    return ''
  }
  if ([System.IO.Path]::IsPathRooted($Path)) {
    return [System.IO.Path]::GetFullPath($Path)
  }
  return [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $Path))
}

function Get-PythonProfileEntry {
  param(
    [psobject]$Config,
    [string]$ProfileName
  )

  $profile = $Config.python.profiles.PSObject.Properties | Where-Object { $_.Name -eq $ProfileName } | Select-Object -First 1
  if (-not $profile) {
    throw "unknown Python profile: $ProfileName"
  }
  return $profile.Value
}

function Get-RequirementsEntries {
  param([string]$Path)

  if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path $Path)) {
    return @()
  }

  return @(Get-Content -Path $Path | ForEach-Object { $_.Trim() } | Where-Object { $_ -and -not $_.StartsWith('#') })
}

function Write-TextUtf8NoBom {
  param(
    [string]$Path,
    [string]$Content
  )

  $encoding = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Content, $encoding)
}

function Get-ArchiveEntry {
  param(
    [psobject]$Config,
    [string]$Arch,
    [string]$Section
  )

  if ($Section -eq 'python') {
    if ($Arch -eq 'ia32') {
      return $Config.python.archives.ia32
    }
    return $Config.python.archives.x64
  }
  if ($Arch -eq 'ia32') {
    return $Config.git.archives.ia32
  }
  return $Config.git.archives.x64
}

function Initialize-EmbeddedPythonLayout {
  param([string]$PythonRoot)

  $sitePackagesDir = Join-Path $PythonRoot 'Lib\site-packages'
  Ensure-Directory -Path $sitePackagesDir

  $pthFile = Get-ChildItem -Path $PythonRoot -Filter 'python*._pth' -File | Select-Object -First 1
  if (-not $pthFile) {
    throw "python embeddable ._pth file not found under $PythonRoot"
  }

  $zipEntry = Get-ChildItem -Path $PythonRoot -Filter 'python*.zip' -File | Select-Object -First 1
  $zipName = if ($zipEntry) { $zipEntry.Name } else { 'python38.zip' }
  $pthContent = @(
    $zipName,
    '.',
    'Lib',
    'Lib/site-packages',
    'import site'
  ) -join "`r`n"
  Write-TextUtf8NoBom -Path $pthFile.FullName -Content ($pthContent + "`r`n")

  $pythonExe = Join-Path $PythonRoot 'python.exe'
  if (-not (Test-Path $pythonExe)) {
    throw "python.exe missing from embedded Python root: $PythonRoot"
  }
  $python3Cmd = Join-Path $PythonRoot 'python3.cmd'
  $python3CmdContent = @'
@echo off
"%~dp0python.exe" %*
'@
  Write-TextUtf8NoBom -Path $python3Cmd -Content $python3CmdContent

  $pyCmd = Join-Path $PythonRoot 'py.cmd'
  $pyCmdContent = @'
@echo off
"%~dp0python.exe" %*
'@
  Write-TextUtf8NoBom -Path $pyCmd -Content $pyCmdContent

  $readme = @(
    'Wunder embedded Python runtime for Windows 7.',
    'This package uses CPython 3.8.10 embeddable distribution.',
    'Lib/site-packages is pre-created so Wunder can add app-local Python packages later if needed.'
  ) -join "`r`n"
  Write-TextUtf8NoBom -Path (Join-Path $PythonRoot 'README-wunder.txt') -Content ($readme + "`r`n")
}

function Install-EmbeddedPythonSupportFiles {
  param(
    [string]$PythonRoot,
    [string]$RepoRoot
  )

  $matplotlibRcSource = Join-Path $RepoRoot 'config\matplotlibrc'
  if (-not (Test-Path $matplotlibRcSource)) {
    return
  }

  $etcDir = Join-Path $PythonRoot 'etc'
  Ensure-Directory -Path $etcDir
  Copy-Item -Path $matplotlibRcSource -Destination (Join-Path $etcDir 'matplotlibrc') -Force
}

function Install-EmbeddedMatplotlibFonts {
  param(
    [string]$PythonRoot,
    [string]$RepoRoot,
    [string[]]$FontList = @()
  )

  $fontsSourceDir = Join-Path $RepoRoot 'fonts'
  if (-not (Test-Path $fontsSourceDir)) {
    return
  }

  # Mirror ARM sidecar behavior so bundled Python keeps stable CJK/Latin rendering on Win7.
  $matplotlibFontsDir = Join-Path $PythonRoot 'Lib\site-packages\matplotlib\mpl-data\fonts\ttf'
  if (-not (Test-Path $matplotlibFontsDir)) {
    return
  }

  $defaultFonts = @(
    'NotoSansSC-VF.ttf',
    'NotoSerifSC-VF.ttf',
    'msyh.ttc',
    'msyhbd.ttc',
    'simsun.ttc',
    'simhei.ttf',
    'arial.ttf',
    'arialbd.ttf',
    'times.ttf',
    'timesbd.ttf',
    'consola.ttf'
  )
  $fontsToCopy = if ($FontList.Count -gt 0) { $FontList } else { $defaultFonts }

  foreach ($font in $fontsToCopy) {
    $name = $font.Trim()
    if ([string]::IsNullOrWhiteSpace($name)) {
      continue
    }
    $sourcePath = Join-Path $fontsSourceDir $name
    if (-not (Test-Path $sourcePath)) {
      continue
    }
    Copy-Item -Path $sourcePath -Destination (Join-Path $matplotlibFontsDir $name) -Force
  }
}

function Install-EmbeddedPythonPackages {
  param(
    [string]$PythonRoot,
    [string]$BuildRoot,
    [string]$RequirementsPath,
    [string]$PackageIndexUrl,
    [psobject]$Config,
    [switch]$Refresh
  )

  if ([string]::IsNullOrWhiteSpace($RequirementsPath)) {
    return
  }

  $pythonExe = Join-Path $PythonRoot 'python.exe'
  if (-not (Test-Path $pythonExe)) {
    throw "python.exe missing from embedded Python root: $PythonRoot"
  }

  $bootstrapDir = Join-Path $BuildRoot 'bootstrap'
  $scriptsDir = Join-Path $PythonRoot 'Scripts'
  Ensure-Directory -Path $bootstrapDir
  Ensure-Directory -Path $scriptsDir

  $getPipPath = Join-Path $bootstrapDir 'get-pip.py'
  $bootstrap = $Config.python.bootstrap
  Invoke-DownloadIfNeeded -Url $bootstrap.getPipUrl -Destination $getPipPath -Refresh:$Refresh

  Write-Step 'bootstrapping pip into embedded Python'
  & $pythonExe $getPipPath $bootstrap.pipSpec $bootstrap.setuptoolsSpec $bootstrap.wheelSpec
  if ($LASTEXITCODE -ne 0) {
    throw "embedded Python pip bootstrap failed with exit code $LASTEXITCODE"
  }

  $pipArgs = @(
    '-m',
    'pip',
    'install',
    '--disable-pip-version-check',
    '--no-warn-script-location',
    '--only-binary=:all:',
    '--no-cache-dir',
    '--no-compile'
  )
  if (-not [string]::IsNullOrWhiteSpace($PackageIndexUrl)) {
    $pipArgs += @('--index-url', $PackageIndexUrl)
  }
  $pipArgs += @('-r', $RequirementsPath)

  Write-Step ("installing Python packages from {0}" -f (Split-Path -Leaf $RequirementsPath))
  & $pythonExe @pipArgs
  if ($LASTEXITCODE -ne 0) {
    throw "embedded Python package install failed with exit code $LASTEXITCODE"
  }
}

function Write-SupplementReadme {
  param(
    [string]$Destination,
    [string]$Arch,
    [psobject]$Config,
    [string]$PythonProfile,
    [string[]]$PythonPackages
  )

  $content = @(
    'Wunder Windows 7 supplement package',
    '',
    "Arch: $Arch",
    "Python: $($Config.python.version)",
    "Python profile: $PythonProfile",
    "Git: $($Config.git.flavor) $($Config.git.version)",
    '',
    'Usage:',
    '1. Close Wunder Desktop.',
    '2. Extract this zip directly into the desktop install directory.',
    '3. Ensure the install directory now contains opt\python and opt\git.',
    '4. Start Wunder Desktop again; the Electron runtime will prepend opt\python and opt\git to PATH automatically.',
    '',
    'Notes:',
    '- This supplement package is intended for the Win7 Electron desktop build.',
    '- Python 3.8 is the last official CPython line with Windows 7 support.',
    '- Git for Windows 2.46.2 is the last official line supporting Windows 7 / 8 / 8.1.',
    '- Python 3.8 embeddable package works best on Windows 7 with KB2533623 and the Universal CRT update installed.'
  )
  if ($PythonPackages.Count -gt 0) {
    $content += ''
    $content += 'Python packages:'
    $content += '- pip / setuptools / wheel'
    foreach ($package in $PythonPackages) {
      $content += ("- {0}" -f $package)
    }
  }
  $content = $content -join "`r`n"
  Write-TextUtf8NoBom -Path $Destination -Content ($content + "`r`n")
}

function Write-SupplementManifest {
  param(
    [string]$Destination,
    [string]$Arch,
    [string]$PythonUrl,
    [string]$GitUrl,
    [string]$BuildRoot,
    [psobject]$Config,
    [string]$PythonProfile,
    [string]$PythonRequirementsPath,
    [string[]]$PythonPackages
  )

  $payload = [ordered]@{
    generatedAt = (Get-Date).ToString('o')
    arch = $Arch
    buildRoot = $BuildRoot
    packageName = $Config.packageName
    python = [ordered]@{
      version = $Config.python.version
      profile = $PythonProfile
      source = $Config.python.source
      url = $PythonUrl
      requirementsPath = $PythonRequirementsPath
      packages = $PythonPackages
      bootstrap = [ordered]@{
        getPipUrl = $Config.python.bootstrap.getPipUrl
        pipSpec = $Config.python.bootstrap.pipSpec
        setuptoolsSpec = $Config.python.bootstrap.setuptoolsSpec
        wheelSpec = $Config.python.bootstrap.wheelSpec
      }
    }
    git = [ordered]@{
      version = $Config.git.version
      flavor = $Config.git.flavor
      source = $Config.git.source
      url = $GitUrl
    }
    install = [ordered]@{
      extractInto = 'desktop install root'
      pythonRoot = 'opt/python'
      gitRoot = 'opt/git'
    }
  }
  $json = $payload | ConvertTo-Json -Depth 5
  Write-TextUtf8NoBom -Path $Destination -Content ($json + "`r`n")
}

function Test-StagedRuntime {
  param(
    [string]$PythonRoot,
    [string]$GitRoot,
    [string]$BuildRoot,
    [string]$PythonProfile,
    [string[]]$ValidateImports,
    [switch]$PlotProbe
  )

  $pythonExe = Join-Path $PythonRoot 'python.exe'
  if (-not (Test-Path $pythonExe)) {
    throw "staged python.exe missing: $pythonExe"
  }
  $pythonVersion = & $pythonExe --version 2>&1
  if ($LASTEXITCODE -ne 0) {
    throw "staged python runtime failed validation"
  }
  Write-Step "validated Python: $pythonVersion"

  $gitExeCandidates = @(
    (Join-Path $GitRoot 'cmd\git.exe'),
    (Join-Path $GitRoot 'bin\git.exe')
  )
  $gitExe = $gitExeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
  if (-not $gitExe) {
    throw "staged git.exe missing under $GitRoot"
  }
  $gitVersion = & $gitExe --version 2>&1
  if ($LASTEXITCODE -ne 0) {
    throw "staged git runtime failed validation"
  }
  Write-Step "validated Git: $gitVersion"

  if ($ValidateImports.Count -gt 0 -or $PlotProbe) {
    $probeModules = $ValidateImports | ConvertTo-Json -Compress
    $plotProbePath = Join-Path $BuildRoot 'matplotlib-probe.png'
    $probePath = Join-Path $BuildRoot 'validate-python-profile.py'
    $plotProbeEnabled = if ($PlotProbe.IsPresent) { 'True' } else { 'False' }
    $probeScript = @"
import importlib
import json
from pathlib import Path

modules = json.loads(r'''$probeModules''')
for module_name in modules:
    importlib.import_module(module_name)

if $($plotProbeEnabled):
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt

    plt.plot([1, 2, 3], [1, 4, 9])
    plt.title('wunder-win7-python-profile')
    plt.savefig(Path(r'''$plotProbePath'''))
"@
    Write-TextUtf8NoBom -Path $probePath -Content $probeScript
    & $pythonExe $probePath
    if ($LASTEXITCODE -ne 0) {
      throw "staged Python profile validation failed for $PythonProfile"
    }
    Write-Step "validated Python profile: $PythonProfile"
  }
}

Add-Type -AssemblyName System.IO.Compression.FileSystem

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '..\..\..')).Path
$manifestPath = Join-Path $scriptDir 'win7-supplement-manifest.json'
$config = Get-Content -Path $manifestPath -Raw | ConvertFrom-Json
$pythonProfileConfig = Get-PythonProfileEntry -Config $config -ProfileName $PythonProfile

$resolvedBuildRoot = if ($BuildRoot) { $BuildRoot } else { Join-Path $repoRoot $config.defaultBuildRoot }
$resolvedBuildRoot = [System.IO.Path]::GetFullPath($resolvedBuildRoot)
$downloadsDir = Join-Path $resolvedBuildRoot $config.downloadsDir
$stageDir = Join-Path $resolvedBuildRoot $config.stageDir
$distDir = Join-Path $resolvedBuildRoot $config.distDir
$packageRoot = Join-Path $stageDir 'package-root'
$pythonRoot = Join-Path $packageRoot $config.layout.pythonRoot
$gitRoot = Join-Path $packageRoot $config.layout.gitRoot
$resolvedPythonRequirementsPath = if ($PythonRequirementsPath) {
  Resolve-RepoRelativePath -Path $PythonRequirementsPath -RepoRoot $repoRoot
} else {
  Resolve-RepoRelativePath -Path $pythonProfileConfig.requirementsPath -RepoRoot $repoRoot
}
$resolvedPythonPackageIndexUrl = if ($PythonPackageIndexUrl) { $PythonPackageIndexUrl } else { $config.python.defaultPackageIndexUrl }
$pythonRequirementsEntries = Get-RequirementsEntries -Path $resolvedPythonRequirementsPath

Ensure-Directory -Path $resolvedBuildRoot
Ensure-Directory -Path $downloadsDir
Ensure-Directory -Path $distDir
Remove-DirectoryIfExists -Path $stageDir
Ensure-Directory -Path $packageRoot
Ensure-Directory -Path $pythonRoot
Ensure-Directory -Path $gitRoot

$pythonArchive = Get-ArchiveEntry -Config $config -Arch $Arch -Section 'python'
$gitArchive = Get-ArchiveEntry -Config $config -Arch $Arch -Section 'git'
$cachedPythonArchivePath = Join-Path $downloadsDir $pythonArchive.fileName
$cachedGitArchivePath = Join-Path $downloadsDir $gitArchive.fileName
$hasProvidedPythonArchive = -not [string]::IsNullOrWhiteSpace($PythonArchivePath)
$hasProvidedGitArchive = -not [string]::IsNullOrWhiteSpace($GitArchivePath)
$pythonArchivePath = Resolve-ArchiveInput -ProvidedPath $PythonArchivePath -CachedPath $cachedPythonArchivePath
$gitArchivePath = Resolve-ArchiveInput -ProvidedPath $GitArchivePath -CachedPath $cachedGitArchivePath
$pythonDownloadUrl = if ($pythonArchive.PSObject.Properties['downloadUrl']) { $pythonArchive.downloadUrl } else { $pythonArchive.url }
$gitDownloadUrl = if ($gitArchive.PSObject.Properties['downloadUrl']) { $gitArchive.downloadUrl } else { $gitArchive.url }

if (-not $hasProvidedPythonArchive) {
  Invoke-DownloadIfNeeded -Url $pythonDownloadUrl -Destination $pythonArchivePath -Refresh:$RefreshDownloads -ValidateZip
}
if (-not $hasProvidedGitArchive) {
  Invoke-DownloadIfNeeded -Url $gitDownloadUrl -Destination $gitArchivePath -Refresh:$RefreshDownloads -ValidateZip
}

Write-Step "extracting Python runtime"
Expand-ZipArchive -Archive $pythonArchivePath -Destination $pythonRoot
Initialize-EmbeddedPythonLayout -PythonRoot $pythonRoot
Install-EmbeddedPythonSupportFiles -PythonRoot $pythonRoot -RepoRoot $repoRoot
Install-EmbeddedPythonPackages -PythonRoot $pythonRoot -BuildRoot $resolvedBuildRoot -RequirementsPath $resolvedPythonRequirementsPath -PackageIndexUrl $resolvedPythonPackageIndexUrl -Config $config -Refresh:$RefreshDownloads
Install-EmbeddedMatplotlibFonts -PythonRoot $pythonRoot -RepoRoot $repoRoot

Write-Step "extracting Git runtime"
Expand-ZipArchive -Archive $gitArchivePath -Destination $gitRoot
Test-StagedRuntime -PythonRoot $pythonRoot -GitRoot $gitRoot -BuildRoot $resolvedBuildRoot -PythonProfile $PythonProfile -ValidateImports @($pythonProfileConfig.validateImports) -PlotProbe:([bool]$pythonProfileConfig.plotProbe)

$readmePath = Join-Path $packageRoot 'README-win7-supplement.txt'
$manifestOutPath = Join-Path $packageRoot 'wunder-win7-supplement.json'
Write-SupplementReadme -Destination $readmePath -Arch $Arch -Config $config -PythonProfile $PythonProfile -PythonPackages $pythonRequirementsEntries
Write-SupplementManifest -Destination $manifestOutPath -Arch $Arch -PythonUrl $pythonArchive.url -GitUrl $gitArchive.url -BuildRoot $resolvedBuildRoot -Config $config -PythonProfile $PythonProfile -PythonRequirementsPath $resolvedPythonRequirementsPath -PythonPackages $pythonRequirementsEntries

$zipName = if ($PythonProfile -eq 'minimal') {
  "{0}-win7-{1}.zip" -f $config.packageName, $Arch
} else {
  "{0}-win7-{1}-{2}.zip" -f $config.packageName, $Arch, $PythonProfile
}
$zipPath = Join-Path $distDir $zipName
if (Test-Path $zipPath) {
  Remove-Item -Path $zipPath -Force
}

Write-Step "packing supplement zip: $zipPath"
[System.IO.Compression.ZipFile]::CreateFromDirectory($packageRoot, $zipPath, [System.IO.Compression.CompressionLevel]::Optimal, $false)

Write-Step "output zip: $zipPath"
Write-Step "package root: $packageRoot"
if (-not $KeepStage) {
  Remove-DirectoryIfExists -Path $stageDir
} else {
  Write-Step "kept staged runtime at $packageRoot"
}
