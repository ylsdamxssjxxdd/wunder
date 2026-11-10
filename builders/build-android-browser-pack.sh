#!/usr/bin/env bash
# Build 安卓版's offline browser overlay for the bundled Ubuntu rootfs.
#
# The overlay adds three things to the installed rootfs:
#   /opt/browser/chrome-linux/chrome        Playwright's arm64 Chromium build
#   /usr/local/lib/python3.12/dist-packages  Playwright + deps (arm64 wheels)
#   system libraries Chromium links against
#
# It is a plain tar.xz overlay; UbuntuRuntime extracts it into the rootfs and
# writes a marker. Release builds bundle it next to common.tar.xz so the
# 知识短片 skill can render out of the box:
#
#   WUNDER_UBUNTU_ROOTFS=/path/rootfs.tar.xz builders/build-android-browser-pack.sh
#   WUNDER_PACKS_SRC=/tmp/wunder-packs builders/stage-android-ubuntu.sh
#
# Env:
#   WUNDER_PACK_ABI       arm64 (default) or x86_64
#   WUNDER_BROWSER_PACK_OUT output archive (default /tmp/wunder-packs/browser.tar.xz)
#   WUNDER_BROWSER_CHROMIUM_URL  Chromium archive override
#   WUNDER_BROWSER_PACK_WORK  build directory (default /tmp/wunder-browser-pack-<abi>)
#   WUNDER_COMMON_PACK    common overlay to stack on (default: staged arm64 pack or
#                         /tmp/wunder-packs/common-<abi>.tar.xz); required, so the
#                         overlay is never diffed against the bare rootfs.
#
# The overlay is diffed against base + common, matching the device install
# order, so it never replaces files the common pack already owns.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ABI="${WUNDER_PACK_ABI:-arm64}"
BASE_ARCHIVE="${WUNDER_UBUNTU_ROOTFS:-$ROOT/crates/wunder-android/android/app/src/main/assets/ubuntu/rootfs.tar.xz}"
WORK="${WUNDER_BROWSER_PACK_WORK:-/tmp/wunder-browser-pack-$ABI}"
OUT="${WUNDER_BROWSER_PACK_OUT:-/tmp/wunder-packs/browser.tar.xz}"
REQUIREMENTS="$ROOT/packaging/python/requirements-android-browser.txt"
COMMON_PACK="${WUNDER_COMMON_PACK:-}"
if [ -z "$COMMON_PACK" ]; then
    if [ "$ABI" = "arm64" ] && [ -f "$ROOT/crates/wunder-android/android/app/src/main/assets/ubuntu/packs/common.tar.xz" ]; then
        COMMON_PACK="$ROOT/crates/wunder-android/android/app/src/main/assets/ubuntu/packs/common.tar.xz"
    elif [ -f "/tmp/wunder-packs/common-$ABI.tar.xz" ]; then
        COMMON_PACK="/tmp/wunder-packs/common-$ABI.tar.xz"
    fi
fi
[ -n "$COMMON_PACK" ] && [ -f "$COMMON_PACK" ] || {
    echo "missing common pack: set WUNDER_COMMON_PACK to the common-<abi> pack" >&2
    exit 2
}

# Chromium runtime dependencies. Ubuntu 24.04 renamed several of these with
# the t64 suffix; keep the names explicit because the arm64 rootfs runs the
# noble archive.
PACKAGES="${WUNDER_BROWSER_PACKAGES:-libasound2t64 libatk-bridge2.0-0t64 libatk1.0-0t64 \
libatspi2.0-0t64 libcairo2 libcups2t64 libdbus-1-3 libdrm2 libgbm1 libglib2.0-0t64 \
libnspr4 libnss3 libpango-1.0-0 libx11-6 libx11-xcb1 libxcb1 libxcb-shm0 \
libxcb-render0 libxcb-randr0 libxcb-xfixes0 libxcomposite1 libxdamage1 libxext6 \
libxfixes3 libxkbcommon0 libxrandr2 libxss1 libxtst6 libegl1 libgles2 \
fonts-liberation fontconfig}"

