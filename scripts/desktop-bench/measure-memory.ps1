# measure-memory.ps1 - Black-box process memory sampling (M1-M4 endpoints).
# Sums Working Set + Private Bytes across ALL processes matching the name
# (Electron spawns many; the native app has one). Writes CSV + summary JSON.
param(
  [Parameter(Mandatory = $true)][string]$ProcessName,
  [int]$DurationSec = 600,
  [int]$IntervalMs = 1000,
  [string]$Label = "sample",
  [string]$OutCsv = "",
  [string]$OutJson = ""
)

$ErrorActionPreference = "Stop"

$rows = New-Object System.Collections.Generic.List[object]
$sw = [System.Diagnostics.Stopwatch]::StartNew()

while ($sw.Elapsed.TotalSeconds -lt $DurationSec) {
  $procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
  $wsSum = 0.0
  $privSum = 0.0
  $count = 0
  foreach ($p in $procs) {
    try {
      $p.Refresh()
      $wsSum += $p.WorkingSet64
      $privSum += $p.PrivateMemorySize64
      $count += 1
    } catch { }
  }
  $rows += [pscustomobject]@{
    timestamp_s = [math]::Round($sw.Elapsed.TotalSeconds, 3)
    process_count = $count
    working_set_mb = [math]::Round($wsSum / 1MB, 2)
    private_mb = [math]::Round($privSum / 1MB, 2)
  }
  Start-Sleep -Milliseconds $IntervalMs
}

if ($OutCsv -ne "") {
  $rows | Export-Csv -Path $OutCsv -NoTypeInformation -Encoding utf8
  Write-Host "csv written: $OutCsv"
}

$first = $rows[0]
$last = $rows[$rows.Count - 1]
$peakWs = ($rows | Measure-Object -Property working_set_mb -Maximum).Maximum
$peakPriv = ($rows | Measure-Object -Property private_mb -Maximum).Maximum
# Steady state: average of the final 10 samples.
$tail = @($rows | Select-Object -Last 10)
$steadyWs = ($tail | Measure-Object -Property working_set_mb -Average).Average
$steadyPriv = ($tail | Measure-Object -Property private_mb -Average).Average

$report = [pscustomobject]@{
  kind = "memory"
  label = $Label
  process_name = $ProcessName
  duration_sec = $DurationSec
  interval_ms = $IntervalMs
  sample_count = $rows.Count
  start_working_set_mb = $first.working_set_mb
  start_private_mb = $first.private_mb
  start_process_count = $first.process_count
  end_working_set_mb = $last.working_set_mb
  end_private_mb = $last.private_mb
  end_process_count = $last.process_count
  peak_working_set_mb = $peakWs
  peak_private_mb = $peakPriv
  steady_working_set_mb = [math]::Round($steadyWs, 2)
  steady_private_mb = [math]::Round($steadyPriv, 2)
  captured_at = (Get-Date).ToString("o")
}

if ($OutJson -ne "") {
  $report | ConvertTo-Json -Depth 3 | Out-File -FilePath $OutJson -Encoding utf8
  Write-Host "json written: $OutJson"
}
Write-Host ("steady ws={0}MB priv={1}MB | peak ws={2}MB priv={3}MB | processes={4}" -f $report.steady_working_set_mb, $report.steady_private_mb, $peakWs, $peakPriv, $report.end_process_count)
