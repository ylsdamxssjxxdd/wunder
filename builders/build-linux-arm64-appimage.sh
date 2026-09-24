#!/usr/bin/env bash
set -euo pipefail

# Build the native Slint desktop as a type-2 AppImage. The binary is built
# against Ubuntu 18.04/glibc 2.27 and the XCB/X11 closure is copied into the
# AppDir; durable desktop state is kept outside the read-only image by AppRun.

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
builder_root="${WUNDER_BUILDER_ROOT:-/builder/kylin2}"
offline_root="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}"
vendor_root="${WUNDER_CARGO_VENDOR:-$offline_root/cargo-vendor-slint}"
cargo_home="${CARGO_HOME:-$repo_root/target/linux-arm64-ubuntu18-slint/cargo-home}"
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
    echo "[wunder-slint-appimage] failed during $phase (exit $status); no new AppImage was published" >&2
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

manifest="$repo_root/frontend-slint/Cargo.toml"
require_file "$manifest"
[[ -d "$vendor_root" ]] || fail "offline Cargo vendor is missing: $vendor_root"
[[ -n "$runtime_source" ]] || fail "WUNDER_APPIMAGE_RUNTIME must point to an ARM64 type-2 AppImage runtime"
require_file "$runtime_source"
export PATH="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin:$PATH"
for command_name in cargo rustc readelf strip file ldd mksquashfs dd awk grep sort mktemp stat; do
  require_command "$command_name"
done

runtime_machine="$(LC_ALL=C readelf -h "$runtime_source" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[[:space:]]+/, "", $2); print $2}')"
[[ "$runtime_machine" == "AArch64" ]] || fail "AppImage runtime must be AArch64, got: ${runtime_machine:-unknown}"

version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "invalid package version: $version"

mkdir -p "$cargo_home" "$target_dir" "$output_dir"
if [[ -d "$vendor_root/axum" ]]; then
  cat > "$cargo_home/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$vendor_root"
[net]
offline = true
EOF
else
  if [[ "${WUNDER_ALLOW_ONLINE:-0}" != "1" ]]; then
    fail "offline Cargo vendor is incomplete (runtime dependencies are missing): $vendor_root; pass WUNDER_ALLOW_ONLINE=1 only for an intentional online build"
  fi
  echo "[wunder-slint-appimage] WUNDER_ALLOW_ONLINE=1; using the configured online Cargo registry"
  rm -f "$cargo_home/config.toml"
fi

export CARGO_HOME="$cargo_home"
export CARGO_TARGET_DIR="$target_dir"
if [[ -f "$cargo_home/config.toml" ]]; then export CARGO_NET_OFFLINE=true; else unset CARGO_NET_OFFLINE; fi

echo "[wunder-slint-appimage] cargo: $(cargo --version)"
echo "[wunder-slint-appimage] rustc: $(rustc --version)"
echo "[wunder-slint-appimage] baseline: $(ldd --version | head -n 1)"
cargo_args=(build --locked --release --manifest-path "$manifest" --bin wunder-frontend-slint)
if [[ -f "$cargo_home/config.toml" ]]; then cargo_args+=(--offline); fi
cargo "${cargo_args[@]}"

phase=validation
binary="$target_dir/release/wunder-frontend-slint"
require_file "$binary"
machine="$(LC_ALL=C readelf -h "$binary" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[[:space:]]+/, "", $2); print $2}')"
[[ "$machine" == "AArch64" ]] || fail "expected AArch64 ELF, got: ${machine:-unknown}"
required_glibc="$(LC_ALL=C readelf --version-info "$binary" 2>/dev/null | LC_ALL=C grep -oE 'GLIBC_[0-9.]+' | LC_ALL=C sed 's/GLIBC_//' | LC_ALL=C sort -Vu | LC_ALL=C tail -n 1 || true)"
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
strip --strip-all --strip-unneeded "$appdir/usr/bin/wunder-frontend-slint"

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
library_cache="$(ldconfig -p 2>/dev/null || true)"
for library in "${x11_libraries[@]}"; do
  # Consume the complete ldconfig output before selecting a path. An early
  # awk exit can SIGPIPE ldconfig under pipefail on large multi-arch caches.
  library_path="$(printf '%s\n' "$library_cache" | LC_ALL=C awk -v name="$library" '$1 == name { if (!first) first=$NF; if (/AArch64/) arm=$NF } END { if (arm) print arm; else if (first) print first }')"
  if [[ -n "$library_path" && -f "$library_path" ]]; then cp -L -f "$library_path" "$appdir/usr/lib/$library"; fi
done
for library in libX11.so.6 libxcb.so.1 libxcb-xkb.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0; do
  [[ -f "$appdir/usr/lib/$library" ]] || fail "required bundled X11/XCB library is missing: $library"
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

runtime_offset="$(LC_ALL=C grep -oba -m 1 'hsqs' "$runtime_source" | awk -F: '{print $1}')"
[[ "$runtime_offset" =~ ^[0-9]+$ && "$runtime_offset" -gt 0 ]] || fail "could not locate embedded AppImage runtime"
squashfs="$stage_dir/$app_name.squashfs"
runtime="$stage_dir/$app_name.runtime"
staged_appimage="$stage_dir/$app_name.AppImage"
appimage="$output_dir/$app_name.AppImage"
mksquashfs "$appdir" "$squashfs" -noappend -comp gzip -all-root -no-xattrs -b 1048576 >/dev/null
dd if="$runtime_source" of="$runtime" bs=1 count="$runtime_offset" status=none
cat "$runtime" "$squashfs" > "$staged_appimage"
chmod +x "$staged_appimage"
# Publish atomically only after the complete image has been written.
mv -f -- "$staged_appimage" "$appimage"
echo "[wunder-slint-appimage] produced: $appimage ($(stat -c '%s' "$appimage") bytes)"
