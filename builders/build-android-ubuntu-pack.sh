#!/usr/bin/env bash
# Build 安卓版's offline tool overlay for the bundled Ubuntu rootfs.
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
#   WUNDER_PACK_PACKAGES  apt package list (default: base + Android image/recovery/network toolbox)
#   WUNDER_PY_REQUIREMENTS  pip requirements (default packaging/python/requirements-android.txt)
#   WUNDER_VIDEO_FONTS_DIR  知识短片字体源目录（默认 config/fonts/video；没有时按固定 URL 下载）
#   WUNDER_COMMON_FONTS_DIR 公文/文档通用字体源目录（默认 config/fonts/common）
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
# Fonts keep the terminal, Python imaging and headless Chromium from rendering
# tofu: DejaVu is the default Latin/mono family, Liberation substitutes the
# Microsoft metric-compatible faces, WenQuanYi Micro Hei covers CJK at ~5 MB
# (fonts-noto-cjk would add ~90 MB) and Noto Color Emoji covers emoji.
# The runtime tools cover what the base rootfs leaves out: HTTP downloads,
# zip archives, process/file inspection and the `python` command name.
PACKAGES="${WUNDER_PACK_PACKAGES:-python3 python3-pip python3-venv git openssh-client ripgrep 7zip \
adb android-sdk-libsparse-utils erofs-utils e2fsprogs squashfs-tools cpio zstd lz4 xz-utils \
mtools dosfstools exfatprogs ntfs-3g testdisk binwalk foremost gddrescue \
sqlite3 jq tree less vim-tiny htop ncdu tmux rsync socat netcat-openbsd nmap \
bind9-dnsutils whois mtr-tiny iperf3 gnupg binutils patchelf upx-ucl \
file xxd hexedit libarchive-tools lzop \
curl wget unzip zip procps psmisc lsof bc bzip2 time python-is-python3 iproute2 \
fontconfig fonts-dejavu-core fonts-liberation fonts-noto-color-emoji fonts-wqy-microhei}"
PY_REQUIREMENTS="${WUNDER_PY_REQUIREMENTS:-$ROOT/packaging/python/requirements-android.txt}"
VIDEO_FONTS_DIR="${WUNDER_VIDEO_FONTS_DIR:-$ROOT/config/fonts/video}"
VIDEO_FONTS_SCRIPT="$ROOT/packaging/fonts/fetch_video_fonts.py"
COMMON_FONTS_DIR="${WUNDER_COMMON_FONTS_DIR:-$ROOT/config/fonts/common}"
COMMON_FONTS_MANIFEST="$ROOT/packaging/fonts/common-fonts.json"

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

# A killed build can leave bind mounts under $WORK. Removing the work tree
# while a host /dev or /sys bind mount is still attached would traverse the
# mount and delete real host device nodes, so refuse to continue until every
# mount below $WORK is gone.
unmount_work_mounts() {
    local mount
    while IFS= read -r mount; do
        [ -n "$mount" ] || continue
        sudo -n umount "$mount" 2>/dev/null \
            || sudo -n umount -l "$mount" 2>/dev/null \
            || true
    done < <(grep -F " $WORK/" /proc/self/mountinfo 2>/dev/null \
        | awk '{print $5}' | sort -r)
    if grep -qF " $WORK/" /proc/self/mountinfo 2>/dev/null; then
        echo "refusing to remove $WORK: bind mounts are still attached:" >&2
        grep -F " $WORK/" /proc/self/mountinfo | awk '{print "  " $5}' >&2
        return 1
    fi
}
unmount_work_mounts || exit 2

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
    local mount
    for mount in "${MOUNTS[@]}"; do
        sudo -n umount "$mount" 2>/dev/null \
            || sudo -n umount -l "$mount" 2>/dev/null \
            || true
    done
}
trap cleanup EXIT INT TERM
for dir in proc sys; do
    mkdir -p "$WORK/work/$dir"
    sudo -n mount --bind "/$dir" "$WORK/work/$dir"
    MOUNTS+=("$WORK/work/$dir")
