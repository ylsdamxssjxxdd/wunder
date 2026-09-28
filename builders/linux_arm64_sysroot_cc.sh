#!/usr/bin/env bash
set -euo pipefail
sysroot="${RCHO_ARM64_SYSROOT:?RCHO_ARM64_SYSROOT is required}"
compiler="${RCHO_ARM64_CC:-$sysroot/usr/bin/aarch64-linux-gnu-gcc-7}"
[[ -x "$compiler" ]] || { echo "ARM64 SDK compiler is missing: $compiler" >&2; exit 2; }
exec "$compiler" --sysroot="$sysroot" -Wl,--sysroot="$sysroot" "$@"
