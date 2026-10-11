# measure-idle.ps1 - Idle tunnel budget sampler (section 13.6 of the cloud-local interlink design).
#
# The loopback harness runs two full engines in one process (server router plus
# local form), and each engine has its own housekeeping timers. Measuring the
# whole process would blame the tunnel for that traffic, so the ignored test
# runs in two phases and this script measures both:
#
#   phase 1  baseline - the same process, tunnel down
#   phase 2  tunnel   - one live tunnel, no commands
#
# The budget applies to the increment: tunnel cpu minus baseline cpu must stay
# under -MaxCpuPercent of one core, and the tunnel phase must not grow the
# working set by more than -MaxRssDeltaMb.
#
# Usage:
#   powershell -File scripts/interlink-bench/measure-idle.ps1 -IdleSeconds 60
#   powershell -File scripts/interlink-bench/measure-idle.ps1 -IdleSeconds 120 -Release
# Optional: -OutCsv path -OutJson path -IntervalMs 500 -CompileBudgetSec 900
#
# -IdleSeconds is the length of *each* phase. Out paths must be Windows-style:
# a shell that maps $TEMP to /tmp would otherwise write next to the current
# drive. Pass -Release for the number that goes into docs: the budget is a
# product budget, and a debug build costs several times more per tick.

param(
  [int]$IdleSeconds = 60,
  [int]$IntervalMs = 500,
  [int]$CompileBudgetSec = 900,
  [double]$MaxCpuPercent = 1.0,
  [double]$MaxRssDeltaMb = 30.0,
  [string]$OutCsv = "",
  [string]$OutJson = "",
  [switch]$Release
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Set-Location $repoRoot

# The ignored test must run alone: the tunnel session is process-global.
$env:INTERLINK_IDLE_SECONDS = "$IdleSeconds"
$stdout = Join-Path ([System.IO.Path]::GetTempPath()) ("interlink-idle-{0}.log" -f [Guid]::NewGuid().ToString("N"))
$stderr = Join-Path ([System.IO.Path]::GetTempPath()) ("interlink-idle-err-{0}.log" -f [Guid]::NewGuid().ToString("N"))

$cargoArgs = @(
  "test", "-p", "wunder-runtime", "--test", "interlink_loopback",
  "--features", "sqlite-storage"
)
if ($Release) { $cargoArgs += "--release" }
$cargoArgs += @("--", "--ignored", "--nocapture", "--test-threads=1", "idle_tunnel")

$test = Start-Process -FilePath "cargo" `
  -ArgumentList $cargoArgs `
  -PassThru -NoNewWindow `
  -RedirectStandardOutput $stdout -RedirectStandardError $stderr

function Read-Log([string] $Path) {
  if (-not (Test-Path $Path)) { return "" }
  $text = Get-Content -Path $Path -Raw -ErrorAction SilentlyContinue
  if ($null -eq $text) { return "" }
  return $text
}

# Wait for compilation plus the phase marker.
$marker = $null
$waitStart = [System.Diagnostics.Stopwatch]::StartNew()
while ($null -eq $marker) {
  if ($waitStart.Elapsed.TotalSeconds -gt $CompileBudgetSec) {
    throw "no INTERLINK_IDLE_PID marker within ${CompileBudgetSec}s (test may not have started)"
  }
  if ($test.HasExited) {
    throw "test process exited before printing the marker`n$(Read-Log $stdout)"
  }
  Start-Sleep -Milliseconds 500
  $match = [regex]::Match((Read-Log $stdout), "INTERLINK_IDLE_PID=(\d+) BASELINE_SECONDS=(\d+) TUNNEL_SECONDS=(\d+)")
  if ($match.Success) { $marker = $match }
}

$measuredPid = [int]$marker.Groups[1].Value
$baselineSec = [int]$marker.Groups[2].Value
$tunnelSec = [int]$marker.Groups[3].Value
$totalSec = $baselineSec + $tunnelSec

function Get-Phase([string] $Path, [double] $ElapsedSec) {
  if ($ElapsedSec -lt 1.0) { return $null }
  if ((Read-Log $Path) -match "INTERLINK_TUNNEL_UP") { return "tunnel" }
  return "baseline"
}

$rows = New-Object System.Collections.Generic.List[object]
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$phase = "baseline"
$phaseSwapSec = -1.0

while ($sw.Elapsed.TotalSeconds -lt $totalSec) {
  $p = Get-Process -Id $measuredPid -ErrorAction SilentlyContinue
  if ($null -eq $p) { break }
  $p.Refresh()
  $next = Get-Phase $stdout $sw.Elapsed.TotalSeconds
  if ($null -ne $next -and $next -ne $phase) {
    $phase = $next
    $phaseSwapSec = $sw.Elapsed.TotalSeconds
  }
  $rows.Add([pscustomobject]@{
      elapsed_s   = [math]::Round($sw.Elapsed.TotalSeconds, 3)
      phase       = $phase
      working_mb  = [math]::Round([int64]$p.WorkingSet64 / 1MB, 2)
      private_mb  = [math]::Round([int64]$p.PrivateMemorySize64 / 1MB, 2)
      cpu_ms      = [math]::Round($p.TotalProcessorTime.TotalMilliseconds, 1)
  })
  Start-Sleep -Milliseconds $IntervalMs
}

