[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$BaseCommit,
    [Parameter(Mandatory = $true, Position = 1)]
    [string]$OutputDir,
    [switch]$KeepExisting
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [Console]::OutputEncoding

function Get-NormalizedFullPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    return [System.IO.Path]::GetFullPath($Path).TrimEnd('\')
}

function Resolve-VerifiedCommit {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Revision
    )

    $trimmed = $Revision.Trim()
    if ([string]::IsNullOrWhiteSpace($trimmed)) {
        throw 'Base commit cannot be empty.'
    }

    if ($trimmed -match '^[0-9a-fA-F]+$' -and $trimmed.Length -ne 40) {
        throw "Base commit looks like a raw SHA-1 but length is $($trimmed.Length), expected 40: $trimmed"
    }

    $resolved = & git rev-parse --quiet --verify "$trimmed`^{commit}" 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace(($resolved | Select-Object -First 1))) {
        throw "Base commit not found or is not a commit object: $trimmed"
    }

    return ($resolved | Select-Object -First 1).Trim()
}

$repoRoot = (Get-Location).Path
$repoRootFull = Get-NormalizedFullPath -Path $repoRoot

$resolvedBaseCommit = Resolve-VerifiedCommit -Revision $BaseCommit

$outputFull = Get-NormalizedFullPath -Path $OutputDir
if ($outputFull -eq $repoRootFull) {
    throw 'OutputDir cannot be the repository root.'
}
if ($outputFull.StartsWith("$repoRootFull\", [System.StringComparison]::OrdinalIgnoreCase)) {
    throw 'OutputDir cannot be inside the repository root.'
}

if (Test-Path -LiteralPath $outputFull) {
    if (-not (Test-Path -LiteralPath $outputFull -PathType Container)) {
        throw "Output path exists and is not a directory: $outputFull"
    }
    if (-not $KeepExisting) {
        Get-ChildItem -LiteralPath $outputFull -Force | Remove-Item -Recurse -Force
    }
} else {
    New-Item -ItemType Directory -Path $outputFull -Force | Out-Null
}

$statusLines = git -c core.quotepath=false diff --name-status --find-renames "$resolvedBaseCommit^" HEAD
if ($LASTEXITCODE -ne 0) {
    throw 'git diff failed'
}

$copyPaths = New-Object System.Collections.Generic.List[string]
$deletePaths = New-Object System.Collections.Generic.List[string]

foreach ($line in $statusLines) {
    if ([string]::IsNullOrWhiteSpace($line)) {
        continue
    }
    $parts = $line -split "`t"
    $status = $parts[0]
    if ($status.StartsWith('R') -or $status.StartsWith('C')) {
        if ($parts.Length -lt 3) {
            throw "Unexpected rename/copy status line: $line"
        }
        if ($status.StartsWith('R')) {
            $deletePaths.Add($parts[1])
        }
        $copyPaths.Add($parts[2])
        continue
    }
    if ($parts.Length -lt 2) {
        throw "Unexpected status line: $line"
    }
    switch -Regex ($status) {
        '^D' {
            $deletePaths.Add($parts[1])
            continue
        }
        '^(A|M|T|U)$' {
            $copyPaths.Add($parts[1])
            continue
        }
        default {
            throw "Unhandled git status: $line"
        }
    }
}

$copiedCount = 0
foreach ($relative in ($copyPaths | Sort-Object -Unique)) {
    $source = Join-Path $repoRootFull ($relative -replace '/', '\')
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Source file missing: $source"
    }
    $destination = Join-Path $outputFull ($relative -replace '/', '\')
    $destinationDir = Split-Path -Parent $destination
    if (-not (Test-Path -LiteralPath $destinationDir -PathType Container)) {
        New-Item -ItemType Directory -Path $destinationDir -Force | Out-Null
    }
    Copy-Item -LiteralPath $source -Destination $destination -Force
    $copiedCount++
}

$deleteListPath = Join-Path $outputFull '__deleted_files.txt'
$manifestPath = Join-Path $outputFull '__patch_manifest.txt'

$uniqueDeletePaths = $deletePaths | Sort-Object -Unique
$deleteContent = if ($uniqueDeletePaths.Count -gt 0) {
    ($uniqueDeletePaths -join "`r`n")
} else {
    ''
}
[System.IO.File]::WriteAllText($deleteListPath, $deleteContent, [System.Text.UTF8Encoding]::new($false))

$manifestLines = @(
    "base_commit_inclusive=$resolvedBaseCommit",
    "comparison=$resolvedBaseCommit^..HEAD",
    "export_source=$repoRootFull",
    "export_target=$outputFull",
    "copied_files=$copiedCount",
    "deleted_files=$($uniqueDeletePaths.Count)",
    "generated_at=$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
)
[System.IO.File]::WriteAllText($manifestPath, ($manifestLines -join "`r`n"), [System.Text.UTF8Encoding]::new($false))

Write-Host "Export completed: $outputFull"
Write-Host "Copied files : $copiedCount"
Write-Host "Deleted files: $($uniqueDeletePaths.Count)"
Write-Host "Manifest     : $manifestPath"
Write-Host "Delete list  : $deleteListPath"
