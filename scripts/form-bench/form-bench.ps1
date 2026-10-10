<#
  form-bench unified runner.

  Drives the per-metric collectors for every form defined in
  config/forms.json, aggregates raw JSON into one report, and renders
  markdown + csv via summarize.py.

  Usage:
    powershell -ExecutionPolicy Bypass -File scripts\form-bench\form-bench.ps1 -DryRun
    powershell -ExecutionPolicy Bypass -File scripts\form-bench\form-bench.ps1 -AutoStart
    powershell -ExecutionPolicy Bypass -File scripts\form-bench\form-bench.ps1 `
      -Forms hull,honeycomb -Metrics package,startup,memory,cpu
#>
param(
  [string]$Forms = "",
  [string]$Metrics = "package,startup,memory,cpu,concurrency,chat",
  [string]$OutDir = "",
  [string]$BaseUrl = "",
  [int]$Port = 0,
  [int]$DurationSec = 0,
  [int]$IntervalMs = 0,
  [int]$StartupRuns = 0,
  [int]$StartupWarmup = 0,
  [string]$Python = "",
  [string]$Cargo = "",
  [string]$Node = "node",
  [switch]$AutoStart,
  [switch]$DryRun,
  [switch]$SkipSummary,
  [switch]$Help
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

if ($Help) {
  Write-Host @"
form-bench runner

Options:
  -Forms <list>       Comma-separated form ids (default: all in config/forms.json)
  -Metrics <list>     Comma-separated metrics (default: package,startup,memory,cpu,concurrency,chat)
  -OutDir <path>      Output dir (default: target/form-bench)
  -BaseUrl <url>      Override base URL
  -Port <n>           Override server port
  -DurationSec <n>    Override sampling duration for memory/cpu
  -IntervalMs <n>     Override sampling interval
  -StartupRuns <n>    Override startup runs
  -StartupWarmup <n>  Override startup warmup runs
  -AutoStart          Start server / dev server before probing, stop afterwards
  -DryRun             Print planned commands only
  -SkipSummary        Do not render markdown/csv
"@
  exit 0
}

$RepoRoot = Get-RepoRoot -ScriptRoot $PSScriptRoot
$FormBench = Get-FormBenchRoot -ScriptRoot $PSScriptRoot
$config = Read-JsonFile -Path (Join-Path $FormBench "config\forms.json")

# Resolve the PowerShell host running this script, so child collectors launch
# even when only pwsh (not Windows PowerShell) is on PATH.
$script:PowerShellExe = $null
try { $script:PowerShellExe = (Get-Process -Id $PID).Path } catch { }
if (-not $script:PowerShellExe) { $script:PowerShellExe = "powershell" }

# ---- resolve effective options -------------------------------------------
$defaults = $config.defaults
$effPort = if ($Port -gt 0) { $Port } else { [int]$defaults.serverPort }
$effBaseUrl = if ($BaseUrl) { $BaseUrl } else { [string]$defaults.baseUrl }
$effDuration = if ($DurationSec -gt 0) { $DurationSec } else { [int]$defaults.durationSec }
$effInterval = if ($IntervalMs -gt 0) { $IntervalMs } else { [int]$defaults.intervalMs }
$effStartupRuns = if ($StartupRuns -gt 0) { $StartupRuns } else { [int]$defaults.startupRuns }
$effStartupWarmup = if ($StartupWarmup -gt 0) { $StartupWarmup } else { [int]$defaults.startupWarmup }
$effPython = $Python
if (-not $effPython) {
  # Prefer the configured default; fall back through common aliases so the
  # runner still works when python lives under another name on PATH.
  foreach ($cand in @([string]$defaults.python, "python3", "py")) {
    if (-not $cand) { continue }
    $resolved = Get-Command $cand -ErrorAction SilentlyContinue
    if ($resolved) { $effPython = $resolved.Source; break }
  }
  if (-not $effPython) { $effPython = [string]$defaults.python }
}
$effCargo = if ($Cargo) { $Cargo } else { [string]$defaults.cargo }
$effOutDir = if ($OutDir) { $OutDir } else { "target/form-bench" }
$outRoot = Resolve-RepoPath -Path $effOutDir -RepoRoot $RepoRoot
$rawDir = Join-Path $outRoot "raw"
if (-not $DryRun) { New-Item -ItemType Directory -Force -Path $rawDir | Out-Null }

$metricSet = @($Metrics.Split(",") | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$formSet = @($Forms.Split(",") | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$selectedForms = @($config.forms | Where-Object { $formSet.Count -eq 0 -or $formSet -contains $_.id })

$script:StartedProcs = @()

function Start-Service {
  param([string]$Key)
  if (-not $AutoStart -or $DryRun) { return }
  $proc = $config.processes.$Key
  if ($null -eq $proc) { return }
  $cwd = $RepoRoot
  if ($proc.PSObject.Properties.Name -contains "cwd" -and $proc.cwd) {
    $cwd = Resolve-RepoPath -Path $proc.cwd -RepoRoot $RepoRoot
  }
  $exe = Resolve-RepoPath -Path $proc.exe -RepoRoot $RepoRoot
  Write-Host "[form-bench] autostart $Key : $exe"
  $p = Start-Process -FilePath $exe -ArgumentList @($proc.args) -WorkingDirectory $cwd -PassThru -WindowStyle Hidden
  $script:StartedProcs += $p
  $port = 0
  if ($proc.PSObject.Properties.Name -contains "readyPort") { $port = [int]$proc.readyPort }
  if ($port -gt 0) {
    $deadline = (Get-Date).AddMilliseconds([int]$proc.startTimeoutMs)
    while ((Get-Date) -lt $deadline) {
      if (Test-TcpPort -HostName "127.0.0.1" -Port $port -TimeoutMs 300) { break }
      Start-Sleep -Milliseconds 250
    }
  }
}

function Stop-StartedServices {
  foreach ($p in $script:StartedProcs) {
    if ($p -and -not $p.HasExited) { Stop-ProcessTree -ProcessId $p.Id }
  }
  $script:StartedProcs = @()
}

function Invoke-Step {
  param(
    [string]$Name,
    [string]$Exe,
    [string[]]$ChildArgs
  )
  $display = "$Exe " + (($ChildArgs | ForEach-Object {
    if ("$_" -match '[\s"]') { '"' + ("$_" -replace '"', '\"') + '"' } else { "$_" }
  }) -join " ")
  Write-Host "[form-bench] $Name"
  Write-Host "            $display"
  if ($DryRun) { return }
  # Windows PowerShell refuses to pipe a child powershell.exe ("cannot run a
  # document in the middle of a pipeline"), so launch via Start-Process with
  # file redirection instead of `& $Exe ... | Out-String`.
  $rawDirFull = Join-Path $outRoot "raw"
  if (-not (Test-Path $rawDirFull)) { New-Item -ItemType Directory -Force -Path $rawDirFull | Out-Null }
  $stdoutPath = Join-Path $rawDirFull ($Name + ".out.txt")
  $stderrPath = Join-Path $rawDirFull ($Name + ".err.txt")
  $proc = Start-Process -FilePath $Exe -ArgumentList $ChildArgs -NoNewWindow -Wait -PassThru `
    -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
  $out = ""
  if (Test-Path $stdoutPath) { $out += (Read-Utf8Text -Path $stdoutPath) }
  if (Test-Path $stderrPath) { $out += (Read-Utf8Text -Path $stderrPath) }
  $out = $out + "`n[exit] " + $proc.ExitCode
  Write-Utf8File -Path (Join-Path $rawDirFull ($Name + ".log")) -Text $out
}

