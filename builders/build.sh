#!/usr/bin/env bash
# Unified release entry for the Linux ARM64 build host. Root build.sh forwards
# here so all architecture-specific builders remain under builders/.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd -P)}"
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}"
offline_root="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}"
target=""
arch=""
build_all=0
mode=native
package=0

usage() {
  cat <<'EOF'
Usage:
  build.sh -t desktop|cli -a linux-arm64|linux-amd64|win7-x86 [--appimage] [--native|--docker]
  build.sh -all [--native|--docker]

Options:
  -t, --target     Build one program: desktop or cli.
  -a, --arch       Select one distribution architecture.
                  linux-arm64: Desktop AppImage or CLI ELF.
                  linux-amd64: Desktop ELF (or AppImage with --appimage) or CLI ELF.
                  win7-x86:    Win7-compatible Desktop or CLI PE built by kylin-arm
                              (i686-win7-windows-gnu with -Z build-std).
  -all, --all      Build every distribution available on this host: both programs for
                  Linux ARM64, Linux amd64 and Win7 x86. Linux Desktop outputs are AppImages.
  --appimage       Package a linux-amd64 Desktop build as an AppImage. CLI never uses AppImage.
  --native         Build on an ARM64 Linux host (default).
  --docker         Run the ARM64 Ubuntu 18.04 build image. Useful from non-ARM Linux hosts.

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
    -t|--target)
      shift
      (($#)) || fail "missing value after --target"
      target="$1"
      ;;
    -a|--arch)
      shift
      (($#)) || fail "missing value after --arch"
      arch="$(normalize_arch "$1")" || fail "unsupported architecture: $1"
      ;;
    -all|--all) build_all=1 ;;
    --appimage) package=1 ;;
    --native) mode=native ;;
    --docker) mode=docker ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unsupported option: $1" ;;
  esac
  shift
done

if [[ "$build_all" == 1 ]]; then
  [[ -z "$target" && -z "$arch" ]] || fail "-all already selects every program and architecture; do not combine it with --target or --arch"
  [[ "$package" == 0 ]] || fail "-all already packages every Linux Desktop distribution as AppImage"
else
  [[ "$target" == desktop || "$target" == cli ]] || fail "--target must be desktop or cli"
  [[ -n "$arch" ]] || fail "--arch is required unless -all is used"
fi

if [[ "$package" == 1 && "$target" != desktop ]]; then
  fail "--appimage is only valid for a Desktop build; CLI outputs are plain executables"
fi
if [[ "$package" == 1 && "$arch" != linux-amd64 ]]; then
  fail "--appimage is only valid for linux-amd64; linux-arm64 Desktop is always an AppImage"
fi

runtime_for() {
  local runtime_arch="$1"
  local runtime=""
  case "$runtime_arch" in
    linux-arm64)
      runtime="${WUNDER_APPIMAGE_RUNTIME_ARM64:-}"
      [[ -n "$runtime" || "$build_all" == 1 ]] || runtime="${WUNDER_APPIMAGE_RUNTIME:-}"
      ;;
    linux-amd64)
      runtime="${WUNDER_APPIMAGE_RUNTIME_AMD64:-}"
      [[ -n "$runtime" || "$build_all" == 1 ]] || runtime="${WUNDER_APPIMAGE_RUNTIME:-}"
      ;;
  esac
  local variable_name="WUNDER_APPIMAGE_RUNTIME_${runtime_arch#linux-}"
  variable_name="${variable_name^^}"
  [[ -n "$runtime" ]] || fail "set ${variable_name} for the ${runtime_arch} AppImage runtime"
  [[ -f "$runtime" ]] || fail "AppImage runtime does not exist: $runtime"
  printf '%s\n' "$runtime"
}

