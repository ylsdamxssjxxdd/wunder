[CmdletBinding()]
param(
  [ValidateRange(1, 10)]
  [int]$Runs = 3,
  [ValidateRange(0, 5)]
  [int]$WarmupRuns = 0,
  [ValidateRange(1000, 120000)]
  [int]$TimeoutMs = 30000,
  [string]$DataRoot = '',
  [switch]$ResetData
)

$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$electronExe = Join-Path $repoRoot 'node_modules\electron\dist\electron.exe'
$electronAppRoot = Join-Path $repoRoot 'desktop\electron'
$bridgeExe = Join-Path $repoRoot 'target\release\wunder-desktop-bridge.exe'
$frontendRoot = Join-Path $repoRoot 'frontend\dist'
$skillsRoot = Join-Path $repoRoot 'config\skills'

foreach ($requiredPath in @($electronExe, $electronAppRoot, $bridgeExe, $frontendRoot, $skillsRoot)) {
  if (!(Test-Path -LiteralPath $requiredPath)) {
    throw "Startup benchmark prerequisite is missing: $requiredPath"
  }
}

if (![string]::IsNullOrWhiteSpace($DataRoot)) {
  if (![IO.Path]::IsPathRooted($DataRoot)) {
    $DataRoot = Join-Path $repoRoot $DataRoot
  }
  $DataRoot = [IO.Path]::GetFullPath($DataRoot)
} else {
  $DataRoot = Join-Path $repoRoot 'temp_dir\desktop-electron-startup-benchmark'
}

if ($ResetData -and (Test-Path -LiteralPath $DataRoot)) {
  Remove-Item -LiteralPath $DataRoot -Recurse -Force
}
New-Item -ItemType Directory -Path $DataRoot -Force | Out-Null

function Get-DescendantProcessIds {
  param([int]$RootProcessId)

  $allProcesses = @(Get-CimInstance Win32_Process)
  $pending = [Collections.Generic.Queue[int]]::new()
  $pending.Enqueue($RootProcessId)
  $descendants = [Collections.Generic.List[int]]::new()

  while ($pending.Count -gt 0) {
    $parentId = $pending.Dequeue()
    foreach ($child in $allProcesses | Where-Object { [int]$_.ParentProcessId -eq $parentId }) {
      $childId = [int]$child.ProcessId
      $descendants.Add($childId)
      $pending.Enqueue($childId)
    }
  }

  return $descendants
}

function Stop-BenchmarkProcessTree {
  param([Diagnostics.Process]$Process)

  if ($null -eq $Process) {
    return
  }
  $processIds = @(Get-DescendantProcessIds -RootProcessId $Process.Id)
  $processIds += $Process.Id
  foreach ($processId in $processIds | Sort-Object -Descending -Unique) {
    Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
  }
}

function Find-StartupTotalMs {
  param(
    [string]$Text,
    [string]$Scope,
    [string]$Kind,
    [string]$Name
  )

  $escapedName = [regex]::Escape($Name)
  $pattern = "\[startup\]\[$([regex]::Escape($Scope))\] $Kind=$escapedName\b[^\r\n]*\btotal_ms=([0-9.]+)"
  $match = [regex]::Match($Text, $pattern)
  if (!$match.Success) {
    return $null
  }
  return [double]$match.Groups[1].Value
}

function Find-RendererStageTotalMs {
  param(
    [string]$Text,
    [string]$Stage
  )

  $pattern = "\[startup\]\[renderer\] point=stage total_ms=([0-9.]+)\b[^\r\n]*\bstage=$([regex]::Escape($Stage))\b"
  $match = [regex]::Match($Text, $pattern)
  if (!$match.Success) {
    return $null
  }
  return [double]$match.Groups[1].Value
}

