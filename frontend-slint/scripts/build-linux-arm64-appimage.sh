#!/usr/bin/env bash
set -euo pipefail

# Build the native Slint desktop as a type-2 AppImage. The binary is built
# against Ubuntu 18.04/glibc 2.27 and the XCB/X11 closure is copied into the
# AppDir; durable desktop state is kept outside the read-only image by AppRun.

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/../.." && pwd)}"
builder_root="${WUNDER_BUILDER_ROOT:-/builder/kylin2}"
offline_root="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}"
vendor_root="${WUNDER_CARGO_VENDOR:-$offline_root/cargo-vendor-slint}"
cargo_home="${CARGO_HOME:-$repo_root/target/linux-arm64-ubuntu18-slint/cargo-home}"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/linux-arm64-ubuntu18-slint/cargo}"
output_dir="${WUNDER_OUTPUT_DIR:-$repo_root/target/slint/dist/linux-arm64}"
runtime_source="${WUNDER_APPIMAGE_RUNTIME:-/builder/appimage-runtime/rcho-arm64.AppImage}"
max_glibc="${WUNDER_SLINT_LINUX_MAX_GLIBC:-2.27}"

fail() { echo "[wunder-slint-appimage] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }
require_command() { command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"; }

manifest="$repo_root/frontend-slint/Cargo.toml"
require_file "$manifest"
[[ -d "$vendor_root" ]] || fail "offline Cargo vendor is missing: $vendor_root"
require_file "$runtime_source"
export PATH="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin:$PATH"
for command_name in cargo rustc readelf strip file mksquashfs dd grep; do
  require_command "$command_name"
done

version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "invalid package version: $version"

mkdir -p "$cargo_home" "$target_dir" "$output_dir"
if [[ -d "$vendor_root/axum" || -f "$vendor_root/.cargo-checksum.json" ]]; then
  cat > "$cargo_home/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$vendor_root"
[net]
offline = true
EOF
else
  if [[ "${WUNDER_OFFLINE:-0}" == "1" ]]; then
    fail "offline Cargo vendor is incomplete (axum is missing): $vendor_root"
  fi
  echo "[wunder-slint-appimage] vendor is incomplete; using the configured online Cargo registry"
  rm -f "$cargo_home/config.toml"
  unset CARGO_HOME
fi

export CARGO_HOME="$cargo_home"
export CARGO_TARGET_DIR="$target_dir"
if [[ -f "$cargo_home/config.toml" ]]; then export CARGO_NET_OFFLINE=true; fi

echo "[wunder-slint-appimage] cargo: $(cargo --version)"
echo "[wunder-slint-appimage] rustc: $(rustc --version)"
echo "[wunder-slint-appimage] baseline: $(ldd --version | head -n 1)"
cargo_args=(build --locked --release --manifest-path "$manifest")
if [[ -f "$cargo_home/config.toml" ]]; then cargo_args+=(--offline); fi
cargo "${cargo_args[@]}"

binary="$target_dir/release/wunder-frontend-slint"
require_file "$binary"
strip --strip-all --strip-unneeded "$binary"
machine="$(readelf -h "$binary" | awk -F: '/Machine:/{gsub(/^[[:space:]]+/, "", $2); print $2}')"
[[ "$machine" == "AArch64" ]] || fail "expected AArch64 ELF, got: ${machine:-unknown}"
required_glibc="$(readelf --version-info "$binary" 2>/dev/null | grep -oE 'GLIBC_[0-9.]+' | sed 's/GLIBC_//' | sort -Vu | tail -n 1 || true)"
[[ -n "$required_glibc" ]] || fail "could not determine executable GLIBC requirement"
[[ "$(printf '%s\n%s\n' "$max_glibc" "$required_glibc" | sort -V | tail -n 1)" == "$max_glibc" ]] || fail "GLIBC_$required_glibc exceeds release limit GLIBC_$max_glibc"

app_name="wunder-slint-$version-linux-arm64"
appdir="$target_dir/appimage-root"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/lib" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/256x256/apps" "$appdir/config" "$appdir/images"
cp -f "$binary" "$appdir/usr/bin/wunder-frontend-slint"

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
  libXi.so.6 libXrandr.so.2 libXcursor.so.1 libXinerama.so.1 libXau.so.6
  libXdmcp.so.6 libxcb.so.1 libxcb-render.so.0 libxcb-shape.so.0
  libxcb-xfixes.so.0 libxcb-randr.so.0 libxcb-xkb.so.1 libxcb-image.so.0
  libxcb-keysyms.so.1 libxcb-icccm.so.4 libxcb-util.so.1 libxcb-shm.so.0
  libxcb-sync.so.1 libxcb-present.so.0 libxcb-glx.so.0 libxcb-dri2.so.0
  libxcb-dri3.so.0 libxkbcommon.so.0 libxkbcommon-x11.so.0
)
for library in "${x11_libraries[@]}"; do
  library_path="$(ldconfig -p 2>/dev/null | awk -v name="$library" '$1 == name {print $NF; exit}')"
  if [[ -n "$library_path" && -f "$library_path" ]]; then cp -L -f "$library_path" "$appdir/usr/lib/$library"; fi
done

# Bundle non-glibc direct dependencies reported by ldd, such as fontconfig and
# freetype, because old distributions may not provide matching versions.
while IFS= read -r library_path; do
  [[ -f "$library_path" ]] || continue
  case "$library_path" in
    /lib/*/libc.so.*|/lib/*/libm.so.*|/lib/*/libpthread.so.*|/lib/*/libdl.so.*|/lib/*/librt.so.*|/lib/*/ld-linux-*) continue ;;
  esac
  cp -L -f "$library_path" "$appdir/usr/lib/$(basename "$library_path")"
done < <(ldd "$binary" | awk '/=> \/|^\// {for (i=1; i<=NF; i++) if ($i ~ /^\//) {print $i; break}}' | sort -u)

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

runtime_offset="$(grep -oba 'hsqs' "$runtime_source" | head -n 1 | cut -d: -f1)"
[[ "$runtime_offset" =~ ^[0-9]+$ && "$runtime_offset" -gt 0 ]] || fail "could not locate embedded AppImage runtime"
squashfs="$target_dir/$app_name.squashfs"
appimage="$output_dir/$app_name.AppImage"
rm -f "$squashfs" "$appimage" "$target_dir/$app_name.runtime"
mksquashfs "$appdir" "$squashfs" -noappend -comp gzip -all-root -no-xattrs -b 1048576 >/dev/null
dd if="$runtime_source" of="$target_dir/$app_name.runtime" bs=1 count="$runtime_offset" status=none
cat "$target_dir/$app_name.runtime" "$squashfs" > "$appimage"
chmod +x "$appimage"
rm -f "$target_dir/$app_name.runtime" "$squashfs"
echo "[wunder-slint-appimage] produced: $appimage ($(stat -c '%s' "$appimage") bytes)"
