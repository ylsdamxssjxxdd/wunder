#!/usr/bin/env bash
set -euo pipefail
SRC=${1:?export directory}; OUT=${2:?output directory}; mkdir -p "$OUT/offline"; cp -a "$SRC"/. "$OUT/offline/"; python3 "$(dirname "$0")/sdk_manifest.py" "$OUT"; echo "Kylin x86 SDK staged at $OUT; copy it offline under RUST_BUILDER_ROOT/kylin-x86."
