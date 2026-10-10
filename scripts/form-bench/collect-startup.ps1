<#
  Collect "startup" latency for one form.

  Modes:
    tcp     start an exe, measure ms until TCP port accepts a connection (server)
    window  delegate to scripts\desktop-bench\measure-startup.ps1 (desktop main window)
    cli     start an exe, measure ms until a sentinel line appears on stdout
    web     delegate to collect-web-metrics.mjs (browser navigation timing)

  Output record: metric="startup", value = median ms, details = { median_ms, min_ms,
  max_ms, valid_runs, samples[], mode }.

  Always emits a record (value = $null + details.error on failure) so the runner can
  keep going.

  Usage:
    powershell -ExecutionPolicy Bypass -File scripts\form-bench\collect-startup.ps1 ^
      -FormId hull -Mode tcp -StartExe target\release\wunder-server.exe -Port 18000 ^
      -EnvPairs WUNDER_PORT=18000 -OutJson target\form-bench\raw\hull-startup.json
#>
param(
  [Parameter(Mandatory = $true)][string]$FormId,
  [Parameter(Mandatory = $true)][ValidateSet("tcp", "window", "cli", "web")][string]$Mode,
  [string]$RepoRoot = "",
  [string]$OutJson = "",
  [int]$Runs = 5,
  [int]$Warmup = 1,
  [int]$TimeoutMs = 90000,
  [string]$HostName = "127.0.0.1",
  [int]$Port = 0,
  [int]$ReadyPort = 0,
  [string]$Url = "",
  [string]$StartExe = "",
  [string[]]$StartArgs = @(),
  [string[]]$EnvPairs = @(),
  [string]$Exe = "",
  [string[]]$ExeArgs = @(),
  [string]$Sentinel = "",
  [string]$Node = "node"
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

if (-not $RepoRoot) { $RepoRoot = Get-RepoRoot -ScriptRoot $PSScriptRoot }

function Get-Stats {
  param([double[]]$Values)
  $vals = @($Values)
  if ($vals.Count -eq 0) { return $null }
  $sorted = @($vals | Sort-Object)
  $n = $sorted.Count
  $median = $sorted[[int][math]::Floor($n / 2)]
  if ($n % 2 -eq 0) { $median = ($sorted[$n / 2 - 1] + $sorted[$n / 2]) / 2.0 }
  return [ordered]@{
    median = [double]$median
    min    = [double]$sorted[0]
    max    = [double]$sorted[-1]
    count  = $n
  }
}

function Apply-EnvPairs {
  param([string[]]$Pairs)
  $saved = @{}
  foreach ($pair in @($Pairs)) {
    $kv = $pair.Split("=", 2)
    if ($kv.Length -eq 2) {
      $saved[$kv[0]] = [Environment]::GetEnvironmentVariable($kv[0])
      [Environment]::SetEnvironmentVariable($kv[0], $kv[1])
    }
  }
  return $saved
}

function Restore-Env {
  param([hashtable]$Saved)
  foreach ($k in $Saved.Keys) { [Environment]::SetEnvironmentVariable($k, $Saved[$k]) }
}

$samples = New-Object System.Collections.ArrayList
$errorText = ""

try {
  switch ($Mode) {
    "tcp" {
      $exePath = Resolve-RepoPath -Path $StartExe -RepoRoot $RepoRoot
      if (-not (Test-Path -LiteralPath $exePath)) { throw "start exe not found: $exePath" }
      $probe = if ($Port -gt 0) { $Port } elseif ($ReadyPort -gt 0) { $ReadyPort } else { throw "tcp mode needs -Port or -ReadyPort" }
      $attempts = $Runs + $Warmup
      for ($i = 0; $i -lt $attempts; $i++) {
        $saved = Apply-EnvPairs -Pairs $EnvPairs
        $outTmp = [System.IO.Path]::GetTempFileName()
        $errTmp = [System.IO.Path]::GetTempFileName()
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $proc = Start-Process -FilePath $exePath -ArgumentList $StartArgs -PassThru -NoNewWindow `
          -RedirectStandardOutput $outTmp -RedirectStandardError $errTmp
        $ready = $false
        while ($sw.Elapsed.TotalMilliseconds -lt $TimeoutMs) {
          if (Test-TcpPort -HostName $HostName -Port $probe -TimeoutMs 300) { $ready = $true; break }
          if ($proc.HasExited) { break }
          Start-Sleep -Milliseconds 25
        }
        $elapsed = $sw.Elapsed.TotalMilliseconds
        Stop-ProcessTree -ProcessId $proc.Id
        Remove-Item -LiteralPath $outTmp, $errTmp -Force -ErrorAction SilentlyContinue
        Restore-Env -Saved $saved
        if ($ready -and $i -ge $Warmup) { [void]$samples.Add($elapsed) }
        if (-not $ready) { $errorText = "attempt $i not ready within $TimeoutMs ms" }
        Start-Sleep -Milliseconds 400
      }
    }
    "window" {
      $db = Join-Path $RepoRoot "scripts\desktop-bench\measure-startup.ps1"
      if (-not (Test-Path -LiteralPath $db)) { throw "measure-startup.ps1 not found: $db" }
      $exePath = Resolve-RepoPath -Path $Exe -RepoRoot $RepoRoot
      if (-not (Test-Path -LiteralPath $exePath)) { throw "dastop exe not found: $exePath" }
      $tmp = [System.IO.Path]::GetTempFileName()
      & $db -ExePath $exePath -Runs $Runs -WarmupRuns $Warmup -TimeoutMs $TimeoutMs -OutJson $tmp | Out-Null
      $j = (Read-Utf8Text -Path $tmp) | ConvertFrom-Json
      Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
      $names = @($j.PSObject.Properties.Name)
      if ($names -contains "samples") { foreach ($s in @($j.samples)) { [void]$samples.Add([double]$s) } }
      elseif ($names -contains "median_ms") { [void]$samples.Add([double]$j.median_ms) }
      elseif ($names -contains "median") { [void]$samples.Add([double]$j.median) }
    }
    "cli" {
      $exePath = Resolve-RepoPath -Path $Exe -RepoRoot $RepoRoot
      if (-not (Test-Path -LiteralPath $exePath)) { throw "cli exe not found: $exePath" }
      if (-not $Sentinel) { throw "cli mode needs -Sentinel" }
      $rx = [regex]::Escape($Sentinel)
      $attempts = $Runs + $Warmup
      for ($i = 0; $i -lt $attempts; $i++) {
        $outTmp = [System.IO.Path]::GetTempFileName()
        $errTmp = [System.IO.Path]::GetTempFileName()
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $proc = Start-Process -FilePath $exePath -ArgumentList $ExeArgs -PassThru -NoNewWindow `
          -RedirectStandardOutput $outTmp -RedirectStandardError $errTmp
        $ready = $false
        while ($sw.Elapsed.TotalMilliseconds -lt $TimeoutMs) {
          if (Test-Path -LiteralPath $outTmp) {
            $content = ""
            try { $content = [System.IO.File]::ReadAllText($outTmp) } catch { }
            if ($content -match $rx) { $ready = $true; break }
          }
          if ($proc.HasExited) { break }
          Start-Sleep -Milliseconds 20
        }
        $elapsed = $sw.Elapsed.TotalMilliseconds
        Stop-ProcessTree -ProcessId $proc.Id
        Remove-Item -LiteralPath $outTmp, $errTmp -Force -ErrorAction SilentlyContinue
        if ($ready -and $i -ge $Warmup) { [void]$samples.Add($elapsed) }
        if (-not $ready) { $errorText = "attempt $i no sentinel within $TimeoutMs ms" }
        Start-Sleep -Milliseconds 200
      }
    }
    "web" {
      $mjs = Join-Path $PSScriptRoot "collect-web-metrics.mjs"
      if (-not (Test-Path -LiteralPath $mjs)) { throw "collect-web-metrics.mjs not found: $mjs" }
      $raw = & $Node $mjs --mode startup --url $Url --runs $Runs --warmup $Warmup 2>&1
      $text = ($raw | Out-String)
      $j = $text | ConvertFrom-Json
      if ($j.value -ne $null) { [void]$samples.Add([double]$j.value) }
      if ($j.error) { $errorText = [string]$j.error }
    }
  }
} catch {
  $errorText = $_.Exception.Message
}

$stats = Get-Stats -Values @($samples)
$value = $null
$details = [ordered]@{ mode = $Mode; runs = $Runs; warmup = $Warmup; timeout_ms = $TimeoutMs }
if ($stats) {
  $value = Format-Number $stats.median 1
  $details.median_ms = Format-Number $stats.median 1
  $details.min_ms = Format-Number $stats.min 1
  $details.max_ms = Format-Number $stats.max 1
  $details.valid_runs = $stats.count
  $details.samples = @($samples | ForEach-Object { Format-Number $_ 1 })
}
if ($errorText) { $details.error = $errorText }

Write-BenchOutput -FormId $FormId -Collector "startup" -OutJson $OutJson -Records @(
  (New-BenchRecord -Metric "startup" -Value $value -Unit "ms" -Source $Mode -Details $details)
)