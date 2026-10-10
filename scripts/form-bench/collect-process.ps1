<#
  Collect memory + CPU for a named native process over a sampling window.

  CPU is reported as percent-of-total-machine:
      cpu_pct = cpu_time_ms / (wall_ms * logical_cores) * 100

  Usage:
    powershell -ExecutionPolicy Bypass -File scripts\form-bench\collect-process.ps1 ^
      -FormId hull -ProcessName wunder-server -DurationSec 30 -IntervalMs 500 ^
      -OutJson target\form-bench\raw\hull-process.json
#>
param(
  [Parameter(Mandatory = $true)][string]$FormId,
  [Parameter(Mandatory = $true)][string]$ProcessName,
  [string]$RepoRoot = "",
  [string]$OutJson = "",
  [int]$DurationSec = 30,
  [int]$IntervalMs = 500
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

$cores = [Environment]::ProcessorCount
if ($cores -lt 1) { $cores = 1 }

function Get-MatchProcess {
  param([string]$Name)
  return @(Get-Process -Name $Name -ErrorAction SilentlyContinue)
}

function Get-CpuMs {
  param($Procs)
  $total = 0.0
  foreach ($p in $Procs) {
    try { $total += [double]$p.TotalProcessorTime.TotalMilliseconds } catch { }
  }
  return $total
}

$procs = Get-MatchProcess $ProcessName
if ($procs.Count -eq 0) {
  Write-BenchOutput -FormId $FormId -Collector "process" -OutJson $OutJson -Records @(
    (New-BenchRecord -Metric "memory" -Value $null -Unit "MB" -Source $ProcessName `
      -Details @{ error = "process not running" }),
    (New-BenchRecord -Metric "cpu" -Value $null -Unit "%" -Source $ProcessName `
      -Details @{ error = "process not running" })
  )
  return
}

$t0 = Get-Date
$cpu0 = Get-CpuMs $procs
$memSamples = New-Object System.Collections.ArrayList
$peak = 0L
$deadline = $t0.AddSeconds($DurationSec)
while ((Get-Date) -lt $deadline) {
  $cur = Get-MatchProcess $ProcessName
  $sum = 0L
  foreach ($p in $cur) {
    try { $sum += [int64]$p.WorkingSet64 } catch { }
  }
  if ($sum -gt 0) {
    [void]$memSamples.Add($sum)
    if ($sum -gt $peak) { $peak = $sum }
  }
  Start-Sleep -Milliseconds $IntervalMs
}
$t1 = Get-Date
$cur = Get-MatchProcess $ProcessName
$cpu1 = Get-CpuMs $cur

$wallMs = ($t1 - $t0).TotalMilliseconds
$cpuMs = [math]::Max(0.0, ($cpu1 - $cpu0))
$cpuPct = 0.0
if ($wallMs -gt 0) { $cpuPct = $cpuMs / ($wallMs * $cores) * 100.0 }

$avgMem = 0.0
$endMem = 0L
if ($memSamples.Count -gt 0) {
  $avgMem = ($memSamples | Measure-Object -Average).Average
  $endMem = [int64]$memSamples[$memSamples.Count - 1]
}

$memRecord = New-BenchRecord -Metric "memory" -Value (Format-Number ($avgMem / 1MB) 2) `
  -Unit "MB" -Source $ProcessName -Details ([ordered]@{
    avg_mb        = (Format-Number ($avgMem / 1MB) 2)
    peak_mb       = (Format-Number ($peak / 1MB) 2)
    end_mb        = (Format-Number ($endMem / 1MB) 2)
    samples       = $memSamples.Count
    process_count = $cur.Count
    duration_sec  = $DurationSec
  })

$cpuRecord = New-BenchRecord -Metric "cpu" -Value (Format-Number $cpuPct 2) `
  -Unit "%" -Source $ProcessName -Details ([ordered]@{
    percent_of_total = (Format-Number $cpuPct 2)
    logical_cores    = $cores
    cpu_ms           = (Format-Number $cpuMs 0)
    wall_ms          = (Format-Number $wallMs 0)
  })

Write-BenchOutput -FormId $FormId -Collector "process" -OutJson $OutJson -Records @($memRecord, $cpuRecord)