#!/usr/bin/env bash
set -euo pipefail

# Build the ARM64 Slint desktop as an AppDir directory archive. The binary is
# built against Ubuntu 18.04/glibc 2.27 and the XCB/X11 closure is copied into
# the AppDir. The .run installer and the portable launcher both expand this
# archive, so the target machine needs no FUSE, libfuse2 or AppImage runtime.

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
host_arch="$(uname -m)"
case "$host_arch" in
  aarch64) host_profile=kylin-arm ;;
  x86_64) host_profile=kylin-x86 ;;
  *) echo "[wunder-slint-arm64-desktop] unsupported Linux host: ${host_arch:-unknown}" >&2; exit 2 ;;
esac
# A native ARM64 Linux checkout keeps the reusable SDK beside the repository.
# Docker mounts it at /builder/kylin-arm; x86_64 hosts default to kylin-x86.
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/$host_profile}"
offline_root="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}"
vendor_root="${WUNDER_CARGO_VENDOR:-$offline_root/cargo-vendor-slint}"
cargo_home="${CARGO_HOME:-$repo_root/target/linux-arm64-ubuntu18-slint/cargo-home}"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/linux-arm64-ubuntu18-slint/cargo}"
output_dir="${WUNDER_OUTPUT_DIR:-$repo_root/target/slint/dist/linux-arm64}"
max_glibc="${WUNDER_SLINT_LINUX_MAX_GLIBC:-2.27}"
sysroot="$offline_root/linux-arm64-ubuntu18/root"
sysroot_lib="$sysroot/usr/lib/aarch64-linux-gnu"
gcc_lib="$sysroot/usr/lib/gcc/aarch64-linux-gnu/7"
stage_dir=""
phase=preflight

