<#
  form-bench shared helpers.

  Dot-source from collectors:
      . (Join-Path $PSScriptRoot "common.ps1")

  Notes:
    * All JSON is read/written as UTF-8 without BOM. Windows PowerShell 5.1
      defaults to the system ANSI code page (GBK on zh-CN) which corrupts
      UTF-8 payloads, so never use Get-Content/Set-Content for JSON here.
    * This file is intentionally ASCII-only so it parses identically under
      any code page.
#>

$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

function Get-RepoRoot {
  param([Parameter(Mandatory = $true)][string]$ScriptRoot)
  # <repo>\scripts\form-bench -> <repo>
  $formBench = Split-Path -Parent $ScriptRoot
  return (Split-Path -Parent $formBench)
}

function Get-FormBenchRoot {
  param([Parameter(Mandatory = $true)][string]$ScriptRoot)
  # collectors live directly in form-bench, so the root is the script dir itself
  return $ScriptRoot
}

function Resolve-RepoPath {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$RepoRoot
  )
  if ([System.IO.Path]::IsPathRooted($Path)) { return $Path }
  return (Join-Path $RepoRoot $Path)
}

# ---------------------------------------------------------------------------
# UTF-8 IO
# ---------------------------------------------------------------------------

function Read-Utf8Text {
  param([Parameter(Mandatory = $true)][string]$Path)
  return [System.IO.File]::ReadAllText($Path, [System.Text.Encoding]::UTF8)
}

function Write-Utf8File {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [AllowEmptyString()][string]$Text = ""
  )
  $parent = Split-Path -Parent $Path
  if ($parent -and -not (Test-Path -LiteralPath $parent)) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
  }
  $enc = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Text, $enc)
}

function Read-JsonFile {
  param([Parameter(Mandatory = $true)][string]$Path)
  return ((Read-Utf8Text -Path $Path) | ConvertFrom-Json)
}

function Write-JsonFile {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)]$Object,
    [int]$Depth = 16
  )
  $json = $Object | ConvertTo-Json -Depth $Depth
  Write-Utf8File -Path $Path -Text ($json + "`n")
}

# ---------------------------------------------------------------------------
# Result envelope
# ---------------------------------------------------------------------------

function New-BenchRecord {
  param(
    [Parameter(Mandatory = $true)][string]$Metric,
    $Value,
    [string]$Unit = "",
    [string]$Source = "",
    [string]$Command = "",
    $Details = $null
  )
  return [ordered]@{
    metric  = $Metric
    value   = $Value
    unit    = $Unit
    source  = $Source
    command = $Command
    details = $Details
  }
}

function Write-BenchOutput {
  param(
    [Parameter(Mandatory = $true)][string]$FormId,
    [Parameter(Mandatory = $true)][string]$Collector,
    [Parameter(Mandatory = $true)][array]$Records,
    [string]$OutJson = ""
  )
  $envelope = [ordered]@{
    form      = $FormId
    collector = $Collector
    generated = (Get-Date).ToUniversalTime().ToString("o")
    records   = @($Records)
  }
  $json = $envelope | ConvertTo-Json -Depth 16
  if ($OutJson) { Write-Utf8File -Path $OutJson -Text ($json + "`n") }
  Write-Output $json
}

function Format-Number {
  param($Value, [int]$Digits = 2)
  if ($null -eq $Value) { return $null }
  return [math]::Round([double]$Value, $Digits)
}

# ---------------------------------------------------------------------------
# Process / network probes
# ---------------------------------------------------------------------------

function Test-TcpPort {
  param(
    [Parameter(Mandatory = $true)][string]$HostName,
    [Parameter(Mandatory = $true)][int]$Port,
    [int]$TimeoutMs = 500
  )
  $client = New-Object System.Net.Sockets.TcpClient
  try {
    $iar = $client.BeginConnect($HostName, $Port, $null, $null)
    if ($iar.AsyncWaitHandle.WaitOne($TimeoutMs)) {
      $client.EndConnect($iar)
      return $true
    }
    return $false
  } catch {
    return $false
  } finally {
    $client.Close()
  }
}

function Stop-ProcessTree {
  param([Parameter(Mandatory = $true)][int]$ProcessId)
  try {
    & taskkill /PID $ProcessId /T /F 2>$null | Out-Null
  } catch {
    # best effort
  }
}

function Get-PythonExe {
  param([string]$Python = "python")
  return $Python
}