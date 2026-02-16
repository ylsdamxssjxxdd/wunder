param(
    # Path to the backup archive (*.tgz / *.tar.gz). If relative, it's resolved relative to the current directory.
    [Parameter(Mandatory = $true)]
    [string]$InFile,

    # Docker Compose file path. If relative, it's resolved relative to the repo root.
    [string]$ComposeFile = "docker-compose-x86.yml",

    # Optional override. If not provided, the script derives it from `docker compose config` or COMPOSE_PROJECT_NAME.
    [string]$ProjectName = "",

    # Compose volume names to restore (default: runtime volumes).
    [string[]]$ComposeVolumes = @("wunder_logs", "wunder_workspaces"),

    # Image used to run tar (already required by this repo's compose stack).
    [string]$TarImage = "postgres:16",

    # Skip SHA256 verification even if <archive>.sha256 exists.
    [switch]$SkipHashCheck,

    # Do not stop the compose stack before restore.
    [switch]$NoStop,

    # Do not start the compose stack after restore.
    [switch]$NoStart
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-RepoRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
}

function Resolve-PathFromRepoRoot([string]$RepoRoot, [string]$PathLike) {
    if ([string]::IsNullOrWhiteSpace($PathLike)) {
        throw "Empty path"
    }

    if ([System.IO.Path]::IsPathRooted($PathLike)) {
        return $PathLike
    }

    return (Join-Path $RepoRoot $PathLike)
}

function Get-ComposeProjectName([string]$ComposePath) {
    if (-not [string]::IsNullOrWhiteSpace($env:COMPOSE_PROJECT_NAME)) {
        return $env:COMPOSE_PROJECT_NAME.Trim()
    }

    $configLines = & docker compose -f $ComposePath config 2>$null
    if ($LASTEXITCODE -eq 0) {
        $m = $configLines | Select-String -Pattern '^\s*name:\s*([^\s#]+)\s*$' | Select-Object -First 1
        if ($m) {
            return $m.Matches[0].Groups[1].Value.Trim()
        }
    }

    return (Split-Path -Leaf (Get-Location))
}

function Get-ComposeVolumeNameOrEmpty([string]$Project, [string]$ComposeVolumeName) {
    $names = & docker volume ls `
        --filter "label=com.docker.compose.project=$Project" `
        --filter "label=com.docker.compose.volume=$ComposeVolumeName" `
        --format "{{.Name}}"

    if ($LASTEXITCODE -ne 0) {
        throw "Failed to list docker volumes (project=$Project, volume=$ComposeVolumeName)."
    }

    $items = @($names | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($items.Count -eq 0) {
        return ""
    }
    if ($items.Count -gt 1) {
        throw "Multiple volumes found for project=$Project volume=$($ComposeVolumeName): $($items -join ', ')."
    }

    return $items[0].Trim()
}

function Ensure-ComposeVolume([string]$Project, [string]$ComposeVolumeName) {
    $existing = Get-ComposeVolumeNameOrEmpty $Project $ComposeVolumeName
    if (-not [string]::IsNullOrWhiteSpace($existing)) {
        return $existing
    }

    $name = "${Project}_${ComposeVolumeName}"
    Write-Host "Compose volume not found. Creating volume: $name" -ForegroundColor Yellow
    & docker volume create `
        --label "com.docker.compose.project=$Project" `
        --label "com.docker.compose.volume=$ComposeVolumeName" `
        $name | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "docker volume create failed: $name"
    }
    return $name
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "docker not found in PATH"
}

if ($ComposeVolumes.Count -eq 0) {
    throw "ComposeVolumes is empty"
}

$archivePath = (Resolve-Path $InFile).Path
$archiveName = Split-Path -Leaf $archivePath
$archiveDir = Split-Path -Parent $archivePath

if (-not (Test-Path -LiteralPath $archivePath)) {
    throw "Archive not found: $archivePath"
}

if (-not $SkipHashCheck) {
    $hashPath = "$archivePath.sha256"
    if (Test-Path -LiteralPath $hashPath) {
        $expected = (Get-Content -LiteralPath $hashPath -Raw).Trim().ToLowerInvariant()
        $actual = (Get-FileHash -Algorithm SHA256 -Path $archivePath).Hash.ToLowerInvariant()
        if ($expected -ne $actual) {
            throw "SHA256 mismatch. expected=$expected actual=$actual"
        }
        Write-Host "SHA256 OK: $actual" -ForegroundColor Green
    }
}

$repoRoot = Resolve-RepoRoot
$composePath = (Resolve-Path (Resolve-PathFromRepoRoot $repoRoot $ComposeFile)).Path

Push-Location $repoRoot
try {
    if ([string]::IsNullOrWhiteSpace($ProjectName)) {
        $ProjectName = Get-ComposeProjectName $composePath
    }

    $resolvedVolumes = @()
    foreach ($composeVolume in $ComposeVolumes) {
        $name = $composeVolume.Trim()
        if ([string]::IsNullOrWhiteSpace($name)) {
            continue
        }
        $actual = Ensure-ComposeVolume $ProjectName $name
        $resolvedVolumes += [pscustomobject]@{
            Compose = $name
            Actual  = $actual
        }
    }

    if ($resolvedVolumes.Count -eq 0) {
        throw "No valid compose volumes to restore"
    }

    if (-not $NoStop) {
        Write-Host "Stopping compose stack (project=$ProjectName)..." -ForegroundColor Cyan
        & docker compose --project-name $ProjectName -f $composePath down
        if ($LASTEXITCODE -ne 0) {
            throw "docker compose down failed"
        }
    }

    Write-Host "Restoring compose volumes: $($resolvedVolumes.Compose -join ', ')" -ForegroundColor Cyan

    $dockerArgs = @("run", "--rm")
    foreach ($item in $resolvedVolumes) {
        $dockerArgs += "-v"
        $dockerArgs += "$($item.Actual):/to/$($item.Compose)"
    }
    $dockerArgs += "-v"
    $dockerArgs += "${archiveDir}:/backup"
    $dockerArgs += "--user"
    $dockerArgs += "0"
    $dockerArgs += $TarImage
    $dockerArgs += "bash"
    $dockerArgs += "-lc"
    $dockerArgs += "set -euo pipefail; rm -rf /to/* /to/.[!.]* /to/..?* 2>/dev/null || true; cd /to && tar -xzf /backup/$archiveName"

    & docker @dockerArgs
    if ($LASTEXITCODE -ne 0) {
        throw "docker run (untar) failed"
    }

    Write-Host "Restore OK" -ForegroundColor Green

    if (-not $NoStart) {
        Write-Host "Starting compose stack..." -ForegroundColor Cyan
        & docker compose --project-name $ProjectName -f $composePath up -d
        if ($LASTEXITCODE -ne 0) {
            throw "docker compose up failed"
        }
    }
} finally {
    Pop-Location
}

