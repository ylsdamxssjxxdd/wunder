param(
  [string]$OutDir = "target\bench",
  [string]$Label = "",
  [string]$Baseline = "",
  [switch]$Quick,
  [switch]$SkipBackendSim,
  [switch]$IncludeRuntimeBoundary,
  [string]$BaseUrl = "http://127.0.0.1:18001/wunder",
  [string]$ApiKey = "",
  [string]$AuthToken = "",
  [string]$UserId = "",
  [string]$RuntimeScenario = "monitor,channels",
  [int]$RuntimeConcurrency = 8,
  [int]$RuntimeRounds = 3,
  [string]$Python = "python",
  [string]$CargoBin = "cargo",
  [double]$MaxLatencyRegressionPct = 10.0,
  [double]$MaxLatencyRegressionMs = 50.0,
  [double]$MaxThroughputRegressionPct = 5.0,
  [double]$MaxRateRegressionAbs = 0.001,
  [switch]$Help
)

$ErrorActionPreference = "Stop"

function Write-HelpText() {
  Write-Host @"
wunder benchmark

Usage:
  powershell -ExecutionPolicy Bypass -File scripts\benchmark.ps1 -Label baseline
  powershell -ExecutionPolicy Bypass -File scripts\benchmark.ps1 -Baseline target\bench\<run>\benchmark.json -Label after-change

Options:
  -Quick                    Run quick backend_sim scenarios.
  -Baseline <path>          Compare current results with a previous benchmark.json.
  -IncludeRuntimeBoundary   Also run scripts\runtime_boundary_stress.py against a live server.
  -BaseUrl <url>            Runtime boundary server base URL. Default: $BaseUrl
  -SkipBackendSim           Skip backend_sim scenarios.
"@
}

if ($Help) {
  Write-HelpText
  exit 0
}

function Resolve-RepoPath([string]$Path) {
  if ([System.IO.Path]::IsPathRooted($Path)) {
    return $Path
  }
  return (Join-Path (Get-Location).Path $Path)
}

function Format-Slug([string]$Value) {
  $slug = $Value -replace "[^A-Za-z0-9_.-]+", "-"
  if ([string]::IsNullOrWhiteSpace($slug)) {
    return "run"
  }
  return $slug.Trim("-")
}

function Write-Utf8File([string]$Path, [string]$Text) {
  $parent = Split-Path -Parent $Path
  if (![string]::IsNullOrWhiteSpace($parent)) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
  }
  $encoding = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Text, $encoding)
}

function Try-CommandText([string]$Exe, [string[]]$CommandArgs) {
  try {
    $output = & $Exe @CommandArgs 2>$null
    if ($LASTEXITCODE -ne 0) {
      return ""
    }
    return [string]::Join("`n", @($output))
  } catch {
    return ""
  }
}

function Quote-ProcessArg([string]$Arg) {
  if ($null -eq $Arg) {
    return '""'
  }
  if ($Arg -notmatch '[\s"]') {
    return $Arg
  }
  return '"' + ($Arg -replace '"', '\"') + '"'
}

function Invoke-LoggedCommand(
  [string]$Name,
  [string]$Exe,
  [string[]]$CommandArgs,
  [string]$LogPath
) {
  Write-Host "[benchmark] run $Name"
  Write-Host "[benchmark] cmd $Exe $($CommandArgs -join ' ')"
  $argText = [string]::Join(" ", @($CommandArgs | ForEach-Object { Quote-ProcessArg $_ }))
  $stdoutPath = [System.IO.Path]::GetTempFileName()
  $stderrPath = [System.IO.Path]::GetTempFileName()
  $process = Start-Process `
    -FilePath $Exe `
    -ArgumentList $argText `
    -NoNewWindow `
    -Wait `
    -PassThru `
    -RedirectStandardOutput $stdoutPath `
    -RedirectStandardError $stderrPath
  $stdout = if (Test-Path -LiteralPath $stdoutPath) { Get-Content -LiteralPath $stdoutPath -Raw } else { "" }
  $stderr = if (Test-Path -LiteralPath $stderrPath) { Get-Content -LiteralPath $stderrPath -Raw } else { "" }
  Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
  Write-Utf8File $LogPath (($stdout + $stderr).TrimEnd() + "`n")
  $exitCode = $process.ExitCode
  if ($exitCode -ne 0) {
    throw "$Name failed with exit code $exitCode. See log: $LogPath"
  }
}