function Get-Median {
  param([double[]]$Values)

  if (!$Values -or $Values.Count -eq 0) {
    return $null
  }
  $sorted = @($Values | Sort-Object)
  $middle = [int][Math]::Floor($sorted.Count / 2)
  if ($sorted.Count % 2 -eq 1) {
    return $sorted[$middle]
  }
  return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

$startupEnvironment = @{
  WUNDER_STARTUP_TIMING = '1'
  WUNDER_BRIDGE_PATH = $bridgeExe
  WUNDER_FRONTEND_ROOT = $frontendRoot
  WUNDER_BUILTIN_SKILLS_ROOT = $skillsRoot
  WUNDER_DESKTOP_PACKAGED = '1'
  WUNDER_BRIDGE_LOG_VERBOSE = '0'
  ELECTRON_ENABLE_LOGGING = '1'
}
$previousEnvironment = @{}
foreach ($name in $startupEnvironment.Keys) {
  $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
  [Environment]::SetEnvironmentVariable($name, $startupEnvironment[$name], 'Process')
}

$samples = @()
$totalRuns = $WarmupRuns + $Runs
try {
  for ($index = 1; $index -le $totalRuns; $index += 1) {
    $kind = if ($index -le $WarmupRuns) { 'warmup' } else { 'measure' }
    $logBase = Join-Path $DataRoot ("electron-startup-$index")
    $stdoutPath = "$logBase.log"
    $stderrPath = "$logBase.err.log"
    Remove-Item -LiteralPath $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue

    $process = Start-Process -FilePath $electronExe `
      -ArgumentList @($electronAppRoot, "--user-data-dir=$DataRoot") `
      -WorkingDirectory $repoRoot `
      -RedirectStandardOutput $stdoutPath `
      -RedirectStandardError $stderrPath `
      -PassThru `
      -WindowStyle Hidden
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    $bootstrapCompleted = $false
    try {
      do {
        Start-Sleep -Milliseconds 50
        $stdout = if (Test-Path -LiteralPath $stdoutPath) {
          Get-Content -LiteralPath $stdoutPath -Raw -ErrorAction SilentlyContinue
        } else {
          ''
        }
        $bootstrapCompleted = $stdout -match '\[startup\]\[renderer\] point=stage total_ms=[0-9.]+ stage=messenger-bootstrap-finish\b'
      } while (!$bootstrapCompleted -and [DateTime]::UtcNow -lt $deadline -and !$process.HasExited)

      if (!$bootstrapCompleted) {
        $stderr = if (Test-Path -LiteralPath $stderrPath) {
          Get-Content -LiteralPath $stderrPath -Raw -ErrorAction SilentlyContinue
        } else {
          ''
        }
        throw "Electron startup did not reach messenger-bootstrap-finish within ${TimeoutMs}ms. $stderr"
      }

      $samples += [ordered]@{
        run = $index
        kind = $kind
        window_ready_to_show_ms = Find-StartupTotalMs -Text $stdout -Scope 'electron' -Kind 'point' -Name 'window_ready_to_show'
        bridge_ready_ms = Find-StartupTotalMs -Text $stdout -Scope 'electron' -Kind 'segment' -Name 'bridge_start_total'
        document_loaded_ms = Find-StartupTotalMs -Text $stdout -Scope 'electron' -Kind 'point' -Name 'main_ui_loaded'
        app_mounted_ms = Find-RendererStageTotalMs -Text $stdout -Stage 'app-mounted'
        messenger_ready_ms = Find-RendererStageTotalMs -Text $stdout -Stage 'messenger-bootstrap-finish'
        stdout_path = $stdoutPath
        stderr_path = $stderrPath
      }
    } finally {
      Stop-BenchmarkProcessTree -Process $process
      Start-Sleep -Milliseconds 150
    }
  }
} finally {
  foreach ($name in $startupEnvironment.Keys) {
    [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
  }
}

$measuredSamples = @($samples | Where-Object { $_.kind -eq 'measure' })
$metrics = @('window_ready_to_show_ms', 'bridge_ready_ms', 'document_loaded_ms', 'app_mounted_ms', 'messenger_ready_ms')
$median = [ordered]@{}
foreach ($metric in $metrics) {
  $values = @($measuredSamples | ForEach-Object { [double]$_[$metric] })
  $median[$metric] = Get-Median -Values $values
}

$result = [ordered]@{
  timestamp_utc = [DateTime]::UtcNow.ToString('o')
  repository_root = $repoRoot
  electron_exe = $electronExe
  bridge_exe = $bridgeExe
  frontend_root = $frontendRoot
  data_root = $DataRoot
  warmup_runs = $WarmupRuns
  measured_runs = $Runs
  samples = $samples
  median_ms = $median
}
$resultPath = Join-Path $DataRoot 'results.json'
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $resultPath -Encoding utf8
$result | ConvertTo-Json -Depth 6
