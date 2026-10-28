#!/usr/bin/env bash
# aarch64 target compiler for a cross build from x86_64, or the native compiler
# on an ARM64 host. The kylin-x86 SDK keeps the x86_64-hosted cross compiler
# (and its support libraries) in host-tools-amd64; the ARM64 userspace comes
# from the pinned Ubuntu 18.04 sysroot.
set -euo pipefail

sysroot="${RCHO_ARM64_SYSROOT:?RCHO_ARM64_SYSROOT is required}"

if [[ "$(uname -m)" == x86_64 ]]; then
  host_root="${WUNDER_ARM64_CROSS_HOST_ROOT:?WUNDER_ARM64_CROSS_HOST_ROOT is required on an x86_64 host}"
  gcc_lib="$host_root/usr/lib/gcc-cross/aarch64-linux-gnu/7"
  cross_bin="$host_root/usr/aarch64-linux-gnu/bin"
  compiler="$host_root/usr/bin/aarch64-linux-gnu-gcc-7"
  if [[ "${1:-}" == --cxx ]]; then
    compiler="$host_root/usr/bin/aarch64-linux-gnu-g++-7"
    shift
  fi
  for path in "$compiler" "$gcc_lib/cc1" "$cross_bin/ld" "$sysroot/usr/lib/aarch64-linux-gnu"; do
    [[ -e "$path" ]] || { echo "[wunder-arm64-cc] missing cross toolchain path: $path" >&2; exit 2; }
  done
  export LD_LIBRARY_PATH="$host_root/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  export PATH="$cross_bin:$host_root/usr/bin:$PATH"
  unset GCC_EXEC_PREFIX COMPILER_PATH LIBRARY_PATH
  exec "$compiler" --sysroot="$sysroot" \
    -B"$gcc_lib/" -B"$cross_bin/" \
    -Wl,-rpath-link,"$sysroot/usr/lib/aarch64-linux-gnu" \
    -L"$sysroot/usr/lib/gcc/aarch64-linux-gnu/7" \
    -L"$sysroot/usr/lib/aarch64-linux-gnu" "$@"
fi

compiler="${RCHO_ARM64_CC:-$sysroot/usr/bin/aarch64-linux-gnu-gcc-7}"
if [[ "${1:-}" == --cxx ]]; then
  compiler="${RCHO_ARM64_CXX:-$sysroot/usr/bin/aarch64-linux-gnu-g++-7}"
  shift
fi
[[ -x "$compiler" ]] || { echo "ARM64 SDK compiler is missing: $compiler" >&2; exit 2; }
gcc_lib="$sysroot/usr/lib/gcc/aarch64-linux-gnu/7"
libdir="$sysroot/usr/lib/aarch64-linux-gnu"
runtime_libdir="$sysroot/lib/aarch64-linux-gnu"
cc1="$gcc_lib/cc1"
[[ -x "$cc1" ]] || { echo "ARM64 SDK GCC backend is missing: $cc1" >&2; exit 2; }
unset GCC_EXEC_PREFIX COMPILER_PATH LIBRARY_PATH
exec "$compiler" --sysroot="$sysroot" -Wl,--sysroot="$sysroot" \
  -B"$gcc_lib/" -Wl,-rpath-link,"$runtime_libdir" \
  -L"$gcc_lib" -L"$libdir" "$@"
