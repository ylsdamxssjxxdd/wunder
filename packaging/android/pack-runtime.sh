#!/usr/bin/env bash
# Package the uncommitted Android runtime inputs into the release archive that
# .github/workflows/desktop-nightly.yml downloads from the android-runtime
# release.
#
# The APK embeds a Linux rootfs, an offline tool pack and static proot/busybox
# libraries. None of these are committed (tens to hundreds of megabytes), so
# they are staged by builders/stage-android-ubuntu.sh and
# builders/build-android-ubuntu-pack.sh, then packaged here once and uploaded:
#
#   packaging/android/pack-runtime.sh            # build target/android/runtime/
#   packaging/android/pack-runtime.sh --publish  # also upload to GitHub
#
# Inputs default to the staged app tree; override with WUNDER_ANDROID_APP_MAIN
# (expects assets/ubuntu/{rootfs.tar.xz,NOTICE.txt}, assets/ubuntu/packs/common.tar.xz
# and jniLibs/arm64-v8a/{liboperit_proot.so,liboperit_loader.so,libbusybox.so}).
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
MAIN="${WUNDER_ANDROID_APP_MAIN:-$ROOT/crates/wunder-android/android/app/src/main}"
OUT_DIR="${WUNDER_ANDROID_RUNTIME_OUT:-$ROOT/target/android/runtime}"
TAG="${WUNDER_ANDROID_RUNTIME_TAG:-android-runtime}"
REPO="${WUNDER_ANDROID_RUNTIME_REPO:-ylsdamxssjxxdd/wunder}"

required=(
    "assets/ubuntu/rootfs.tar.xz"
    "assets/ubuntu/NOTICE.txt"
    "assets/ubuntu/packs/common.tar.xz"
    "jniLibs/arm64-v8a/liboperit_proot.so"
    "jniLibs/arm64-v8a/liboperit_loader.so"
    "jniLibs/arm64-v8a/libbusybox.so"
)
for entry in "${required[@]}"; do
    [[ -f "$MAIN/$entry" ]] || {
        echo "runtime input missing: $MAIN/$entry" >&2
        echo "stage it first: builders/stage-android-ubuntu.sh + builders/build-android-ubuntu-pack.sh" >&2
        exit 2
    }
done

mkdir -p "$OUT_DIR"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/assets/ubuntu/packs" "$STAGE/jniLibs/arm64-v8a"
cp -f "$MAIN/assets/ubuntu/rootfs.tar.xz" "$STAGE/assets/ubuntu/"
cp -f "$MAIN/assets/ubuntu/NOTICE.txt" "$STAGE/assets/ubuntu/"
cp -f "$MAIN/assets/ubuntu/packs/common.tar.xz" "$STAGE/assets/ubuntu/packs/"
cp -f "$MAIN/jniLibs/arm64-v8a/liboperit_proot.so" \
      "$MAIN/jniLibs/arm64-v8a/liboperit_loader.so" \
      "$MAIN/jniLibs/arm64-v8a/libbusybox.so" "$STAGE/jniLibs/arm64-v8a/"

ARCHIVE="$OUT_DIR/wunder-android-runtime-arm64.tar.gz"
# Deterministic tar+gzip: the SHA256SUMS asset stays stable for identical
# inputs, so CI and humans can tell whether the runtime actually changed.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='@0' \
    -C "$STAGE" -cf - . | gzip -6 -n > "$ARCHIVE"
(cd "$OUT_DIR" && sha256sum "$(basename "$ARCHIVE")" > SHA256SUMS)
ls -lh "$OUT_DIR"

if [[ "${1:-}" == "--publish" ]]; then
    if ! gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
        gh release create "$TAG" \
            --repo "$REPO" \
            --title "Wunder Android runtime inputs（arm64 rootfs / 离线工具包 / proot）" \
            --notes "安卓版 APK 内嵌的运行时资产，由 .github/workflows/desktop-nightly.yml 下载后打进 APK。rootfs 为 Ubuntu 24.04 arm64（proot-distro 导出），common 包为离线工具叠加层，proot/busybox 为静态 arm64 构建；许可信息见包内 NOTICE.txt。" \
            --latest=false
    fi
    gh release upload "$TAG" "$ARCHIVE" "$OUT_DIR/SHA256SUMS" --repo "$REPO" --clobber
    gh release view "$TAG" --repo "$REPO" --json assets --jq '.assets[].name'
fi