case "$ABI" in
    arm64)
        CHROMIUM_URL="${WUNDER_BROWSER_CHROMIUM_URL:-https://cdn.playwright.dev/builds/chromium/1091/chromium-linux-arm64.zip}"
        CHROMIUM_SHA256="${WUNDER_BROWSER_CHROMIUM_SHA256:-ef686d543ddf32d5d9cc38e1fead33565ef4e4c537d67aa5d6a99f327fbcf351}"
        PACK_PLATFORMS=(--platform manylinux2014_aarch64 --platform manylinux_2_17_aarch64 --platform manylinux_2_28_aarch64)
        ;;
    x86_64)
        # Only for emulator verification; Playwright's x64 Chromium build is
        # fetched from the Chrome for Testing archive instead.
        CHROMIUM_URL="${WUNDER_BROWSER_CHROMIUM_URL:-https://storage.googleapis.com/chrome-for-testing-public/122.0.6261.111/linux64/chrome-linux64.zip}"
        CHROMIUM_SHA256="${WUNDER_BROWSER_CHROMIUM_SHA256:-b059c80216abd74f25a35c08c6ad601a30dfdb09588af27828a9bc7adde65bd3}"
        PACK_PLATFORMS=(--platform manylinux2014_x86_64 --platform manylinux_2_17_x86_64 --platform manylinux_2_28_x86_64)
        ;;
    *)
        echo "unsupported ABI: $ABI" >&2
        exit 2
        ;;
esac

[ -f "$BASE_ARCHIVE" ] || { echo "missing rootfs: $BASE_ARCHIVE" >&2; exit 2; }
[ -f "$REQUIREMENTS" ] || { echo "missing requirements: $REQUIREMENTS" >&2; exit 2; }
if [ "$ABI" = "arm64" ]; then
    [ -x /usr/bin/qemu-aarch64-static ] || { echo "install qemu-user-static" >&2; exit 2; }
fi
sudo -n true || { echo "passwordless sudo is required for chroot builds" >&2; exit 2; }

# A killed build can leave /proc, /dev or /sys bind mounts under $WORK.
# Removing the tree while they are attached would traverse the mount and
# delete real host device nodes, so refuse to continue until they are gone.
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
mkdir -p "$WORK/base" "$WORK/work" "$WORK/down" "$WORK/wheels"
tar -xJf "$BASE_ARCHIVE" -C "$WORK/base" \
    --exclude='*/dev/*' --exclude='dev/*' 2>/dev/null
if [ -f "$WORK/base/etc/os-release" ]; then
    BASE_TOP="$WORK/base"
else
    BASE_TOP="$(find "$WORK/base" -mindepth 1 -maxdepth 1 -type d | head -1)"
fi
[ -n "$BASE_TOP" ] || { echo "rootfs archive has no top directory" >&2; exit 2; }
cp -a "$BASE_TOP/." "$WORK/work/"

# Match the device install order: the browser overlay layers on top of the
# common tools, so diff against that tree instead of the bare rootfs. A wrong
# or stale common pack would otherwise duplicate the whole tool overlay.
echo "==> stack common pack: $COMMON_PACK"
sudo -n tar -xJf "$COMMON_PACK" -C "$WORK/work" --numeric-owner
sudo -n chown -R "$(id -u):$(id -g)" "$WORK/work"
[ -f "$WORK/work/opt/wunder/wheels/requirements-android.txt" ] || {
    echo "common pack has no Python wheelhouse: $COMMON_PACK" >&2
    exit 2
}
REF="$WORK/ref"
sudo -n rm -rf "$REF"
cp -a "$WORK/work" "$REF"

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

run_guest() {
    sudo -n chroot "$WORK/work" /usr/bin/env -i HOME=/root \
        PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        DEBIAN_FRONTEND=noninteractive LC_ALL=C LANG=C.UTF-8 \
        /bin/bash -lc "$1"
}

echo "==> apt-get update ($ABI)"
run_guest "apt-get update -o Acquire::ForceIPv4=true" >/dev/null
echo "==> apt-get install: Chromium runtime libraries"
run_guest "apt-get install -y --no-install-recommends $PACKAGES" >/dev/null

echo "==> playwright wheels"
python3 -m pip download --only-binary=:all: "${PACK_PLATFORMS[@]}" \
    --python-version 312 --implementation cp --abi cp312 \
    -d "$WORK/wheels" -r "$REQUIREMENTS"
if find "$WORK/wheels" -type f ! -name '*.whl' | grep -q .; then
    echo "wheelhouse contains a source archive" >&2
    exit 2
