#!/usr/bin/env bash
# Populate the Ubuntu 18.04 arm64 runtime closure used by the ARM64 offline
# SDK. This is a one-time SDK maintenance action, never a release-build
# fallback: normal desktop and CLI builds remain fully offline. Run it in an
# aarch64 Ubuntu 18.04 container with the kylin-arm SDK mounted read/write:
#
#   docker run --rm --network host --platform linux/arm64 \
#     -v /path/to/kylin-arm:/builder/kylin-arm \
#     -v /path/to/wunder:/workspace:ro \
#     -w /workspace rcho-slint-arm64-ubuntu18:latest \
#     env WUNDER_BUILDER_ROOT=/builder/kylin-arm \
#     bash builders/prepare-linux-arm64-runtime-sysroot.sh
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd -P)}"
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-arm}"
offline_root="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}"
sdk="$offline_root/linux-arm64-ubuntu18"
root="$sdk/root"
debs="$sdk/debs/runtime-arm64"

fail() { echo "[wunder-arm64-runtime-sdk] $*" >&2; exit 2; }

[[ "$(uname -m)" == aarch64 ]] || fail "run this one-time SDK preparation on an aarch64 Ubuntu 18.04 environment"
[[ -r /etc/os-release ]] || fail "cannot identify Linux distribution"
. /etc/os-release
[[ "$ID:$VERSION_ID" == ubuntu:18.04 ]] || fail "Ubuntu 18.04 is required to preserve the GLIBC_2.27 release floor"

# Bionic's ports archive still carries the release, updates and security
# indexes for arm64. Pin this maintenance-only action to it rather than
# inheriting a stale base-image source list; normal release builds never
# access the network.
cat >/etc/apt/sources.list <<'EOF'
deb [trusted=yes] http://ports.ubuntu.com/ubuntu-ports bionic main universe multiverse restricted
deb [trusted=yes] http://ports.ubuntu.com/ubuntu-ports bionic-updates main universe multiverse restricted
deb [trusted=yes] http://ports.ubuntu.com/ubuntu-ports bionic-security main universe multiverse restricted
EOF
printf 'Acquire::Check-Valid-Until "false";\n' >/etc/apt/apt.conf.d/99wunder-no-valid-until

# These are the libraries loaded directly by Winit/Slint and by the X11/XTest/
# ALSA desktop features. glibc stays system-owned in the AppImage; all other
# runtime dependencies are resolved by apt into the same SDK root. libbsd0 is
# listed explicitly because libXdmcp pulls it in and a --download-only pass
# otherwise skips packages the maintenance container already has installed.
packages=(
  # libasound2-dev supplies libasound.so for the linker. The AppImage receives
  # only the corresponding libasound.so.2 runtime object.
  libasound2 libasound2-dev
  libbsd0
  libxtst6
  libx11-6 libx11-xcb1 libxext6 libxfixes3 libxrender1 libxi6 libxrandr2 libxcursor1 libxinerama1
  libxau6 libxdmcp6
  libxcb1 libxcb-render0 libxcb-shape0 libxcb-xfixes0 libxcb-randr0 libxcb-xkb1
  libxcb-image0 libxcb-keysyms1 libxcb-icccm4 libxcb-util1 libxcb-shm0 libxcb-sync1
  libxcb-present0 libxcb-glx0 libxcb-dri2-0 libxcb-dri3-0
  libxkbcommon0 libxkbcommon-x11-0
  # mksquashfs and its compression libraries let a minimal ARM64 host package
  # the AppImage entirely from the SDK when squashfs-tools is not installed.
  squashfs-tools liblz4-1 liblzo2-2 liblzma5 libzstd1 zlib1g
)

mkdir -p "$debs/partial" "$root"
apt-get update -o Acquire::Check-Valid-Until=false
apt-get install -y --no-install-recommends binutils
# `--reinstall` matters when this script runs in a populated maintenance
# image: apt otherwise considers an already installed host library satisfied
# and leaves no .deb in the portable SDK cache. Resolve the closure together,
# then retain the archives so the SDK can be audited or rebuilt offline.
apt-get install --download-only --reinstall --yes --no-install-recommends \
  -o Dir::Cache::archives="$debs" \
  -o Dir::Cache::archives/partial="$debs/partial" \
  "${packages[@]}"

shopt -s nullglob
archives=("$debs"/*.deb)
((${#archives[@]})) || fail "apt did not download any arm64 runtime packages"
for archive in "${archives[@]}"; do
  dpkg-deb -x "$archive" "$root"
done
(cd "$debs" && sha256sum ./*.deb > SHA256SUMS)

required=(libasound.so.2 libXtst.so.6 libX11.so.6 libxcb.so.1 libxcb-xkb.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0 libbsd.so.0)
for library in "${required[@]}"; do
  found=""
  for candidate in "$root"/usr/lib/aarch64-linux-gnu/"$library" "$root"/lib/aarch64-linux-gnu/"$library"; do
    if [[ -e "$candidate" ]]; then found="$candidate"; break; fi
  done
  [[ -n "$found" ]] || fail "runtime library was not installed into arm64 sysroot: $library"
  machine="$(LC_ALL=C readelf -h "$found" | LC_ALL=C awk -F: '/Machine:/{print $2}' | LC_ALL=C sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
  [[ "$machine" == "AArch64" ]] || fail "$library is not an arm64 ELF: ${machine:-unknown}"
done

[[ -x "$root/usr/bin/mksquashfs" ]] || fail "mksquashfs was not installed into arm64 sysroot: $root/usr/bin/mksquashfs"

printf '[wunder-arm64-runtime-sdk] installed %s Ubuntu 18.04 arm64 runtime packages into %s\n' "${#archives[@]}" "$root"