# Give the test a moment to finish its assertions.
$test.WaitForExit(120000) | Out-Null
# A -PassThru handle can still report no ExitCode; fall back to the captured
# libtest summary so a failed assertion can never read as a pass.
$testExit = 0
$rawExit = $null
try { $rawExit = $test.ExitCode } catch { $rawExit = $null }
if ($null -ne $rawExit) {
  $testExit = [int]$rawExit
} elseif ((Read-Log $stdout) -match "test result: FAILED") {
  $testExit = 1
}

function Get-Rate($rows, [string] $phase) {
  # CPU percent of one core over the phase, plus the working-set growth.
  $picked = @($rows | Where-Object { $_.phase -eq $phase })
  if ($picked.Count -lt 2) { return $null }
  $first = $picked[0]
  $last = $picked[$picked.Count - 1]
  $span = $last.elapsed_s - $first.elapsed_s
  if ($span -le 0) { return $null }
  $peak = 0.0
  $min = $first.working_mb
  foreach ($row in $picked) {
    if ($row.working_mb -gt $peak) { $peak = $row.working_mb }
    if ($row.working_mb -lt $min) { $min = $row.working_mb }
  }
  [pscustomobject]@{
    samples   = $picked.Count
    seconds   = [math]::Round($span, 2)
    cpu_ms    = [math]::Round($last.cpu_ms - $first.cpu_ms, 1)
    cpu_pct   = (($last.cpu_ms - $first.cpu_ms) / 1000.0 / $span) * 100.0
    start_mb  = $min
    peak_mb   = $peak
    delta_mb  = [math]::Round($peak - $min, 2)
  }
}

$baseline = Get-Rate $rows "baseline"
$tunnel = Get-Rate $rows "tunnel"
if ($null -eq $baseline -or $null -eq $tunnel) {
  throw "both phases must be sampled (baseline=$($null -ne $baseline), tunnel=$($null -ne $tunnel)); raise -IdleSeconds or check the log"
}

$tunnelCpu = [math]::Round($tunnel.cpu_pct - $baseline.cpu_pct, 3)
$tunnelDelta = [math]::Round($tunnel.delta_mb, 2)
$cpuOk = $tunnelCpu -le $MaxCpuPercent
$rssOk = $tunnelDelta -le $MaxRssDeltaMb
$pass = ($testExit -eq 0) -and $cpuOk -and $rssOk

$summary = [ordered]@{
  pid                   = $measuredPid
  release               = [bool]$Release
  phase_swap_s          = [math]::Round($phaseSwapSec, 2)
  baseline_seconds      = $baseline.seconds
  baseline_cpu_percent  = [math]::Round($baseline.cpu_pct, 3)
  baseline_delta_mb     = $baseline.delta_mb
  tunnel_seconds        = $tunnel.seconds
  tunnel_cpu_percent    = [math]::Round($tunnel.cpu_pct, 3)
  tunnel_delta_mb       = $tunnel.delta_mb
  tunnel_net_cpu_percent = $tunnelCpu
  tunnel_net_delta_mb   = $tunnelDelta
  max_cpu_percent       = $MaxCpuPercent
  max_delta_mb          = $MaxRssDeltaMb
  test_exit             = $testExit
  pass                  = [bool]$pass
}

Write-Host ("engines idle: cpu {0} %   tunnel up: cpu {1} %   tunnel cost: {2} % of one core" -f `
    [math]::Round($baseline.cpu_pct, 3), [math]::Round($tunnel.cpu_pct, 3), $tunnelCpu)
Write-Host ("working set: baseline {0} -> {1} MB, tunnel {2} -> {3} MB (net growth {4} MB, limit {5})" -f `
    $baseline.start_mb, $baseline.peak_mb, $tunnel.start_mb, $tunnel.peak_mb, $tunnelDelta, $MaxRssDeltaMb)

foreach ($target in @($OutCsv, $OutJson)) {
  if ($target -eq "") { continue }
  $parent = Split-Path -Parent $target
  if ($parent -and -not (Test-Path $parent)) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
  }
}
if ($OutCsv -ne "" -and $rows.Count -gt 0) {
  $rows | Export-Csv -Path $OutCsv -NoTypeInformation -Encoding utf8
}
if ($OutJson -ne "") {
  $summary | ConvertTo-Json | Set-Content -Path $OutJson -Encoding utf8
}

if (-not $pass) {
  Write-Host ("BUDGET EXCEEDED: tunnel cpu {0} % (limit {1}), rss growth {2} MB (limit {3}), test exit {4}" -f `
      $tunnelCpu, $MaxCpuPercent, $tunnelDelta, $MaxRssDeltaMb, $testExit)
  exit 1
}
exit 0