fail() { echo "[wunder-slint-arm64-desktop] $*" >&2; exit 2; }
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
    echo "[wunder-slint-arm64-desktop] failed during $phase (exit $status); command: ${BASH_COMMAND:-unknown}; no new archive was published" >&2
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
[[ -f "$sysroot_lib/crt1.o" && -f "$sysroot_lib/libc.so" ]] || fail "Ubuntu 18 ARM64 development sysroot is incomplete: $sysroot"
gcc_shared="$gcc_lib/libgcc_s.so"
[[ ! -L "$gcc_shared" || "$(readlink "$gcc_shared")" != /* ]] \
  || fail "Ubuntu 18 ARM64 sysroot has an absolute libgcc_s.so link: $gcc_lib (run builders/prepare-linux-arm64-devel-sysroot.sh --repair)"
[[ -e "$gcc_shared" ]] \
  || fail "Ubuntu 18 ARM64 sysroot has a dangling libgcc_s.so link: $gcc_lib (run builders/prepare-linux-arm64-devel-sysroot.sh --repair)"
for library in libgmp.so.10 libisl.so.19 libmpc.so.3 libmpfr.so.6; do
  [[ -e "$sysroot/usr/lib/aarch64-linux-gnu/$library" || -e "$sysroot/lib/aarch64-linux-gnu/$library" ]] \
    || fail "ARM64 SDK GCC runtime is missing: $library (run builders/prepare-linux-arm64-devel-sysroot.sh)"
done
# Native ARM64 uses the SDK's own GCC; x86_64 uses the amd64-hosted cross
# compiler from host-tools-amd64 and its binutils.
strip_cmd=(strip)
cargo_target_args=()
if [[ "$host_arch" == x86_64 ]]; then
  host_root="$(bash "$repo_root/builders/prepare-x86-arm64-cross.sh")"
  export WUNDER_ARM64_CROSS_HOST_ROOT="$host_root"
  rust="$offline_root/rust/toolchains/nightly-2026-03-14-x86_64-unknown-linux-gnu"
  [[ -x "$rust/bin/cargo" ]] || rust="$rust/nightly-2026-03-14-x86_64-unknown-linux-gnu"
  export PATH="$rust/bin:$host_root/usr/bin:$PATH"
  strip_cmd=(env "LD_LIBRARY_PATH=$host_root/usr/lib/x86_64-linux-gnu" "$host_root/usr/bin/aarch64-linux-gnu-strip")
  cargo_target_args=(--target aarch64-unknown-linux-gnu)
else
  [[ -x "$gcc_lib/cc1" ]] \
    || fail "ARM64 SDK GCC backend is missing: $gcc_lib/cc1 (run builders/prepare-linux-arm64-devel-sysroot.sh)"
  export PATH="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin:$PATH"
fi
for command_name in cargo rustc readelf awk grep sort mktemp stat tr tar; do
  require_command "$command_name"
done
[[ "$host_arch" == x86_64 ]] || require_command ldd

version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "invalid package version: $version"

mkdir -p "$cargo_home" "$target_dir" "$output_dir"
# kylin-arm contains the complete union vendor for both shared Slint projects.
# Always force Cargo offline so a release cannot silently depend on the host cache.
cat > "$cargo_home/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$vendor_root"
[net]
offline = true
EOF

# CLI --config outranks every config file: cargo >=1.96 resolves its config
# home from the passwd entry rather than HOME or CARGO_HOME, so a
# developer-wide ~/.cargo/config.toml (mirror setups) would otherwise defeat
# the vendored configuration written into the per-build cargo home.
WUNDER_VENDORED_CONFIG=(--config 'source.crates-io.replace-with="vendored-sources"')
# The repo may be checked out without the executable bit on builder scripts;
# cargo needs a directly executable linker wrapper.
arm64_cc="$repo_root/builders/linux_arm64_sysroot_cc.sh"
if [[ ! -x "$arm64_cc" ]]; then
  mkdir -p "$target_dir"
  arm64_cc="$target_dir/linux_arm64_sysroot_cc.sh"
  cp -f "$repo_root/builders/linux_arm64_sysroot_cc.sh" "$arm64_cc"
  chmod 755 "$arm64_cc"
fi
export CARGO_HOME="$cargo_home"
export CARGO_TARGET_DIR="$target_dir"
export CARGO_NET_OFFLINE=true
export RCHO_ARM64_SYSROOT="$sysroot"
export RCHO_ARM64_CC="$sysroot/usr/bin/aarch64-linux-gnu-gcc-7"
export CC_aarch64_unknown_linux_gnu="$arm64_cc"
export CXX_aarch64_unknown_linux_gnu="$arm64_cc --cxx"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$arm64_cc"
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=--sysroot=$sysroot -C link-arg=-L$sysroot_lib"
if [[ "$host_arch" == x86_64 ]]; then
  # Archive tooling is object-format agnostic; the SDK's aarch64 binutils need
  # bundled libbfd on LD_LIBRARY_PATH, so prefer the host tools here.
  export AR_aarch64_unknown_linux_gnu=ar
  export RANLIB_aarch64_unknown_linux_gnu=ranlib
  export NM_aarch64_unknown_linux_gnu=nm
  export PKG_CONFIG_ALLOW_CROSS=1
  export PKG_CONFIG_SYSROOT_DIR="$sysroot"
  export PKG_CONFIG_LIBDIR="$sysroot/usr/lib/aarch64-linux-gnu/pkgconfig:$sysroot/usr/share/pkgconfig"
fi

echo "[wunder-slint-arm64-desktop] cargo: $(cargo --version)"
echo "[wunder-slint-arm64-desktop] rustc: $(rustc --version)"
echo "[wunder-slint-arm64-desktop] baseline: GLIBC_$max_glibc (Ubuntu 18.04 sysroot)"
cargo "${WUNDER_VENDORED_CONFIG[@]}" build --locked --offline --release --manifest-path "$manifest" \
  --bin wunder-frontend-slint "${cargo_target_args[@]}"

phase=validation
if [[ "$host_arch" == x86_64 ]]; then
  binary="$target_dir/aarch64-unknown-linux-gnu/release/wunder-frontend-slint"
else
  binary="$target_dir/release/wunder-frontend-slint"
fi
require_file "$binary"
machine="$(LC_ALL=C readelf -h "$binary" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[ \t]+/, "", $2); print $2}')"
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
"${strip_cmd[@]}" --strip-all --strip-unneeded "$appdir/usr/bin/wunder-frontend-slint"

# Use the example configuration as the distributable seed; runtime state is
# written outside the pack and no machine-specific paths are packaged.
require_file "$repo_root/config/wunder-example.yaml"
cp -f "$repo_root/config/wunder-example.yaml" "$appdir/config/wunder.yaml"
for resource in prompts skills preset_worker_cards ppt_templates i18n.messages.json fonts.conf matplotlibrc; do
  source="$repo_root/config/$resource"
  if [[ -d "$source" ]]; then cp -a "$source" "$appdir/config/"; fi
  if [[ -f "$source" ]]; then cp -f "$source" "$appdir/config/"; fi
done
icon_source="$repo_root/images/pupa.png"
[[ -f "$icon_source" ]] || icon_source="$repo_root/images/wunder.png"
if [[ -f "$icon_source" ]]; then
  cp -f "$icon_source" "$appdir/usr/share/icons/hicolor/256x256/apps/wunder.png"
  cp -f "$icon_source" "$appdir/images/wunder.png"
fi

cat > "$appdir/wunder.desktop" <<'EOF'
[Desktop Entry]
Name=蛹
Comment=本地 AI 工作台
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
runtime_lib_root="${WUNDER_SLINT_RUNTIME_LIB_DIR:-}"
if [[ -z "$runtime_lib_root" ]]; then
  for candidate in \
    "$offline_root/linux-arm64-ubuntu18/root/usr/lib/aarch64-linux-gnu" \
    "$offline_root/runtime-libs/aarch64-linux-gnu"; do
    if [[ -d "$candidate" ]]; then
      runtime_lib_root="$candidate"
      break
    fi
  done
fi
# A native ARM64 host can consult its multi-arch ldconfig cache. On x86_64 the
# cache lists host libraries, so only the pinned ARM64 sysroot may be used.
library_cache=""
[[ "$host_arch" == x86_64 ]] || library_cache="$(ldconfig -p 2>/dev/null || true)"
for library in "${x11_libraries[@]}"; do
  if [[ -n "$runtime_lib_root" && -f "$runtime_lib_root/$library" ]]; then
    library_path="$runtime_lib_root/$library"
    library_machine="$(LC_ALL=C readelf -h "$library_path" | LC_ALL=C awk -F: '/Machine:/{gsub(/^[ \t]+/, "", $2); print $2}')"
    [[ "$library_machine" == "AArch64" ]] || fail "runtime library is not AArch64: $library_path (got ${library_machine:-unknown}); fix WUNDER_SLINT_RUNTIME_LIB_DIR or the SDK runtime-libs directory"
  else
    # Consume the complete ldconfig output before selecting a path. An early
    # awk exit can SIGPIPE ldconfig under pipefail on large multi-arch caches.
    library_path="$(printf '%s\n' "$library_cache" | LC_ALL=C awk -v name="$library" '$1 == name { if (!first) first=$NF; if (/AArch64/) arm=$NF } END { if (arm) print arm; else if (first) print first }')"
  fi
  if [[ -n "$library_path" && -f "$library_path" ]]; then cp -L -f "$library_path" "$appdir/usr/lib/$library"; fi
done
for library in libX11.so.6 libXtst.so.6 libxcb.so.1 libxcb-xkb.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0 libasound.so.2; do
  [[ -f "$appdir/usr/lib/$library" ]] || fail "required bundled ARM64 X11/XCB library is missing: $library; install the ARM64 packages on the host, use --docker, or set WUNDER_SLINT_RUNTIME_LIB_DIR=/path/to/libs"
done

# Bundle non-glibc direct dependencies, such as fontconfig and freetype,
# because old distributions may not provide matching versions. A native ARM64
# host can use ldd; a cross build walks DT_NEEDED with readelf against the
# pinned sysroot instead.
if [[ "$host_arch" == x86_64 ]]; then
  dependency_dir="$sysroot/usr/lib/aarch64-linux-gnu"
  dependency_dirs=("$appdir/usr/lib" "$dependency_dir" "$sysroot/lib/aarch64-linux-gnu" "$gcc_lib")
  queue=("$binary")
  while ((${#queue[@]})); do
    object="${queue[0]}"
    queue=("${queue[@]:1}")
    while IFS= read -r library; do
      case "$library" in
        libc.so.*|libm.so.*|libpthread.so.*|libdl.so.*|librt.so.*|ld-linux-*) continue ;;
      esac
      [[ -e "$appdir/usr/lib/$library" ]] && continue
      library_path=""
      for directory in "${dependency_dirs[@]}"; do
        if [[ -f "$directory/$library" ]]; then library_path="$directory/$library"; break; fi
      done
      [[ -n "$library_path" ]] || continue
      cp -L -f "$library_path" "$appdir/usr/lib/$library"
      queue+=("$appdir/usr/lib/$library")
    done < <(LC_ALL=C readelf -d "$object" | LC_ALL=C awk '/\(NEEDED\)/{gsub(/[][]/, "", $NF); print $NF}')
  done
else
  while IFS= read -r library_path; do
    [[ -f "$library_path" ]] || continue
    case "$library_path" in
      /lib/*/libc.so.*|/lib/*/libm.so.*|/lib/*/libpthread.so.*|/lib/*/libdl.so.*|/lib/*/librt.so.*|/lib/*/ld-linux-*) continue ;;
    esac
    cp -L -f "$library_path" "$appdir/usr/lib/$(basename "$library_path")"
  done < <(ldd "$binary" | awk '/=> \/|^\// {for (i=1; i<=NF; i++) if ($i ~ /^\//) {print $i; break}}' | sort -u)
fi

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
echo "[wunder-slint-arm64-desktop] produced: $output_dir/$app_name.tar.gz ($(stat -c '%s' "$output_dir/$app_name.tar.gz") bytes)"
