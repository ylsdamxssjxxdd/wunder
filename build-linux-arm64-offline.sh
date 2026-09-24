#!/usr/bin/env bash
# Unified Slint Linux build entry, aligned with the rcho offline workflow.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "$0")" && pwd -P)"
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin2}"
image="${WUNDER_SLINT_LINUX_DOCKER_IMAGE:-rcho-slint-arm64-ubuntu18:latest}"
target=arm64
mode=native
package=1

usage() {
  cat <<'EOF'
Usage: build-linux-arm64-offline.sh [-t arm64|amd64|win32] [--native|--docker]
  arm64  Build and package the native ARM64 Slint AppImage (default)
  amd64  Cross-build the Ubuntu 18.04 x86_64 Slint binary/AppImage
  win32  Cross-build the Win32 i686 Slint executable
EOF
}
while (($#)); do
  case "$1" in
    -t|--target) shift; (($#)) || { usage >&2; exit 2; }; target="$1" ;;
    --native) mode=native ;;
    --docker) mode=docker ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
  shift
done
case "$target" in arm64|amd64|win32) ;; *) echo "Unsupported target: $target" >&2; exit 2 ;; esac

if [[ "$target" == arm64 ]]; then
  script="$repo_root/builders/build-linux-arm64-appimage.sh"
elif [[ "$target" == amd64 ]]; then
  script="$repo_root/builders/build-linux-amd64-$([[ "$package" == 1 ]] && echo appimage || echo offline).sh"
else
  script="$repo_root/builders/build-win32-arm64-offline.sh"
fi

if [[ "$mode" == docker ]]; then
  command -v docker >/dev/null 2>&1 || { echo "Docker is required" >&2; exit 2; }
  runtime_args=()
  if [[ "$target" == arm64 ]]; then
    [[ -n "${WUNDER_APPIMAGE_RUNTIME:-}" ]] || { echo "WUNDER_APPIMAGE_RUNTIME must point to an ARM64 AppImage runtime" >&2; exit 2; }
    runtime_args=(-e WUNDER_APPIMAGE_RUNTIME=/builder/appimage-runtime/runtime.AppImage \
      -v "${WUNDER_APPIMAGE_RUNTIME}:/builder/appimage-runtime/runtime.AppImage:ro")
  fi
  exec docker run --rm --network none --platform linux/arm64 \
    -e WUNDER_REPO_ROOT=/workspace \
    -e WUNDER_BUILDER_ROOT=/builder/kylin2 \
    -e WUNDER_OFFLINE_ROOT=/builder/kylin2/offline \
    "${runtime_args[@]}" \
    -v "$repo_root:/workspace" -v "$builder_root:/builder/kylin2:ro" \
    -w /workspace "$image" bash "/workspace/builders/$(basename "$script")"
fi
exec bash "$script"
