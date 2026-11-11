#!/usr/bin/env bash
# Package the uncommitted Android runtime inputs into the release archive that
# .github/workflows/desktop-nightly.yml downloads from the hidden draft
# `android-runtime` release. The packs are CI inputs, not a user-facing download.
# CI reads the draft with the RELEASE_TOKEN secret because GITHUB_TOKEN cannot
# see draft releases.
#
# The APK embeds a Linux rootfs, offline tool and browser packs, and static
# proot/busybox libraries. None of these are committed (tens to hundreds of
# megabytes), so they are staged by builders/stage-android-ubuntu.sh,
# builders/build-android-ubuntu-pack.sh and
# builders/build-android-browser-pack.sh, then packaged here once and uploaded:
#
#   packaging/android/pack-runtime.sh            # build target/android/runtime/
#   packaging/android/pack-runtime.sh --publish  # also upload to GitHub
#
# Inputs default to the staged app tree; override with WUNDER_ANDROID_APP_MAIN
# (expects assets/ubuntu/{rootfs.tar.xz,NOTICE.txt},
# assets/ubuntu/packs/{common,browser}.tar.xz and
# jniLibs/arm64-v8a/{liboperit_proot.so,liboperit_loader.so,libbusybox.so}).
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
    "assets/ubuntu/packs/browser.tar.xz"
    "jniLibs/arm64-v8a/liboperit_proot.so"
    "jniLibs/arm64-v8a/liboperit_loader.so"
    "jniLibs/arm64-v8a/libbusybox.so"
)
for entry in "${required[@]}"; do
    [[ -f "$MAIN/$entry" ]] || {
        echo "runtime input missing: $MAIN/$entry" >&2
        echo "stage it first: builders/stage-android-ubuntu.sh + builders/build-android-ubuntu-pack.sh + builders/build-android-browser-pack.sh" >&2
        exit 2
    }
done

# BusyBox 1.36 on the device cannot decode xz BCJ filters; such archives only
# fail after installation ("tar: corrupted data" + "tar: short read").
for archive in assets/ubuntu/rootfs.tar.xz \
               assets/ubuntu/packs/common.tar.xz \
               assets/ubuntu/packs/browser.tar.xz; do
    if xz --robot --list -vv "$MAIN/$archive" | awk -F '\t' '$1 == "block" { print $NF }' \
        | grep -qE -- '--(arm64|armthumb|arm|x86|powerpc|ia64|sparc)( |$)'; then
        echo "BCJ filter in $MAIN/$archive: the bundled busybox cannot decode it" >&2
        exit 2
    fi
done

mkdir -p "$OUT_DIR"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/assets/ubuntu/packs" "$STAGE/jniLibs/arm64-v8a"
cp -f "$MAIN/assets/ubuntu/rootfs.tar.xz" "$STAGE/assets/ubuntu/"
cp -f "$MAIN/assets/ubuntu/NOTICE.txt" "$STAGE/assets/ubuntu/"
cp -f "$MAIN/assets/ubuntu/packs/common.tar.xz" "$STAGE/assets/ubuntu/packs/"
cp -f "$MAIN/assets/ubuntu/packs/browser.tar.xz" "$STAGE/assets/ubuntu/packs/"
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
            --title "Wunder Android runtime inputs（内部构建，不对外发布）" \
            --notes "仅由 CI 作为 APK 输入，不对外发布。请下载安装包，不要手动下载本页文件。" \
            --draft --latest=false
    fi
    gh release upload "$TAG" "$ARCHIVE" "$OUT_DIR/SHA256SUMS" --repo "$REPO" --clobber
    # Existing releases were public before this switch; force them back to draft
    # and relabel them as internal CI inputs.
    gh release edit "$TAG" --repo "$REPO" \
        --title "Wunder Android runtime inputs（内部构建，不对外发布）" \
        --notes "仅由 CI 作为 APK 输入，不对外发布。请下载安装包，不要手动下载本页文件。" \
        --draft
    gh release view "$TAG" --repo "$REPO" --json assets --jq '.assets[].name'
fi
