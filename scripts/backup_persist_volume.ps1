param(
    # Docker Compose file path. If relative, it's resolved relative to the repo root.
    [string]$ComposeFile = "docker-compose-x86.yml",

    # Optional override. If not provided, the script derives it from `docker compose config` or COMPOSE_PROJECT_NAME.
    [string]$ProjectName = "",

    # Output directory (relative paths are resolved relative to the repo root).
    [string]$OutDir = "backups",

    # Compose volume names to back up (default: runtime volumes).
    [string[]]$ComposeVolumes = @("wunder_logs", "wunder_workspaces"),

    # Image used to run tar (already required by this repo's compose stack).
    [string]$TarImage = "postgres:16",

    # Do not stop the compose stack before backup (not recommended for DB consistency).
    [switch]$NoStop,

    # Do not start the compose stack after backup.
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

function Get-ComposeVolumeName([string]$Project, [string]$ComposeVolumeName) {
    $names = & docker volume ls `
        --filter "label=com.docker.compose.project=$Project" `
        --filter "label=com.docker.compose.volume=$ComposeVolumeName" `
        --format "{{.Name}}"

    if ($LASTEXITCODE -ne 0) {
        throw "Failed to list docker volumes (project=$Project, volume=$ComposeVolumeName)."
    }

    $items = @($names | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($items.Count -eq 0) {
        throw "Compose volume not found: project=$Project volume=$ComposeVolumeName. Run docker compose up once to create it."
    }
    if ($items.Count -gt 1) {
        throw "Multiple volumes found for project=$Project volume=$($ComposeVolumeName): $($items -join ', ')."
    }

    return $items[0].Trim()
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "docker not found in PATH"
}

if ($ComposeVolumes.Count -eq 0) {
    throw "ComposeVolumes is empty"
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
        $actual = Get-ComposeVolumeName $ProjectName $name
        $resolvedVolumes += [pscustomobject]@{
            Compose = $name
            Actual  = $actual
        }
    }

    if ($resolvedVolumes.Count -eq 0) {
        throw "No valid compose volumes to back up"
    }

    $outDirAbs = Resolve-PathFromRepoRoot $repoRoot $OutDir
    New-Item -ItemType Directory -Force -Path $outDirAbs | Out-Null

    $ts = Get-Date -Format "yyyyMMdd-HHmmss"
    $archiveName = "wunder-volumes-$ProjectName-$ts.tgz"
    $archivePath = Join-Path $outDirAbs $archiveName

    if (-not $NoStop) {
        Write-Host "Stopping compose stack (project=$ProjectName)..." -ForegroundColor Cyan
        & docker compose --project-name $ProjectName -f $composePath down
        if ($LASTEXITCODE -ne 0) {
            throw "docker compose down failed"
        }
    }

    Write-Host "Backing up compose volumes: $($resolvedVolumes.Compose -join ', ')" -ForegroundColor Cyan

    $dockerArgs = @("run", "--rm")
    foreach ($item in $resolvedVolumes) {
        $dockerArgs += "-v"
        $dockerArgs += "$($item.Actual):/from/$($item.Compose)"
    }
    $dockerArgs += "-v"
    $dockerArgs += "${outDirAbs}:/backup"
    $dockerArgs += "--user"
    $dockerArgs += "0"
    $dockerArgs += $TarImage
    $dockerArgs += "bash"
    $dockerArgs += "-lc"
    $dockerArgs += "set -euo pipefail; cd /from && tar -czf /backup/$archiveName ."

    & docker @dockerArgs
    if ($LASTEXITCODE -ne 0) {
        throw "docker run (tar) failed"
    }

    $manifest = [ordered]@{
        project_name    = $ProjectName
        created_at      = (Get-Date).ToString("o")
        compose_volumes = @($resolvedVolumes | ForEach-Object { $_.Compose })
        actual_volumes  = @($resolvedVolumes | ForEach-Object { $_.Actual })
    }
    $manifestPath = "$archivePath.manifest.json"
    $manifest | ConvertTo-Json -Depth 8 | Set-Content -Path $manifestPath -Encoding utf8

    $hash = (Get-FileHash -Algorithm SHA256 -Path $archivePath).Hash.ToLowerInvariant()
    Set-Content -Path "$archivePath.sha256" -Value $hash -NoNewline -Encoding ascii

    Write-Host "Backup OK" -ForegroundColor Green
    Write-Host "Archive : $archivePath"
    Write-Host "Manifest: $manifestPath"
    Write-Host "SHA256  : $hash"

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