function New-BenchmarkResult(
  [string]$Name,
  [string]$Category,
  [string]$Metric,
  [object]$Value,
  [string]$Unit,
  [string]$Direction,
  [double]$ThresholdPct,
  [Nullable[double]]$ThresholdAbs,
  [string]$Source,
  [string]$Command,
  [string[]]$ValidationErrors
) {
  $numericValue = $null
  if ($Value -is [int] -or $Value -is [long] -or $Value -is [double] -or $Value -is [decimal]) {
    $numericValue = [double]$Value
  }
  [pscustomobject]@{
    name = $Name
    category = $Category
    metric = $Metric
    value = $numericValue
    unit = $Unit
    direction = $Direction
    threshold_pct = $ThresholdPct
    threshold_abs = $ThresholdAbs
    baseline_value = $null
    delta_pct = $null
    delta_abs = $null
    passed = [bool]($ValidationErrors.Count -eq 0)
    validation_errors = @($ValidationErrors)
    source = $Source
    command = $Command
  }
}

function Add-ValidationError([object]$Result, [string]$Message) {
  $errors = @($Result.validation_errors)
  $errors += $Message
  $Result.validation_errors = $errors
  $Result.passed = $false
}

function Apply-Baseline([object[]]$Results, [string]$BaselinePath) {
  if ([string]::IsNullOrWhiteSpace($BaselinePath)) {
    return
  }
  $resolved = Resolve-RepoPath $BaselinePath
  if (!(Test-Path -LiteralPath $resolved -PathType Leaf)) {
    throw "baseline not found: $resolved"
  }
  $baselineReport = Get-Content -LiteralPath $resolved -Raw | ConvertFrom-Json
  $baselineByName = @{}
  foreach ($entry in @($baselineReport.results)) {
    $baselineByName[[string]$entry.name] = $entry
  }

  foreach ($entry in $Results) {
    if (!$baselineByName.ContainsKey($entry.name)) {
      continue
    }
    $baseEntry = $baselineByName[$entry.name]
    if ($null -eq $baseEntry.value -or $null -eq $entry.value) {
      continue
    }
    $base = [double]$baseEntry.value
    $current = [double]$entry.value
    $entry.baseline_value = [Math]::Round($base, 6)
    $entry.delta_abs = [Math]::Round($current - $base, 6)
    if ([Math]::Abs($base) -gt 0.0000001) {
      $entry.delta_pct = [Math]::Round((($current - $base) / $base) * 100.0, 3)
    }

    if ($entry.direction -eq "lower") {
      $worseAbs = $current - $base
      $pctExceeded = $false
      if ($null -ne $entry.delta_pct) {
        $pctExceeded = [double]$entry.delta_pct -gt [double]$entry.threshold_pct
      }
      $absExceeded = $true
      if ($null -ne $entry.threshold_abs) {
        $absExceeded = $worseAbs -gt [double]$entry.threshold_abs
      }
      if ($worseAbs -gt 0.0 -and ($pctExceeded -or $null -eq $entry.delta_pct) -and $absExceeded) {
        Add-ValidationError $entry "regression: value increased from $base to $current"
      }
    } elseif ($entry.direction -eq "higher") {
      $worseAbs = $base - $current
      $pctExceeded = $false
      if ($null -ne $entry.delta_pct) {
        $pctExceeded = [double]$entry.delta_pct -lt (-1.0 * [double]$entry.threshold_pct)
      }
      $absExceeded = $true
      if ($null -ne $entry.threshold_abs) {
        $absExceeded = $worseAbs -gt [double]$entry.threshold_abs
      }
      if ($worseAbs -gt 0.0 -and ($pctExceeded -or $null -eq $entry.delta_pct) -and $absExceeded) {
        Add-ValidationError $entry "regression: value decreased from $base to $current"
      }
    }
  }
}

function Find-LatestSummary([string]$Dir, [string]$Mode) {
  $summary = Get-ChildItem -LiteralPath $Dir -Filter "summary.$Mode.*.json" -File |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
  if ($null -eq $summary) {
    throw "backend_sim summary not found in $Dir"
  }
  return $summary.FullName
}

