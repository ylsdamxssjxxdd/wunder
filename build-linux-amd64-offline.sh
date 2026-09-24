#!/usr/bin/env bash
# Unified ARM64-host cross build entry for Linux x86_64 desktop/CLI outputs.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "$0")" && pwd -P)"
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}"
image="${WUNDER_LINUX_CROSS_IMAGE:-rcho-slint-arm64-ubuntu18:latest}"
program=desktop
build_all=0
mode=native
package=0
usage() {
  cat <<'EOF'
Usage: build-linux-amd64-offline.sh [-t desktop|cli] [-all] [--native|--docker] [--appimage]
  -t desktop   Build the x86_64 Slint desktop ELF (default)
  -t cli       Build the x86_64 wunder-cli executable
  -all         Build both desktop and CLI executables
  --appimage   Package the desktop ELF as an AppImage; invalid for CLI or -all
EOF
}
while (($#)); do
  case "$1" in
    -t|--target) shift; (($#)) || { usage >&2; exit 2; }; case "$1" in desktop|cli) program="$1" ;; *) echo 'program must be desktop or cli' >&2; exit 2 ;; esac ;;
    -all|--all) build_all=1 ;;
    --native) mode=native ;;
    --docker) mode=docker ;;
    --appimage) package=1 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
  shift
done
if [[ "$package" == 1 && ( "$program" == cli || "$build_all" == 1 ) ]]; then
  echo "--appimage is only valid for a desktop-only build; CLI builds are plain executables" >&2
  exit 2
fi
if [[ "$mode" == docker && "${WUNDER_IN_DOCKER:-0}" != 1 ]]; then
  command -v docker >/dev/null || { echo 'Docker is required' >&2; exit 2; }
  runtime_args=()
  if [[ "$package" == 1 ]]; then
    [[ -n "${WUNDER_APPIMAGE_RUNTIME:-}" ]] || { echo 'WUNDER_APPIMAGE_RUNTIME must point to an x86_64 AppImage runtime' >&2; exit 2; }
    runtime_args=(-e WUNDER_APPIMAGE_RUNTIME=/builder/appimage-runtime/runtime.AppImage -v "${WUNDER_APPIMAGE_RUNTIME}:/builder/appimage-runtime/runtime.AppImage:ro")
  fi
  nested_args=(--native -t "$program")
  [[ "$build_all" == 1 ]] && nested_args+=(-all)
  [[ "$package" == 1 ]] && nested_args+=(--appimage)
  exec docker run --rm --network none --platform linux/arm64 -e WUNDER_IN_DOCKER=1 -e WUNDER_REPO_ROOT=/workspace -e WUNDER_BUILDER_ROOT=/builder/kylin-arm -e WUNDER_OFFLINE_ROOT=/builder/kylin-arm/offline "${runtime_args[@]}" -v "$repo_root:/workspace" -v "$builder_root:/builder/kylin-arm:ro" -w /workspace "$image" bash /workspace/build-linux-amd64-offline.sh "${nested_args[@]}"
fi
run_one() {
  if [[ "$1" == desktop ]]; then
    if [[ "$package" == 1 ]]; then
      bash "$repo_root/builders/build-linux-amd64-appimage.sh"
    else
      bash "$repo_root/builders/build-linux-amd64-offline.sh"
    fi
  else
    bash "$repo_root/builders/build-cli-linux-amd64-offline.sh"
  fi
}
if [[ "$build_all" == 1 ]]; then run_one desktop; run_one cli; else run_one "$program"; fi
