#!/usr/bin/env bash
# Unified release entry for the Linux ARM64 build host. Root build.sh forwards
# here so all architecture-specific builders remain under builders/.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd -P)}"
host_arch="$(uname -m)"
case "$host_arch" in
  aarch64) host_profile=kylin-arm ;;
  x86_64) host_profile=kylin-x86 ;;
  *) echo "[wunder-build] unsupported Linux host: ${host_arch:-unknown}" >&2; exit 2 ;;
esac
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/$host_profile}"
offline_root="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}"
# The Docker image is an ARM64 Ubuntu 18.04 builder and always reads kylin-arm,
# even when the host itself defaults to kylin-x86.
docker_builder_root="${WUNDER_DOCKER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}"
target=""
arch=""
build_all=0
build_egui=0
mode=native

usage() {
  cat <<'EOF'
Usage:
  build.sh -t desktop|cli -a linux-arm64|linux-amd64|win7-x86 [--native|--docker]
  build.sh -all [--native|--docker]
  build.sh --egui

Options:
  -t, -target, --target  Build one program: desktop or cli.
  -a, -arch, --arch      Select one distribution architecture.
                         linux-arm64: Desktop AppDir archive or CLI ELF.
                         linux-amd64: Desktop AppDir archive or CLI ELF.
                         win7-x86:    Win7-compatible Desktop or CLI PE
                                      (i686-win7-windows-gnu with -Z build-std).
  -all, --all            Build every distribution available on this host: both programs for
                         Linux ARM64, Linux amd64 and Win7 x86. Linux Desktop outputs are AppDir archives.
  --egui                 Build the native wunder-egui desktop into target/wunder-egui/release.
  --native         Build directly on this Linux host (default).
  --docker         Run the ARM64 Ubuntu 18.04 build image. Requires Rust-builder/kylin-arm.

Host profiles (chosen from uname -m, override the SDK with WUNDER_BUILDER_ROOT):
  aarch64  Rust-builder/kylin-arm builds every target natively.
  x86_64   Rust-builder/kylin-x86 builds Linux amd64 natively and cross-builds
           Linux ARM64 and Win7 x86 with the SDK toolchains.

On Windows, use build.bat. Its win7-x86 target uses Rust-builder/win7 directly.
EOF
}

normalize_arch() {
  case "$1" in
    linux-arm64|arm64) printf '%s\n' linux-arm64 ;;
    linux-amd64|amd64|linux-x86_64) printf '%s\n' linux-amd64 ;;
    win7-x86|win7|win32-x86|win32|windows-x86) printf '%s\n' win7-x86 ;;
    *) return 1 ;;
  esac
}

fail() {
  echo "[wunder-build] $*" >&2
  exit 2
}

while (($#)); do
  case "$1" in
    -t|-target|--target)
      shift
      (($#)) || fail "missing value after -t"
      target="$1"
      ;;
    -a|-arch|--arch)
      shift
      (($#)) || fail "missing value after -a"
      arch="$(normalize_arch "$1")" || fail "unsupported architecture: $1"
      ;;
    -all|--all) build_all=1 ;;
    --egui) build_egui=1 ;;
    --native) mode=native ;;
    --docker) mode=docker ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unsupported option: $1" ;;
  esac
  shift
done

if [[ "$build_egui" == 1 ]]; then
  [[ "$build_all" == 0 && -z "$target" && -z "$arch" ]] || fail "--egui cannot be combined with --all, --target or --arch"
  [[ "$mode" == native ]] || fail "--egui currently builds on the host; omit --docker"
  egui_target_dir="$repo_root/target/wunder-egui"
  mkdir -p "$egui_target_dir"
  CARGO_TARGET_DIR="$egui_target_dir" cargo build --locked --release -p wunder-egui --features native
  printf '[wunder-build] egui native artifact: %s\n' "$egui_target_dir/release/wunder-egui"
  exit 0
fi

