param(
  [string]$Version = "",
  [string]$DesktopExe = "",
  [string]$CliExe = "",
  [string]$Supplement = "",
  [string]$OutputDir = "",
  [switch]$CoreOnly,
  [switch]$NoDownload,
  [string]$Iscc = "",
  [string]$Compression = "lzma2/normal"
)

# Build the per-user Windows installer. Inputs are the release desktop/CLI
# artifacts plus the optional supplement archive; nothing is rebuilt here.
$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$installerRoot = Join-Path $repoRoot "packaging\windows\installer"
$stageRoot = Join-Path $repoRoot "temp_dir\installer\win7-x86"
# Single source of truth for the supplement component order. The [Code] section
# of wunder.iss indexes its ComponentTasks/ComponentLabels arrays in exactly
# this order, so a component added here must also bump ComponentCount and the
# array bounds there.
$ComponentOrder = @("python", "git", "rg", "ffmpeg", "7zip", "powershell", "browser")
# Inno parses task names as expressions, so a name may not start with a digit.
$ComponentTaskNames = @{ "7zip" = "sevenzip" }

function Get-TaskName([string]$Component) {
  if ($ComponentTaskNames.ContainsKey($Component)) { return $ComponentTaskNames[$Component] }
  return $Component
}
$win7BuilderRoot = if ($env:WUNDER_WIN7_BUILDER_ROOT) {
  $env:WUNDER_WIN7_BUILDER_ROOT
} elseif ($env:WUNDER_BUILDER_ROOT) {
  $env:WUNDER_BUILDER_ROOT
} else {
  Join-Path (Split-Path $repoRoot -Parent) "Rust-builder\win7"
}
$offlineRoot = Join-Path $win7BuilderRoot "offline"
$innoVersion = "6.7.3"
$innoUrl = "https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe"
$innoSha256 = "9C73C3BAE7ED48D44112A0F48E66742C00090BDB5BEF71D9D3C056C66E97B732"

function Require-Path([string]$Path, [string]$Label) {
  if (-not (Test-Path -LiteralPath $Path)) { throw "$Label is missing: $Path" }
}

