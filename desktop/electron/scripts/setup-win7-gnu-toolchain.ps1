param(
  [ValidateSet('x64', 'ia32')]
  [string]$Arch = 'x64',
  [string]$LabRoot = '',
  [switch]$StaticRuntime,
  [switch]$Doctor,
  [string]$RustToolchain = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $scriptDir 'win7-gnu.common.ps1')

$repoRoot = Resolve-Win7GnuRepoRoot -ScriptPath $MyInvocation.MyCommand.Path
$context = New-Win7GnuBuildContext -RepoRoot $repoRoot -Arch $Arch -LabRoot $LabRoot -RustToolchain $RustToolchain

if ($Doctor) {
  Test-Win7GnuPrerequisites -Context $context
  Write-Win7GnuStep "toolchain profile: $($context.ProfilePath)"
  Write-Win7GnuStep "target: $($context.Target)"
  Write-Win7GnuStep "MinGW bin: $($context.MinGwBin)"
  Write-Win7GnuStep "lab root: $($context.LabRoot)"
  return
}

Initialize-Win7GnuToolchain -Context $context -StaticRuntime:$StaticRuntime
Write-Win7GnuStep "toolchain manifest: $($context.ToolchainManifestPath)"
Write-Win7GnuStep "next build: powershell -ExecutionPolicy Bypass -File desktop/electron/scripts/build-win7-gnu.ps1 -Arch $Arch -SkipBootstrap"
