#!/usr/bin/env bash
# Package the cross-built x86_64 executable as a type-2 AppImage.  The
# runtime must be an x86_64 AppImage (or another compatible type-2 runtime);
# it is deliberately supplied by the caller so offline builds never download
# an architecture-mismatched runner.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin2}/offline}"
sdk="$offline_root/linux-amd64-ubuntu18/root"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/linux-amd64-ubuntu18-slint/cargo}"
output_dir="${WUNDER_OUTPUT_DIR:-$repo_root/target/slint/dist/linux-amd64}"
runtime_source="${WUNDER_APPIMAGE_RUNTIME:-}"
manifest="$repo_root/frontend-slint/Cargo.toml"
max_glibc="${WUNDER_SLINT_LINUX_MAX_GLIBC:-2.27}"
stage_dir=""

# The packaging process runs after the child cross-build process exits, so set
# up the cross SDK again instead of relying on child-shell exports.
export PATH="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin:$sdk/usr/bin:$PATH"
export WUNDER_AMD64_SDK_ROOT="$sdk"

fail() { echo "[wunder-slint-amd64-appimage] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }
require_command() { command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"; }
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$stage_dir" ]]; then
    case "$stage_dir" in "$output_dir"/.wunder-amd64-package.*) rm -rf -- "$stage_dir" ;; esac
  fi
  if ((status != 0)); then echo "[wunder-slint-amd64-appimage] failed; no new AppImage was published" >&2; fi
  exit "$status"
}
trap cleanup EXIT

[[ -n "$runtime_source" ]] || fail "WUNDER_APPIMAGE_RUNTIME must point to an x86_64 AppImage runtime"
require_file "$runtime_source"
require_file "$manifest"
[[ -d "$sdk" ]] || fail "amd64 sysroot is missing: $sdk"
for tool in mksquashfs dd grep awk sort sed tail mktemp stat find cut x86_64-linux-gnu-readelf; do require_command "$tool"; done
runtime_machine="$(LC_ALL=C x86_64-linux-gnu-readelf -h "$runtime_source" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[[:space:]]+/, "", $2); print $2}')"
[[ "$runtime_machine" == "Advanced Micro Devices X86-64" ]] || fail "AppImage runtime must be x86_64, got: ${runtime_machine:-unknown}"

# Build first. The child script also validates the x86_64 ELF and glibc floor.
bash "$repo_root/builders/build-linux-amd64-offline.sh"
version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "invalid package version"
binary="$output_dir/wunder-frontend-slint-$version-linux-amd64"
require_file "$binary"
app_name="wunder-slint-$version-linux-amd64"
mkdir -p "$output_dir"
stage_dir="$(mktemp -d "$output_dir/.wunder-amd64-package.XXXXXXXX")"
appdir="$stage_dir/appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/lib" "$appdir/usr/share/applications" "$appdir/config"
cp -f "$binary" "$appdir/usr/bin/wunder-frontend-slint"

require_file "$repo_root/config/wunder-example.yaml"
cp -f "$repo_root/config/wunder-example.yaml" "$appdir/config/wunder.yaml"
for resource in prompts skills preset_worker_cards ppt_templates i18n.messages.json fonts.conf matplotlibrc; do
  source="$repo_root/config/$resource"
  if [[ -d "$source" ]]; then cp -a "$source" "$appdir/config/"; fi
  if [[ -f "$source" ]]; then cp -f "$source" "$appdir/config/"; fi
done
cat > "$appdir/wunder.desktop" <<'EOF'
[Desktop Entry]
Name=Wunder
Comment=Native Slint desktop client
Exec=wunder-frontend-slint
Terminal=false
Type=Application
Categories=Utility;
EOF
cp -f "$appdir/wunder.desktop" "$appdir/usr/share/applications/wunder.desktop"

# Cross-built binaries cannot be passed to host ldd. Resolve the known Winit
# X11/XCB closure from the x86_64 sysroot instead; glibc stays system-owned.
libraries=(libX11.so.6 libX11-xcb.so.1 libXext.so.6 libXfixes.so.3 libXrender.so.1 libXi.so.6 libXrandr.so.2 libXcursor.so.1 libXinerama.so.1 libXau.so.6 libXdmcp.so.6 libxcb.so.1 libxcb-render.so.0 libxcb-shape.so.0 libxcb-xfixes.so.0 libxcb-randr.so.0 libxcb-xkb.so.1 libxcb-image.so.0 libxcb-keysyms.so.1 libxcb-icccm.so.4 libxcb-util.so.1 libxcb-shm.so.0 libxcb-sync.so.1 libxcb-present.so.0 libxcb-glx.so.0 libxcb-dri2.so.0 libxcb-dri3.so.0 libxkbcommon.so.0 libxkbcommon-x11.so.0)
for library in "${libraries[@]}"; do
  found=""
  for candidate in "$sdk/usr/x86_64-linux-gnu/lib/$library" "$sdk/lib/x86_64-linux-gnu/$library" "$sdk/usr/lib/x86_64-linux-gnu/$library"; do
    if [[ -f "$candidate" ]]; then found="$candidate"; break; fi
  done
  [[ -n "$found" ]] && cp -L -f "$found" "$appdir/usr/lib/$library"
done
for library in libX11.so.6 libxcb.so.1 libxcb-xkb.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0; do
  [[ -f "$appdir/usr/lib/$library" ]] || fail "required bundled X11/XCB library is missing from the amd64 sysroot: $library"
done
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
set -eu
APPDIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
data_root="${XDG_DATA_HOME:-${HOME:-/tmp}/.local/share}/wunder"
export WUNDER_DESKTOP_TEMP_ROOT="${WUNDER_DESKTOP_TEMP_ROOT:-$data_root/WUNDER_TEMPD}"
export WUNDER_DESKTOP_WORKSPACE_ROOT="${WUNDER_DESKTOP_WORKSPACE_ROOT:-$data_root/WUNDER_WORK}"
exec "$APPDIR/usr/bin/wunder-frontend-slint" "$@"
EOF
chmod +x "$appdir/AppRun"

runtime_offset="$(LC_ALL=C grep -oba -m 1 'hsqs' "$runtime_source" | LC_ALL=C awk -F: '{print $1}')"
[[ "$runtime_offset" =~ ^[0-9]+$ && "$runtime_offset" -gt 0 ]] || fail "could not locate embedded AppImage runtime"
squashfs="$stage_dir/$app_name.squashfs"
runtime="$stage_dir/$app_name.runtime"
staged="$stage_dir/$app_name.AppImage"
mksquashfs "$appdir" "$squashfs" -noappend -comp gzip -all-root -no-xattrs -b 1048576 >/dev/null
dd if="$runtime_source" of="$runtime" bs=1 count="$runtime_offset" status=none
cat "$runtime" "$squashfs" > "$staged"
chmod +x "$staged"
mv -f -- "$staged" "$output_dir/$app_name.AppImage"
echo "[wunder-slint-amd64-appimage] produced: $output_dir/$app_name.AppImage ($(stat -c '%s' "$output_dir/$app_name.AppImage") bytes)"
