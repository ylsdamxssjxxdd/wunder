#!/usr/bin/env bash
set -euo pipefail

# Cross-build the ARM64 Slint desktop on an amd64 Ubuntu 18.04 host.
# against Ubuntu 18.04/glibc 2.27 and the XCB/X11 closure is copied into the
# AppDir; durable desktop state is kept outside the read-only image by AppRun.

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
# A native ARM64 Linux checkout keeps the reusable SDK beside the repository.
# Docker mounts it at /builder/kylin-arm, so retain that location as fallback.
if [[ -n "${WUNDER_BUILDER_ROOT:-}" ]]; then
  builder_root="$WUNDER_BUILDER_ROOT"
elif [[ -d "$repo_root/../Rust-builder/kylin-arm" ]]; then
  builder_root="$repo_root/../Rust-builder/kylin-arm"
else
  builder_root="/builder/kylin-arm"
fi
offline_root="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/linux-arm64-ubuntu18-slint/cargo}"
output_dir="${WUNDER_OUTPUT_DIR:-$repo_root/target/slint/dist/linux-arm64}"
runtime_source="${WUNDER_APPIMAGE_RUNTIME:-}"
max_glibc="${WUNDER_SLINT_LINUX_MAX_GLIBC:-2.27}"
stage_dir=""
phase=preflight