function Add-BackendSimResults(
  [System.Collections.Generic.List[object]]$Results,
  [string]$SummaryPath,
  [string]$Command
) {
  $summary = Get-Content -LiteralPath $SummaryPath -Raw | ConvertFrom-Json
  foreach ($item in @($summary.results)) {
    $scenario = [string]$item.scenario
    $metrics = $item.metrics
    $errors = @()
    if (-not [bool]$item.succeeded) {
      $errors += "backend_sim scenario failed"
    }

    if ($null -ne $metrics.p95_ms) {
      $Results.Add((New-BenchmarkResult "backend_sim.$scenario.p95_latency" "backend_sim" "p95_latency" $metrics.p95_ms "ms" "lower" $MaxLatencyRegressionPct $MaxLatencyRegressionMs $SummaryPath $Command $errors))
    }
    if ($null -ne $metrics.p99_ms) {
      $Results.Add((New-BenchmarkResult "backend_sim.$scenario.p99_latency" "backend_sim" "p99_latency" $metrics.p99_ms "ms" "lower" $MaxLatencyRegressionPct $MaxLatencyRegressionMs $SummaryPath $Command $errors))
    }
    if ($null -ne $metrics.first_event_p95_ms) {
      $Results.Add((New-BenchmarkResult "backend_sim.$scenario.first_event_p95" "backend_sim" "first_event_p95" $metrics.first_event_p95_ms "ms" "lower" $MaxLatencyRegressionPct $MaxLatencyRegressionMs $SummaryPath $Command $errors))
    }
    if ($null -ne $metrics.throughput_rps) {
      $Results.Add((New-BenchmarkResult "backend_sim.$scenario.throughput_rps" "backend_sim" "throughput_rps" $metrics.throughput_rps "rps" "higher" $MaxThroughputRegressionPct $null $SummaryPath $Command $errors))
    }
    if ($null -ne $metrics.success_rate) {
      $Results.Add((New-BenchmarkResult "backend_sim.$scenario.success_rate" "backend_sim" "success_rate" $metrics.success_rate "ratio" "higher" 0.1 $MaxRateRegressionAbs $SummaryPath $Command $errors))
    }
    if ($null -ne $metrics.final_event_missing_rate) {
      $Results.Add((New-BenchmarkResult "backend_sim.$scenario.final_event_missing_rate" "backend_sim" "final_event_missing_rate" $metrics.final_event_missing_rate "ratio" "lower" 0.1 $MaxRateRegressionAbs $SummaryPath $Command $errors))
    }
  }
}

function Add-RuntimeBoundaryResults(
  [System.Collections.Generic.List[object]]$Results,
  [string]$OutputPath,
  [string]$Command
) {
  $payload = Get-Content -LiteralPath $OutputPath -Raw | ConvertFrom-Json
  $requestErrors = @()
  if ([int]$payload.requests.failed -gt 0) {
    $requestErrors += "runtime boundary requests failed"
  }
  $alertCount = @($payload.delta.alerts).Count
  if ($alertCount -gt 0) {
    $requestErrors += "runtime boundary produced alerts"
  }
  $Results.Add((New-BenchmarkResult "runtime_boundary.requests_failed" "runtime_boundary" "requests_failed" $payload.requests.failed "count" "lower" 0.0 0.0 $OutputPath $Command $requestErrors))
  $Results.Add((New-BenchmarkResult "runtime_boundary.alerts" "runtime_boundary" "alerts" $alertCount "count" "lower" 0.0 0.0 $OutputPath $Command $requestErrors))
  $Results.Add((New-BenchmarkResult "runtime_boundary.blocking_queue_timeouts" "runtime_boundary" "blocking_queue_timeouts" $payload.delta.blocking_queue_timeouts "count" "lower" 0.0 0.0 $OutputPath $Command @()))
  $Results.Add((New-BenchmarkResult "runtime_boundary.blocking_exec_timeouts" "runtime_boundary" "blocking_exec_timeouts" $payload.delta.blocking_exec_timeouts "count" "lower" 0.0 0.0 $OutputPath $Command @()))
  $Results.Add((New-BenchmarkResult "runtime_boundary.long_task_warnings" "runtime_boundary" "long_task_warnings" $payload.delta.long_task_warnings "count" "lower" 0.0 0.0 $OutputPath $Command @()))
  $Results.Add((New-BenchmarkResult "runtime_boundary.queue_busy" "runtime_boundary" "queue_busy" $payload.delta.queue_busy "count" "lower" 25.0 $null $OutputPath $Command @()))
}

