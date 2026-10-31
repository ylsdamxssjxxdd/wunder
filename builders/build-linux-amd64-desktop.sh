#!/usr/bin/env bash
# Package the cross-built x86_64 desktop as an AppDir directory archive.
#
# The archive is the only desktop distribution unit: the .run installer
# expands it under <install root>/app/desktop and the portable launcher
# expands it under <home>/app/desktop. Neither needs FUSE, libfuse2 or an
# AppImage runtime, which is why Ubuntu 24.04 runs without extra packages.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
if [[ "$(uname -m)" == x86_64 ]]; then host_profile=kylin-x86; else host_profile=kylin-arm; fi
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/$host_profile}/offline}"
sdk="$offline_root/linux-amd64-ubuntu18/root"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/linux-amd64-ubuntu18-slint/cargo}"
output_dir="${WUNDER_OUTPUT_DIR:-$repo_root/target/slint/dist/linux-amd64}"
manifest="$repo_root/frontend-slint/Cargo.toml"
max_glibc="${WUNDER_SLINT_LINUX_MAX_GLIBC:-2.27}"
stage_dir=""

# The packaging process runs after the child cross-build process exits, so set
# up the cross SDK again instead of relying on child-shell exports. On x86_64
# the host gcc is wrapped with the bionic sysroot; on ARM64 the SDK's cross
# compiler is required.
if [[ "$(uname -m)" == x86_64 ]]; then
  desktop_rust="$offline_root/rust/toolchains/nightly-2026-03-14-x86_64-unknown-linux-gnu"
  [[ -x "$desktop_rust/bin/cargo" ]] || desktop_rust="$desktop_rust/nightly-2026-03-14-x86_64-unknown-linux-gnu"
  export PATH="$desktop_rust/bin:$PATH"
else
  export PATH="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin:$sdk/usr/bin:$PATH"
fi
export WUNDER_AMD64_SDK_ROOT="$sdk"

fail() { echo "[wunder-slint-amd64-desktop] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }
require_command() { command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"; }
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$stage_dir" ]]; then
    case "$stage_dir" in "$output_dir"/.wunder-amd64-package.*) rm -rf -- "$stage_dir" ;; esac
  fi
  if ((status != 0)); then echo "[wunder-slint-amd64-desktop] failed; no new archive was published" >&2; fi
  exit "$status"
}
trap cleanup EXIT

require_file "$manifest"
[[ -d "$sdk" ]] || fail "amd64 sysroot is missing: $sdk"
for tool in grep awk sort sed tail mktemp stat find cut tar; do require_command "$tool"; done

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
Name=蛹
Comment=本地 AI 工作台
Exec=wunder-frontend-slint
Terminal=false
Type=Application
Categories=Utility;
EOF
cp -f "$appdir/wunder.desktop" "$appdir/usr/share/applications/wunder.desktop"

# Cross-built binaries cannot be passed to host ldd. Resolve the known Winit
# X11/XCB closure from the x86_64 sysroot instead; glibc stays system-owned.
libraries=(libX11.so.6 libX11-xcb.so.1 libXext.so.6 libXfixes.so.3 libXrender.so.1 libXi.so.6 libXrandr.so.2 libXcursor.so.1 libXinerama.so.1 libXtst.so.6 libXau.so.6 libXdmcp.so.6 libxcb.so.1 libxcb-render.so.0 libxcb-shape.so.0 libxcb-xfixes.so.0 libxcb-randr.so.0 libxcb-xkb.so.1 libxcb-image.so.0 libxcb-keysyms.so.1 libxcb-icccm.so.4 libxcb-util.so.1 libxcb-shm.so.0 libxcb-sync.so.1 libxcb-present.so.0 libxcb-glx.so.0 libxcb-dri2.so.0 libxcb-dri3.so.0 libxkbcommon.so.0 libxkbcommon-x11.so.0 libasound.so.2)
for library in "${libraries[@]}"; do
  found=""
  for candidate in "$sdk/usr/x86_64-linux-gnu/lib/$library" "$sdk/lib/x86_64-linux-gnu/$library" "$sdk/usr/lib/x86_64-linux-gnu/$library"; do
    if [[ -f "$candidate" ]]; then found="$candidate"; break; fi
  done
  [[ -n "$found" ]] && cp -L -f "$found" "$appdir/usr/lib/$library"
done
for library in libX11.so.6 libXtst.so.6 libxcb.so.1 libxcb-xkb.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0 libasound.so.2; do
  [[ -f "$appdir/usr/lib/$library" ]] || fail "required bundled X11/XCB library is missing from the amd64 sysroot: $library"
done
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
set -eu
APPDIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export APPDIR
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
# The engine resolves WUNDER_HOME from the caller or, when this tree sits at
# <root>/app/desktop, from the portable marker two levels up.
if [ -z "${WUNDER_HOME:-}" ]; then
  install_root=$(CDPATH= cd -- "$APPDIR/../.." 2>/dev/null && pwd) || install_root=""
  if [ -n "$install_root" ] && [ -f "$install_root/.wunder-home" ]; then
    WUNDER_HOME="$install_root"
    export WUNDER_HOME
  fi
fi
exec "$APPDIR/usr/bin/wunder-frontend-slint" "$@"
EOF
chmod +x "$appdir/AppRun"

# Publish atomically: the installer and the portable launcher both consume this
# archive, so a truncated file must never appear in the output directory.
tarball="$stage_dir/$app_name.tar.gz"
tar --sort=name --numeric-owner -czf "$tarball" -C "$appdir" .
mv -f -- "$tarball" "$output_dir/$app_name.tar.gz"
echo "[wunder-slint-amd64-desktop] produced: $output_dir/$app_name.tar.gz ($(stat -c '%s' "$output_dir/$app_name.tar.gz") bytes)"
