#!/usr/bin/env bash
set -euo pipefail
sysroot="${RCHO_ARM64_SYSROOT:?RCHO_ARM64_SYSROOT is required}"
compiler="${RCHO_ARM64_CC:-$sysroot/usr/bin/aarch64-linux-gnu-gcc-7}"
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
