#!/usr/bin/env bash
# Build the native Slint desktop for Linux x86_64 from an ARM64 Ubuntu 18.04
# host using the relocatable cross SDK from Rust-builder/kylin-arm.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}/offline}"
offline_root="$(cd "$offline_root" && pwd -P)"
sdk="$offline_root/linux-amd64-ubuntu18/root"
rust="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu"
target="x86_64-unknown-linux-gnu"
manifest="$repo_root/frontend-slint/Cargo.toml"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/linux-amd64-ubuntu18-slint/cargo}"
output_dir="${WUNDER_OUTPUT_DIR:-$repo_root/target/slint/dist/linux-amd64}"
max_glibc="${WUNDER_SLINT_LINUX_MAX_GLIBC:-2.27}"

fail() { echo "[wunder-slint-linux-amd64] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }
require_command() { command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"; }

[[ "$(uname -m)" == aarch64 ]] || fail "ARM64 Linux host required (use the Docker wrapper on other hosts)"
require_file "$manifest"
[[ -d "$sdk" ]] || fail "Ubuntu 18.04 amd64 SDK is missing: $sdk"
[[ -x "$rust/bin/cargo" ]] || fail "ARM64 Rust toolchain is missing: $rust/bin/cargo"
[[ -d "$rust/lib/rustlib/$target/lib" ]] || fail "Rust std for $target is missing: $rust/lib/rustlib/$target/lib"
vendor_root="${WUNDER_CARGO_VENDOR:-$offline_root/cargo-vendor-slint}"
[[ -d "$vendor_root" ]] || fail "offline Cargo vendor is missing: $vendor_root"
export PATH="$rust/bin:$sdk/usr/bin:$PATH"
export LD_LIBRARY_PATH="$sdk/usr/lib/aarch64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export WUNDER_AMD64_SDK_ROOT="$sdk"
unset GCC_EXEC_PREFIX COMPILER_PATH LIBRARY_PATH
alsa_linker=""
for candidate in "$sdk/usr/x86_64-linux-gnu/lib/libasound.so" "$sdk/usr/lib/x86_64-linux-gnu/libasound.so" "$sdk/lib/x86_64-linux-gnu/libasound.so"; do
  if [[ -e "$candidate" ]]; then alsa_linker="$candidate"; break; fi
done
[[ -n "$alsa_linker" ]] || fail "ALSA linker library is missing from the amd64 SDK; run builders/prepare-linux-amd64-runtime-sysroot.sh once in x86_64 Ubuntu 18.04"
for tool in cc x86_64-linux-gnu-gcc-7 x86_64-linux-gnu-readelf x86_64-linux-gnu-strip awk grep sed sort tail; do
  require_command "$tool"
done

export CARGO_HOME="${CARGO_HOME:-$repo_root/target/linux-amd64-ubuntu18-slint/cargo-home}"
export CARGO_TARGET_DIR="$target_dir"
export CARGO_NET_OFFLINE=true
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$repo_root/builders/linux-amd64-cross-cc.sh"
export CC_x86_64_unknown_linux_gnu="$repo_root/builders/linux-amd64-cross-cc.sh"
export CXX_x86_64_unknown_linux_gnu="$repo_root/builders/linux-amd64-cross-cc.sh --cxx"
export AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar
export CFLAGS_x86_64_unknown_linux_gnu="--sysroot=$sdk -I$sdk/usr/x86_64-linux-gnu/include"
export CXXFLAGS_x86_64_unknown_linux_gnu="$CFLAGS_x86_64_unknown_linux_gnu"

mkdir -p "$CARGO_HOME" "$target_dir" "$output_dir"
cat > "$CARGO_HOME/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$vendor_root"
[target.$target]
linker = "$repo_root/builders/linux-amd64-cross-cc.sh"
rustflags = ["-C", "link-arg=--sysroot=$sdk", "-C", "link-arg=-L$sdk/usr/x86_64-linux-gnu/lib"]
[net]
offline = true
EOF

echo "[wunder-slint-linux-amd64] cargo: $(cargo --version)"
echo "[wunder-slint-linux-amd64] sysroot: $sdk"
cd "$repo_root"
cargo build --locked --offline --release --manifest-path "$manifest" \
  --bin wunder-frontend-slint --target "$target" -j "${WUNDER_BUILD_JOBS:-2}"

binary="$target_dir/$target/release/wunder-frontend-slint"
require_file "$binary"
version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "could not read a safe package version"
release_binary="$output_dir/wunder-frontend-slint-$version-linux-amd64"
cp -f "$binary" "$release_binary"
x86_64-linux-gnu-strip --strip-all --strip-unneeded "$release_binary"
machine="$(LC_ALL=C x86_64-linux-gnu-readelf -h "$release_binary" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[[:space:]]+/, "", $2); print $2}')"
[[ "$machine" == "Advanced Micro Devices X86-64" ]] || fail "expected x86-64 ELF, got: ${machine:-unknown}"
required_glibc="$(LC_ALL=C x86_64-linux-gnu-readelf --version-info "$release_binary" | LC_ALL=C grep -oE 'GLIBC_[0-9.]+' | LC_ALL=C sed 's/GLIBC_//' | LC_ALL=C sort -Vu | LC_ALL=C tail -n 1 || true)"
[[ -n "$required_glibc" ]] || fail "could not determine executable GLIBC requirement"
[[ "$(printf '%s\n%s\n' "$max_glibc" "$required_glibc" | LC_ALL=C sort -V | tail -n 1)" == "$max_glibc" ]] || fail "GLIBC_$required_glibc exceeds release limit GLIBC_$max_glibc"
echo "[wunder-slint-linux-amd64] produced: $release_binary (GLIBC_$required_glibc)"