# Build only the requested target in an isolated container. All architecture
# selection is forwarded as arguments instead of being reconstructed from a
# shell string, preventing an empty optional argument from changing the call.
run_in_docker() {
  command -v docker >/dev/null 2>&1 || fail "Docker is required for --docker"
  [[ -d "$builder_root/offline" ]] || fail "kylin-arm offline SDK is missing: $builder_root/offline"

  local -a docker_args=(
    run --rm --network none --platform linux/arm64
    -e WUNDER_IN_DOCKER=1
    -e WUNDER_REPO_ROOT=/workspace
    -e WUNDER_BUILDER_ROOT=/builder/kylin-arm
    -e WUNDER_OFFLINE_ROOT=/builder/kylin-arm/offline
    -v "$repo_root:/workspace"
    -v "$builder_root:/builder/kylin-arm:ro"
  )
  local runtime
  if [[ "$build_all" == 1 || ( "$target" == desktop && "$arch" == linux-arm64 ) ]]; then
    runtime="$(runtime_for linux-arm64)"
    docker_args+=(-e WUNDER_APPIMAGE_RUNTIME_ARM64=/builder/appimage-runtime/linux-arm64.AppImage -v "$runtime:/builder/appimage-runtime/linux-arm64.AppImage:ro")
  fi
  if [[ "$build_all" == 1 || ( "$target" == desktop && "$arch" == linux-amd64 && "$package" == 1 ) ]]; then
    runtime="$(runtime_for linux-amd64)"
    docker_args+=(-e WUNDER_APPIMAGE_RUNTIME_AMD64=/builder/appimage-runtime/linux-amd64.AppImage -v "$runtime:/builder/appimage-runtime/linux-amd64.AppImage:ro")
  fi

  local -a nested_args=(--native)
  if [[ "$build_all" == 1 ]]; then
    nested_args+=(-all)
  else
    nested_args+=(-t "$target" -a "$arch")
    [[ "$package" == 1 ]] && nested_args+=(--appimage)
  fi
  exec docker "${docker_args[@]}" -w /workspace "${WUNDER_SLINT_LINUX_DOCKER_IMAGE:-rcho-slint-arm64-ubuntu18:latest}" bash /workspace/build.sh "${nested_args[@]}"
}

if [[ "$mode" == docker && "${WUNDER_IN_DOCKER:-0}" != 1 ]]; then
  run_in_docker
fi

[[ "$(uname -s)" == Linux && "$(uname -m)" == aarch64 ]] || fail "an ARM64 Linux host is required; use --docker on another Linux host"
[[ -d "$offline_root" ]] || fail "kylin-arm offline SDK is missing: $offline_root"

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
  local as_appimage="${3:-0}"
  local script=""
  local runtime=""

  case "$selected_arch:$selected_target" in
    linux-arm64:desktop)
      script="$repo_root/builders/build-linux-arm64-appimage.sh"
      runtime="$(runtime_for linux-arm64)"
      WUNDER_APPIMAGE_RUNTIME="$runtime" run_builder "$script" env
      ;;
    linux-arm64:cli)
      run_builder "$repo_root/builders/build-cli-linux-arm64-offline.sh" env
      ;;
    linux-amd64:desktop)
      if [[ "$as_appimage" == 1 ]]; then
        runtime="$(runtime_for linux-amd64)"
        WUNDER_APPIMAGE_RUNTIME="$runtime" run_builder "$repo_root/builders/build-linux-amd64-appimage.sh" env
      else
        run_builder "$repo_root/builders/build-linux-amd64-offline.sh" env
      fi
      ;;
    linux-amd64:cli)
      run_builder "$repo_root/builders/build-cli-linux-amd64-offline.sh" env
      ;;
    win7-x86:desktop)
      run_builder "$repo_root/builders/build-win7-arm64-offline.sh" env
      ;;
    win7-x86:cli)
      run_builder "$repo_root/builders/build-cli-win7-arm64-offline.sh" env
      ;;
    *) fail "unsupported build matrix entry: $selected_target/$selected_arch" ;;
  esac
}

if [[ "$build_all" == 1 ]]; then
  # Resolve both runtimes before publishing any artifact. This makes a release
  # invocation fail early instead of leaving a partial architecture set.
  runtime_for linux-arm64 >/dev/null
  runtime_for linux-amd64 >/dev/null
  build_one desktop linux-arm64 1
  build_one cli linux-arm64 0
  build_one desktop linux-amd64 1
  build_one cli linux-amd64 0
  build_one desktop win7-x86 0
  build_one cli win7-x86 0
else
  build_one "$target" "$arch" "$package"
fi