function Format-Value([object]$Value, [string]$Unit) {
  if ($null -eq $Value) {
    return "n/a"
  }
  $number = [double]$Value
  if ($Unit -eq "ratio") {
    return ("{0:N2}%" -f ($number * 100.0))
  }
  if ($Unit -eq "ms") {
    return ("{0:N2}ms" -f $number)
  }
  return ("{0:N3} {1}" -f $number, $Unit).Trim()
}

function Write-MarkdownReport([string]$Path, [object]$Report) {
  $lines = @()
  $lines += "# wunder benchmark"
  $lines += ""
  $lines += "- generated_at: $($Report.generated_at)"
  $lines += "- label: $($Report.label)"
  $lines += "- baseline: $($Report.baseline)"
  $lines += "- passed: $($Report.summary.passed)"
  $lines += ""
  $lines += "| case | value | baseline | delta | status |"
  $lines += "| --- | ---: | ---: | ---: | --- |"
  foreach ($entry in @($Report.results)) {
    $value = Format-Value $entry.value $entry.unit
    $baseline = if ($null -eq $entry.baseline_value) { "" } else { Format-Value $entry.baseline_value $entry.unit }
    $delta = if ($null -eq $entry.delta_pct) { "" } else { ("{0:N2}%" -f [double]$entry.delta_pct) }
    $status = if ($entry.passed) { "PASS" } else { "FAIL" }
    $lines += "| $($entry.name) | $value | $baseline | $delta | $status |"
  }
  $lines += ""
  $failed = @($Report.results | Where-Object { -not $_.passed })
  if ($failed.Count -gt 0) {
    $lines += "## failures"
    $lines += ""
    foreach ($entry in $failed) {
      $lines += "- $($entry.name): $([string]::Join('; ', @($entry.validation_errors)))"
    }
    $lines += ""
  }
  Write-Utf8File $Path ([string]::Join("`n", $lines) + "`n")
}

function Write-CsvReport([string]$Path, [object[]]$Results) {
  $rows = foreach ($entry in $Results) {
    [pscustomobject]@{
      name = $entry.name
      category = $entry.category
      metric = $entry.metric
      value = $entry.value
      unit = $entry.unit
      direction = $entry.direction
      baseline_value = $entry.baseline_value
      delta_pct = $entry.delta_pct
      delta_abs = $entry.delta_abs
      passed = $entry.passed
      validation_errors = [string]::Join("; ", @($entry.validation_errors))
      source = $entry.source
    }
  }
  $parent = Split-Path -Parent $Path
  if (![string]::IsNullOrWhiteSpace($parent)) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
  }
  $rows | Export-Csv -LiteralPath $Path -NoTypeInformation -Encoding UTF8
}

$repoRoot = (Get-Location).Path
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$slug = Format-Slug $Label
$runDir = Resolve-RepoPath (Join-Path $OutDir "$timestamp-$slug")
$logsDir = Join-Path $runDir "logs"
$backendDir = Join-Path $runDir "backend_sim"
$runtimeDir = Join-Path $runDir "runtime_boundary"
New-Item -ItemType Directory -Force -Path $logsDir | Out-Null

$results = New-Object "System.Collections.Generic.List[object]"
$commands = New-Object "System.Collections.Generic.List[string]"

if ($SkipBackendSim -and -not $IncludeRuntimeBoundary) {
  throw "no benchmark cases selected"
}

if (-not $SkipBackendSim) {
  $env:CARGO_BUILD_JOBS = "8"
  $backendArgs = @(
    "scripts/run_backend_sim_workflow.py",
    "baseline",
    "--base-dir",
    $backendDir,
    "--cargo-bin",
    $CargoBin
  )
  if ($Quick) {
    $backendArgs += "--quick"
  }
  $commandText = "$Python $($backendArgs -join ' ')"
  $commands.Add($commandText) | Out-Null
  Invoke-LoggedCommand "backend_sim" $Python $backendArgs (Join-Path $logsDir "backend_sim.log")
  $backendSummary = Find-LatestSummary $backendDir "baseline"
  Add-BackendSimResults $results $backendSummary $commandText
}