done
# Use a private minimal /dev instead of bind-mounting the host /dev. apt/dpkg
# only need null/zero/random/urandom and a tty; sharing host device nodes with
# a build tree that is later removed with rm -rf is how host /dev gets wiped.
sudo -n mkdir -p "$WORK/work/dev"
while read -r name kind major minor; do
    [ -n "$name" ] || continue
    sudo -n mknod -m 666 "$WORK/work/dev/$name" "$kind" "$major" "$minor" 2>/dev/null || true
done <<'DEV'
null c 1 3
zero c 1 5
full c 1 7
random c 1 8
urandom c 1 9
tty c 5 0
console c 5 1
DEV
sudo -n chmod 666 "$WORK/work/dev/tty" "$WORK/work/dev/console" 2>/dev/null || true
sudo -n mkdir -p "$WORK/work/dev/pts" "$WORK/work/dev/shm"
sudo -n mount -t devpts devpts "$WORK/work/dev/pts"
MOUNTS+=("$WORK/work/dev/pts")
sudo -n mount -t tmpfs -o mode=1777 tmpfs "$WORK/work/dev/shm"
MOUNTS+=("$WORK/work/dev/shm")
sudo -n ln -sf pts/ptmx "$WORK/work/dev/ptmx"
sudo -n ln -sf /proc/self/fd "$WORK/work/dev/fd"
sudo -n ln -sf /proc/self/fd/0 "$WORK/work/dev/stdin"
sudo -n ln -sf /proc/self/fd/1 "$WORK/work/dev/stdout"
sudo -n ln -sf /proc/self/fd/2 "$WORK/work/dev/stderr"
sudo -n cp /etc/resolv.conf "$WORK/work/etc/resolv.conf"

# apt/dpkg must run outside proot: fake-root proot makes dpkg's access checks
# fail, while binfmt_misc already runs the arm64 tree through qemu.
run_guest() {
    sudo -n chroot "$WORK/work" /usr/bin/env -i HOME=/root \
        PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        DEBIAN_FRONTEND=noninteractive LC_ALL=C \
        /bin/bash -lc "$1"
}

echo "==> apt-get update ($ABI)"
run_guest "apt-get update -o Acquire::ForceIPv4=true" >/dev/null
echo "==> apt-get install: $PACKAGES"
run_guest "apt-get install -y --no-install-recommends $PACKAGES" >/dev/null

# Python library set mirroring the 桌面版 supplement. The wheels are shipped as-is
# and installed on the device with `pip --no-index`; re-compressing an already
# installed site-packages tree would add several hundred MB to the APK.
echo "==> python wheelhouse"
[ -f "$PY_REQUIREMENTS" ] || { echo "missing requirements: $PY_REQUIREMENTS" >&2; exit 2; }
WHEELS="$WORK/work/opt/wunder/wheels"
mkdir -p "$WHEELS"
case "$ABI" in
    arm64)
        PACK_PLATFORMS=(--platform manylinux2014_aarch64 --platform manylinux_2_28_aarch64)
        ;;
    x86_64)
        PACK_PLATFORMS=(--platform manylinux2014_x86_64 --platform manylinux_2_28_x86_64)
        ;;
esac
python3 -m pip download --only-binary=:all: "${PACK_PLATFORMS[@]}" \
    --python-version 312 --implementation cp --abi cp312 \
    -d "$WHEELS" -r "$PY_REQUIREMENTS"
cp "$PY_REQUIREMENTS" "$WHEELS/requirements-android.txt"
if find "$WHEELS" -type f ! -name '*.whl' ! -name 'requirements-android.txt' | grep -q .; then
    echo "wheelhouse contains a source archive" >&2
    exit 2
fi
if ! compgen -G "$WHEELS/scipy-*.whl" >/dev/null; then
    echo "wheelhouse is missing the scipy wheel required by 知识短片/audio.py" >&2
    exit 2
fi

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

