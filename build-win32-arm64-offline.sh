#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "$0")" && pwd -P)"
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin2}"
if [[ "${1:-}" == --docker ]]; then
  shift
  command -v docker >/dev/null || { echo 'Docker is required' >&2; exit 2; }
  exec docker run --rm --network none --platform linux/arm64 \
    -e WUNDER_REPO_ROOT=/workspace -e WUNDER_BUILDER_ROOT=/builder/kylin2 \
    -e WUNDER_OFFLINE_ROOT=/builder/kylin2/offline \
    -v "$repo_root:/workspace" -v "$builder_root:/builder/kylin2:ro" \
    -w /workspace rcho-slint-arm64-ubuntu18:latest bash /workspace/builders/build-win32-arm64-offline.sh "$@"
fi
exec bash "$repo_root/builders/build-win32-arm64-offline.sh" "$@"
