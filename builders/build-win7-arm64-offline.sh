#!/usr/bin/env bash
# Cross-build the Win7-compatible wunder Slint desktop executable from an
# ARM64 Linux host. Mirrors build-win7-slint.ps1: i686-win7-windows-gnu is a
# Tier 3 target without prebuilt std, so std is rebuilt with -Z build-std;
# the kylin-arm portable MinGW SDK supplies linker, CRT and import libraries.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}/offline}"
offline_root="$(cd "$offline_root" && pwd -P)"
toolchain="$offline_root/rust/toolchains/nightly-2026-03-14-aarch64-unknown-linux-gnu"
mingw_root="$offline_root/mingw-i686-windows-gnu"
vendor_root="${WUNDER_CARGO_VENDOR:-$offline_root/cargo-vendor-slint}"
manifest="$repo_root/frontend-slint/Cargo.toml"
target="i686-win7-windows-gnu"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/win7-x86-arm64/cargo}"
cargo_home="${CARGO_HOME:-$repo_root/target/win7-x86-arm64/cargo-home}"
output_dir="${WUNDER_OUTPUT_DIR:-$repo_root/target/slint/dist/win7-x86}"

fail() { echo "[wunder-slint-win7-arm64] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }

[[ "$(uname -m)" == aarch64 ]] || fail "ARM64 Linux host required"
require_file "$manifest"
[[ -x "$toolchain/bin/cargo" && -x "$toolchain/bin/rustc" ]] || fail "Rust toolchain missing: $toolchain/bin"
[[ -d "$toolchain/lib/rustlib/src/rust" ]] || fail "rust-src component missing (required by -Z build-std): $toolchain"
[[ -f "$mingw_root/sdk-version" ]] || fail "portable MinGW SDK is missing: $mingw_root"
case "$(< "$mingw_root/sdk-version")" in
  wunder-win32-arm64-portable-v2|rcho-win32-arm64-portable-v2) ;;
  *) fail "unsupported portable MinGW SDK stamp: $(< "$mingw_root/sdk-version")" ;;
esac
[[ -x "$mingw_root/portable-bin/i686-w64-mingw32-gcc" ]] || fail "portable MinGW compiler is missing"
[[ -x "$mingw_root/llvm-tools/bin/llvm-readobj" && -f "$mingw_root/rust-target/lib/libgcc_eh.a" ]] || fail "portable MinGW SDK runtime or PE inspection tools are missing"
[[ -d "$vendor_root" ]] || fail "shared offline Cargo vendor is missing: $vendor_root"

export RUSTC="$toolchain/bin/rustc"
export PATH="$toolchain/bin:$mingw_root/portable-bin:$PATH"
# frontend-slint's build.rs probes this dir for the windres that embeds the
# exe icon resource (same variable the Windows-host win7 build exports).
export WUNDER_WIN7_MINGW_BIN="$mingw_root/portable-bin"
export CARGO_HOME="$cargo_home"
export CARGO_TARGET_DIR="$target_dir"
export CARGO_NET_OFFLINE=true
export CC_i686_win7_windows_gnu=i686-w64-mingw32-gcc
export CXX_i686_win7_windows_gnu=i686-w64-mingw32-g++
export AR_i686_win7_windows_gnu=i686-w64-mingw32-ar
export RANLIB_i686_win7_windows_gnu=i686-w64-mingw32-ranlib
export CARGO_TARGET_I686_WIN7_WINDOWS_GNU_LINKER=i686-w64-mingw32-rust-gcc
export RAYON_NUM_THREADS="${WUNDER_BUILD_JOBS:-2}"
unset GCC_EXEC_PREFIX COMPILER_PATH LIBRARY_PATH

# windows-targets build scripts only recognize the pc/uwp target_vendor, so
# resolve the vendored GNU import libraries explicitly for the win7 vendor
# target, exactly like build-win7-slint.ps1 does on the Windows host. Note
# 0.42-era crates ship libwindows.a without the version infix.
native_link_flags=()
for lib_dir in "$vendor_root"/windows_i686_gnu-*/lib; do
  ls "$lib_dir"/libwindows*.a >/dev/null 2>&1 || fail "missing Windows import library in: $lib_dir"
  native_link_flags+=("-Lnative=$lib_dir")
done
[[ ${#native_link_flags[@]} -gt 0 ]] || fail "windows_i686_gnu not found in vendored sources: $vendor_root"
export CARGO_TARGET_I686_WIN7_WINDOWS_GNU_RUSTFLAGS="${native_link_flags[*]}"

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

echo "[wunder-slint-win7-arm64] cargo: $(cargo --version)"
cd "$repo_root"
# build-std rebuilds std with the win7 vendor gating; --locked pins the
# application dependency graph while std resolves from the shipped rust-src
# against the same vendored registry.
cargo -Z build-std=std,panic_abort build --locked --offline --release \
  --manifest-path "$manifest" --bin wunder-frontend-slint \
  --target "$target" -j "${WUNDER_BUILD_JOBS:-2}"

exe="$target_dir/$target/release/wunder-frontend-slint.exe"
require_file "$exe"
i686-w64-mingw32-readobj --file-headers "$exe" > "$target_dir/pe-headers.txt"
grep -q 'IMAGE_FILE_MACHINE_I386' "$target_dir/pe-headers.txt" || fail 'expected a 32-bit Windows PE'
imports="$(i686-w64-mingw32-readobj --coff-imports "$exe")"
if printf '%s\n' "$imports" | grep -Eiq 'combase\.dll|api-ms-win-|ext-ms-win-|GetDpiForWindow|WaitOnAddress|WakeByAddress|SetThreadDescription'; then
  fail 'Win7 cross output imports Win7-incompatible DLLs or symbols'
fi
i686-w64-mingw32-strip --strip-all "$exe"
# The SDK image ships mawk, whose regex lacks [[:space:]]; use grep+cut.
version="$(grep -m1 '^version *=' "$manifest" | cut -d'"' -f2)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail 'invalid package version'
release="$output_dir/wunder-frontend-slint-$version-win7-x86.exe"
cp -f "$exe" "$release"
printf '%s\n' "$imports" > "$output_dir/imports.txt"
echo "[wunder-slint-win7-arm64] Win7 PE import gate passed."
echo "[wunder-slint-win7-arm64] produced: $release"