if ($IncludeRuntimeBoundary) {
  New-Item -ItemType Directory -Force -Path $runtimeDir | Out-Null
  $runtimeOutput = Join-Path $runtimeDir "runtime_boundary.json"
  $runtimeArgs = @(
    "scripts/runtime_boundary_stress.py",
    "--base-url",
    $BaseUrl,
    "--concurrency",
    [string]$RuntimeConcurrency,
    "--rounds",
    [string]$RuntimeRounds,
    "--scenario",
    $RuntimeScenario,
    "--fail-on-alert"
  )
  if (![string]::IsNullOrWhiteSpace($ApiKey)) {
    $runtimeArgs += @("--api-key", $ApiKey)
  }
  if (![string]::IsNullOrWhiteSpace($AuthToken)) {
    $runtimeArgs += @("--auth-token", $AuthToken)
  }
  if (![string]::IsNullOrWhiteSpace($UserId)) {
    $runtimeArgs += @("--user-id", $UserId)
  }
  $runtimeCommandText = "$Python $($runtimeArgs -join ' ')"
  $commands.Add($runtimeCommandText) | Out-Null
  Invoke-LoggedCommand "runtime_boundary" $Python $runtimeArgs $runtimeOutput
  Copy-Item -LiteralPath $runtimeOutput -Destination (Join-Path $logsDir "runtime_boundary.json") -Force
  Add-RuntimeBoundaryResults $results $runtimeOutput $runtimeCommandText
}

Apply-Baseline $results.ToArray() $Baseline

$failed = @($results | Where-Object { -not $_.passed })
$resolvedBaseline = ""
if (![string]::IsNullOrWhiteSpace($Baseline)) {
  $resolvedBaseline = Resolve-RepoPath $Baseline
}
$backendArtifactDir = ""
if (-not $SkipBackendSim) {
  $backendArtifactDir = $backendDir
}
$runtimeArtifactDir = ""
if ($IncludeRuntimeBoundary) {
  $runtimeArtifactDir = $runtimeDir
}
$gitHead = Try-CommandText "git" @("rev-parse", "--short", "HEAD")
$gitDirty = Try-CommandText "git" @("status", "--short")
$rustcVersion = Try-CommandText "rustc" @("--version")
$cargoVersion = Try-CommandText $CargoBin @("--version")
$pythonVersion = Try-CommandText $Python @("--version")
$commandItems = [string[]]$commands.ToArray()
$resultItems = [object[]]$results.ToArray()
$report = [pscustomobject]@{
  benchmark = "wunder-performance"
  generated_at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  label = $Label
  quick = [bool]$Quick
  baseline = $resolvedBaseline
  repo = [pscustomobject]@{
    root = $repoRoot
    git_head = $gitHead
    git_dirty = $gitDirty
  }
  environment = [pscustomobject]@{
    os = [System.Environment]::OSVersion.VersionString
    powershell = $PSVersionTable.PSVersion.ToString()
    rustc = $rustcVersion
    cargo = $cargoVersion
    python = $pythonVersion
  }
  config = [pscustomobject]@{
    max_latency_regression_pct = $MaxLatencyRegressionPct
    max_latency_regression_ms = $MaxLatencyRegressionMs
    max_throughput_regression_pct = $MaxThroughputRegressionPct
    max_rate_regression_abs = $MaxRateRegressionAbs
    include_runtime_boundary = [bool]$IncludeRuntimeBoundary
    runtime_base_url = $BaseUrl
    runtime_scenario = $RuntimeScenario
    runtime_concurrency = $RuntimeConcurrency
    runtime_rounds = $RuntimeRounds
  }
  artifacts = [pscustomobject]@{
    run_dir = $runDir
    logs_dir = $logsDir
    backend_sim_dir = $backendArtifactDir
    runtime_boundary_dir = $runtimeArtifactDir
  }
  commands = $commandItems
  summary = [pscustomobject]@{
    total = $results.Count
    failed = $failed.Count
    passed = [bool]($failed.Count -eq 0)
  }
  results = $resultItems
}

$jsonPath = Join-Path $runDir "benchmark.json"
$csvPath = Join-Path $runDir "benchmark.csv"
$mdPath = Join-Path $runDir "benchmark.md"
Write-Utf8File $jsonPath ((ConvertTo-Json $report -Depth 16) + "`n")
Write-CsvReport $csvPath $results.ToArray()
Write-MarkdownReport $mdPath $report

Write-Host ""
Write-Host "benchmark report:"
Write-Host "  $jsonPath"
Write-Host "  $csvPath"
Write-Host "  $mdPath"

if ($failed.Count -gt 0) {
  Write-Host ""
  Write-Host "benchmark validation failed:"
  foreach ($entry in $failed) {
    Write-Host "  - $($entry.name): $([string]::Join('; ', @($entry.validation_errors)))"
  }
  exit 1
}

exit 0
