#!/usr/bin/env bash
# Build 茧's offline tool overlay for the bundled Ubuntu rootfs.
#
# The pack is a plain tar.xz overlay of the files that the package manager adds
# or changes, so UbuntuRuntime only has to extract it into the installed rootfs.
# No network is needed on the device; this script needs one build-time network
# pass to fetch the arm64 packages.
#
#   WUNDER_UBUNTU_ROOTFS=/path/rootfs.tar.xz \
#   WUNDER_PACK_ABI=arm64 \
#   builders/build-android-ubuntu-pack.sh
#
# Env:
#   WUNDER_PACK_ABI       arm64 (default) or x86_64
#   WUNDER_PACK_OUT       output archive (default /tmp/wunder-packs/common-<abi>.tar.xz)
#   WUNDER_PACK_PACKAGES  package list (default python/git/rg/7zip set)
#   WUNDER_PACK_FFMPEG_URL  static ffmpeg archive override
#   WUNDER_PACK_WORK      build directory (default /tmp/wunder-pack-<abi>)
#
# arm64 builds run the guest through qemu-user-static (binfmt) in a chroot.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ABI="${WUNDER_PACK_ABI:-arm64}"
BASE_ARCHIVE="${WUNDER_UBUNTU_ROOTFS:-$ROOT/crates/wunder-android/android/app/src/main/assets/ubuntu/rootfs.tar.xz}"
WORK="${WUNDER_PACK_WORK:-/tmp/wunder-pack-$ABI}"
OUT="${WUNDER_PACK_OUT:-/tmp/wunder-packs/common-$ABI.tar.xz}"
PACKAGES="${WUNDER_PACK_PACKAGES:-python3 python3-pip python3-venv git openssh-client ripgrep 7zip}"

case "$ABI" in
    arm64)
        DEFAULT_FFMPEG_URL="https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-arm64-static.tar.xz"
        ;;
    x86_64)
        DEFAULT_FFMPEG_URL="https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz"
        ;;
    *)
        DEFAULT_FFMPEG_URL=""
        ;;
esac
FFMPEG_URL="${WUNDER_PACK_FFMPEG_URL:-$DEFAULT_FFMPEG_URL}"

[ -f "$BASE_ARCHIVE" ] || { echo "missing rootfs: $BASE_ARCHIVE" >&2; exit 2; }

case "$ABI" in
    arm64)
        [ -x /usr/bin/qemu-aarch64-static ] || { echo "install qemu-user-static" >&2; exit 2; }
        ;;
    x86_64) ;;
    *) echo "unsupported ABI: $ABI" >&2; exit 2 ;;
esac

sudo -n true || { echo "passwordless sudo is required for chroot builds" >&2; exit 2; }

sudo -n rm -rf "$WORK"
mkdir -p "$WORK/base" "$WORK/work" "$WORK/tmp"
tar -xJf "$BASE_ARCHIVE" -C "$WORK/base" \
    --exclude='*/dev/*' --exclude='dev/*' 2>/dev/null

if [ -f "$WORK/base/etc/os-release" ]; then
    # Some archives (Ubuntu base tarballs) already extract at the root.
    BASE_TOP="$WORK/base"
else
    BASE_TOP="$(find "$WORK/base" -mindepth 1 -maxdepth 1 -type d | head -1)"
fi
[ -n "$BASE_TOP" ] || { echo "rootfs archive has no top directory" >&2; exit 2; }
cp -a "$BASE_TOP/." "$WORK/work/"

# apt-key and maintainer scripts run as unprivileged users; the guest tmp
# directories must be world-writable before the first apt call.
mkdir -p "$WORK/work/tmp" "$WORK/work/var/tmp"
sudo -n chmod 1777 "$WORK/work/tmp" "$WORK/work/var/tmp"

MOUNTS=()
cleanup() {
    for mount in "${MOUNTS[@]}"; do
        sudo -n umount "$mount" 2>/dev/null || true
    done
}
trap cleanup EXIT INT TERM
for dir in proc dev sys; do
    mkdir -p "$WORK/work/$dir"
    sudo -n mount --bind "/$dir" "$WORK/work/$dir"
    MOUNTS+=("$WORK/work/$dir")
done
sudo -n cp /etc/resolv.conf "$WORK/work/etc/resolv.conf"

# apt/dpkg must run outside proot: fake-root proot makes dpkg's access checks
# fail, while binfmt_misc already runs the arm64 tree through qemu.
run_guest() {
    sudo -n chroot "$WORK/work" /usr/bin/env -i HOME=/root \
        PATH=/usr/sbin:/usr/bin:/sbin:/bin \
        DEBIAN_FRONTEND=noninteractive LC_ALL=C \
        /bin/bash -lc "$1"
}

