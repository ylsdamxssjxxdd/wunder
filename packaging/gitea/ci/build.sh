#!/usr/bin/env bash
set -euo pipefail

case "${ARCH:?ARCH is required}" in
    arm64) runtime=runtime-aarch64 ;;
    amd64) runtime=runtime-x86_64 ;;
    *) echo 'Only arm64 and amd64 are supported.' >&2; exit 2 ;;
esac
[[ "$(uname -m)" == aarch64 ]] || { echo 'An ARM64 build worker is required.' >&2; exit 2; }
export WUNDER_OFFLINE_ROOT="${WUNDER_OFFLINE_ROOT:-/opt/offline-sdk}"
[[ -d "$WUNDER_OFFLINE_ROOT/cargo-vendor-slint" \
    && -x "$WUNDER_OFFLINE_ROOT/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin/rustc" ]] || {
    echo 'The read-only Rust-builder SDK mount is missing or incomplete. Run prepare-sdk.sh before starting the runner.' >&2
    exit 2
}
cache_key="$(printf '%s' "${CI_REPOSITORY:?CI_REPOSITORY is required}" | sha256sum | cut -c 1-32)"
cache="/ci-cache/$cache_key/$ARCH"
mkdir -p "$cache/desktop" "$cache/cli" "$cache/desktop-home" "$cache/cli-home" release-assets
export WUNDER_APPIMAGE_RUNTIME="$WUNDER_OFFLINE_ROOT/appimage-runtime/$runtime"
export WUNDER_OUTPUT_DIR="$PWD/release-assets"
export WUNDER_CLI_OUTPUT_DIR="$PWD/release-assets"
export CARGO_BUILD_JOBS="${WUNDER_BUILD_JOBS:-4}"
chmod +x builders/linux_arm64_sysroot_cc.sh builders/linux-amd64-cross-cc.sh

CARGO_HOME="$cache/desktop-home" CARGO_TARGET_DIR="$cache/desktop" \
    bash "builders/build-linux-$ARCH-appimage.sh"
CARGO_HOME="$cache/cli-home" CARGO_TARGET_DIR="$cache/cli" \
    bash "builders/build-cli-linux-$ARCH-offline.sh"

python3 /opt/offline-ci/verify.py release-assets "$ARCH"
(cd release-assets; find . -maxdepth 1 -type f ! -name 'SHA256SUMS-*' -print0 | sort -z | xargs -0 sha256sum > "SHA256SUMS-linux-$ARCH.txt")
printf '\nArtifacts ready for Linux %s\n' "$ARCH"
