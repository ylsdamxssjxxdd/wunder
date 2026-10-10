<#
  Collect "chat page performance" for one form.

  Modes:
    playwright  run a Playwright perf spec (frontend/tests/e2e/*) and report the
                spec wall-clock duration (ms) plus pass/fail
    slint       placeholder: native Slint frame metrics require instrumentation
                inside frontend-slint (see README todo T-chat-native)
    none        metric is not applicable to this form (e.g. server / cli)

  Always emits a record; value = $null with details.error / details.note when the
  metric cannot be produced.

  Usage:
    powershell -ExecutionPolicy Bypass -File scripts\form-bench\collect-chat.ps1 ^
      -FormId bridge -Mode playwright ^
      -Spec frontend\tests\e2e\chat-durable-stream-performance.spec.ts ^
      -OutJson target\form-bench\raw\bridge-chat.json
#>
param(
  [Parameter(Mandatory = $true)][string]$FormId,
  [Parameter(Mandatory = $true)][ValidateSet("playwright", "slint", "none")][string]$Mode,
  [string]$RepoRoot = "",
  [string]$OutJson = "",
  [string]$Spec = "",
  [string]$Cwd = "frontend",
  [int]$TimeoutMs = 600000
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

if (-not $RepoRoot) { $RepoRoot = Get-RepoRoot -ScriptRoot $PSScriptRoot }

$value = $null
$details = [ordered]@{ mode = $Mode }

try {
  switch ($Mode) {
    "playwright" {
      if (-not $Spec) { throw "playwright mode needs -Spec" }
      $specAbs = Resolve-RepoPath -Path $Spec -RepoRoot $RepoRoot
      if (-not (Test-Path -LiteralPath $specAbs)) { throw "spec not found: $specAbs" }
      $workdir = Resolve-RepoPath -Path $Cwd -RepoRoot $RepoRoot
      if (-not (Test-Path -LiteralPath $workdir)) { $workdir = $RepoRoot }

      $sw = [System.Diagnostics.Stopwatch]::StartNew()
      Push-Location $workdir
      try {
        $output = & npx playwright test $specAbs --reporter=json 2>&1
        $exit = $LASTEXITCODE
      } finally {
        Pop-Location
      }
      $sw.Stop()
      $text = ($output | Out-String)

      $stats = $null
      $start = $text.IndexOf("{")
      if ($start -ge 0) {
        try { $stats = $text.Substring($start) | ConvertFrom-Json } catch { }
      }

      $details.spec = $Spec
      $details.exit_code = $exit
      $details.wall_ms = (Format-Number $sw.Elapsed.TotalMilliseconds 1)
      if ($stats -and $stats.stats) {
        $details.duration_ms = $stats.stats.duration
        $details.expected = $stats.stats.expected
        $details.unexpected = $stats.stats.unexpected
        $value = Format-Number $stats.stats.duration 0
        $details.metric_source = "playwright stats.duration (ms)"
      } else {
        $value = Format-Number $sw.Elapsed.TotalMilliseconds 0
        $details.metric_source = "spec wall-clock (ms)"
      }
      if ($exit -ne 0) { $details.error = "playwright exit code $exit" }
    }
    "slint" {
      $details.supported = $true
      $details.note = "native Slint frame metrics need instrumentation in frontend-slint (todo T-chat-native)"
    }
    "none" {
      $details.supported = $false
      $details.note = "chat page metric is not applicable to this form"
    }
  }
} catch {
  $details.error = $_.Exception.Message
}

Write-BenchOutput -FormId $FormId -Collector "chat" -OutJson $OutJson -Records @(
  (New-BenchRecord -Metric "chat_page_perf" -Value $value -Unit "ms" -Source $Mode -Details $details)
)