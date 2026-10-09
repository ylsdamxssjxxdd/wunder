# measure-startup.ps1 - Black-box app startup timing (T1).
# Same script measures both sides (native app / Electron) for a fair comparison.
# Metric: process start -> main window handle visible. Poll granularity ~10ms.
param(
  [Parameter(Mandatory = $true)][string]$ExePath,
  [string]$AppArgs = "",
  [int]$Runs = 5,
  [int]$WarmupRuns = 1,
  [int]$TimeoutMs = 30000,
  [int]$SettleMs = 500,
  [string]$OutJson = ""
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $ExePath)) {
  Write-Error "ExePath not found: $ExePath"
  exit 1
}

$fullPath = (Resolve-Path $ExePath).Path
$procName = [System.IO.Path]::GetFileNameWithoutExtension($fullPath)

function Invoke-OneRun {
  param([string]$Exe, [string]$ArgList, [int]$Timeout, [int]$Settle, [string]$Name)

  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  # PS 5.1 rejects an empty -ArgumentList, so only pass it when present.
  if ([string]::IsNullOrWhiteSpace($ArgList)) {
    $proc = Start-Process -FilePath $Exe -PassThru
  } else {
    $proc = Start-Process -FilePath $Exe -ArgumentList $ArgList -PassThru
  }
  $windowMs = -1

  while ($sw.ElapsedMilliseconds -lt $Timeout) {
    Start-Sleep -Milliseconds 10
    # Poll every process matching the app name: covers Electron multi-process
    # trees where the launcher spawns the real window owner as a child.
    $candidates = Get-Process -Name $Name -ErrorAction SilentlyContinue
    foreach ($p in $candidates) {
      try { $p.Refresh() } catch { continue }
      if ($p.MainWindowHandle -ne 0) {
        $windowMs = $sw.ElapsedMilliseconds
        break
      }
    }
    if ($windowMs -ge 0) { break }
  }

  # Let the window finish presenting, then tear down the whole tree.
  if ($windowMs -ge 0) { Start-Sleep -Milliseconds $Settle }
  $started = Get-Process -Name $Name -ErrorAction SilentlyContinue
  foreach ($p in $started) {
    try { taskkill /PID $p.Id /T /F 2>$null | Out-Null } catch { }
  }
  try { $proc.WaitForExit(5000) | Out-Null } catch { }

  return [pscustomobject]@{
    window_visible_ms = $windowMs
    timed_out = ($windowMs -lt 0)
  }
}

$samples = @()
for ($i = 0; $i -lt $WarmupRuns; $i++) {
  Write-Host "warmup $($i + 1)/$WarmupRuns"
  Invoke-OneRun -Exe $fullPath -ArgList $AppArgs -Timeout $TimeoutMs -Settle $SettleMs -Name $procName | Out-Null
}
for ($i = 0; $i -lt $Runs; $i++) {
  Write-Host "run $($i + 1)/$Runs"
  $r = Invoke-OneRun -Exe $fullPath -ArgList $AppArgs -Timeout $TimeoutMs -Settle $SettleMs -Name $procName
  $samples += $r
}

$valid = @($samples | Where-Object { -not $_.timed_out } | ForEach-Object { [double]$_.window_visible_ms } | Sort-Object)
$median = -1.0
if ($valid.Count -gt 0) {
  $mid = [math]::Floor(($valid.Count - 1) / 2)
  if (($valid.Count % 2) -eq 0) {
    $median = ($valid[$mid] + $valid[$mid + 1]) / 2.0
  } else {
    $median = $valid[$mid]
  }
}

$report = [pscustomobject]@{
  kind = "startup"
  exe = $fullPath
  process_name = $procName
  runs = $Runs
  warmup_runs = $WarmupRuns
  timeout_ms = $TimeoutMs
  samples = $samples
  valid_count = $valid.Count
  median_ms = $median
  min_ms = $(if ($valid.Count) { $valid[0] } else { -1.0 })
  max_ms = $(if ($valid.Count) { $valid[$valid.Count - 1] } else { -1.0 })
  captured_at = (Get-Date).ToString("o")
}

if ($OutJson -ne "") {
  $report | ConvertTo-Json -Depth 4 | Out-File -FilePath $OutJson -Encoding utf8
  Write-Host "written: $OutJson"
}
Write-Host ("median={0}ms min={1}ms max={2}ms valid={3}/{4}" -f $median, $report.min_ms, $report.max_ms, $valid.Count, $Runs)
