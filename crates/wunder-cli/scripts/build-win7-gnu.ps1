param(
  [ValidateSet('x64', 'ia32')]
  [string]$Arch = 'ia32',
  [string]$LabRoot = '',
  [switch]$StaticRuntime,
  [switch]$SkipBootstrap,
  [switch]$Doctor,
  [switch]$NoStrip,
  [switch]$SkipSmoke,
  [string]$RustToolchain = '',
  [string]$LockfilePath = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir '..\..\..')).Path
$commonScript = Join-Path $repoRoot 'desktop\electron\scripts\win7-gnu.common.ps1'
if (-not (Test-Path $commonScript)) {
  throw "Win7 GNU common script missing: $commonScript"
}
. $commonScript

function Resolve-Win7CliDistRoot {
  param([hashtable]$Context)

  Join-Path $Context.LabRoot ("cli-win7-{0}\dist" -f $Context.Arch)
}

function Resolve-Win7CliBuiltExe {
  param([hashtable]$Context)

  Join-Path $Context.BridgeTargetDir (Join-Path $Context.Target 'release\wunder-cli.exe')
}

function Resolve-Win7CliTool {
  param(
    [hashtable]$Context,
    [string]$Name
  )

  $candidate = Join-Path $Context.MinGwBin $Name
  if (Test-Path $candidate) {
    return $candidate
  }
  return $Name
}

function Assert-Win7CliNoForbiddenImports {
  param(
    [hashtable]$Context,
    [string]$ExePath
  )

  $objdump = Resolve-Win7CliTool -Context $Context -Name 'objdump.exe'
  $imports = & $objdump -p $ExePath 2>&1 | Select-String -Pattern 'DLL Name|api-ms|winrt'
  if ($LASTEXITCODE -ne 0) {
    throw "objdump import inspection failed with exit code $LASTEXITCODE"
  }
  $forbidden = @($imports | Where-Object { $_.Line -match 'api-ms|winrt' })
  if ($forbidden.Count -gt 0) {
    $details = ($forbidden | ForEach-Object { $_.Line.Trim() }) -join '; '
    throw "Win7-incompatible DLL import detected: $details"
  }
}

function Assert-Win7CliPeI386 {
  param(
    [hashtable]$Context,
    [string]$ExePath
  )

  $objdump = Resolve-Win7CliTool -Context $Context -Name 'objdump.exe'
  $headers = & $objdump -f $ExePath 2>&1 | Out-String
  if ($LASTEXITCODE -ne 0) {
    throw "objdump header inspection failed with exit code $LASTEXITCODE"
  }
  if ($Context.Arch -eq 'ia32' -and $headers -notmatch 'file format pei-i386') {
    throw "unexpected PE format for ia32 build: $headers"
  }
}

$context = New-Win7GnuBuildContext -RepoRoot $repoRoot -Arch $Arch -LabRoot $LabRoot -RustToolchain $RustToolchain
$resolvedLockfilePath = Ensure-Win7GnuLockfile -Context $context -LockfilePath $LockfilePath

if ($Doctor) {
  Test-Win7GnuPrerequisites -Context $context
  Write-Win7GnuStep "CLI Win7 GNU toolchain profile: $($context.ProfilePath)"
  Write-Win7GnuStep "target: $($context.Target)"
  Write-Win7GnuStep "MinGW bin: $($context.MinGwBin)"
  Write-Win7GnuStep "lab root: $($context.LabRoot)"
  Write-Win7GnuStep "isolated lockfile: $resolvedLockfilePath"
  Write-Win7GnuStep "output directory: $(Resolve-Win7CliDistRoot -Context $context)"
  return
}

Initialize-Win7GnuToolchain `
  -Context $context `
  -SkipRustup:$SkipBootstrap `
  -SkipFetch:$SkipBootstrap `
  -StaticRuntime:$StaticRuntime `
  -LockfilePath $resolvedLockfilePath

Write-Win7GnuStep "building Win7 GNU wunder-cli for $($context.Target)"
$buildArgs = @(
  '--config',
  $context.CargoPatchConfigPath,
  '-Z',
  'unstable-options',
  'build',
  '--release',
  '-j',
  '8',
  '-p',
  'wunder-cli',
  '--bin',
  'wunder-cli',
  '-Zbuild-std=std,panic_abort',
  '--target',
  $context.Target
)
Invoke-Win7GnuCargo -Context $context -LockfilePath $resolvedLockfilePath -CargoArgs $buildArgs
if ($LASTEXITCODE -ne 0) {
  throw "Win7 GNU wunder-cli build failed with exit code $LASTEXITCODE"
}

$builtExe = Resolve-Win7CliBuiltExe -Context $context
if (-not (Test-Path $builtExe)) {
  throw "Win7 GNU wunder-cli binary missing: $builtExe"
}

if (-not $NoStrip) {
  $strip = Resolve-Win7CliTool -Context $context -Name 'strip.exe'
  Write-Win7GnuStep "stripping release binary"
  & $strip $builtExe
  if ($LASTEXITCODE -ne 0) {
    throw "strip failed with exit code $LASTEXITCODE"
  }
}

Assert-Win7CliPeI386 -Context $context -ExePath $builtExe
Assert-Win7CliNoForbiddenImports -Context $context -ExePath $builtExe

if (-not $SkipSmoke) {
  Write-Win7GnuStep "running CLI smoke test: --help"
  & $builtExe --help | Select-Object -First 8 | Out-Host
  if ($LASTEXITCODE -ne 0) {
    throw "wunder-cli --help smoke test failed with exit code $LASTEXITCODE"
  }
}

$distRoot = Resolve-Win7CliDistRoot -Context $context
Ensure-Win7GnuDirectory -Path $distRoot
$distExe = Join-Path $distRoot 'wunder-cli.exe'
Copy-Item -Path $builtExe -Destination $distExe -Force

$artifact = Get-Item $distExe
$hash = Get-FileHash -Algorithm SHA256 $distExe
Write-Win7GnuStep "CLI artifact: $distExe"
Write-Win7GnuStep "size: $($artifact.Length) bytes"
Write-Win7GnuStep "sha256: $($hash.Hash)"
Write-Win7GnuStep "toolchain manifest: $($context.ToolchainManifestPath)"
Write-Win7GnuStep "isolated lockfile: $resolvedLockfilePath"
