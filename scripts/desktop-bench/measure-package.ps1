# measure-package.ps1 - Package size metrics (P1-P3, P5).
# Accepts an installer file or an installed/extracted directory.
param(
  [Parameter(Mandatory = $true)][string]$Target,
  [string]$Label = "package",
  [string]$OutJson = ""
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Target)) {
  Write-Error "Target not found: $Target"
  exit 1
}

$item = Get-Item $Target
$report = [ordered]@{
  kind = "package"
  label = $Label
  target = $item.FullName
  captured_at = (Get-Date).ToString("o")
}

if ($item.PSIsContainer) {
  $files = Get-ChildItem -Path $item.FullName -Recurse -File -Force
  $totalBytes = ($files | Measure-Object -Property Length -Sum).Sum
  if (-not $totalBytes) { $totalBytes = 0 }
  $report.target_type = "directory"
  $report.total_size_mb = [math]::Round($totalBytes / 1MB, 2)
  $report.file_count = $files.Count
  $report.exe_count = @($files | Where-Object { $_.Extension -ieq ".exe" }).Count
  $report.dll_count = @($files | Where-Object { $_.Extension -ieq ".dll" }).Count
  $report.node_count = @($files | Where-Object { $_.Extension -ieq ".node" }).Count
  $report.asar_count = @($files | Where-Object { $_.Extension -ieq ".asar" }).Count
  $top = $files | Sort-Object Length -Descending | Select-Object -First 10
  $report.top10_files = @($top | ForEach-Object {
    [ordered]@{ path = $_.FullName.Substring($item.FullName.Length + 1); size_mb = [math]::Round($_.Length / 1MB, 2) }
  })
} else {
  $report.target_type = "file"
  $report.installer_size_mb = [math]::Round($item.Length / 1MB, 2)
}

$obj = [pscustomobject]$report
if ($OutJson -ne "") {
  $obj | ConvertTo-Json -Depth 4 | Out-File -FilePath $OutJson -Encoding utf8
  Write-Host "json written: $OutJson"
}
if ($obj.target_type -eq "directory") {
  Write-Host ("dir total={0}MB files={1} exe={2} dll={3} node={4} asar={5}" -f $obj.total_size_mb, $obj.file_count, $obj.exe_count, $obj.dll_count, $obj.node_count, $obj.asar_count)
} else {
  Write-Host ("installer size={0}MB" -f $obj.installer_size_mb)
}
