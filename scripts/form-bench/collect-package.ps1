<#
  Collect "package size" for one form (dir or single file).

  Usage:
    powershell -ExecutionPolicy Bypass -File scripts\form-bench\collect-package.ps1 ^
      -FormId hull -Target target\release -Kind dir ^
      -OutJson target\form-bench\raw\hull-package.json
#>
param(
  [Parameter(Mandatory = $true)][string]$FormId,
  [Parameter(Mandatory = $true)][string]$Target,
  [ValidateSet("dir", "file")][string]$Kind = "dir",
  [string]$RepoRoot = "",
  [string]$OutJson = ""
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

if (-not $RepoRoot) { $RepoRoot = Get-RepoRoot -ScriptRoot $PSScriptRoot }
$resolved = Resolve-RepoPath -Path $Target -RepoRoot $RepoRoot

if (-not (Test-Path -LiteralPath $resolved)) {
  Write-BenchOutput -FormId $FormId -Collector "package" -OutJson $OutJson -Records @(
    (New-BenchRecord -Metric "package_size" -Value $null -Unit "MB" -Source $resolved `
      -Details @{ error = "target not found; build the form first"; path = $resolved })
  )
  return
}

$item = Get-Item -LiteralPath $resolved
if ($item.PSIsContainer) {
  $files = @(Get-ChildItem -LiteralPath $resolved -Recurse -File -ErrorAction SilentlyContinue)
  $total = 0L
  $byExt = @{}
  foreach ($f in $files) {
    $total += [int64]$f.Length
    $ext = $f.Extension
    if (-not $ext) { $ext = "(none)" }
    if ($byExt.ContainsKey($ext)) { $byExt[$ext] = [int64]$byExt[$ext] + [int64]$f.Length }
    else { $byExt[$ext] = [int64]$f.Length }
  }
  $top = @($files | Sort-Object -Property Length -Descending | Select-Object -First 10 | ForEach-Object {
    [ordered]@{ name = $_.Name; mb = (Format-Number ($_.Length / 1MB) 3) }
  })
  $details = [ordered]@{
    type               = "dir"
    path               = $resolved
    file_count         = $files.Count
    bytes              = $total
    by_extension_bytes = $byExt
    top_files          = $top
  }
  $record = New-BenchRecord -Metric "package_size" -Value (Format-Number ($total / 1MB) 2) `
    -Unit "MB" -Source $resolved -Details $details
} else {
  $details = [ordered]@{ type = "file"; path = $resolved; bytes = [int64]$item.Length }
  $record = New-BenchRecord -Metric "package_size" -Value (Format-Number ($item.Length / 1MB) 3) `
    -Unit "MB" -Source $resolved -Details $details
}

Write-BenchOutput -FormId $FormId -Collector "package" -OutJson $OutJson -Records @($record)