# 知识短片 skill 的 6 个拉丁/等宽 OFL 字体（中文用共用字体）：优先从部署机 config/fonts/video 复制，
# 没有就按 packaging/fonts/video-fonts.json 的固定 URL + SHA-256 下载。
echo "==> video fonts"
[ -f "$VIDEO_FONTS_SCRIPT" ] || { echo "missing video font fetcher: $VIDEO_FONTS_SCRIPT" >&2; exit 2; }
VIDEO_FONTS_CACHE="$WORK/video-fonts"
fetch_args=(--dest "$VIDEO_FONTS_CACHE" --licenses "$ROOT/config/skills/知识短片/engine/fonts")
[[ -d "$VIDEO_FONTS_DIR" ]] && fetch_args+=(--source-dir "$VIDEO_FONTS_DIR")
python3 "$VIDEO_FONTS_SCRIPT" "${fetch_args[@]}"
sudo -n mkdir -p "$WORK/work/usr/share/fonts/wunder/video"
sudo -n cp -f "$VIDEO_FONTS_CACHE"/*.ttf "$WORK/work/usr/share/fonts/wunder/video/"
sudo -n cp -f "$VIDEO_FONTS_CACHE"/OFL-*.txt "$WORK/work/usr/share/fonts/wunder/video/" 2>/dev/null || true
[ "$(find "$WORK/work/usr/share/fonts/wunder/video" -maxdepth 1 -name '*.ttf' | wc -l)" -ge 6 ] \
    || { echo "video fonts incomplete under /usr/share/fonts/wunder/video" >&2; exit 2; }

# 公文/文档通用字体：仿宋、楷体、黑体、宋体、方正小标宋、微软雅黑、等线与
# Times New Roman，连同字体名映射一起装进 rootfs；优先从部署机
# config/fonts/common 复制，没有就按 packaging/fonts/common-fonts.json 下载。
echo "==> common document fonts"
COMMON_FONTS_CACHE="$WORK/common-fonts"
fetch_args=(--manifest "$COMMON_FONTS_MANIFEST" --dest "$COMMON_FONTS_CACHE")
[[ -d "$COMMON_FONTS_DIR" ]] && fetch_args+=(--source-dir "$COMMON_FONTS_DIR")
python3 "$VIDEO_FONTS_SCRIPT" "${fetch_args[@]}"
sudo -n mkdir -p "$WORK/work/usr/share/fonts/wunder/common" "$WORK/work/etc/fonts/conf.d"
sudo -n cp -f "$COMMON_FONTS_CACHE"/*.ttf "$WORK/work/usr/share/fonts/wunder/common/"
sudo -n cp -f "$COMMON_FONTS_CACHE"/*.ttc "$WORK/work/usr/share/fonts/wunder/common/" 2>/dev/null || true
sudo -n cp -f "$ROOT/packaging/fonts/99-wunder-common.conf" "$WORK/work/etc/fonts/conf.d/99-wunder-common.conf"
# 补充包内的字体在 chroot 里由 fontconfig 扫描；fc-cache 必须在两份字体都落盘后跑一次。
run_guest "fc-cache -f >/dev/null"
COMMON_FONT_EXPECTED="$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1], encoding="utf-8"))["files"]))' "$COMMON_FONTS_MANIFEST")"
[ "$(find "$WORK/work/usr/share/fonts/wunder/common" -maxdepth 1 \( -name '*.ttf' -o -name '*.ttc' \) | wc -l)" -ge "$COMMON_FONT_EXPECTED" ] \
    || { echo "common fonts incomplete under /usr/share/fonts/wunder/common" >&2; exit 2; }

echo "==> verify tools"
run_guest "python3 --version; git --version; rg --version | head -1; ffmpeg -version | head -1; 7z i | head -2"
run_guest "for t in adb simg2img debugfs dump.erofs mksquashfs photorec binwalk testdisk \
ddrescue sqlite3 jq nmap dig rsync socat tmux htop ncdu objdump readelf patchelf upx file xxd \
curl wget unzip zip ps pkill killall lsof bc bzip2 ip ss python; do \
command -v \$t >/dev/null || { echo missing \$t; exit 1; }; done; \
[ -x /usr/bin/time ] || { echo missing /usr/bin/time; exit 1; }; \
python -V; curl --version | head -1; wget --version | head -1; unzip -v | head -1; \
ps --version; ip -V; ss -V; \
adb version | head -1; ddrescue --version | head -1; binwalk --help >/dev/null 2>&1 || true"

echo "==> verify fonts"
run_guest "fc-list | grep -qi 'DejaVu Sans' || { echo 'DejaVu Sans missing'; exit 1; }; \
fc-list | grep -qi 'Liberation Sans' || { echo 'Liberation Sans missing'; exit 1; }; \
fc-list | grep -qi 'WenQuanYi Micro Hei' || { echo 'WenQuanYi Micro Hei missing'; exit 1; }; \
fc-list | grep -qi 'Noto Color Emoji' || { echo 'Noto Color Emoji missing'; exit 1; }; \
fc-list | grep -qi 'Noto Serif SC' || { echo 'Noto Serif SC missing'; exit 1; }; \
fc-list | grep -qi 'Cormorant Garamond' || { echo 'Cormorant Garamond missing'; exit 1; }; \
fc-list | grep -qi 'JetBrains Mono' || { echo 'JetBrains Mono missing'; exit 1; }; \
fc-match -f '%{family}\n' '仿宋_GB2312' | grep -qi 'FangSong' || { echo '仿宋_GB2312 mapping missing'; exit 1; }; \
fc-match -f '%{family}\n' '楷体_GB2312' | grep -qi 'KaiTi' || { echo '楷体_GB2312 mapping missing'; exit 1; }; \
fc-match -f '%{family}\n' '黑体' | grep -qi 'SimHei' || { echo '黑体 mapping missing'; exit 1; }; \
fc-match -f '%{family}\n' '宋体' | grep -qi 'SimSun' || { echo '宋体 mapping missing'; exit 1; }; \
fc-match -f '%{family}\n' '方正小标宋简体' | grep -qi 'FZXiaoBiaoSong' || { echo '小标宋 mapping missing'; exit 1; }; \
fc-match -f '%{family}\n' 'Times New Roman' | grep -qi 'Times New Roman' || { echo 'Times New Roman missing'; exit 1; }; \
fc-match -f '%{family}\n' ':charset=4e2d' | grep -qi 'WenQuanYi' \
    || { echo 'no CJK fallback for U+4E2D'; exit 1; }; \
fc-match -f '%{family}\n' ':charset=1f9d1' | grep -qi 'Noto Color Emoji' \
    || { echo 'no emoji fallback for U+1F9D1'; exit 1; }; \
echo \"fonts ok: \$(fc-list | wc -l) faces\""

cleanup
MOUNTS=()
if ! unmount_work_mounts; then
    echo "refusing to package: mounts remain under $WORK" >&2
    exit 2
fi
for dir in dev proc sys; do
    if mountpoint -q "$WORK/work/$dir"; then
        echo "refusing to package: $WORK/work/$dir is still a mount point" >&2
        exit 2
    fi
done

# Volatile or host-specific state must never travel in the pack.
sudo -n rm -rf "$WORK/work/host-rootfs" "$WORK/work/var/lib/apt/lists" \
       "$WORK/work/var/cache/apt" "$WORK/work/var/log" "$WORK/work/tmp" \
       "$WORK/work/root" "$WORK/work/dev" "$WORK/work/proc" "$WORK/work/sys" \
       "$WORK/work/etc/resolv.conf" "$WORK/work/var/lib/dpkg/lock"* \
       "$WORK/work/var/lib/dpkg/triggers/Lock" "$WORK/work/var/lib/dpkg/updates" \
       "$WORK/work/root/.cache" "$WORK/work/home/"*"/.cache"
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
XZ_OPT="-T0 -6" tar -C "$WORK/work" --numeric-owner --no-recursion \
    -T "$WORK/files.txt" -J -cf "$OUT"
chmod 644 "$OUT"
echo "pack: $OUT"
ls -lh "$OUT"