if [[ "$build_all" == 1 ]]; then
  [[ -z "$target" && -z "$arch" ]] || fail "-all already selects every program and architecture; do not combine it with --target or --arch"
else
  [[ "$target" == desktop || "$target" == cli ]] || fail "--target must be desktop or cli"
  [[ -n "$arch" ]] || fail "--arch is required unless -all is used"
fi

# Build only the requested target in an isolated container. All architecture
# selection is forwarded as arguments instead of being reconstructed from a
# shell string, preventing an empty optional argument from changing the call.
run_in_docker() {
  command -v docker >/dev/null 2>&1 || fail "Docker is required for --docker"
  [[ -d "$docker_builder_root/offline" ]] || fail "kylin-arm offline SDK is missing: $docker_builder_root/offline (set WUNDER_DOCKER_BUILDER_ROOT)"

  local -a docker_args=(
    run --rm --network none --platform linux/arm64
    -e WUNDER_IN_DOCKER=1
    -e WUNDER_REPO_ROOT=/workspace
    -e WUNDER_BUILDER_ROOT=/builder/kylin-arm
    -e WUNDER_OFFLINE_ROOT=/builder/kylin-arm/offline
    -v "$repo_root:/workspace"
    -v "$docker_builder_root:/builder/kylin-arm:ro"
  )
  local -a nested_args=(--native)
  if [[ "$build_all" == 1 ]]; then
    nested_args+=(-all)
  else
    nested_args+=(-t "$target" -a "$arch")
  fi
  exec docker "${docker_args[@]}" -w /workspace "${WUNDER_SLINT_LINUX_DOCKER_IMAGE:-rcho-slint-arm64-ubuntu18:latest}" bash /workspace/build.sh "${nested_args[@]}"
}

if [[ "$mode" == docker && "${WUNDER_IN_DOCKER:-0}" != 1 ]]; then
  run_in_docker
fi

[[ "$(uname -s)" == Linux ]] || fail "Linux host required; use build.bat on Windows"
[[ -d "$offline_root" ]] || fail "offline SDK is missing: $offline_root (expected the $host_profile profile)"

run_builder() {
  local script="$1"
  shift
  WUNDER_REPO_ROOT="$repo_root" \
    WUNDER_BUILDER_ROOT="$builder_root" \
    WUNDER_OFFLINE_ROOT="$offline_root" \
    "$@" bash "$script"
}

build_one() {
  local selected_target="$1"
  local selected_arch="$2"
  local script=""

  case "$selected_arch:$selected_target" in
    linux-arm64:desktop)
      run_builder "$repo_root/builders/build-linux-arm64-desktop.sh" env
      ;;
    linux-arm64:cli)
      run_builder "$repo_root/builders/build-cli-linux-arm64-offline.sh" env
      ;;
    linux-amd64:desktop)
      run_builder "$repo_root/builders/build-linux-amd64-desktop.sh" env
      ;;
    linux-amd64:cli)
      run_builder "$repo_root/builders/build-cli-linux-amd64-offline.sh" env
      ;;
    win7-x86:desktop)
      if [[ "$host_arch" == x86_64 ]]; then
        run_builder "$repo_root/builders/build-win7-x86-offline.sh" env
      else
        run_builder "$repo_root/builders/build-win7-arm64-offline.sh" env
      fi
      ;;
    win7-x86:cli)
      if [[ "$host_arch" == x86_64 ]]; then
        run_builder "$repo_root/builders/build-cli-win7-x86-offline.sh" env
      else
        run_builder "$repo_root/builders/build-cli-win7-arm64-offline.sh" env
      fi
      ;;
    *) fail "unsupported build matrix entry: $selected_target/$selected_arch" ;;
  esac
}

if [[ "$build_all" == 1 ]]; then
  build_one desktop linux-arm64
  build_one cli linux-arm64
  build_one desktop linux-amd64
  build_one cli linux-amd64
  build_one desktop win7-x86
  build_one cli win7-x86
else
  build_one "$target" "$arch"
fi
