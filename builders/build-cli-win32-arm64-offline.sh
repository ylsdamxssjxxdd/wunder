#!/usr/bin/env bash
# Cross-build the plain Win7-compatible wunder CLI executable from ARM64 Linux.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}/offline}"
offline_root="$(cd "$offline_root" && pwd -P)"
toolchain="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu"
mingw_root="$offline_root/mingw-i686-windows-gnu"
vendor_root="${WUNDER_CARGO_VENDOR:-$offline_root/cargo-vendor-slint}"
target="i686-pc-windows-gnu"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/cli-win32-x86-arm64/cargo}"
cargo_home="${CARGO_HOME:-$repo_root/target/cli-win32-x86-arm64/cargo-home}"
output_dir="${WUNDER_CLI_OUTPUT_DIR:-$repo_root/target/cli/dist/win32-x86}"

fail() { echo "[wunder-cli-win32-arm64] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }

[[ "$(uname -m)" == aarch64 ]] || fail "ARM64 Linux host required"
require_file "$repo_root/Cargo.toml"
[[ -x "$toolchain/bin/cargo" ]] || fail "Rust toolchain missing: $toolchain/bin/cargo"
[[ -f "$mingw_root/sdk-version" && "$(< "$mingw_root/sdk-version")" == wunder-win32-arm64-portable-v2 ]] || fail "portable MinGW SDK v2 is missing"
[[ -x "$mingw_root/portable-bin/i686-w64-mingw32-gcc" ]] || fail "portable MinGW compiler is missing"
[[ -d "$toolchain/lib/rustlib/$target" ]] || fail "Rust std for $target is missing"
[[ -x "$mingw_root/llvm-tools/bin/llvm-readobj" ]] || fail "PE inspection tool is missing"
[[ -d "$vendor_root" ]] || fail "shared offline Cargo vendor is missing: $vendor_root"

export RUSTC="$toolchain/bin/rustc"
export PATH="$toolchain/bin:$mingw_root/portable-bin:$PATH"
export CARGO_HOME="$cargo_home"
export CARGO_TARGET_DIR="$target_dir"
export CARGO_NET_OFFLINE=true
export CC_i686_pc_windows_gnu=i686-w64-mingw32-gcc
export CXX_i686_pc_windows_gnu=i686-w64-mingw32-g++
export AR_i686_pc_windows_gnu=i686-w64-mingw32-ar
export RANLIB_i686_pc_windows_gnu=i686-w64-mingw32-ranlib
export CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER=i686-w64-mingw32-rust-gcc
export RAYON_NUM_THREADS="${WUNDER_BUILD_JOBS:-2}"
unset GCC_EXEC_PREFIX COMPILER_PATH LIBRARY_PATH
mkdir -p "$cargo_home" "$target_dir" "$output_dir"
cat > "$cargo_home/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$vendor_root"
[target.$target]
linker = "i686-w64-mingw32-rust-gcc"
ar = "i686-w64-mingw32-ar"
[net]
offline = true
EOF

cd "$repo_root"
echo "[wunder-cli-win32-arm64] cargo: $(cargo --version)"
cargo build --locked --offline --release -p wunder-cli --bin wunder-cli --target "$target" -j "${WUNDER_BUILD_JOBS:-2}"

exe="$target_dir/$target/release/wunder-cli.exe"
require_file "$exe"
i686-w64-mingw32-readobj --file-headers "$exe" > "$target_dir/pe-headers.txt"
grep -q 'IMAGE_FILE_MACHINE_I386' "$target_dir/pe-headers.txt" || fail 'expected a 32-bit Windows PE'
imports="$(i686-w64-mingw32-readobj --coff-imports "$exe")"
if printf '%s\n' "$imports" | grep -Eiq 'combase\.dll|api-ms-win-|ext-ms-win-|GetDpiForWindow|WaitOnAddress|WakeByAddress|SetThreadDescription'; then
  fail 'Win32 cross output imports Win7-incompatible DLLs or symbols'
fi
i686-w64-mingw32-strip --strip-all "$exe"
version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$repo_root/Cargo.toml" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail 'could not read a safe workspace version'
release="$output_dir/wunder-cli-$version-win32-x86.exe"
cp -f "$exe" "$release"
printf '%s\n' "$imports" > "$output_dir/imports.txt"
echo "[wunder-cli-win32-arm64] produced: $release"
