#!/usr/bin/env bash
# Cross-build Win7 x86 from an x86_64 Linux host using the portable SDK in kylin-arm.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd -P)"
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}/offline}"
export WUNDER_OFFLINE_ROOT="$offline_root"
exec "$repo_root/builders/build-win7-arm64-offline.sh" "$@"