fi
DIST_PACKAGES="$WORK/work/usr/local/lib/python3.12/dist-packages"
mkdir -p "$DIST_PACKAGES"
for wheel in "$WORK/wheels"/*.whl; do
    # Wheels are zip files; unzipping is what pip would do for these pure /
    # prebuilt payloads, and it keeps the build independent of the guest pip.
    unzip -q -o "$wheel" -d "$DIST_PACKAGES"
done

echo "==> Chromium"
curl -fL --retry 3 --connect-timeout 20 -o "$WORK/down/chromium.zip" "$CHROMIUM_URL"
if [ -n "${CHROMIUM_SHA256:-}" ]; then
    echo "$CHROMIUM_SHA256  $WORK/down/chromium.zip" | sha256sum -c -
fi
mkdir -p "$WORK/chromium" "$WORK/work/opt/browser"
unzip -q "$WORK/down/chromium.zip" -d "$WORK/chromium"
CHROME_BIN="$(find "$WORK/chromium" -type f -name chrome | head -1)"
[ -n "$CHROME_BIN" ] || { echo "chromium archive has no chrome binary" >&2; exit 2; }
cp -a "$(dirname "$CHROME_BIN")" "$WORK/work/opt/browser/chrome-linux"
chmod 755 "$WORK/work/opt/browser/chrome-linux/chrome"

echo "==> verify browser runtime"
run_guest "python3 -c 'import playwright, greenlet, pyee, typing_extensions; print(\"playwright import ok\")'"
MISSING="$(run_guest "ldd /opt/browser/chrome-linux/chrome 2>/dev/null | grep 'not found' || true")"
if [ -n "$MISSING" ]; then
    echo "Chromium has unresolved libraries:" >&2
    echo "$MISSING" >&2
    exit 2
fi
run_guest "/opt/browser/chrome-linux/chrome --version"
cat > "$WORK/work/tmp/browser-smoke.py" <<'PY'
from playwright.sync_api import sync_playwright

with sync_playwright() as p:
    browser = p.chromium.launch(
        executable_path="/opt/browser/chrome-linux/chrome",
        headless=True,
        args=[
            "--no-sandbox",
            "--no-zygote",
            "--disable-dev-shm-usage",
            "--disable-gpu",
        ],
    )
    page = browser.new_page()
    page.set_content("<h1>ok</h1>")
    assert page.inner_text("h1") == "ok"
    browser.close()
print("playwright launch ok")
PY
run_guest "python3 /tmp/browser-smoke.py"

cleanup
MOUNTS=()

# Volatile/host state must not travel in the pack.
sudo -n rm -rf "$WORK/work/var/lib/apt/lists" "$WORK/work/var/cache/apt" \
       "$WORK/work/var/log" "$WORK/work/tmp" "$WORK/work/root" \
       "$WORK/work/dev" "$WORK/work/proc" "$WORK/work/sys" \
       "$WORK/work/etc/resolv.conf" "$WORK/work/var/lib/dpkg/lock"* \
       "$WORK/work/var/lib/dpkg/triggers/Lock" "$WORK/work/var/lib/dpkg/updates"
sudo -n chown -R "$(id -u):$(id -g)" "$WORK/work"
mkdir -p "$WORK/work/dev" "$WORK/work/proc" "$WORK/work/sys" "$WORK/work/tmp" \
         "$WORK/work/var/tmp"
chmod 1777 "$WORK/work/tmp" "$WORK/work/var/tmp"

echo "==> diff base+common -> browser overlay"
python3 - "$REF" "$WORK/work" "$WORK/files.txt" <<'PY'
import hashlib
import os
import sys

base, work, out = sys.argv[1], sys.argv[2], sys.argv[3]
SKIP_TOP = {"proc", "sys", "dev", "tmp", "run", "root"}
SKIP_PREFIX = (
    "var/lib/apt/lists/", "var/cache/apt/", "var/log/",
    "var/lib/dpkg/lock", "var/lib/dpkg/triggers/Lock",
    "var/lib/dpkg/updates/", "etc/resolv.conf",
    # 字体缓存归 common 包所有：浏览器包叠加安装后若带着旧缓存，
    # 会覆盖 common 里包含视频字体的新缓存。
    "var/cache/fontconfig/",
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

changed = [rel for rel, info in work_rows.items()
           if rel not in base_rows or base_rows[rel] != info]
with open(out, "w", encoding="utf-8") as fh:
    for rel in sorted(changed):
        fh.write(rel + "\n")
print(f"files: {len(changed)} ({sum(1 for rel in changed if rel not in base_rows)} new)")
PY

echo "==> pack $(wc -l < "$WORK/files.txt") entries"
mkdir -p "$(dirname "$OUT")"
XZ_OPT="-T0 -6" tar -C "$WORK/work" --numeric-owner --no-recursion \
    -T "$WORK/files.txt" -J -cf "$OUT"
chmod 644 "$OUT"
echo "pack: $OUT"
ls -lh "$OUT"
