#!/usr/bin/env bash
# Unified ARM64-host cross build entry for Win32/Win7-compatible desktop/CLI outputs.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "$0")" && pwd -P)"
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}"
image="${WUNDER_LINUX_CROSS_IMAGE:-rcho-slint-arm64-ubuntu18:latest}"
program=desktop
build_all=0
mode=native
usage() { echo "Usage: $0 [-t desktop|cli] [-all] [--native|--docker]"; }
while (($#)); do
  case "$1" in
    -t|--target) shift; (($#)) || { usage >&2; exit 2; }; case "$1" in desktop|cli) program="$1" ;; *) echo 'program must be desktop or cli' >&2; exit 2 ;; esac ;;
    -all|--all) build_all=1 ;;
    --native) mode=native ;;
    --docker) mode=docker ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
  shift
done
if [[ "$mode" == docker && "${WUNDER_IN_DOCKER:-0}" != 1 ]]; then
  command -v docker >/dev/null || { echo 'Docker is required' >&2; exit 2; }
  nested_args=(--native -t "$program")
  [[ "$build_all" == 1 ]] && nested_args+=(-all)
  exec docker run --rm --network none --platform linux/arm64 -e WUNDER_IN_DOCKER=1 -e WUNDER_REPO_ROOT=/workspace -e WUNDER_BUILDER_ROOT=/builder/kylin-arm -e WUNDER_OFFLINE_ROOT=/builder/kylin-arm/offline -v "$repo_root:/workspace" -v "$builder_root:/builder/kylin-arm:ro" -w /workspace "$image" bash /workspace/build-win32-arm64-offline.sh "${nested_args[@]}"
fi
run_one() {
  if [[ "$1" == desktop ]]; then bash "$repo_root/builders/build-win32-arm64-offline.sh"; else bash "$repo_root/builders/build-cli-win32-arm64-offline.sh"; fi
}
if [[ "$build_all" == 1 ]]; then run_one desktop; run_one cli; else run_one "$program"; fi
