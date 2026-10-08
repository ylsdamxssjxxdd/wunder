# Host tools build Cargo build scripts/proc macros. Target tools still emit i686.
function Set-Win7HostTools {
  param([Parameter(Mandatory = $true)][string]$HostMingwBin)

  $hostGcc = Join-Path $HostMingwBin 'gcc.exe'
  if (!(Test-Path -LiteralPath $hostGcc)) {
    throw "Missing x64 host GCC: $hostGcc (set WUNDER_WIN7_HOST_MINGW_BIN)"
  }
  $machine = (& $hostGcc -dumpmachine).Trim()
  if ($LASTEXITCODE -ne 0 -or $machine -ne 'x86_64-w64-mingw32') {
    throw "Host GCC must target x86_64-w64-mingw32; found '$machine'"
  }
  $env:WUNDER_WIN7_HOST_MINGW_BIN = $HostMingwBin
  $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = $hostGcc
  $env:CC_x86_64_pc_windows_gnu = $hostGcc
  $env:CXX_x86_64_pc_windows_gnu = Join-Path $HostMingwBin 'g++.exe'
  $env:AR_x86_64_pc_windows_gnu = Join-Path $HostMingwBin 'gcc-ar.exe'
  $env:RANLIB_x86_64_pc_windows_gnu = Join-Path $HostMingwBin 'gcc-ranlib.exe'
}

function Assert-Win7RustHost {
  param([Parameter(Mandatory = $true)][string[]]$VersionDetails)
  if (!($VersionDetails -contains 'host: x86_64-pc-windows-gnu')) {
    throw 'Win7 release builds require x86_64-pc-windows-gnu host Rust to avoid the 32-bit LLVM address-space limit.'
  }
}
