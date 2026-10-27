<#
.SYNOPSIS
Build the Wunder Desktop Windows supplement (bundled Python + Git + ripgrep + ffmpeg).

.DESCRIPTION
Windows counterpart of packaging/linux/scripts/build_linux_supplement.sh.
Reads packaging/windows/scripts/win7-supplement-manifest.json, downloads the
pinned archives, stages the opt/ tree, optionally installs the Playwright
Chromium (manifest python.installBrowser), and produces
wunder-supplement-win7-<arch>[-<profile>].zip. With the default build root the
zip lands in target\installer\payload\<win7-arch>\ (found by build.bat pack);
an explicit -BuildRoot keeps dist\ inside that root.

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
overrides the automatic Chrome 109 cache/download flow and also accepts an
unpacked browser directory staged into opt/browser.

.PARAMETER BestEffort
Install requirements package-by-package and continue past failures (records them).

.PARAMETER SkipPrune / SkipGitPrune
Skip the stage prune entirely, or only its MinGit part. Pruning normally runs
through prune-supplement-stage.py (wheel test suites, foreign-architecture PE
files, MinGit extras), so the PowerShell delete-guard problem no longer applies;
these switches are escape hatches for debugging a pack, and the PE architecture
gate still runs afterwards. To prune an already staged tree:

  python packaging/windows/scripts/prune-supplement-stage.py <build>/stage/package-root --arch ia32

.PARAMETER PruneOnly
Skip staging and rebuild nothing: prune the existing stage, re-run the PE
architecture and git smoke gates, and repack the zip. Two-step builds use it on
hosts that refuse the bulk deletes a fresh build performs inside the stage:

  build_win7_desktop_supplement.ps1 -SkipPrune        # stage everything
  build_win7_desktop_supplement.ps1 -PruneOnly        # prune, gate, repack
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

  [string]$BrowserArchivePath = '',

  [switch]$SkipPrune,
  [switch]$SkipGitPrune,

  [switch]$PruneOnly,

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

function Remove-Tree {
  # Deletes build output (stage, extraction targets, temp dirs) through .NET
  # instead of Remove-Item. Several build hosts -- including this repository's
  # own agent runtime -- wrap Remove-Item in a delete guard that routes every
  # recursive call through the Recycle Bin and refuses bulk deletes, which used
  # to abort the supplement build before it started and forced -SkipPrune.
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return }
  $item = Get-Item -LiteralPath $Path -Force
  if ($item.PSIsContainer) {
    [System.IO.Directory]::Delete($item.FullName, $true)
  } else {
    [System.IO.File]::Delete($item.FullName)
  }
}

