param(
  [Parameter(Position = 0)][ValidateSet("pack")][string]$Action = "pack",
  [Alias("a")][ValidateSet("win7-x86", "linux-amd64", "linux-arm64")][string]$Arch = "",
  [string]$Desktop = "",
  [string]$Cli = "",
  [string]$Supplement = "",
  [string]$OutputDir = "",
  [switch]$CoreOnly,
  [switch]$NoDownload,
  [string]$Iscc = "",
  [ValidateSet("lzma2/fast", "lzma2/normal", "lzma2/max", "lzma2/ultra64", "none")]
  [string]$Compression = "lzma2/normal"
)

# Public pack dispatcher behind `build.bat pack`. It never rebuilds release
# artifacts: it validates and combines what the build matrix already produced.
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$win7BuilderRoot = if ($env:WUNDER_WIN7_BUILDER_ROOT) {
  $env:WUNDER_WIN7_BUILDER_ROOT
} elseif ($env:WUNDER_BUILDER_ROOT) {
  $env:WUNDER_BUILDER_ROOT
} else {
  Join-Path (Split-Path $repoRoot -Parent) "Rust-builder\win7"
}
$offlineRoot = Join-Path $win7BuilderRoot "offline"

function Resolve-RepoPath([string]$Path) {
  if (-not $Path) { return "" }
  if ([IO.Path]::IsPathRooted($Path)) { return [IO.Path]::GetFullPath($Path) }
  return [IO.Path]::GetFullPath((Join-Path $repoRoot $Path))
}

function Find-Python {
  $candidates = @()
  $offlinePython = Join-Path $offlineRoot "tools\python\python.exe"
  if (Test-Path -LiteralPath $offlinePython -PathType Leaf) {
    $candidates += @{ Exe = $offlinePython; Prefix = @() }
  }
  $candidates += @(
    @{ Exe = "python"; Prefix = @() },
    @{ Exe = "python3"; Prefix = @() },
    @{ Exe = "py"; Prefix = @("-3") }
  )
  foreach ($candidate in $candidates) {
    $command = Get-Command $candidate.Exe -ErrorAction SilentlyContinue
    if (-not $command -or -not $command.Source) { continue }
    $arguments = @($candidate.Prefix + @("--version"))
    $output = & $command.Source @arguments 2>&1
    if ($LASTEXITCODE -eq 0 -and "$output" -match '^Python 3\.') {
      return @{ Exe = $command.Source; Prefix = $candidate.Prefix }
    }
  }
  throw "Python 3 is required to assemble the Linux installer. Install Python 3 and retry."
}

if (-not $Arch) { $Arch = "win7-x86" }

if ($Arch -eq "win7-x86") {
  $script = Join-Path $repoRoot "packaging\windows\scripts\build_win7_installer.ps1"
  try {
    & $script `
      -DesktopExe $Desktop `
      -CliExe $Cli `
      -Supplement $Supplement `
      -OutputDir $OutputDir `
      -CoreOnly:$CoreOnly `
      -NoDownload:$NoDownload `
      -Iscc $Iscc `
      -Compression $Compression
  } catch {
    throw
  }
  if (-not $?) { throw "Windows installer pack failed" }
  exit 0
}

$python = Find-Python
Write-Host "[wunder-pack] Python: $($python.Exe)"
$scriptPath = Join-Path $repoRoot "packaging\linux\scripts\build_linux_installer.py"
$pythonArgs = @($python.Prefix) + @($scriptPath, "--arch", $Arch.Substring(6))
if ($Desktop) { $pythonArgs += @("--desktop", (Resolve-RepoPath $Desktop)) }
if ($Cli) { $pythonArgs += @("--cli", (Resolve-RepoPath $Cli)) }
if ($Supplement) { $pythonArgs += @("--supplement", (Resolve-RepoPath $Supplement)) }
if ($OutputDir) { $pythonArgs += @("--output-dir", (Resolve-RepoPath $OutputDir)) }
if ($CoreOnly) { $pythonArgs += "--core-only" }
& $python.Exe @pythonArgs
if ($LASTEXITCODE -ne 0) { throw "Linux installer assembly failed with exit code $LASTEXITCODE" }
Write-Host "[wunder-pack] on Windows the .run file cannot carry a Unix execute bit; run chmod +x after copying it to Linux"
exit 0