fail() { echo "[wunder-slint-appimage] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }
require_command() { command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"; }

cleanup() {
  local status=$?
  trap - EXIT
  # Only remove the private staging directory created by mktemp.  A failed
  # build must leave an earlier release untouched and must not delete a caller
  # supplied output directory.
  if [[ -n "$stage_dir" ]]; then
    case "$stage_dir" in
      "$output_dir"/.wunder-arm-package.*) rm -rf -- "$stage_dir" ;;
    esac
  fi
  if ((status != 0)); then
    echo "[wunder-slint-appimage] failed during $phase (exit $status); command: ${BASH_COMMAND:-unknown}; no new AppImage was published" >&2
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

manifest="$repo_root/frontend-slint/Cargo.toml"
require_file "$manifest"
[[ -n "$runtime_source" ]] || fail "WUNDER_APPIMAGE_RUNTIME must point to an ARM64 type-2 AppImage runtime"
require_file "$runtime_source"
target="aarch64-unknown-linux-gnu"
sysroot="${WUNDER_ARM64_SYSROOT:-/}"
linker="${WUNDER_ARM64_LINKER:-aarch64-linux-gnu-gcc}"
readelf_tool="${WUNDER_ARM64_READELF:-aarch64-linux-gnu-readelf}"
strip_tool="${WUNDER_ARM64_STRIP:-aarch64-linux-gnu-strip}"
command -v "$linker" >/dev/null 2>&1 || linker="aarch64-linux-gnu-gcc"
[[ -d "$sysroot" ]] || fail "ARM64 cross sysroot is missing: $sysroot"
for command_name in cargo rustc "$readelf_tool" "$strip_tool" mksquashfs dd awk grep sort mktemp stat od tr sed; do
  require_command "$command_name"
done

runtime_machine="$(LC_ALL=C "$readelf_tool" -h "$runtime_source" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[ \t]+/, "", $2); print $2}')"
[[ "$runtime_machine" == "AArch64" ]] || fail "AppImage runtime must be AArch64, got: ${runtime_machine:-unknown}"

version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "invalid package version: $version"

mkdir -p "$target_dir" "$output_dir"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$linker"
export CC_aarch64_unknown_linux_gnu="$linker"
export CXX_aarch64_unknown_linux_gnu="aarch64-linux-gnu-g++"
export AR_aarch64_unknown_linux_gnu="aarch64-linux-gnu-ar"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_SYSROOT_DIR="$sysroot"
export PKG_CONFIG_PATH="$sysroot/usr/lib/aarch64-linux-gnu/pkgconfig:$sysroot/usr/share/pkgconfig"
# CI obtains dependencies through Cargo.lock; no ARM binary is executed during this build.
export CARGO_TARGET_DIR="$target_dir"

echo "[wunder-slint-appimage] cargo: $(cargo --version)"
echo "[wunder-slint-appimage] rustc: $(rustc --version)"
echo "[wunder-slint-appimage] baseline: $(ldd --version | head -n 1)"
cargo build --locked --release --manifest-path "$manifest" --bin wunder-frontend-slint --target "$target"

phase=validation
binary="$target_dir/$target/release/wunder-frontend-slint"
require_file "$binary"
machine="$(LC_ALL=C "$readelf_tool" -h "$binary" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[ \t]+/, "", $2); print $2}')"
[[ "$machine" == "AArch64" ]] || fail "expected AArch64 ELF, got: ${machine:-unknown}"
required_glibc="$(LC_ALL=C "$readelf_tool" --version-info "$binary" 2>/dev/null | LC_ALL=C grep -oE 'GLIBC_[0-9.]+' | LC_ALL=C sed 's/GLIBC_//' | LC_ALL=C sort -Vu | LC_ALL=C tail -n 1 || true)"
[[ -n "$required_glibc" ]] || fail "could not determine executable GLIBC requirement"
[[ "$(printf '%s\n%s\n' "$max_glibc" "$required_glibc" | sort -V | tail -n 1)" == "$max_glibc" ]] || fail "GLIBC_$required_glibc exceeds release limit GLIBC_$max_glibc"

app_name="wunder-slint-$version-linux-arm64"
phase=packaging
stage_dir="$(mktemp -d "$output_dir/.wunder-arm-package.XXXXXXXX")"
appdir="$stage_dir/appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/lib" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps" "$appdir/config" "$appdir/images"
# Strip a private copy only; Cargo's incremental/release cache remains usable
# after packaging and can be reused by the next build.
cp -f "$binary" "$appdir/usr/bin/wunder-frontend-slint"
"$strip_tool" --strip-all --strip-unneeded "$appdir/usr/bin/wunder-frontend-slint"

# Use the example configuration as the distributable seed; runtime state is
# written outside the AppImage and no machine-specific paths are packaged.
require_file "$repo_root/config/wunder-example.yaml"
cp -f "$repo_root/config/wunder-example.yaml" "$appdir/config/wunder.yaml"
for resource in prompts skills preset_worker_cards ppt_templates i18n.messages.json fonts.conf matplotlibrc; do
  source="$repo_root/config/$resource"
  if [[ -d "$source" ]]; then cp -a "$source" "$appdir/config/"; fi
  if [[ -f "$source" ]]; then cp -f "$source" "$appdir/config/"; fi
done
if [[ -f "$repo_root/images/wunder.png" ]]; then
  cp -f "$repo_root/images/wunder.png" "$appdir/usr/share/icons/hicolor/256x256/apps/wunder.png"
  cp -f "$repo_root/images/wunder.png" "$appdir/images/wunder.png"
fi

cat > "$appdir/wunder.desktop" <<'EOF'
[Desktop Entry]
Name=Wunder
Comment=Native Slint desktop client
Exec=wunder-frontend-slint
Icon=wunder
Terminal=false
Type=Application
Categories=Utility;
EOF
cp -f "$appdir/wunder.desktop" "$appdir/usr/share/applications/wunder.desktop"

# Winit loads several X11/XKB libraries dynamically, so ldd alone is not a
# sufficient detector. Copy the known closure when present; glibc stays system-owned.
x11_libraries=(
  libX11.so.6 libX11-xcb.so.1 libXext.so.6 libXfixes.so.3 libXrender.so.1
  libXi.so.6 libXrandr.so.2 libXcursor.so.1 libXinerama.so.1 libXtst.so.6 libXau.so.6
  libXdmcp.so.6 libxcb.so.1 libxcb-render.so.0 libxcb-shape.so.0
  libxcb-xfixes.so.0 libxcb-randr.so.0 libxcb-xkb.so.1 libxcb-image.so.0
  libxcb-keysyms.so.1 libxcb-icccm.so.4 libxcb-util.so.1 libxcb-shm.so.0
  libxcb-sync.so.1 libxcb-present.so.0 libxcb-glx.so.0 libxcb-dri2.so.0
  libxcb-dri3.so.0 libxkbcommon.so.0 libxkbcommon-x11.so.0
  libasound.so.2
)
# The ARM64 host may not carry these libraries in ldconfig (minimal images,
# or a multi-arch host whose ARM64 entries are absent). Prefer the offline
# SDK's runtime library directory or an explicit override, then fall back to
# ldconfig per library for the rest of the closure.
runtime_lib_root="${WUNDER_SLINT_RUNTIME_LIB_DIR:-$sysroot/usr/lib/aarch64-linux-gnu}"
[[ -d "$runtime_lib_root" ]] || runtime_lib_root="$sysroot/lib/aarch64-linux-gnu"
for library in "${x11_libraries[@]}"; do
  if [[ -n "$runtime_lib_root" && -f "$runtime_lib_root/$library" ]]; then
    library_path="$runtime_lib_root/$library"
    library_machine="$(LC_ALL=C "$readelf_tool" -h "$library_path" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[ \t]+/, "", $2); print $2}')"
    [[ "$library_machine" == "AArch64" ]] || fail "runtime library is not AArch64: $library_path (got ${library_machine:-unknown}); fix WUNDER_SLINT_RUNTIME_LIB_DIR or the SDK runtime-libs directory"
  else
    library_path=""
  fi
  if [[ -n "$library_path" && -f "$library_path" ]]; then cp -L -f "$library_path" "$appdir/usr/lib/$library"; fi
done
for library in libX11.so.6 libXtst.so.6 libxcb.so.1 libxcb-xkb.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0 libasound.so.2; do
  [[ -f "$appdir/usr/lib/$library" ]] || fail "required bundled ARM64 X11/XCB library is missing: $library; install the ARM64 packages on the host, use --docker, or set WUNDER_SLINT_RUNTIME_LIB_DIR=/path/to/libs"
done

# Cross builds cannot execute ldd on the ARM64 binary; the sysroot libraries
# above are copied explicitly and the loader resolves their transitive closure.

cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
set -eu
APPDIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
data_root="${XDG_DATA_HOME:-${HOME:-/tmp}/.local/share}/wunder"
temp_root="$data_root/WUNDER_TEMPD"
workspace_root="$data_root/WUNDER_WORK"
export WUNDER_DESKTOP_TEMP_ROOT="${WUNDER_DESKTOP_TEMP_ROOT:-$temp_root}"
export WUNDER_DESKTOP_WORKSPACE_ROOT="${WUNDER_DESKTOP_WORKSPACE_ROOT:-$workspace_root}"
exec "$APPDIR/usr/bin/wunder-frontend-slint" "$@"
EOF
chmod +x "$appdir/AppRun"

# The runtime blob is prepended to the squashfs verbatim. A fat runtime with
# an embedded tools filesystem must be cut at that image; a bare blob (the
# AppImageKit runtime-* downloads) contains none and is used whole. The hsqs
# magic also appears as look-alike bytes inside the ELF, so a hit is trusted
# only when a squashfs 4.0 superblock parses at the same offset.
runtime_offset="$(LC_ALL=C grep -oba -m 1 'hsqs' "$runtime_source" | LC_ALL=C awk -F: '{print $1}' || true)"
if [[ "$runtime_offset" =~ ^[0-9]+$ && "$runtime_offset" -gt 0 ]]; then
  block_size="$(dd if="$runtime_source" bs=1 skip=$((runtime_offset + 12)) count=4 status=none | od -An -tu4 | tr -d ' ')"
  sb_major="$(dd if="$runtime_source" bs=1 skip=$((runtime_offset + 28)) count=2 status=none | od -An -tu2 | tr -d ' ')"
  sb_minor="$(dd if="$runtime_source" bs=1 skip=$((runtime_offset + 30)) count=2 status=none | od -An -tu2 | tr -d ' ')"
  if [[ "$sb_major" != 4 || "$sb_minor" != 0 ]] || (( block_size < 4096 || block_size > 1048576 || (block_size & (block_size - 1)) != 0 )); then
    runtime_offset=""
  fi
fi
# Bare runtime blobs carry no embedded squashfs; prepend the whole file.
[[ "$runtime_offset" =~ ^[0-9]+$ && "$runtime_offset" -gt 0 ]] || runtime_offset="$(stat -c '%s' "$runtime_source")"
squashfs="$stage_dir/$app_name.squashfs"
runtime="$stage_dir/$app_name.runtime"
staged_appimage="$stage_dir/$app_name.AppImage"
appimage="$output_dir/$app_name.AppImage"
squashfs_processors="${WUNDER_SQUASHFS_PROCESSORS:-1}"
[[ "$squashfs_processors" =~ ^[1-9][0-9]*$ ]] || fail "WUNDER_SQUASHFS_PROCESSORS must be a positive integer"
echo "[wunder-slint-appimage] packaging SquashFS with $squashfs_processors processor(s)"
mksquashfs "$appdir" "$squashfs" -noappend -comp gzip -all-root -no-xattrs -b 1048576 -processors "$squashfs_processors"
dd if="$runtime_source" of="$runtime" bs=1 count="$runtime_offset" status=none
cat "$runtime" "$squashfs" > "$staged_appimage"
chmod +x "$staged_appimage"
# Publish atomically only after the complete image has been written.
mv -f -- "$staged_appimage" "$appimage"
echo "[wunder-slint-appimage] produced: $appimage ($(stat -c '%s' "$appimage") bytes)"
