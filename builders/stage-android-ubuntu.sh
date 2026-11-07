#!/usr/bin/env bash
# Stage 茧's bundled proot Ubuntu runtime.
#
# The rootfs and the static proot/busybox binaries are not committed: they are
# tens of megabytes and are reused unchanged from a reference Android terminal
# implementation. Point WUNDER_UBUNTU_SRC at that checkout once, then rebuild
# the APK.
#
#   WUNDER_UBUNTU_SRC=/path/to/operit-terminal builders/stage-android-ubuntu.sh
#
# Expected inputs under WUNDER_UBUNTU_SRC:
#   src/main/assets/ubuntu-noble-aarch64-pd-v4.18.0.tar.xz
#   src/main/jniLibs/arm64-v8a/liboperit_proot.so
#   src/main/jniLibs/arm64-v8a/liboperit_loader.so
#   src/main/jniLibs/arm64-v8a/libbusybox.so
#
# The binaries go into jniLibs because Android 10+ cannot execve anything under
# the app's private data directory; UbuntuRuntime symlinks them from the
# package-managed native library directory at install time.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/crates/wunder-android/android/app/src/main"
DEST_ASSETS="$APP/assets/ubuntu"
DEST_LIBS="$APP/jniLibs/arm64-v8a"
SRC="${WUNDER_UBUNTU_SRC:-}"

if [ -z "$SRC" ]; then
    echo "WUNDER_UBUNTU_SRC is not set; point it at the reference terminal checkout." >&2
    exit 2
fi

ROOTFS="$SRC/src/main/assets/ubuntu-noble-aarch64-pd-v4.18.0.tar.xz"
LIBS="$SRC/src/main/jniLibs/arm64-v8a"

for file in "$ROOTFS" "$LIBS/liboperit_proot.so" "$LIBS/liboperit_loader.so" "$LIBS/libbusybox.so"; do
    if [ ! -f "$file" ]; then
        echo "missing input: $file" >&2
        exit 2
    fi
done

mkdir -p "$DEST_ASSETS" "$DEST_LIBS"
cp -f "$ROOTFS" "$DEST_ASSETS/rootfs.tar.xz"
cp -f "$LIBS/liboperit_proot.so" "$DEST_LIBS/liboperit_proot.so"
cp -f "$LIBS/liboperit_loader.so" "$DEST_LIBS/liboperit_loader.so"
cp -f "$LIBS/libbusybox.so" "$DEST_LIBS/libbusybox.so"
chmod 644 "$DEST_ASSETS"/* "$DEST_LIBS"/liboperit_*.so

# Offline tool pack: python3, git, ripgrep, ffmpeg and 7zip overlaid on the
# same rootfs. Built by builders/build-android-ubuntu-pack.sh so the APK needs
# no network at install time.
PACKS="${WUNDER_PACKS_SRC:-}"
if [ -n "$PACKS" ]; then
    if [ ! -f "$PACKS/common.tar.xz" ]; then
        echo "missing input: $PACKS/common.tar.xz" >&2
        exit 2
    fi
    mkdir -p "$DEST_ASSETS/packs"
    cp -f "$PACKS/common.tar.xz" "$DEST_ASSETS/packs/common.tar.xz"
    chmod 644 "$DEST_ASSETS/packs/common.tar.xz"
fi

cat > "$DEST_ASSETS/NOTICE.txt" <<'EOF'
Bundled Ubuntu runtime for 茧
=============================

rootfs.tar.xz
  Ubuntu 24.04 LTS (noble) arm64 root filesystem, exported by proot-distro.
  Ubuntu is distributed under the licenses of its component packages; see
  /usr/share/doc/*/copyright inside the root filesystem.

liboperit_proot.so, liboperit_loader.so, libbusybox.so
  Static Android arm64 builds reused from a reference Android terminal
  implementation. proot is GPL-2.0-or-later; busybox is GPL-2.0-only.
  The reference project is LGPL-3.0. This application is a non-commercial
  project and reuses these unmodified binaries with the author's permission.

packs/common.tar.xz
  Offline tool overlay built from Ubuntu 24.04 arm64 packages: Python 3
  (PSF-2.0), Git and OpenSSH (GPL-2.0 / BSD), ripgrep (MIT / Unlicense),
  FFmpeg (LGPL-2.1-or-later, parts GPL) and 7-Zip (LGPL-2.1-or-later with
  the unRAR restriction). Source packages are available from
  https://ports.ubuntu.com/ubuntu-ports/ under the Ubuntu archive; each
  package's copyright file is installed under /usr/share/doc.

The binaries are executed locally on the device inside the app sandbox; no
network service is started by the runtime itself.
EOF

du -sh "$DEST_ASSETS" "$DEST_LIBS"/liboperit_*.so
