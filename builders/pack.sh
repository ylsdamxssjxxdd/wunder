#!/usr/bin/env bash
# Assemble the Linux .run installer from already-built artifacts. This entry
# runs on any Linux host with Python 3; it never rebuilds Rust binaries.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "$0")/.." && pwd -P)"
script="$repo_root/packaging/linux/scripts/build_linux_installer.py"
arch=""
desktop=""
cli=""
supplement=""
output_dir=""
core_only=0

usage() {
  cat <<'EOF'
Usage:
  build.sh pack -a linux-amd64|linux-arm64 [options]

Options:
  -a, --arch            Target architecture. Defaults to the host architecture.
      --desktop PATH    Desktop AppImage. Defaults to target/slint/dist/linux-<arch>/.
      --cli PATH        CLI ELF. Defaults to target/cli/dist/linux-<arch>/.
      --supplement PATH Supplement .tar.gz. Defaults to the known build output
                        locations, or dist/linux-<arch>/.
  -o, --output-dir DIR  Output directory. Defaults to target/installer/dist/linux-<arch>/.
      --core-only       Build an installer without any optional components.
  -h, --help            Show this help.

The Windows installer is built with `build.bat pack -a win7-x86` on Windows.
EOF
}

fail() {
  echo "[wunder-pack] $*" >&2
  exit 2
}

while (($#)); do
  case "$1" in
    -a|--arch)
      shift
      (($#)) || fail "missing value after $1"
      arch="$1"
      ;;
    --desktop)
      shift
      (($#)) || fail "missing value after --desktop"
      desktop="$1"
      ;;
    --cli)
      shift
      (($#)) || fail "missing value after --cli"
      cli="$1"
      ;;
    --supplement)
      shift
      (($#)) || fail "missing value after --supplement"
      supplement="$1"
      ;;
    -o|--output-dir)
      shift
      (($#)) || fail "missing value after $1"
      output_dir="$1"
      ;;
    --core-only) core_only=1 ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unsupported option: $1" ;;
  esac
  shift
done

if [[ -z "$arch" ]]; then
  case "$(uname -m)" in
    x86_64|amd64) arch="linux-amd64" ;;
    aarch64|arm64) arch="linux-arm64" ;;
    *) fail "could not infer the host architecture; pass -a linux-amd64|linux-arm64" ;;
  esac
fi

case "$arch" in
  linux-amd64|linux-arm64) ;;
  win7-x86) fail "the Windows installer must be built with build.bat pack on Windows" ;;
  *) fail "unsupported architecture: $arch" ;;
esac

python=""
for candidate in python3 python; do
  if command -v "$candidate" >/dev/null 2>&1 &&
    "$candidate" -c 'import sys; raise SystemExit(0 if sys.version_info[0] == 3 else 1)' >/dev/null 2>&1; then
    python="$candidate"
    break
  fi
done
[[ -n "$python" ]] || fail "Python 3 is required to assemble the Linux installer"

args=(--arch "${arch#linux-}")
[[ -n "$desktop" ]] && args+=(--desktop "$desktop")
[[ -n "$cli" ]] && args+=(--cli "$cli")
[[ -n "$supplement" ]] && args+=(--supplement "$supplement")
[[ -n "$output_dir" ]] && args+=(--output-dir "$output_dir")
[[ "$core_only" == 1 ]] && args+=(--core-only)

exec "$python" "$script" "${args[@]}"