function Get-Sha256Hex([string]$Path) {
  # Get-FileHash is a module cmdlet and disappears on hosts with a trimmed
  # PSModulePath; the .NET provider is always available.
  $stream = [IO.File]::OpenRead($Path)
  try {
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
      ($sha.ComputeHash($stream) | ForEach-Object { $_.ToString('X2') }) -join ''
    } finally {
      $sha.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

function Resolve-RepoPath([string]$Path) {
  if (-not $Path) { return "" }
  if ([IO.Path]::IsPathRooted($Path)) { return [IO.Path]::GetFullPath($Path) }
  return [IO.Path]::GetFullPath((Join-Path $repoRoot $Path))
}

function Read-CargoVersion([string]$ManifestPath) {
  $text = Get-Content -LiteralPath $ManifestPath -Raw
  $match = [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"')
  if (-not $match.Success) { throw "Could not read a package version from: $ManifestPath" }
  return $match.Groups[1].Value
}

function Get-PeMachine([string]$Path) {
  $stream = [IO.File]::OpenRead($Path)
  try {
    $reader = New-Object IO.BinaryReader($stream)
    if ($reader.ReadUInt16() -ne 0x5A4D) { throw "Not a PE image: $Path" }
    $stream.Position = 0x3C
    $peOffset = $reader.ReadInt32()
    if ($peOffset -le 0 -or $peOffset -ge $stream.Length - 6) { throw "Invalid PE header offset: $Path" }
    $stream.Position = $peOffset
    if ($reader.ReadUInt32() -ne 0x00004550) { throw "Missing PE signature: $Path" }
    return $reader.ReadUInt16()
  } finally {
    $stream.Dispose()
  }
}

function Get-NumericVersion([string]$VersionText) {
  $parts = @($VersionText -split '[^0-9]+' | Where-Object { $_ -ne '' } | Select-Object -First 3)
  while ($parts.Count -lt 3) { $parts += '0' }
  return (($parts | ForEach-Object { [int]$_ }) -join '.') + '.0'
}

function Find-Supplement([string]$Root) {
  $directories = @(
    (Join-Path $Root "dist\win7-x86"),
    (Join-Path $Root "target\installer\payload\win7-x86"),
    (Join-Path $Root "temp_dir\win7-gnu-lab\win7-supplement\dist"),
    (Join-Path $Root "temp_dir\win7-supplement\dist")
  )
  $found = @()
  foreach ($directory in $directories) {
    if (-not (Test-Path -LiteralPath $directory -PathType Container)) { continue }
    $found += @(Get-ChildItem -LiteralPath $directory -Filter "wunder-supplement-win7-*.zip" -File -ErrorAction SilentlyContinue)
  }
  $found = @($found | Sort-Object -Property FullName -Unique)
  if ($found.Count -eq 0) { return "" }
  if ($found.Count -gt 1) {
    $list = ($found | ForEach-Object { $_.FullName }) -join [Environment]::NewLine
    throw "Multiple supplement archives were found. Pass -Supplement explicitly:$([Environment]::NewLine)$list"
  }
  return $found[0].FullName
}

function Get-ZipSupplementInfo([string]$Path) {
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $known = $ComponentOrder
  $bytes = @{}
  $files = @{}
  $unknown = @{}
  foreach ($name in $known) { $bytes[$name] = [int64]0; $files[$name] = 0 }
  $meta = $null
  $zip = [IO.Compression.ZipFile]::OpenRead($Path)
  try {
    foreach ($entry in $zip.Entries) {
      $name = $entry.FullName.Replace('\', '/')
      if ($name.StartsWith('/') -or $name -match '(^|/)\.\.(/|$)') {
        throw "Supplement archive contains an unsafe path: $name"
      }
      $parts = $name.Split('/')
      if ($parts.Count -ge 3 -and $parts[0] -eq 'opt') {
        if ($known -notcontains $parts[1]) {
          $unknown[$parts[1]] = $true
          continue
        }
        $component = $parts[1]
        if (-not $name.EndsWith('/')) { $files[$component] += 1 }
        $bytes[$component] += [int64]$entry.Length
      }
      if ($name -eq 'wunder-win7-supplement.json') {
        $reader = New-Object IO.StreamReader($entry.Open(), [Text.Encoding]::UTF8)
        try { $meta = ($reader.ReadToEnd() | ConvertFrom-Json) } finally { $reader.Dispose() }
      }
    }
  } finally {
    $zip.Dispose()
  }
  if ($unknown.Count -gt 0) {
    throw ("Supplement contains components the installer does not declare (they would be skipped silently): " +
      (($unknown.Keys | Sort-Object) -join ', '))
  }
  return @{ Bytes = $bytes; Files = $files; Metadata = $meta }
}

function Get-DirectorySupplementInfo([string]$Path) {
  $known = $ComponentOrder
  $bytes = @{}
  $files = @{}
  $optRoot = Join-Path $Path "opt"
  if (Test-Path -LiteralPath $optRoot -PathType Container) {
    $unknown = @(Get-ChildItem -LiteralPath $optRoot -Directory |
      Where-Object { $known -notcontains $_.Name } |
      ForEach-Object { $_.Name } | Sort-Object)
    if ($unknown.Count -gt 0) {
      throw ("Supplement contains components the installer does not declare (they would be skipped silently): " +
        ($unknown -join ', '))
    }
  }
  foreach ($name in $known) {
    $bytes[$name] = [int64]0
    $files[$name] = 0
    $directory = Join-Path $Path "opt\$name"
    if (-not (Test-Path -LiteralPath $directory -PathType Container)) { continue }
    $items = @(Get-ChildItem -LiteralPath $directory -Recurse -File -ErrorAction SilentlyContinue)
    $files[$name] = $items.Count
    foreach ($item in $items) { $bytes[$name] += $item.Length }
  }
  $meta = $null
  $metaPath = Join-Path $Path "wunder-win7-supplement.json"
  if (Test-Path -LiteralPath $metaPath) {
    $meta = Get-Content -LiteralPath $metaPath -Raw -Encoding UTF8 | ConvertFrom-Json
  }
  return @{ Bytes = $bytes; Files = $files; Metadata = $meta }
}

function Get-MetadataVersion($Metadata, [string]$Name) {
  if ($null -eq $Metadata) { return "" }
  $entry = $Metadata.PSObject.Properties[$Name]
  if ($null -eq $entry -or $null -eq $entry.Value) { return "" }
  $version = $entry.Value.PSObject.Properties['version']
  if ($null -eq $version -or $null -eq $version.Value) { return "" }
  return [string]$version.Value
}

function Install-SupplementStage([string]$Archive, [string]$Destination) {
  $hash = (Get-Sha256Hex $Archive).ToLowerInvariant()
  $marker = Join-Path $Destination ".wunder-pack.json"
  if (Test-Path -LiteralPath $marker -PathType Leaf) {
    try {
      $record = Get-Content -LiteralPath $marker -Raw -Encoding UTF8 | ConvertFrom-Json
      if ($record.sha256 -eq $hash -and (Test-Path -LiteralPath (Join-Path $Destination "opt") -PathType Container)) {
        Write-Host "[wunder-pack] reusing supplement stage: $Destination"
        return
      }
    } catch {
      Write-Host "[wunder-pack] stale supplement stage marker ignored"
    }
  }

  $fullDestination = [IO.Path]::GetFullPath($Destination)
  $fullStage = [IO.Path]::GetFullPath($stageRoot)
  if (-not $fullDestination.StartsWith($fullStage, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to replace a supplement stage outside the pack staging root: $fullDestination"
  }

  $temporary = "$Destination.tmp"
  if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Recurse -Force }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  [IO.Compression.ZipFile]::ExtractToDirectory($Archive, $temporary)
  $record = [ordered]@{
    sha256 = $hash
    source = (Split-Path -Leaf $Archive)
    extractedAt = (Get-Date).ToUniversalTime().ToString("o")
  } | ConvertTo-Json
  [IO.File]::WriteAllText((Join-Path $temporary ".wunder-pack.json"), $record + "`r`n", (New-Object Text.UTF8Encoding $false))
  if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Recurse -Force }
  Move-Item -LiteralPath $temporary -Destination $Destination
}

function Download-File([string]$Url, [string]$Destination) {
  $directory = Split-Path -Parent $Destination
  New-Item -ItemType Directory -Force -Path $directory | Out-Null
  $previous = $ProgressPreference
  $ProgressPreference = 'SilentlyContinue'
  try {
    for ($attempt = 1; $attempt -le 3; $attempt++) {
      try {
        Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing -MaximumRedirection 10
        if ((Test-Path -LiteralPath $Destination) -and ((Get-Item -LiteralPath $Destination).Length -gt 0)) { return }
      } catch {
        if ($attempt -eq 3) { throw }
        Start-Sleep -Seconds 2
      }
    }
  } finally {
    $ProgressPreference = $previous
  }
  throw "Download failed: $Url"
}

function Find-Iscc([string]$Explicit) {
  $candidates = @()
  if ($Explicit) { $candidates += (Resolve-RepoPath $Explicit) }
  if ($env:WUNDER_ISCC) { $candidates += (Resolve-RepoPath $env:WUNDER_ISCC) }
  $candidates += (Join-Path $offlineRoot "tools\inno-setup\app\ISCC.exe")
  $candidates += (Join-Path $offlineRoot "tools\inno-setup\ISCC.exe")
  $candidates += (Join-Path $repoRoot "temp_dir\tools\inno-setup\app\ISCC.exe")
  if (${env:ProgramFiles(x86)}) { $candidates += (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe") }
  if ($env:ProgramFiles) { $candidates += (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe") }
  $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
  if ($command -and $command.Source) { $candidates += $command.Source }
  foreach ($candidate in $candidates) {
    if (Test-Path -LiteralPath $candidate -PathType Leaf) { return (Resolve-Path -LiteralPath $candidate).Path }
  }
  return ""
}

function Ensure-Iscc([string]$Explicit, [bool]$AllowDownload) {
  $iscc = Find-Iscc $Explicit
  if ($iscc) { return $iscc }
  if (-not $AllowDownload) {
    throw "Inno Setup ISCC.exe was not found. Place the offline compiler at $offlineRoot\tools\inno-setup\app\ISCC.exe, install Inno Setup $innoVersion, or pass -Iscc."
  }
  Write-Host "[wunder-pack] Inno Setup $innoVersion was not found in the offline SDK; preparing it in temp_dir\tools\inno-setup (one-time)"
  $toolRoot = Join-Path $repoRoot "temp_dir\tools\inno-setup"
  $archive = Join-Path $toolRoot "innosetup-$innoVersion.exe"
  New-Item -ItemType Directory -Force -Path $toolRoot | Out-Null
  $valid = $false
  if (Test-Path -LiteralPath $archive -PathType Leaf) {
    $valid = ((Get-Sha256Hex $archive) -eq $innoSha256)
    if (-not $valid) { Remove-Item -LiteralPath $archive -Force }
  }
  if (-not $valid) {
    Write-Host "[wunder-pack] downloading $innoUrl"
    Download-File $innoUrl $archive
    $actual = Get-Sha256Hex $archive
    if ($actual -ne $innoSha256) { throw "Inno Setup SHA256 mismatch: expected $innoSha256, got $actual" }
  }
  $installDir = Join-Path $toolRoot "app"
  $isccPath = Join-Path $installDir "ISCC.exe"
  if (-not (Test-Path -LiteralPath $isccPath -PathType Leaf)) {
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    & $archive /CURRENTUSER /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- "/DIR=$installDir"
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup extraction failed with exit code $LASTEXITCODE" }
  }
  if (-not (Test-Path -LiteralPath $isccPath -PathType Leaf)) {
    throw "Inno Setup did not produce ISCC.exe under: $installDir"
  }
  return (Resolve-Path -LiteralPath $isccPath).Path
}

function Read-AppVersion([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) { throw "App version file was not found: $Path" }
  $payload = Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
  if (-not $payload.version) { throw "Missing version in: $Path" }
  return [string]$payload.version
}

$appVersion = Read-AppVersion (Join-Path $repoRoot "config\app_version.json")
$version = if ($Version) { $Version } else { $appVersion }
$workspaceVersion = Read-CargoVersion (Join-Path $repoRoot "Cargo.toml")
$frontendVersion = Read-CargoVersion (Join-Path $repoRoot "frontend-slint\Cargo.toml")
if ($workspaceVersion -ne $version -or $frontendVersion -ne $version) {
  throw "Version mismatch: app_version=$version workspace=$workspaceVersion frontend-slint=$frontendVersion. Run scripts/sync_app_version.py and rebuild both artifacts from one revision."
}
$numericVersion = Get-NumericVersion $version
$outputBase = "wunder-desktop-$version-win7-x86-setup"

$desktopPath = if ($DesktopExe) { Resolve-RepoPath $DesktopExe } else {
  Join-Path $repoRoot "target\frontend-slint\dist\win7-x86\wunder-desktop-$version-win7-x86.exe"
}
$cliPath = if ($CliExe) { Resolve-RepoPath $CliExe } else {
  Join-Path $repoRoot "target\cli\dist\win7-x86\wunder-cli-$version-win7-x86.exe"
}
$supplementPath = if ($Supplement) { Resolve-RepoPath $Supplement } elseif ($CoreOnly) { "" } else { Find-Supplement $repoRoot }

Require-Path $desktopPath "Desktop executable"
Require-Path $cliPath "CLI executable"
if ((Get-PeMachine $desktopPath) -ne 0x014C) { throw "Desktop executable must be a 32-bit i386 PE: $desktopPath" }
if ((Get-PeMachine $cliPath) -ne 0x014C) { throw "CLI executable must be a 32-bit i386 PE: $cliPath" }
if (-not $CoreOnly) {
  if (-not $supplementPath) {
    throw "Supplement archive was not found. Build it or pass -Supplement, or use -CoreOnly for a core-only installer."
  }
  Require-Path $supplementPath "Supplement archive"
}

if (-not $OutputDir) { $OutputDir = Join-Path $repoRoot "target\installer\dist\win7-x86" }
$OutputDir = Resolve-RepoPath $OutputDir
New-Item -ItemType Directory -Force -Path $OutputDir, $stageRoot | Out-Null

$coreStage = Join-Path $stageRoot "core"
if (Test-Path -LiteralPath $coreStage) { Remove-Item -LiteralPath $coreStage -Recurse -Force }
New-Item -ItemType Directory -Force -Path $coreStage | Out-Null
Copy-Item -LiteralPath $desktopPath -Destination (Join-Path $coreStage "wunder-desktop.exe") -Force
Copy-Item -LiteralPath $cliPath -Destination (Join-Path $coreStage "wunder-cli.exe") -Force
$markerPath = Join-Path $coreStage ".wunder-home"
[IO.File]::WriteAllText($markerPath, "portable`r`n", (New-Object Text.UTF8Encoding $false))

$available = @()
$componentBytes = @{}
$componentFiles = @{}
$metadata = $null
$supplementRoot = ""
if (-not $CoreOnly) {
  $supplementItem = Get-Item -LiteralPath $supplementPath
  if ($supplementItem.PSIsContainer) {
    $supplementRoot = $supplementItem.FullName
    $info = Get-DirectorySupplementInfo $supplementRoot
    foreach ($name in $ComponentOrder) {
      if (Test-Path -LiteralPath (Join-Path $supplementRoot "opt\$name") -PathType Container) { $available += $name }
    }
  } else {
    $info = Get-ZipSupplementInfo $supplementItem.FullName
    foreach ($name in $ComponentOrder) {
      if ($info.Files[$name] -gt 0) { $available += $name }
    }
    if ($available.Count -eq 0) { throw "Supplement archive has no opt/<component> files: $supplementPath" }
    $supplementRoot = Join-Path $stageRoot "supplement"
    Install-SupplementStage $supplementItem.FullName $supplementRoot
  }
  $componentBytes = $info.Bytes
  $componentFiles = $info.Files
  $metadata = $info.Metadata
  $sevenZipMeta = $null
  if ($metadata -and $metadata.PSObject.Properties['7zip']) { $sevenZipMeta = $metadata.'7zip' }
  if ($sevenZipMeta -and $sevenZipMeta.bundled -eq $true -and $componentFiles['7zip'] -eq 0) {
    throw "Supplement metadata declares 7-Zip as bundled, but opt\7zip is missing: $supplementPath"
  }
}

$labels = Get-Content -LiteralPath (Join-Path $installerRoot "component-labels.json") -Raw -Encoding UTF8 | ConvertFrom-Json
function Format-ComponentLabel([string]$Name, [string]$Version, [int64]$Bytes) {
  $template = [string]$labels.PSObject.Properties[$Name].Value
  if (-not $template) { $template = $Name }
  $label = $template.Replace("{version}", $Version).Replace("  ", " ").Trim()
  $size = [math]::Ceiling($Bytes / 1MB)
  return $label + ([string]$labels.approxSize).Replace("{size}", "$size")
}

$lines = New-Object System.Collections.Generic.List[string]
$lines.Add("[Tasks]")
# The app bodies (Desktop/CLI) are selectable tasks too, checked by default; leaving
# them unchecked skips their executables so a components-only install is possible.
# Checking Desktop also creates the desktop shortcut, so there is no separate task for it.
$coreBytes = @{
  desktop = (Get-Item -LiteralPath $desktopPath).Length
  cli = (Get-Item -LiteralPath $cliPath).Length
}
foreach ($name in @("desktop", "cli")) {
  $label = Format-ComponentLabel $name $version $coreBytes[$name]
  $lines.Add(('Name: "{0}"; Description: "{1}"' -f $name, $label))
}
if (-not $CoreOnly) {
  # Every supplement component is checked by default: a plain wizard run
  # installs a complete workstation. Rows can be unchecked individually (or
  # via the select-all box) and /TASKS= still narrows scripted installs.
  foreach ($name in $available) {
    $task = Get-TaskName $name
    if ($task -notmatch '^[A-Za-z_][A-Za-z0-9_]*$') {
      throw "Inno task names must start with a letter or underscore; map component '$name' in `$ComponentTaskNames."
    }
    $versionForLabel = Get-MetadataVersion $metadata $name
    $taskBytes = $componentBytes[$name]
    $label = Format-ComponentLabel $name $versionForLabel $taskBytes
    $lines.Add(('Name: "{0}"; Description: "{1}"' -f $task, $label))
  }
}
$lines.Add("")
$lines.Add("[Files]")
$lines.Add(('Source: "{0}"; DestDir: "{{app}}\bin"; DestName: "wunder-desktop.exe"; Flags: ignoreversion; Tasks: desktop' -f (Join-Path $coreStage "wunder-desktop.exe")))
$lines.Add(('Source: "{0}"; DestDir: "{{app}}\bin"; DestName: "wunder-cli.exe"; Flags: ignoreversion; Tasks: cli' -f (Join-Path $coreStage "wunder-cli.exe")))
$lines.Add(('Source: "{0}"; DestDir: "{{app}}"; DestName: ".wunder-home"; Flags: ignoreversion' -f $markerPath))
if (-not $CoreOnly) {
  foreach ($name in $available) {
    $source = Join-Path $supplementRoot "opt\$name\*"
    $lines.Add(('Source: "{0}"; DestDir: "{{app}}\opt\{1}"; Flags: ignoreversion recursesubdirs createallsubdirs; Tasks: {2}' -f $source, $name, (Get-TaskName $name)))
  }
}
$generatedPath = Join-Path $stageRoot "payload.generated.iss"
[IO.File]::WriteAllText($generatedPath, ($lines -join "`r`n") + "`r`n", (New-Object Text.UTF8Encoding $false))

Copy-Item -LiteralPath (Join-Path $installerRoot "wunder.iss") -Destination (Join-Path $stageRoot "wunder.iss") -Force
Copy-Item -LiteralPath (Join-Path $installerRoot "ChineseSimplified.isl") -Destination (Join-Path $stageRoot "ChineseSimplified.isl") -Force
Copy-Item -LiteralPath (Join-Path $repoRoot "images\eva01-head.ico") -Destination (Join-Path $stageRoot "eva01-head.ico") -Force
Copy-Item -LiteralPath (Join-Path $repoRoot "images\eva01-head.png") -Destination (Join-Path $stageRoot "eva01-head.png") -Force
Copy-Item -LiteralPath (Join-Path $repoRoot "images\wizard-banner.png") -Destination (Join-Path $stageRoot "wizard-banner.png") -Force

$isccPath = Ensure-Iscc $Iscc (-not $NoDownload)
Write-Host "[wunder-pack] Inno Setup compiler: $isccPath"
Write-Host "[wunder-pack] staging: $stageRoot"
Write-Host "[wunder-pack] components: $(if ($available.Count) { $available -join ', ' } else { 'core only' })"

$isccArgs = @(
  "/Qp",
  "/O$OutputDir",
  "/F$outputBase",
  "/DAppVersion=$version",
  "/DVersionInfoVersion=$numericVersion",
  "/DOutputBaseFilename=$outputBase",
  "/DPackCompression=$Compression",
  "/DComponentsShown=$($available.Count)",
  (Join-Path $stageRoot "wunder.iss")
)
# Start-Process instead of `&`: when this script is launched from a wrapper shell
# (build.bat -> powershell -File, Git Bash, CI), `&` can leave $LASTEXITCODE
# unset even though ISCC ran and produced the installer, so a successful build
# was reported as "failed with exit code " (empty). -Wait -PassThru gives the
# real exit code and keeps ISCC's console output on the current console.
$isccArgLine = ($isccArgs | ForEach-Object {
  if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ }
}) -join ' '
$isccProcess = Start-Process -FilePath $isccPath -ArgumentList $isccArgLine -NoNewWindow -Wait -PassThru
if ($isccProcess.ExitCode -ne 0) { throw "Inno Setup compilation failed with exit code $($isccProcess.ExitCode)" }

$installerPath = Join-Path $OutputDir "$outputBase.exe"
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) {
  throw "Installer was not produced: $installerPath"
}
$installer = Get-Item -LiteralPath $installerPath
$installerHash = (Get-Sha256Hex $installerPath).ToLowerInvariant()

$componentManifest = @()
# The manifest inventories every opt directory that entered the installer.
foreach ($name in $ComponentOrder) {
  if ($componentFiles[$name] -gt 0) {
    $componentManifest += [ordered]@{
      name = $name
      version = (Get-MetadataVersion $metadata $name)
      bytes = $componentBytes[$name]
      files = $componentFiles[$name]
      defaultSelected = $true
    }
  }
}

$inputs = @()
foreach ($input in @(
  @{ Path = $desktopPath; Role = "desktop" },
  @{ Path = $cliPath; Role = "cli" },
  @{ Path = $supplementPath; Role = "supplement" }
)) {
  if (-not $input.Path) { continue }
  $item = Get-Item -LiteralPath $input.Path
  if ($item.PSIsContainer) {
    $inputs += [ordered]@{
      role = $input.Role
      file = $item.Name
      directory = $true
      bytes = $null
      sha256 = $null
    }
  } else {
    $inputs += [ordered]@{
      role = $input.Role
      file = $item.Name
      bytes = $item.Length
      sha256 = (Get-Sha256Hex $input.Path).ToLowerInvariant()
    }
  }
}

$manifest = [ordered]@{
  product = "wunder"
  version = $version
  target = "win7-x86"
  installer = [ordered]@{
    file = $installer.Name
    bytes = $installer.Length
    sha256 = $installerHash
  }
  compression = $Compression
  innoSetup = $innoVersion
  builtAt = (Get-Date).ToUniversalTime().ToString("o")
  coreOnly = [bool]$CoreOnly
  components = $componentManifest
  inputs = $inputs
}
$manifestPath = Join-Path $OutputDir "$outputBase.manifest.json"
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 8) + "`r`n", (New-Object Text.UTF8Encoding $false))
$shaPath = Join-Path $OutputDir "$outputBase.exe.sha256"
[IO.File]::WriteAllText($shaPath, "$installerHash  $($installer.Name)`r`n", (New-Object Text.UTF8Encoding $false))

Write-Host "[wunder-pack] installer: $installerPath"
Write-Host "[wunder-pack] size: $([math]::Round($installer.Length / 1MB, 1)) MB"
Write-Host "[wunder-pack] sha256: $installerHash"
Write-Host "[wunder-pack] manifest: $manifestPath"
