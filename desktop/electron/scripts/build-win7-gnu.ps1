param(
  [ValidateSet('x64', 'ia32')]
  [string]$Arch = 'x64',
  [string]$LabRoot = '',
  [switch]$StaticRuntime,
  [switch]$SkipBootstrap,
  [string]$RustToolchain = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $scriptDir 'win7-gnu.common.ps1')

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

Write-Win7GnuStep "packaging Electron shell with Win7 GNU bridge"
& (Join-Path $scriptDir 'build-win7.ps1') `
  -Arch $Arch `
  -LabRoot $context.LabRoot `
  -ElectronVersion $context.ElectronVersion `
  -ElectronBuilderVersion $context.ElectronBuilderVersion
if ($LASTEXITCODE -ne 0) {
  throw "Electron packaging failed with exit code $LASTEXITCODE"
}

Write-Win7GnuStep "toolchain manifest: $($context.ToolchainManifestPath)"