function Get-FormCollectors {
  param($Form, [string[]]$Metrics)

  $steps = @()
  foreach ($metric in $Metrics) {
    switch ($metric) {
      "package" {
        if ($Form.package) {
          $steps += @{ metric = "package"; name = ($Form.id + "-package"); exe = $script:PowerShellExe; args = @(
              "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $FormBench "collect-package.ps1"),
              "-FormId", $Form.id,
              "-Target", [string]$Form.package.target,
              "-Kind", [string]$Form.package.kind,
              "-RepoRoot", $RepoRoot,
              "-OutJson", (Join-Path $rawDir ($Form.id + "-package.json"))
            ) }
        }
      }
      "startup" {
        if ($Form.startup) {
          $mode = [string]$Form.startup.mode
          $a = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $FormBench "collect-startup.ps1"),
            "-FormId", $Form.id, "-Mode", $mode, "-RepoRoot", $RepoRoot,
            "-Runs", "$effStartupRuns", "-Warmup", "$effStartupWarmup",
            "-OutJson", (Join-Path $rawDir ($Form.id + "-startup.json")))
          if ($mode -eq "tcp") {
            $a += @("-StartExe", [string]$config.processes.server.exe, "-Port", "$effPort",
              "-EnvPairs", ("WUNDER_PORT=" + $effPort))
          } elseif ($mode -eq "window" -or $mode -eq "cli") {
            $a += @("-Exe", [string]$Form.startup.exe)
            if ($mode -eq "cli") {
              $a += @("-ExeArgs", [string]$Form.startup.args, "-Sentinel", [string]$Form.startup.sentinel)
            }
          } elseif ($mode -eq "web") {
            $a += @("-Url", [string]$Form.startup.url)
          }
          $steps += @{ metric = "startup"; name = ($Form.id + "-startup"); exe = $script:PowerShellExe; args = $a }
        }
      }
      "memory" {
        $steps += (Get-ProcessCollector -Form $Form -MetricName "memory")
      }
      "cpu" {
        $steps += (Get-ProcessCollector -Form $Form -MetricName "cpu")
      }
      "concurrency" {
        if ($Form.concurrency -and $Form.concurrency.supported) {
          $steps += @{ metric = "concurrency"; name = ($Form.id + "-concurrency"); exe = $effPython; args = @(
              (Join-Path $FormBench "collect-concurrency.py"),
              "--form-id", $Form.id, "--repo-root", $RepoRoot,
              "--base-url", $effBaseUrl, "--python", $effPython, "--cargo", $effCargo,
              "--out-json", (Join-Path $rawDir ($Form.id + "-concurrency.json"))
            ) + $(if ($DryRun) { @("--dry-run") } else { @() }) }
        }
      }
      "chat" {
        if ($Form.chat) {
          $cmode = [string]$Form.chat.mode
          $a = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $FormBench "collect-chat.ps1"),
            "-FormId", $Form.id, "-Mode", $cmode, "-RepoRoot", $RepoRoot,
            "-OutJson", (Join-Path $rawDir ($Form.id + "-chat.json")))
          if ($cmode -eq "playwright") { $a += @("-Spec", [string]$Form.chat.spec) }
          $steps += @{ metric = "chat"; name = ($Form.id + "-chat"); exe = $script:PowerShellExe; args = $a }
        }
      }
    }
  }
  return $steps
}

