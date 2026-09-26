#!/usr/bin/env bash
# Build the native ARM64 Linux wunder CLI from the shared kylin-arm SDK.
# CLI distribution is intentionally a plain executable, never an AppImage.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
if [[ -n "${WUNDER_BUILDER_ROOT:-}" ]]; then
  builder_root="$WUNDER_BUILDER_ROOT"
elif [[ -d "$repo_root/../Rust-builder/kylin-arm" ]]; then
  builder_root="$repo_root/../Rust-builder/kylin-arm"
else
  builder_root="/builder/kylin-arm"
fi
offline_root="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}"
rust="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu"
vendor_root="${WUNDER_CARGO_VENDOR:-$offline_root/cargo-vendor-slint}"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/cli-linux-arm64/cargo}"
cargo_home="${CARGO_HOME:-$repo_root/target/cli-linux-arm64/cargo-home}"
output_dir="${WUNDER_CLI_OUTPUT_DIR:-$repo_root/target/cli/dist/linux-arm64}"
max_glibc="${WUNDER_CLI_LINUX_MAX_GLIBC:-2.27}"

fail() { echo "[wunder-cli-linux-arm64] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }
require_command() { command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"; }

[[ "$(uname -m)" == aarch64 ]] || fail "ARM64 Linux host required"
require_file "$repo_root/Cargo.toml"
[[ -x "$rust/bin/cargo" ]] || fail "ARM64 Rust toolchain is missing: $rust/bin/cargo"
[[ -d "$vendor_root" ]] || fail "shared offline Cargo vendor is missing: $vendor_root"
for tool in cargo rustc readelf strip awk grep sed sort tail; do require_command "$tool"; done

export PATH="$rust/bin:$PATH"
export CARGO_HOME="$cargo_home"
export CARGO_TARGET_DIR="$target_dir"
export CARGO_NET_OFFLINE=true
mkdir -p "$cargo_home" "$target_dir" "$output_dir"
cat > "$cargo_home/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$vendor_root"
[net]
offline = true
EOF

cd "$repo_root"
echo "[wunder-cli-linux-arm64] cargo: $(cargo --version)"
cargo build --locked --offline --release -p wunder-cli --bin wunder-cli -j "${WUNDER_BUILD_JOBS:-2}"

binary="$target_dir/release/wunder-cli"
require_file "$binary"
version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$repo_root/Cargo.toml" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "could not read a safe workspace version"
release_binary="$output_dir/wunder-cli-$version-linux-arm64"
cp -f "$binary" "$release_binary"
strip --strip-all --strip-unneeded "$release_binary"
# The SDK image ships mawk, whose regex lacks [[:space:]]; trim with sed.
machine="$(LC_ALL=C readelf -h "$release_binary" | LC_ALL=C awk -F: '/Machine:/{print $2}' | LC_ALL=C sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
[[ "$machine" == AArch64 ]] || fail "expected AArch64 ELF, got: ${machine:-unknown}"
required_glibc="$(LC_ALL=C readelf --version-info "$release_binary" | LC_ALL=C grep -oE 'GLIBC_[0-9.]+' | LC_ALL=C sed 's/GLIBC_//' | LC_ALL=C sort -Vu | LC_ALL=C tail -n 1 || true)"
[[ -n "$required_glibc" ]] || fail "could not determine executable GLIBC requirement"
[[ "$(printf '%s\n%s\n' "$max_glibc" "$required_glibc" | LC_ALL=C sort -V | tail -n 1)" == "$max_glibc" ]] || fail "GLIBC_$required_glibc exceeds release limit GLIBC_$max_glibc"
echo "[wunder-cli-linux-arm64] produced: $release_binary (GLIBC_$required_glibc)"
