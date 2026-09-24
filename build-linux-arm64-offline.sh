#!/usr/bin/env bash
# Unified ARM64 Linux build entry for the desktop and CLI distributions.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "$0")" && pwd -P)"
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}"
image="${WUNDER_SLINT_LINUX_DOCKER_IMAGE:-rcho-slint-arm64-ubuntu18:latest}"
program=desktop
build_all=0
arch=arm64
mode=native
package=0

usage() {
  cat <<'EOF'
Usage: build-linux-arm64-offline.sh [-t desktop|cli] [-all] [--arch arm64|amd64|win32] [--native|--docker] [--appimage]
  -t desktop   Build the Slint desktop distribution (default; ARM64 AppImage, amd64 optional AppImage)
  -t cli       Build the plain wunder-cli executable (never an AppImage)
  -all         Build both desktop and CLI for the selected architecture
  --arch       Select arm64, amd64, or win32 (default: arm64)
  --appimage   Package the amd64 desktop ELF as an AppImage (ignored for ARM64, invalid for Win32/CLI)
EOF
}
while (($#)); do
  case "$1" in
    -t|--target)
      shift; (($#)) || { usage >&2; exit 2; }
      case "$1" in desktop|cli) program="$1" ;; arm64|amd64|win32) arch="$1" ;; *) echo "Unsupported target: $1 (use desktop or cli)" >&2; exit 2 ;; esac ;;
    -all|--all) build_all=1 ;;
    --arch|-a) shift; (($#)) || { usage >&2; exit 2; }; arch="$1" ;;
    --native) mode=native ;;
    --docker) mode=docker ;;
    --appimage) package=1 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
  shift
done
case "$arch" in arm64|amd64|win32) ;; *) echo "Unsupported architecture: $arch" >&2; exit 2 ;; esac
if [[ "$package" == 1 && "$program" == cli ]]; then
  echo "--appimage cannot be used with -t cli; CLI builds are plain executables" >&2
  exit 2
fi
if [[ "$package" == 1 && "$arch" == win32 ]]; then
  echo "--appimage is only valid for Linux desktop builds" >&2
  exit 2
fi

if [[ "$mode" == docker && "${WUNDER_IN_DOCKER:-0}" != 1 ]]; then
  command -v docker >/dev/null 2>&1 || { echo "Docker is required" >&2; exit 2; }
  runtime_args=()
  if [[ "$program" == desktop || "$build_all" == 1 ]]; then
    if [[ "$arch" == arm64 ]]; then
      [[ -n "${WUNDER_APPIMAGE_RUNTIME:-}" ]] || { echo "WUNDER_APPIMAGE_RUNTIME must point to an ARM64 AppImage runtime" >&2; exit 2; }
      runtime_args=(-e WUNDER_APPIMAGE_RUNTIME=/builder/appimage-runtime/runtime.AppImage -v "${WUNDER_APPIMAGE_RUNTIME}:/builder/appimage-runtime/runtime.AppImage:ro")
    elif [[ "$arch" == amd64 ]]; then
      [[ -n "${WUNDER_APPIMAGE_RUNTIME:-}" ]] || { echo "WUNDER_APPIMAGE_RUNTIME must point to an x86_64 AppImage runtime" >&2; exit 2; }
      runtime_args=(-e WUNDER_APPIMAGE_RUNTIME=/builder/appimage-runtime/runtime.AppImage -v "${WUNDER_APPIMAGE_RUNTIME}:/builder/appimage-runtime/runtime.AppImage:ro")
    fi
  fi
  nested_args=(--native -t "$program" --arch "$arch")
  [[ "$build_all" == 1 ]] && nested_args+=(-all)
  [[ "$package" == 1 ]] && nested_args+=(--appimage)
  exec docker run --rm --network none --platform linux/arm64 \
    -e WUNDER_IN_DOCKER=1 -e WUNDER_REPO_ROOT=/workspace -e WUNDER_BUILDER_ROOT=/builder/kylin-arm -e WUNDER_OFFLINE_ROOT=/builder/kylin-arm/offline \
    "${runtime_args[@]}" -v "$repo_root:/workspace" -v "$builder_root:/builder/kylin-arm:ro" \
    -w /workspace "$image" bash /workspace/build-linux-arm64-offline.sh "${nested_args[@]}"
fi

run_one() {
  local selected="$1"
  local script
  if [[ "$selected" == desktop ]]; then
    case "$arch" in
      arm64) script="$repo_root/builders/build-linux-arm64-appimage.sh" ;;
      amd64)
        if [[ "$package" == 1 ]]; then
          script="$repo_root/builders/build-linux-amd64-appimage.sh"
        else
          script="$repo_root/builders/build-linux-amd64-offline.sh"
        fi
        ;;
      win32) script="$repo_root/builders/build-win32-arm64-offline.sh" ;;
    esac
  else
    case "$arch" in
      arm64) script="$repo_root/builders/build-cli-linux-arm64-offline.sh" ;;
      amd64) script="$repo_root/builders/build-cli-linux-amd64-offline.sh" ;;
      win32) script="$repo_root/builders/build-cli-win32-arm64-offline.sh" ;;
    esac
  fi
  WUNDER_REPO_ROOT="$repo_root" WUNDER_BUILDER_ROOT="$builder_root" WUNDER_OFFLINE_ROOT="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}" bash "$script"
}
if [[ "$build_all" == 1 ]]; then
  run_one desktop
  run_one cli
else
  run_one "$program"
fi