echo "==> apt-get update ($ABI)"
run_guest "apt-get update -o Acquire::ForceIPv4=true" >/dev/null
echo "==> apt-get install: $PACKAGES"
run_guest "apt-get install -y --no-install-recommends $PACKAGES" >/dev/null

# Ubuntu's ffmpeg package drags in Mesa/LLVM for filter acceleration, which is
# useless in a terminal runtime. Use the static upstream build instead: it is
# smaller, self-contained and has the same CLI surface.
echo "==> static ffmpeg"
mkdir -p "$WORK/ffmpeg" "$WORK/work/usr/local/bin"
curl -fL --retry 3 --connect-timeout 20 -o "$WORK/ffmpeg/ffmpeg.tar.xz" "$FFMPEG_URL"
tar -xJf "$WORK/ffmpeg/ffmpeg.tar.xz" -C "$WORK/ffmpeg"
FFMPEG_SRC="$(find "$WORK/ffmpeg" -type f -name ffmpeg | head -1)"
FFPROBE_SRC="$(find "$WORK/ffmpeg" -type f -name ffprobe | head -1)"
[ -n "$FFMPEG_SRC" ] && [ -n "$FFPROBE_SRC" ] || { echo "static ffmpeg archive is incomplete" >&2; exit 2; }
install -m 755 "$FFMPEG_SRC" "$WORK/work/usr/local/bin/ffmpeg"
install -m 755 "$FFPROBE_SRC" "$WORK/work/usr/local/bin/ffprobe"

echo "==> verify tools"
run_guest "python3 --version; git --version; rg --version | head -1; ffmpeg -version | head -1; 7zz i | head -2"

cleanup
MOUNTS=()

# Volatile or host-specific state must never travel in the pack.
sudo -n rm -rf "$WORK/work/host-rootfs" "$WORK/work/var/lib/apt/lists" \
       "$WORK/work/var/cache/apt" "$WORK/work/var/log" "$WORK/work/tmp" \
       "$WORK/work/root" "$WORK/work/dev" "$WORK/work/proc" "$WORK/work/sys" \
       "$WORK/work/etc/resolv.conf" "$WORK/work/var/lib/dpkg/lock"* \
       "$WORK/work/var/lib/dpkg/triggers/Lock" "$WORK/work/var/lib/dpkg/updates"
sudo -n chown -R "$(id -u):$(id -g)" "$WORK/work"
mkdir -p "$WORK/work/dev" "$WORK/work/proc" "$WORK/work/sys" "$WORK/work/tmp" \
         "$WORK/work/var/tmp"
chmod 1777 "$WORK/work/tmp" "$WORK/work/var/tmp"

echo "==> diff base -> installed"
python3 - "$WORK/base" "$WORK/work" "$WORK/files.txt" <<'PY'
import hashlib
import os
import sys

base, work, out = sys.argv[1], sys.argv[2], sys.argv[3]
SKIP_TOP = {"proc", "sys", "dev", "tmp", "run", "root", "host-rootfs"}
SKIP_PREFIX = (
    "var/lib/apt/lists/", "var/cache/apt/", "var/log/",
    "var/lib/dpkg/lock", "var/lib/dpkg/triggers/Lock",
    "var/lib/dpkg/updates/", "etc/resolv.conf",
)

def skip(rel):
    return rel in SKIP_TOP or rel.startswith(SKIP_PREFIX)

def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def record(rel, path, rows):
    if skip(rel):
        return
    st = os.lstat(path)
    kind = "d" if os.path.isdir(path) else "l" if os.path.islink(path) else "f"
    if kind == "l":
        rows[rel] = ("l", os.readlink(path))
    elif kind == "d":
        rows[rel] = ("d", st.st_mode & 0o7777)
    else:
        rows[rel] = ("f", st.st_size, digest(path))

base_rows, work_rows = {}, {}
for root, rows in ((base, base_rows), (work, work_rows)):
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        for name in dirnames + filenames:
            path = os.path.join(dirpath, name)
            rel = os.path.relpath(path, root)
            record(rel, path, rows)

changed = []
for rel, info in work_rows.items():
    if rel not in base_rows or base_rows[rel] != info:
        changed.append(rel)

with open(out, "w", encoding="utf-8") as fh:
    for rel in sorted(changed):
        fh.write(rel + "\n")

added = sum(1 for rel in changed if rel not in base_rows)
print(f"files: {len(changed)} ({added} new)")
PY

echo "==> pack $(wc -l < "$WORK/files.txt") entries"
mkdir -p "$(dirname "$OUT")"
tar -C "$WORK/work" --numeric-owner --hard-dereference -T "$WORK/files.txt" \
    -J -T0 -6 -f "$OUT"
chmod 644 "$OUT"
echo "pack: $OUT"
ls -lh "$OUT"
