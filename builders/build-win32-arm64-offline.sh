#!/usr/bin/env bash
# Build the Win32/Win7-compatible Slint executable on an ARM64 Linux host
# using the portable MinGW SDK prepared in the offline bundle.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin2}/offline}"
offline_root="$(cd "$offline_root" && pwd -P)"
toolchain="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu"
mingw_root="$offline_root/mingw-i686-windows-gnu"
target="i686-pc-windows-gnu"
manifest="$repo_root/frontend-slint/Cargo.toml"
output_dir="${WUNDER_OUTPUT_DIR:-$repo_root/target/slint/dist/win32-x86}"
fail() { echo "[wunder-slint-win32-arm64] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }

[[ "$(uname -m)" == aarch64 ]] || fail "ARM64 Linux host required"
require_file "$manifest"
[[ -x "$toolchain/bin/cargo" ]] || fail "Rust toolchain missing: $toolchain/bin/cargo"
[[ -f "$mingw_root/sdk-version" && "$(< "$mingw_root/sdk-version")" == wunder-win32-arm64-portable-v2 ]] || fail "portable MinGW SDK v2 is missing: $mingw_root"
[[ -x "$mingw_root/portable-bin/i686-w64-mingw32-gcc" ]] || fail "portable MinGW compiler is missing"
[[ -d "$toolchain/lib/rustlib/$target" ]] || fail "Rust std for $target is missing"
[[ -x "$mingw_root/llvm-tools/bin/llvm-readobj" && -f "$mingw_root/rust-target/lib/libgcc_eh.a" ]] || fail "portable MinGW SDK runtime or PE inspection tools are missing"

export RUSTC="$toolchain/bin/rustc"
export PATH="$toolchain/bin:$mingw_root/portable-bin:$PATH"
export CARGO_HOME="${CARGO_HOME:-$repo_root/target/win32-x86-arm64/cargo-home}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo_root/target/win32-x86-arm64/cargo}"
export CARGO_NET_OFFLINE=true
export CC_i686_pc_windows_gnu=i686-w64-mingw32-gcc
export CXX_i686_pc_windows_gnu=i686-w64-mingw32-g++
export AR_i686_pc_windows_gnu=i686-w64-mingw32-ar
export RANLIB_i686_pc_windows_gnu=i686-w64-mingw32-ranlib
export CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER=i686-w64-mingw32-rust-gcc
export RAYON_NUM_THREADS="${WUNDER_BUILD_JOBS:-2}"
unset GCC_EXEC_PREFIX COMPILER_PATH LIBRARY_PATH

vendor_root="${WUNDER_CARGO_VENDOR:-$offline_root/cargo-vendor-slint}"
[[ -d "$vendor_root" ]] || fail "offline Cargo vendor is missing: $vendor_root"
mkdir -p "$CARGO_HOME" "$CARGO_TARGET_DIR" "$output_dir"
cat > "$CARGO_HOME/config.toml" <<EOF
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

echo "[wunder-slint-win32-arm64] cargo: $(cargo --version)"
cd "$repo_root"
cargo build --locked --offline --release --target "$target" --manifest-path "$manifest" \
  --bin wunder-frontend-slint -j "${WUNDER_BUILD_JOBS:-2}"
exe="$CARGO_TARGET_DIR/$target/release/wunder-frontend-slint.exe"
require_file "$exe"
i686-w64-mingw32-readobj --file-headers "$exe" > "$CARGO_TARGET_DIR/pe-headers.txt"
grep -q 'IMAGE_FILE_MACHINE_I386' "$CARGO_TARGET_DIR/pe-headers.txt" || fail 'expected a 32-bit Windows PE'
imports="$(i686-w64-mingw32-readobj --coff-imports "$exe")"
if printf '%s\n' "$imports" | grep -Eiq 'combase\.dll|api-ms-win-|ext-ms-win-|GetDpiForWindow|WaitOnAddress|WakeByAddress|SetThreadDescription'; then
  fail 'Win32 cross output imports Win7-incompatible DLLs or symbols'
fi
i686-w64-mingw32-strip --strip-all "$exe"
version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail 'invalid package version'
release="$output_dir/wunder-frontend-slint-$version-win32-x86.exe"
cp -f "$exe" "$release"
printf '%s\n' "$imports" > "$output_dir/imports.txt"
echo "[wunder-slint-win32-arm64] produced: $release"
