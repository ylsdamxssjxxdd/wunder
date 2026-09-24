#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "$0")" && pwd -P)"
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin2}"
image="${WUNDER_LINUX_CROSS_IMAGE:-rcho-slint-arm64-ubuntu18:latest}"
usage() { echo "Usage: $0 [--native|--docker] [--appimage]"; }
mode=native
package=0
while (($#)); do
  case "$1" in
    --native) mode=native ;;
    --docker) mode=docker ;;
    --appimage) package=1 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
  shift
done
if [[ "$mode" == docker ]]; then
  command -v docker >/dev/null || { echo 'Docker is required' >&2; exit 2; }
  extra_env=()
  if [[ "$package" == 1 && -n "${WUNDER_APPIMAGE_RUNTIME:-}" ]]; then
    extra_env=(-e WUNDER_APPIMAGE_RUNTIME=/builder/appimage-runtime/runtime.AppImage \
      -v "${WUNDER_APPIMAGE_RUNTIME}:/builder/appimage-runtime/runtime.AppImage:ro")
  fi
  exec docker run --rm --network none --platform linux/arm64 \
    -e WUNDER_REPO_ROOT=/workspace -e WUNDER_BUILDER_ROOT=/builder/kylin2 \
    -e WUNDER_OFFLINE_ROOT=/builder/kylin2/offline \
    "${extra_env[@]}" \
    -v "$repo_root:/workspace" -v "$builder_root:/builder/kylin2:ro" \
    -w /workspace "$image" bash "/workspace/builders/$(basename "$([[ "$package" == 1 ]] && echo build-linux-amd64-appimage.sh || echo build-linux-amd64-offline.sh)")"
fi
if [[ "$package" == 1 ]]; then
  exec bash "$repo_root/builders/build-linux-amd64-appimage.sh"
fi
exec bash "$repo_root/builders/build-linux-amd64-offline.sh"
