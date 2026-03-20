param(
  [ValidateSet('x64', 'ia32')]
  [string]$Arch = 'x64',
  [string]$LabRoot = '',
  [switch]$StaticRuntime,
  [switch]$BuildSupplement,
  [switch]$WithSupplement,
  [ValidateSet('minimal', 'common')]
  [string]$SupplementPythonProfile = 'minimal',
  [string]$SupplementPythonRequirementsPath = '',
  [string]$SupplementPythonPackageIndexUrl = '',
  [string]$SupplementPythonArchivePath = '',
  [string]$SupplementGitArchivePath = '',
  [switch]$SkipBootstrap,
  [string]$RustToolchain = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $scriptDir 'win7-gnu.common.ps1')

function Resolve-SupplementArtifactPath {
  param(
    [string]$DistRoot,
    [string]$Arch,
    [string]$PythonProfile
  )

  $fileName = if ($PythonProfile -eq 'common') {
    "wunder补充包-win7-$Arch-common.zip"
  } else {
    "wunder补充包-win7-$Arch.zip"
  }
  Join-Path $DistRoot $fileName
}

$repoRoot = Resolve-Win7GnuRepoRoot -ScriptPath $MyInvocation.MyCommand.Path
$context = New-Win7GnuBuildContext -RepoRoot $repoRoot -Arch $Arch -LabRoot $LabRoot -RustToolchain $RustToolchain

Initialize-Win7GnuToolchain -Context $context -SkipRustup:$SkipBootstrap -SkipFetch:$SkipBootstrap -StaticRuntime:$StaticRuntime

$env:WUNDER_BRIDGE_BIN = Join-Path $context.BridgeTargetDir (Join-Path $context.Target 'release\wunder-desktop-bridge.exe')
Write-Win7GnuStep "building Win7 GNU bridge for $($context.Target)"
cargo --config $context.CargoPatchConfigPath build --release --bin wunder-desktop-bridge '-Zbuild-std=std,panic_abort' --target $context.Target
if ($LASTEXITCODE -ne 0) {
  throw "Win7 GNU bridge build failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path $env:WUNDER_BRIDGE_BIN)) {
  throw "Win7 GNU bridge binary missing: $env:WUNDER_BRIDGE_BIN"
}

$shouldBuildSupplement = $BuildSupplement -or $WithSupplement
if ($WithSupplement -and -not $BuildSupplement) {
  Write-Win7GnuStep "legacy -WithSupplement detected; supplement will be built separately and NOT embedded into setup.exe"
}

$supplementArtifactPath = ''
if ($shouldBuildSupplement) {
  $supplementBuildRoot = Join-Path $context.LabRoot 'win7-supplement'
  $supplementScript = Join-Path $repoRoot 'packaging\windows\scripts\build_win7_desktop_supplement.ps1'
  if (-not (Test-Path $supplementScript)) {
    throw "Win7 supplement script missing: $supplementScript"
  }
  Write-Win7GnuStep "building Win7 supplement package"
  $supplementArgs = @(
    '-ExecutionPolicy',
    'Bypass',
    '-File',
    $supplementScript,
    '-Arch',
    $Arch,
    '-BuildRoot',
    $supplementBuildRoot,
    '-PythonProfile',
    $SupplementPythonProfile,
    '-KeepStage'
  )
  if ($SupplementPythonRequirementsPath) {
    $supplementArgs += @('-PythonRequirementsPath', $SupplementPythonRequirementsPath)
  }
  if ($SupplementPythonPackageIndexUrl) {
    $supplementArgs += @('-PythonPackageIndexUrl', $SupplementPythonPackageIndexUrl)
  }
  if ($SupplementPythonArchivePath) {
    $supplementArgs += @('-PythonArchivePath', $SupplementPythonArchivePath)
  }
  if ($SupplementGitArchivePath) {
    $supplementArgs += @('-GitArchivePath', $SupplementGitArchivePath)
  }
  & powershell.exe @supplementArgs
  if ($LASTEXITCODE -ne 0) {
    throw "Win7 supplement build failed with exit code $LASTEXITCODE"
  }
  $supplementArtifactPath = Resolve-SupplementArtifactPath `
    -DistRoot (Join-Path $supplementBuildRoot 'dist') `
    -Arch $Arch `
    -PythonProfile $SupplementPythonProfile
  if (-not (Test-Path $supplementArtifactPath)) {
    throw "Win7 supplement artifact missing: $supplementArtifactPath"
  }
}

Write-Win7GnuStep "packaging Electron shell with Win7 GNU bridge"
& (Join-Path $scriptDir 'build-win7.ps1') `
  -Arch $Arch `
  -LabRoot $context.LabRoot `
  -ElectronVersion $context.ElectronVersion `
  -ElectronBuilderVersion $context.ElectronBuilderVersion
if ($LASTEXITCODE -ne 0) {
  throw "Electron packaging failed with exit code $LASTEXITCODE"
}

$installerDistRoot = Join-Path $context.LabRoot ("electron-win7-{0}\dist" -f $Arch)
if ($supplementArtifactPath) {
  Copy-Item -Path $supplementArtifactPath -Destination (Join-Path $installerDistRoot (Split-Path $supplementArtifactPath -Leaf)) -Force
  Write-Win7GnuStep "supplement package copied next to installer: $(Join-Path $installerDistRoot (Split-Path $supplementArtifactPath -Leaf))"
  Write-Win7GnuStep "setup.exe remains slim: Python/Git are NOT embedded and must be extracted from the supplement package if needed"
}

Write-Win7GnuStep "installer directory: $installerDistRoot"
Write-Win7GnuStep "toolchain manifest: $($context.ToolchainManifestPath)"
