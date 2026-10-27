#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
bundle="$(pwd -P)"
sdk="${WUNDER_SDK_PATH:-}"
if [[ -z "$sdk" && -f .env ]]; then
    sdk="$(sed -n 's/^WUNDER_SDK_PATH=//p' .env | tail -n 1)"
    case "$sdk" in
        \"*\") sdk="${sdk:1:${#sdk}-2}" ;;
        \'*\') sdk="${sdk:1:${#sdk}-2}" ;;
    esac
fi
sdk="${sdk:-$bundle/rust-builder/offline}"
[[ "$sdk" == /* ]] || { echo 'WUNDER_SDK_PATH must be an absolute host directory.' >&2; exit 2; }
if [[ ! -d "$sdk" && "$sdk" != "$bundle/rust-builder/offline" ]]; then
    echo 'The custom WUNDER_SDK_PATH directory does not exist.' >&2
    exit 2
fi
validate_sdk() {
    local directory="$1"
    local executable
    for executable in rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin/rustc \
        rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin/cargo \
        linux-arm64-ubuntu18/root/usr/lib/gcc/aarch64-linux-gnu/7/cc1 \
        linux-amd64-ubuntu18/root/usr/bin/x86_64-linux-gnu-gcc-7; do
        [[ -x "$directory/$executable" ]] || { echo "SDK executable is missing: $executable" >&2; return 2; }
    done
    [[ -d "$directory/cargo-vendor-slint" && -s "$directory/appimage-runtime/runtime-aarch64" \
        && -s "$directory/appimage-runtime/runtime-x86_64" ]] || { echo 'SDK is incomplete.' >&2; return 2; }
}
[[ -d "$sdk" ]] || {
    (cd rust-builder; sha256sum --check SHA256SUMS)
    staging="$(mktemp -d "$bundle/rust-builder/.extract.XXXXXXXX")"
    trap 'if [[ -n "${staging:-}" ]]; then rm -rf -- "$staging"; fi' EXIT
    tar -xzf rust-builder/offline-sdk.tar.gz -C "$staging" --no-same-owner
    [[ -d "$staging/offline" ]] || { echo 'SDK archive layout is invalid.' >&2; exit 2; }
    validate_sdk "$staging/offline"
    mv -- "$staging/offline" "$sdk"
    rmdir -- "$staging"
    staging=""
}
sdk="$(cd "$sdk" && pwd -P)"
validate_sdk "$sdk"
[[ "$sdk" != *\'* && "$sdk" != *$'\n'* && "$sdk" != *$'\r'* ]] || {
    echo 'SDK mount paths must not contain single quotes or line breaks.' >&2; exit 2;
}
[[ -f .env ]] || cp .env.example .env
environment="$(mktemp "$bundle/.env.XXXXXXXX")"
trap 'rm -f -- "$environment"' EXIT
sed '/^WUNDER_SDK_PATH=/d' .env > "$environment"
printf "\nWUNDER_SDK_PATH='%s'\n" "$sdk" >> "$environment"
mv -f -- "$environment" .env
trap - EXIT
printf '%s\n' 'SDK prepared. Runner and build containers use the same read-only host directory.'