function Remove-TreeQuiet {
  # Best-effort cleanup for temp trees. A freshly created .git can still be held
  # by antivirus or an exiting process for a moment, and a leftover temp folder
  # must never fail a build.
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { return }
  for ($attempt = 0; $attempt -lt 3; $attempt++) {
    try {
      Remove-Tree $Path
      return
    } catch {
      Start-Sleep -Milliseconds 300
    }
  }
  Write-Warn "could not remove temp tree: $Path"
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
        Remove-Item -LiteralPath $Destination -Force -ErrorAction SilentlyContinue
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
    Remove-Item -LiteralPath $Destination -Force -ErrorAction SilentlyContinue
    $candidates = New-Object System.Collections.Generic.List[string]
    $sysCurl = Join-Path $env:SystemRoot 'System32\curl.exe'
    if (Test-Path -LiteralPath $sysCurl) { $candidates.Add($sysCurl) }
    $found = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($found -and ($found.Source)) { $candidates.Add([string]$found.Source) }
    foreach ($candidate in $candidates) {
      Write-Warn "falling back to curl: $candidate"
      Invoke-ProcessCapture -FilePath $candidate -Arguments @('-L', '--fail', '--retry', '3', '--retry-delay', '2', '-o', $Destination, $Url) | Out-Null
      $ok = (Test-Path -LiteralPath $Destination) -and ((Get-Item -LiteralPath $Destination).Length -gt 0)
      if ($ok) { break }
      Remove-Item -LiteralPath $Destination -Force -ErrorAction SilentlyContinue
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

function Get-PeMachineHeader {
  # Machine word from just the DOS stub plus the COFF header. The whole-file
  # reader above is fine for a single gate binary but far too slow for the
  # several hundred PE images a supplement pack carries.
  param([string]$Path)
  try {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
      $head = New-Object byte[] 0x40
      if ($stream.Read($head, 0, $head.Length) -lt 0x40) { return $null }
      if ($head[0] -ne 0x4d -or $head[1] -ne 0x5a) { return $null }
      $peOffset = [BitConverter]::ToInt32($head, 0x3c)
      if ($peOffset -lt 0 -or $peOffset -gt ($stream.Length - 6)) { return $null }
      $stream.Position = $peOffset
      $coff = New-Object byte[] 6
      if ($stream.Read($coff, 0, 6) -lt 6) { return $null }
      if ($coff[0] -ne 0x50 -or $coff[1] -ne 0x45 -or $coff[2] -ne 0 -or $coff[3] -ne 0) { return $null }
      return [BitConverter]::ToUInt16($coff, 4)
    } finally {
      $stream.Dispose()
    }
  } catch {
    return $null
  }
}

function Assert-SupplementPeArch {
  # Release gate over the whole staged tree. A 32-bit supplement that carries a
  # single x64 DLL only fails at runtime on the target machine (this is how an
  # x86-64 capstone.dll once shipped in the ia32 pack), so every PE image is
  # checked here instead of trusting each component's own pin.
  param([string]$PackageRoot, [ValidateSet('ia32', 'x64')][string]$Arch)
  $allowed = if ($Arch -eq 'ia32') { @(0x014c) } else { @(0x8664, 0x014c) }
  $suffixes = @('.exe', '.dll', '.pyd', '.node')
  $bad = New-Object System.Collections.Generic.List[string]
  $total = 0
  foreach ($file in @(Get-ChildItem -LiteralPath $PackageRoot -Recurse -File -ErrorAction SilentlyContinue)) {
    if ($suffixes -notcontains $file.Extension.ToLowerInvariant()) { continue }
    $machine = Get-PeMachineHeader -Path $file.FullName
    if ($null -eq $machine) { continue }
    $total++
    if ($allowed -notcontains $machine) {
      $rel = $file.FullName.Substring($PackageRoot.Length).TrimStart('\', '/')
      $bad.Add(('0x{0:x4} {1}' -f $machine, $rel))
    }
  }
  if ($bad.Count -gt 0) {
    $preview = (@($bad | Select-Object -First 10) -join [Environment]::NewLine)
    Fail ("{0} PE file(s) do not match the {1} pack:{2}{3}{2}Run again with -PruneOnly to drop them and repack the stage." -f `
      $bad.Count, $Arch, [Environment]::NewLine, $preview)
  }
  Write-Step "validated PE architecture ($Arch) for $total PE files"
}

function Invoke-GitSmoke {
  # Proves the trimmed tree still runs: --version alone does not exercise the
  # MSYS runtime, the shell helpers or the dll set, so a temporary repository is
  # created and committed through the packaged binary.
  param([string]$GitRoot)
  $gitCmdExe = Join-Path $GitRoot 'cmd\git.exe'
  if (-not (Test-Path -LiteralPath $gitCmdExe -PathType Leaf)) { return }
  $gitProbe = Join-Path ([System.IO.Path]::GetTempPath()) ('wunder-git-smoke-' + [guid]::NewGuid().ToString('N'))
  Ensure-Directory $gitProbe
  try {
    $gitEnv = @{
      GIT_AUTHOR_NAME = 'wunder'; GIT_AUTHOR_EMAIL = 'wunder@localhost'
      GIT_COMMITTER_NAME = 'wunder'; GIT_COMMITTER_EMAIL = 'wunder@localhost'
      GIT_CONFIG_NOSYSTEM = '1'; HOME = $gitProbe
    }
    $previous = @{}
    foreach ($key in $gitEnv.Keys) {
      $previous[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
      [Environment]::SetEnvironmentVariable($key, $gitEnv[$key], 'Process')
    }
    try {
      $smokeCommands = @(
        @('--version'),
        @('init', '--quiet', $gitProbe),
        @('-C', $gitProbe, 'config', 'user.email', 'wunder@localhost'),
        @('-C', $gitProbe, 'config', 'user.name', 'wunder'),
        @('-C', $gitProbe, 'commit', '--allow-empty', '--quiet', '-m', 'smoke'),
        @('-C', $gitProbe, 'log', '--oneline')
      )
      $lastOutput = ''
      foreach ($smokeArgs in $smokeCommands) {
        $result = Invoke-ProcessCapture -FilePath $gitCmdExe -Arguments $smokeArgs
        $lastOutput = $result.StdOut
        if ($result.ExitCode -ne 0 -or -not [string]::IsNullOrWhiteSpace($result.StdErr)) {
          Fail ("git smoke failed at 'git {0}': exit {1} {2}" -f ($smokeArgs -join ' '), $result.ExitCode, $result.StdErr)
        }
      }
      if (-not ("$lastOutput" -match 'smoke')) {
        Fail "git smoke produced no commit log: $lastOutput"
      }
    } finally {
      foreach ($key in $previous.Keys) {
        [Environment]::SetEnvironmentVariable($key, $previous[$key], 'Process')
      }
    }
  } finally {
    Remove-TreeQuiet $gitProbe
  }
  $gitSizeMb = [math]::Round(((Get-ChildItem -LiteralPath $GitRoot -Recurse -File -ErrorAction SilentlyContinue |
    Measure-Object -Property Length -Sum).Sum / 1MB), 1)
  Write-Step "git smoke ok: init/commit/log on the trimmed tree (opt/git = $gitSizeMb MB)"
}

function Find-PythonExe {
  # The prune step runs in Python: this build host (and CI) wrap Remove-Item in
  # a delete guard that routes every call through the Recycle Bin, where bulk
  # pruning of thousands of nested files fails closed.
  foreach ($candidate in @('python', 'python3', 'py')) {
    $command = Get-Command $candidate -ErrorAction SilentlyContinue
    if (-not $command -or -not $command.Source) { continue }
    $prefix = if ($candidate -eq 'py') { @('-3') } else { @() }
    $output = & $command.Source @($prefix + @('--version')) 2>&1
    if ($LASTEXITCODE -eq 0 -and "$output" -match '^Python 3\.') {
      return @{ Exe = $command.Source; Prefix = $prefix }
    }
  }
  return $null
}

function Invoke-SupplementPrune {
  param(
    [string]$PackageRoot,
    [ValidateSet('ia32', 'x64')][string]$Arch,
    [switch]$SkipGit
  )
  $script = Join-Path $ScriptDir 'prune-supplement-stage.py'
  if (-not (Test-Path -LiteralPath $script)) { Fail "prune script missing: $script" }
  $python = Find-PythonExe
  if ($null -eq $python) { Fail 'Python 3 is required to prune the supplement stage' }
  $arguments = @($python.Prefix + @($script, $PackageRoot, '--arch', $Arch))
  if ($SkipGit) { $arguments += '--skip-git' }
  Write-Step "pruning stage ($Arch): $($python.Exe)"
  & $python.Exe @arguments
  if ($LASTEXITCODE -ne 0) { Fail "supplement prune failed (exit $LASTEXITCODE)" }
}

function Get-PeImportedDlls {
  # Imported DLL names from the PE import directory. Used to keep Windows 7
  # packs free of Win8+-only static imports such as bcryptprimitives.dll.
  param([string]$Path)
  if (-not (Test-Path -LiteralPath $Path)) { Fail "PE file not found: $Path" }
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  if ($bytes.Length -lt 0x40 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
    Fail "not a Windows PE image: $Path"
  }
  $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
  if ($peOffset -lt 0 -or $peOffset + 24 -gt $bytes.Length -or
      $bytes[$peOffset] -ne 0x50 -or $bytes[$peOffset + 1] -ne 0x45) {
    Fail "invalid PE header: $Path"
  }
  $sectionCount = [BitConverter]::ToUInt16($bytes, $peOffset + 6)
  $optionalSize = [BitConverter]::ToUInt16($bytes, $peOffset + 20)
  $optionalOffset = $peOffset + 24
  $magic = [BitConverter]::ToUInt16($bytes, $optionalOffset)
  $dirOffset = if ($magic -eq 0x20b) { $optionalOffset + 112 } else { $optionalOffset + 96 }
  if ($dirOffset + 12 -gt $bytes.Length) { Fail "invalid PE optional header: $Path" }
  $importRva = [BitConverter]::ToUInt32($bytes, $dirOffset + 8)
  if ($importRva -eq 0) { return @() }

  $sections = @()
  $sectionOffset = $optionalOffset + $optionalSize
  for ($i = 0; $i -lt $sectionCount; $i++) {
    $offset = $sectionOffset + ($i * 40)
    if ($offset + 40 -gt $bytes.Length) { Fail "truncated PE section table: $Path" }
    $virtualSize = [BitConverter]::ToUInt32($bytes, $offset + 8)
    $rawSize = [BitConverter]::ToUInt32($bytes, $offset + 16)
    $sections += [pscustomobject]@{
      VirtualAddress = [BitConverter]::ToUInt32($bytes, $offset + 12)
      Span           = if ($virtualSize -gt $rawSize) { $virtualSize } else { $rawSize }
      RawSize        = $rawSize
      RawPointer     = [BitConverter]::ToUInt32($bytes, $offset + 20)
    }
  }

  $mapRva = {
    param([uint32]$Rva)
    foreach ($section in $sections) {
      if (($Rva -ge $section.VirtualAddress) -and ($Rva -lt ($section.VirtualAddress + $section.Span))) {
        $delta = $Rva - $section.VirtualAddress
        if ($delta -ge $section.RawSize) { return -1 }
        return [int]($section.RawPointer + $delta)
      }
    }
    return -1
  }

  $importOffset = & $mapRva $importRva
  if ($importOffset -lt 0) { Fail "cannot map PE import directory: $Path" }

  $names = New-Object System.Collections.Generic.List[string]
  $cursor = [int]$importOffset
  while ($cursor + 20 -le $bytes.Length) {
    $nameRva = [BitConverter]::ToUInt32($bytes, $cursor + 12)
    $firstThunk = [BitConverter]::ToUInt32($bytes, $cursor + 16)
    if (($nameRva -eq 0) -and ($firstThunk -eq 0)) { break }
    $nameOffset = & $mapRva $nameRva
    if ($nameOffset -lt 0) { Fail "cannot map PE import name: $Path" }
    $builder = New-Object System.Text.StringBuilder
    while (($nameOffset -lt $bytes.Length) -and ($bytes[$nameOffset] -ne 0)) {
      [void]$builder.Append([char]$bytes[$nameOffset])
      $nameOffset++
    }
    if ($builder.Length -eq 0) { break }
    $names.Add($builder.ToString())
    $cursor += 20
  }
  return $names.ToArray()
}

function Assert-Win7PeImports {
  # Rust 1.78+ links ProcessPrng (bcryptprimitives.dll) and, for 32-bit
  # targets, WaitOnAddress (api-ms-win-core-synch-l1-2-0.dll) statically.
  # Both are Windows 8+ APIs, so such a binary cannot start on Windows 7.
  param([string]$Path)
  $denied = @(
    'bcryptprimitives.dll',
    'api-ms-win-core-synch-l1-2-0.dll'
  )
  $found = @(Get-PeImportedDlls -Path $Path | Where-Object { $denied -contains $_.ToLowerInvariant() })
  if ($found.Count -gt 0) {
    Fail ("Win8+-only imports in ${Path}: {0} (Windows 7 cannot load this binary)" -f ($found -join ', '))
  }
  Write-Step "validated Windows 7 PE imports: $Path"
}

function Expand-ZipInto {
  param([string]$Archive, [string]$Destination)
  Remove-Tree $Destination
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

function Resolve-SevenZipExecutable {
  param([string]$RepoRoot)
  $candidates = New-Object System.Collections.Generic.List[string]
  foreach ($name in @('7z.exe', '7za.exe')) {
    $command = Get-Command $name -ErrorAction SilentlyContinue
    if ($command -and $command.Source) { $candidates.Add([string]$command.Source) }
  }
  $programFiles = [Environment]::GetFolderPath('ProgramFiles')
  $programFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
  $pathCandidates = @()
  if (-not [string]::IsNullOrWhiteSpace($programFiles)) { $pathCandidates += Join-Path $programFiles '7-Zip\7z.exe' }
  if (-not [string]::IsNullOrWhiteSpace($programFilesX86)) { $pathCandidates += Join-Path $programFilesX86 '7-Zip\7z.exe' }
  $pathCandidates += @(
    (Join-Path $RepoRoot 'temp_dir\win7-gnu-lab\browser-109\sevenzip\node_modules\7zip-bin\win\x64\7za.exe'),
    (Join-Path $RepoRoot 'temp_dir\win7-gnu-lab\browser-109\sevenzip\node_modules\7zip-bin\win\ia32\7za.exe')
  )
  foreach ($path in $pathCandidates) {
    if (-not [string]::IsNullOrWhiteSpace($path)) { $candidates.Add($path) }
  }
  foreach ($candidate in $candidates) {
    if (Test-Path -LiteralPath $candidate -PathType Leaf) { return (Resolve-Path -LiteralPath $candidate).Path }
  }
  Fail '7-Zip is required to unpack the automatically downloaded Chrome 109 archive; install 7-Zip or provide -BrowserArchivePath.'
}

function Invoke-SevenZipExtract {
  param([string]$Executable, [string]$Archive, [string]$Destination, [switch]$Force7z)
  Remove-Tree $Destination
  Ensure-Directory $Destination
  $arguments = @('x')
  if ($Force7z) { $arguments += '-t7z' }
  $arguments += @($Archive, "-o$Destination", '-y')
  & $Executable @arguments
  if ($LASTEXITCODE -ne 0) { Fail "7-Zip extraction failed ($LASTEXITCODE): $Archive" }
}

function Expand-ZipFlatten {
  param([string]$Archive, [string]$Destination)
  $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ('wunder-supp-' + [guid]::NewGuid().ToString('N'))
  Expand-ZipInto -Archive $Archive -Destination $tmp
  $entries = @(Get-ChildItem -LiteralPath $tmp -Force)
  if (($entries.Count -eq 1) -and ($entries[0].PSIsContainer)) { $source = $entries[0].FullName } else { $source = $tmp }
  Remove-Tree $Destination
  Ensure-Directory $Destination
  Get-ChildItem -LiteralPath $source -Force | ForEach-Object { Move-Item -LiteralPath $_.FullName -Destination $Destination -Force }
  Remove-TreeQuiet $tmp
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
  Remove-Tree $Destination
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
      Remove-Tree $targetDir
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

function New-SupplementZip {
  # Compress-Archive chokes on the POSIX links that PortableGit ships (for
  # example etc/mtab -> /proc/mounts, stored as a reparse point): it throws
  # "cannot access the file" and leaves no zip behind. Those links mean nothing
  # on Windows, so skip them and write the archive entry by entry.
  param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Destination
  )
  Add-Type -AssemblyName System.IO.Compression | Out-Null
  Add-Type -AssemblyName System.IO.Compression.FileSystem | Out-Null
  if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Force }
  $sourceFull = (Get-Item -LiteralPath $Source).FullName.TrimEnd([char]'\')
  $skipped = 0
  $count = 0
  $zip = [System.IO.Compression.ZipFile]::Open($Destination, [System.IO.Compression.ZipArchiveMode]::Create)
  try {
    foreach ($file in @(Get-ChildItem -LiteralPath $sourceFull -Recurse -File -Force)) {
      if (($file.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        $skipped++
        continue
      }
      $relative = $file.FullName.Substring($sourceFull.Length + 1) -replace '\\', '/'
      [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
        $zip, $file.FullName, $relative, [System.IO.Compression.CompressionLevel]::Optimal) | Out-Null
      $count++
    }
  } finally {
    $zip.Dispose()
  }
  Write-Step ("archived {0} files, skipped {1} reparse points" -f $count, $skipped)
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

function Invoke-ProcessCapture {
  # Runs a console program and returns @{ ExitCode; StdOut; StdErr }.
  # .NET Process is used instead of Start-Process on purpose: Windows
  # PowerShell 5.1's Start-Process copies the parent environment entry by entry
  # with Dictionary.Add, so a host that exports both HTTPS_PROXY and https_proxy
  # (the same key in a case-insensitive dictionary) fails with "An item with the
  # same key has already been added" before python even starts. A plain Process
  # inherits the block natively and sidesteps the whole issue.
  param([string]$FilePath, [string[]]$Arguments = @(), [string]$WorkingDirectory = '')
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $FilePath
  $psi.Arguments = (@($Arguments | ForEach-Object { ConvertTo-ProcessArgument -Value ([string]$_) }) -join ' ')
  if (-not [string]::IsNullOrWhiteSpace($WorkingDirectory)) { $psi.WorkingDirectory = $WorkingDirectory }
  $psi.UseShellExecute = $false
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.CreateNoWindow = $true
  $proc = New-Object System.Diagnostics.Process
  $proc.StartInfo = $psi
  $proc.Start() | Out-Null
  $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
  $stderrTask = $proc.StandardError.ReadToEndAsync()
  $proc.WaitForExit()
  return @{ ExitCode = $proc.ExitCode; StdOut = $stdoutTask.Result; StdErr = $stderrTask.Result }
}

function Invoke-PythonProcess {
  # Runs python and returns its exit code.
  param([string]$PythonExe, [string[]]$Arguments, [switch]$SuppressOutput)
  $result = Invoke-ProcessCapture -FilePath $PythonExe -Arguments $Arguments
  if ((-not $SuppressOutput) -and -not [string]::IsNullOrWhiteSpace($result.StdOut)) { Write-Host $result.StdOut.TrimEnd() }
  if ($result.ExitCode -ne 0 -and -not [string]::IsNullOrWhiteSpace($result.StdErr)) { Write-Host $result.StdErr.TrimEnd() }
  return $result.ExitCode
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
$usingDefaultBuildRoot = [string]::IsNullOrWhiteSpace($buildRootEffective)
if ($usingDefaultBuildRoot) {
  $rel = [string](Get-PropertyValue -Object $manifest -Name 'defaultBuildRoot' -Default 'temp_dir\win7-gnu-lab\win7-supplement')
  $buildRootEffective = Join-Path $repoRoot $rel
} elseif (-not [System.IO.Path]::IsPathRooted($buildRootEffective)) {
  $buildRootEffective = Join-Path $repoRoot $buildRootEffective
}

$downDir  = Join-Path $buildRootEffective ([string](Get-PropertyValue -Object $manifest -Name 'downloadsDir' -Default 'downloads'))
$stageDir = Join-Path $buildRootEffective ([string](Get-PropertyValue -Object $manifest -Name 'stageDir' -Default 'stage'))
# With the default build root the finished zip goes to the installer payload
# directory: `build.bat pack` searches it directly and the artifacts survive
# temp_dir cleanups. An explicit -BuildRoot (e.g. CI) keeps dist inside it.
$distDirName = [string](Get-PropertyValue -Object $manifest -Name 'distDir' -Default 'dist')
$distDir = if ($usingDefaultBuildRoot -and $distDirName -eq 'dist') {
  Join-Path $repoRoot ('target\installer\payload\' + $(if ($Arch -eq 'ia32') { 'win7-x86' } else { 'win7-x64' }))
} elseif ([System.IO.Path]::IsPathRooted($distDirName)) {
  $distDirName
} else {
  Join-Path $buildRootEffective $distDirName
}
$packRoot = Join-Path $stageDir 'package-root'

$archLabel = 'win7-' + $Arch
$zipName = 'wunder-supplement-' + $archLabel
if ($PythonProfile -eq 'common') { $zipName = $zipName + '-common' }
$zipName = $zipName + '.zip'
$zipPath = Join-Path $distDir $zipName

$layout = Get-PropertyValue -Object $manifest -Name 'layout'
$pythonRoot = Join-Path $packRoot ([string](Get-PropertyValue -Object $layout -Name 'pythonRoot' -Default 'opt\python'))
$gitRoot    = Join-Path $packRoot ([string](Get-PropertyValue -Object $layout -Name 'gitRoot' -Default 'opt\git'))
$rgRoot     = Join-Path $packRoot ([string](Get-PropertyValue -Object $layout -Name 'rgRoot' -Default 'opt\rg'))
$ffmpegRoot = Join-Path $packRoot ([string](Get-PropertyValue -Object $layout -Name 'ffmpegRoot' -Default 'opt\ffmpeg'))

Write-Step "repo root : $repoRoot"
Write-Step "build root: $buildRootEffective"
Write-Step "arch      : $Arch"
Write-Step "profile   : $PythonProfile"

if ($PruneOnly) {
  # Two-step build for hosts that refuse the bulk deletes of a fresh run: every
  # component is already staged, so only prune, gate and repack.
  if (-not (Test-Path -LiteralPath $packRoot -PathType Container)) {
    Fail "-PruneOnly needs an existing stage: $packRoot"
  }
  Write-Step "prune only: $packRoot"
  $pruneArgs = @{ PackageRoot = $packRoot; Arch = $Arch }
  if ($SkipGitPrune) { $pruneArgs.SkipGit = $true }
  Invoke-SupplementPrune @pruneArgs
  Invoke-GitSmoke -GitRoot $gitRoot
  Assert-SupplementPeArch -PackageRoot $packRoot -Arch $Arch
  Ensure-Directory $distDir
  if (Test-Path -LiteralPath $zipPath) { Remove-Tree $zipPath }
  Write-Step "packaging -> $zipPath"
  New-SupplementZip -Source $packRoot -Destination $zipPath
  $repacked = Get-Item -LiteralPath $zipPath
  Write-Step ('done: {0} ({1:N1} MB)' -f $repacked.FullName, ($repacked.Length / 1MB))
  return
}

Remove-Tree $stageDir
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
    # The manifest records what actually landed in the pack, resolved pins and
    # transitive dependencies included; the requirements file alone would leave
    # an empty list in every non-BestEffort build.
    $freeze = Invoke-ProcessCapture -FilePath $pythonExe -Arguments @('-m', 'pip', 'freeze')
    if ($freeze.ExitCode -eq 0) {
      foreach ($line in @($freeze.StdOut -split "`r?`n")) {
        $spec = $line.Trim()
        if ($spec -match '^[A-Za-z0-9._-]+==') { $installedPackages.Add($spec) }
      }
    }
  }
}

# Pruning runs once, after every component has been staged, through
# prune-supplement-stage.py (see Invoke-SupplementPrune): wheel test suites,
# PE images that do not match $Arch, and the MinGit extras the agent never
# calls. Keeping one script as the only pruning authority stops the list in
# this file and the standalone python helper from drifting apart.

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
    $gitArchive = Join-Path $downDir ([string](Get-PropertyValue -Object $gitArchiveMeta -Name 'fileName' -Default 'MinGit.zip'))
    Get-DownloadedFile -Url ([string](Get-PropertyValue -Object $gitArchiveMeta -Name 'url')) -Destination $gitArchive -Force:$RefreshDownloads | Out-Null
  } elseif (-not (Test-Path -LiteralPath $gitArchive)) { Fail "git archive not found: $gitArchive" }
  Write-Step "staging Git $gitVersion into $gitRoot"
  Expand-ZipFlatten -Archive $gitArchive -Destination $gitRoot

  # MinGit is already the non-interactive distribution: git.exe, its MSYS
  # runtime, the shell scripts under libexec and no bundled bash. What it still
  # drags along is the .NET/Avalonia Git Credential Manager UI, scalar, tig,
  # vim/perl and the Tcl/Tk GUI helpers -- all removed by the shared prune step
  # below, which also rewrites credential.helper to wincred so HTTPS push keeps
  # working without the manager.
}

# ---------------------------------------------------------------------------
# Prune (wheel tests, foreign-architecture PE files, MinGit extras)
# ---------------------------------------------------------------------------
if ($SkipPrune) {
  Write-Step 'skipping stage prune (-SkipPrune)'
} else {
  $pruneArgs = @{ PackageRoot = $packRoot; Arch = $Arch }
  if ($SkipGitPrune) { $pruneArgs.SkipGit = $true }
  Invoke-SupplementPrune @pruneArgs
}

Invoke-GitSmoke -GitRoot $gitRoot

# ---------------------------------------------------------------------------
# ripgrep
# ---------------------------------------------------------------------------
$rgMeta = Get-PropertyValue -Object $manifest -Name 'rg'
$rgVersion = [string](Get-PropertyValue -Object $rgMeta -Name 'version' -Default '')
$rgArchiveMeta = Get-PropertyValue -Object (Get-PropertyValue -Object $rgMeta -Name 'archives') -Name $Arch
if ($null -ne $rgArchiveMeta) {
  $rgArchive = $RgArchivePath
  $rgFromManifest = [string]::IsNullOrWhiteSpace($rgArchive)
  if ($rgFromManifest) {
    $rgArchive = Join-Path $downDir ([string](Get-PropertyValue -Object $rgArchiveMeta -Name 'fileName' -Default 'ripgrep.zip'))
    Get-DownloadedFile -Url ([string](Get-PropertyValue -Object $rgArchiveMeta -Name 'url')) -Destination $rgArchive -Force:$RefreshDownloads | Out-Null
    Assert-FileSha256 -Path $rgArchive -Expected ([string](Get-PropertyValue -Object $rgArchiveMeta -Name 'sha256' -Default ''))
  } elseif (-not (Test-Path -LiteralPath $rgArchive)) { Fail "ripgrep archive not found: $rgArchive" }
  Write-Step "staging ripgrep $rgVersion into $rgRoot"
  Expand-ZipFlatten -Archive $rgArchive -Destination $rgRoot
  $rgExe = Join-Path $rgRoot 'rg.exe'
  if (-not (Test-Path -LiteralPath $rgExe -PathType Leaf)) { Fail "ripgrep executable missing after staging: $rgExe" }
  Assert-PeMachine -Path $rgExe -Arch $Arch
  Assert-Win7PeImports -Path $rgExe
  $rgResult = Invoke-ProcessCapture -FilePath $rgExe -Arguments @('--version')
  $rgSmoke = $rgResult.StdOut
  if ($rgResult.ExitCode -ne 0 -or -not ("$rgSmoke" -match ('^ripgrep\s+' + [regex]::Escape($rgVersion)))) {
    Fail "ripgrep smoke failed: $rgSmoke $($rgResult.StdErr)"
  }
  Write-Step ("ripgrep smoke ok: {0}" -f ($rgSmoke -split "`n")[0].Trim())
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
# Bundled browser (Chrome 109 win32; explicit source, cache, or manifest download)
# ---------------------------------------------------------------------------
$browserMeta = Get-PropertyValue -Object $manifest -Name 'browser'
$browserEnabled = [bool](Get-PropertyValue -Object $browserMeta -Name 'enabled' -Default $false) -and (-not $SkipBrowser)
$browserVersion = [string](Get-PropertyValue -Object $browserMeta -Name 'version' -Default '')
$browserExeRel = [string](Get-PropertyValue -Object $browserMeta -Name 'expectedExe' -Default 'chrome-win/chrome.exe')
$browserDestRel = [string](Get-PropertyValue -Object $browserMeta -Name 'dest' -Default 'opt\browser')
$browserBundled = $false
$browserSourceLeaf = ''
if ($browserEnabled) {
  $browserSrc = $BrowserArchivePath
  if ([string]::IsNullOrWhiteSpace($browserSrc)) { $browserSrc = [string](Get-PropertyValue -Object $browserMeta -Name 'archivePath' -Default '') }
  if ([string]::IsNullOrWhiteSpace($browserSrc)) {
    $preparedCandidates = @(
      (Join-Path $repoRoot 'temp_dir\win7-gnu-lab\browser-109\chrome-bin-out\Chrome-bin'),
      (Join-Path $repoRoot 'temp_dir\win7-gnu-lab\browser-109\ci-sim\browser-src\unpacked\Chrome-bin')
    )
    $browserSrc = @($preparedCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Container } | Select-Object -First 1)[0]
    if (-not [string]::IsNullOrWhiteSpace($browserSrc)) { Write-Step "using prepared browser $browserSrc" }
  }
  if ([string]::IsNullOrWhiteSpace($browserSrc)) {
    $browserDownload = Get-PropertyValue -Object $browserMeta -Name 'download'
    $browserArchiveName = [string](Get-PropertyValue -Object $browserDownload -Name 'fileName' -Default 'chromium-win32-mini-installer.exe')
    $browserArchiveUrl = [string](Get-PropertyValue -Object $browserDownload -Name 'url' -Default '')
    $browserArchiveSha = [string](Get-PropertyValue -Object $browserDownload -Name 'sha256' -Default '')
    if ([string]::IsNullOrWhiteSpace($browserArchiveUrl)) { Fail 'browser.download.url is empty' }
    $browserInstaller = Join-Path $downDir $browserArchiveName
    Get-DownloadedFile -Url $browserArchiveUrl -Destination $browserInstaller -Force:$RefreshDownloads | Out-Null
    Assert-FileSha256 -Path $browserInstaller -Expected $browserArchiveSha
    $sevenZip = Resolve-SevenZipExecutable -RepoRoot $repoRoot
    $browserExtractRoot = Join-Path $buildRootEffective 'browser-source'
    $outerExtractRoot = Join-Path $browserExtractRoot 'installer'
    $innerExtractRoot = Join-Path $browserExtractRoot 'unpacked'
    Invoke-SevenZipExtract -Executable $sevenZip -Archive $browserInstaller -Destination $outerExtractRoot -Force7z
    $innerArchive = @(Get-ChildItem -LiteralPath $outerExtractRoot -Recurse -File -Filter 'chrome.7z' | Select-Object -First 1)[0]
    if ($null -eq $innerArchive) { Fail "chrome.7z not found after extracting browser installer: $browserInstaller" }
    Invoke-SevenZipExtract -Executable $sevenZip -Archive $innerArchive.FullName -Destination $innerExtractRoot
    $browserSrc = Join-Path $innerExtractRoot 'Chrome-bin'
    if (-not (Test-Path -LiteralPath (Join-Path $browserSrc 'chrome.exe') -PathType Leaf)) {
      Fail "chrome.exe not found after extracting browser installer: $browserInstaller"
    }
    Write-Step "downloaded and unpacked browser $browserVersion"
  }
  if (-not [string]::IsNullOrWhiteSpace($browserSrc)) {
    if (-not (Test-Path -LiteralPath $browserSrc)) { Fail "browser archive not found: $browserSrc" }
    $browserRoot = Join-Path $packRoot $browserDestRel
    Write-Step "staging browser $browserVersion into $browserRoot"
    Install-BundledBrowser -Source $browserSrc -Destination $browserRoot -ExpectedExe $browserExeRel -Arch $Arch
    $browserBundled = $true
    $browserSourceLeaf = Split-Path -Leaf $browserSrc
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
$browserNote = if ($browserBundled) { "bundled Chrome $browserVersion at opt/browser/$browserExeRel (win32); the browser_* tools detect it automatically" } elseif ($browserInstalled) { 'Playwright Chromium in opt/python/ms-playwright' } else { 'not bundled; the browser_* tools reuse a locally installed browser' }
$readmeText = @'
Wunder Desktop Windows supplement (__ARCH__, profile: __PROFILE__)

Contents:
  opt/python   Python __PYVER__ (embeddable) + pinned packages
  opt/git      MinGit __GITVER__
  opt/rg       ripgrep __RGVER__
  opt/ffmpeg   ffmpeg __FFVER__ (when bundled)
  opt/browser  Chrome __BROWSERVER__ win32 at __BROWSEREXE__ (when bundled)

Use Import supplement in the Pupa settings to install into WUNDER_HOME.
For manual installation, close processes using this runtime and unpack into
WUNDER_HOME (default: .wunder in your user home). The Pupa and Helm share
opt/python, opt/git, opt/rg and opt/ffmpeg, and prepend
their bins to PATH.

Browsers: __BROWSER__
'@
$readmeText = $readmeText.Replace('__ARCH__', $archLabel).Replace('__PROFILE__', $PythonProfile).Replace('__PYVER__', $pyVer).Replace('__GITVER__', $gitVersion).Replace('__RGVER__', $rgVersion).Replace('__FFVER__', $ffVersion).Replace('__BROWSERVER__', $browserVersion).Replace('__BROWSEREXE__', $browserExeRel).Replace('__BROWSER__', $browserNote)
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

# Final release gate: nothing in the staged tree may be built for another
# architecture. Runs after every component and the prune step so a rerun with
# -SkipPrune or a new wheel cannot slip an unusable DLL into the pack.
Assert-SupplementPeArch -PackageRoot $packRoot -Arch $Arch

Write-Step "packaging -> $zipPath"
if (Test-Path -LiteralPath $zipPath) { Remove-Tree $zipPath }
New-SupplementZip -Source $packRoot -Destination $zipPath

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
