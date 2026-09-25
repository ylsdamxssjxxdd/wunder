#!/usr/bin/env bash
# Cross compiler wrapper for the Ubuntu 18.04 x86_64 sysroot shipped in the
# ARM64 offline SDK.  Keeping the sysroot selection here avoids leaking host
# compiler paths into Cargo build scripts.
set -euo pipefail

sdk="${WUNDER_AMD64_SDK_ROOT:?Set WUNDER_AMD64_SDK_ROOT to linux-amd64-ubuntu18/root}"
export GCC_EXEC_PREFIX="$sdk/usr/lib/gcc-cross/"
compiler="x86_64-linux-gnu-gcc-7"
if [[ "${1:-}" == --cxx ]]; then
  compiler="x86_64-linux-gnu-g++-7"
  shift
fi
exec "$sdk/usr/bin/$compiler" \
  "--sysroot=$sdk" \
  "-I$sdk/usr/x86_64-linux-gnu/include" \
  "-L$sdk/usr/x86_64-linux-gnu/lib" \
  "-L$sdk/usr/lib/x86_64-linux-gnu" "$@"