function Get-ProcessCollector {
  param($Form, [string]$MetricName)
  if ($Form.process.mode -eq "web") {
    return @{ metric = $MetricName; name = ($Form.id + "-" + $MetricName); exe = $Node; args = @(
        (Join-Path $FormBench "collect-web-metrics.mjs"), "--mode", $MetricName,
        "--url", [string]$Form.process.url, "--duration-sec", "$effDuration"
      ) }
  }
  return @{ metric = $MetricName; name = ($Form.id + "-" + $MetricName); exe = $script:PowerShellExe; args = @(
      "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (Join-Path $FormBench "collect-process.ps1"),
      "-FormId", $Form.id, "-ProcessName", [string]$Form.process.name,
      "-DurationSec", "$effDuration", "-IntervalMs", "$effInterval", "-RepoRoot", $RepoRoot,
      "-OutJson", (Join-Path $rawDir ($Form.id + "-" + $MetricName + ".json"))
    ) }
}

# ---- run -----------------------------------------------------------------
try {
  # autostart only when a selected form needs a running backend
  $needServer = $false
  foreach ($f in $selectedForms) {
    if ($f.startup.mode -eq "tcp" -and $f.startup.start -eq "server") { $needServer = $true }
    if ($f.startup.requires -eq "server" -or $f.process.mode -eq "web") { $needServer = $true }
    if ($f.startup.requires -eq "beehiveDev") { Start-Service -Key "beehiveDev" }
  }
  if ($needServer) { Start-Service -Key "server" }

  foreach ($form in $selectedForms) {
    Write-Host ""
    Write-Host "============================================================"
    Write-Host "[form-bench] form: $($form.id) ($($form.name))"
    Write-Host "============================================================"
    $steps = Get-FormCollectors -Form $form -Metrics $metricSet
    foreach ($step in $steps) { Invoke-Step -Name $step.name -Exe $step.exe -ChildArgs $step.args }
  }
} finally {
  Stop-StartedServices
}

# ---- aggregate -----------------------------------------------------------
if (-not $DryRun) {
  $records = New-Object System.Collections.ArrayList
  $rawFiles = @(Get-ChildItem -LiteralPath $rawDir -Filter "*.json" -File -ErrorAction SilentlyContinue)
  foreach ($file in $rawFiles) {
    try {
      $env = Read-JsonFile -Path $file.FullName
      foreach ($rec in @($env.records)) {
        [void]$records.Add([ordered]@{
          form      = $env.form
          collector = $env.collector
          metric    = $rec.metric
          value     = $rec.value
          unit      = $rec.unit
          source    = $rec.source
          details   = $rec.details
        })
      }
    } catch {
      Write-Host "[form-bench] warn: skip $($file.Name): $($_.Exception.Message)"
    }
  }

  $report = [ordered]@{
    generated = (Get-Date).ToUniversalTime().ToString("o")
    baseUrl   = $effBaseUrl
    port      = $effPort
    forms     = @($selectedForms | ForEach-Object { $_.id })
    metrics   = $metricSet
    records   = @($records)
  }
  $aggJson = Join-Path $outRoot "form-bench.json"
  Write-JsonFile -Path $aggJson -Object $report
  Write-Host "[form-bench] wrote $aggJson ($($records.Count) records)"

  if (-not $SkipSummary) {
    $summarize = Join-Path $FormBench "summarize.py"
    if (Test-Path -LiteralPath $summarize) {
      $md = Join-Path $outRoot "form-bench.md"
      $csv = Join-Path $outRoot "form-bench.csv"
      $so = Join-Path $rawDir "summary.out.txt"
      $se = Join-Path $rawDir "summary.err.txt"
      $p = Start-Process -FilePath $effPython -ArgumentList @($summarize, "--in", $aggJson, "--out-md", $md, "--out-csv", $csv) `
        -NoNewWindow -Wait -PassThru -RedirectStandardOutput $so -RedirectStandardError $se
      if (Test-Path $so) { Write-Host (Read-Utf8Text -Path $so) }
      if (Test-Path $se) { $errText = Read-Utf8Text -Path $se; if ($errText.Trim()) { Write-Host $errText } }
      Write-Host "[form-bench] summarize exit $($p.ExitCode)"
    }
  }
} else {
  Write-Host ""
  Write-Host "[form-bench] dry run complete (no files written)"
}