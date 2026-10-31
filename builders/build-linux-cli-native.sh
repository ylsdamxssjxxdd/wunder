#!/usr/bin/env bash
set -euo pipefail

# Build the plain Linux CLI inside the Ubuntu 18.04 CI image. This mirrors the
# Native CLI entry: amd64 compiles natively, arm64 cross-compiles with the
# target that the image installs. CLI distribution is always a bare ELF, never
# an archive; prompts and skills are embedded into the binary at build time.

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
output_dir="${WUNDER_CLI_OUTPUT_DIR:-}"
target_dir="${CARGO_TARGET_DIR:-}"
max_glibc="${WUNDER_CLI_LINUX_MAX_GLIBC:-2.27}"

fail() { echo "[wunder-cli-linux] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }
require_command() { command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"; }

case "${ARCH:-$(uname -m)}" in
  x86_64|amd64)
    arch="amd64"
    target="x86_64-unknown-linux-gnu"
    elf_machine="Advanced Micro Devices X86-64"
    native=1
    ;;
  aarch64|arm64)
    arch="arm64"
    target="aarch64-unknown-linux-gnu"
    elf_machine="AArch64"
    native=0
    ;;
  *)
    fail "unsupported architecture: ${ARCH:-$(uname -m)}"
    ;;
esac

output_dir="${output_dir:-$repo_root/target/cli/dist/linux-$arch}"
target_dir="${target_dir:-$repo_root/target/cli-linux-$arch-ubuntu18-native/cargo}"

require_file "$repo_root/Cargo.toml"
for tool in cargo rustc readelf strip awk grep sed sort tail; do
  require_command "$tool"
done

build_args=(--locked --release -p wunder-cli --bin wunder-cli)
if [[ "$native" == 0 ]]; then
  linker="${WUNDER_ARM64_LINKER:-aarch64-linux-gnu-gcc}"
  readelf_tool="${WUNDER_ARM64_READELF:-aarch64-linux-gnu-readelf}"
  strip_tool="${WUNDER_ARM64_STRIP:-aarch64-linux-gnu-strip}"
  for tool in "$linker" aarch64-linux-gnu-g++ aarch64-linux-gnu-ar "$readelf_tool" "$strip_tool"; do
    require_command "$tool"
  done
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$linker"
  export CC_aarch64_unknown_linux_gnu="$linker"
  export CXX_aarch64_unknown_linux_gnu="aarch64-linux-gnu-g++"
  export AR_aarch64_unknown_linux_gnu="aarch64-linux-gnu-ar"
  export PKG_CONFIG_ALLOW_CROSS=1
  export PKG_CONFIG_SYSROOT_DIR=/
  export PKG_CONFIG_PATH="/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig"
  build_args+=(--target "$target")
else
  readelf_tool="readelf"
  strip_tool="strip"
fi

mkdir -p "$target_dir" "$output_dir"
export CARGO_TARGET_DIR="$target_dir"
cd "$repo_root"

echo "[wunder-cli-linux] cargo: $(cargo --version)"
echo "[wunder-cli-linux] rustc: $(rustc --version)"
echo "[wunder-cli-linux] baseline: $(ldd --version | head -n 1)"
cargo build "${build_args[@]}"

binary="$target_dir/release/wunder-cli"
if [[ "$native" == 0 ]]; then
  binary="$target_dir/$target/release/wunder-cli"
fi
require_file "$binary"

machine="$(LC_ALL=C "$readelf_tool" -h "$binary" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[ \t]+/, "", $2); print $2}')"
[[ "$machine" == "$elf_machine" ]] || fail "expected $arch ELF, got: ${machine:-unknown}"

version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$repo_root/Cargo.toml" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "could not read a safe workspace version"
release_binary="$output_dir/wunder-cli-$version-linux-$arch"
cp -f "$binary" "$release_binary"
"$strip_tool" --strip-all --strip-unneeded "$release_binary"

machine="$(LC_ALL=C "$readelf_tool" -h "$release_binary" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[ \t]+/, "", $2); print $2}')"
[[ "$machine" == "$elf_machine" ]] || fail "stripped release is not $arch: ${machine:-unknown}"
required_glibc="$(LC_ALL=C "$readelf_tool" --version-info "$release_binary" 2>/dev/null | LC_ALL=C grep -oE 'GLIBC_[0-9.]+' | LC_ALL=C sed 's/GLIBC_//' | LC_ALL=C sort -Vu | LC_ALL=C tail -n 1 || true)"
[[ -n "$required_glibc" ]] || fail "could not determine executable GLIBC requirement"
[[ "$(printf '%s\n%s\n' "$max_glibc" "$required_glibc" | LC_ALL=C sort -V | LC_ALL=C tail -n 1)" == "$max_glibc" ]] || fail "GLIBC_$required_glibc exceeds release limit GLIBC_$max_glibc"

echo "[wunder-cli-linux] produced: $release_binary (GLIBC_$required_glibc)"
