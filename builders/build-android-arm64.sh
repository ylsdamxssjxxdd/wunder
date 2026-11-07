#!/usr/bin/env bash
# Build 安卓版 (wunder-android): Rust cdylib slice(s) + debug APK.
#
# Required environment (defaults assume the local dev box):
#   ANDROID_HOME       Android SDK root              (default ~/android-sdk)
#   ANDROID_NDK_ROOT   NDK root                      (default $ANDROID_HOME/ndk/28.2.13676358)
#   JAVA_HOME          JDK 17                        (default: resolved from javac)
#   GRADLE             Gradle 8.13+ launcher          (default ~/gradle/gradle-8.13/bin/gradle)
#   WUNDER_ANDROID_ABIS  Comma-separated ABIs         (default arm64-v8a;
#                        x86_64 is accepted for emulator verification)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE="$ROOT/crates/wunder-android"

export ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-$ANDROID_HOME/ndk/28.2.13676358}"
export JAVA_HOME="${JAVA_HOME:-$(dirname "$(dirname "$(readlink -f "$(command -v javac)")")")}"

TOOLCHAIN="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin"
export PATH="$TOOLCHAIN:$ANDROID_HOME/platform-tools:$PATH"

# cc-rs builds SQLite/ring/zstd with the NDK clang; the same driver links the
# Rust cdylib. API 26 matches the manifest's minSdk. The page-size flag is the
# Android 15+ 16 KiB requirement.
export CC_aarch64_linux_android="$TOOLCHAIN/aarch64-linux-android26-clang"
export CXX_aarch64_linux_android="$TOOLCHAIN/aarch64-linux-android26-clang++"
export AR_aarch64_linux_android="$TOOLCHAIN/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/aarch64-linux-android26-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"

export CC_x86_64_linux_android="$TOOLCHAIN/x86_64-linux-android26-clang"
export CXX_x86_64_linux_android="$TOOLCHAIN/x86_64-linux-android26-clang++"
export AR_x86_64_linux_android="$TOOLCHAIN/llvm-ar"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$TOOLCHAIN/x86_64-linux-android26-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"

ABIS="${WUNDER_ANDROID_ABIS:-arm64-v8a}"
IFS=',' read -r -a ABI_LIST <<<"$ABIS"

cd "$CRATE"
for ABI in "${ABI_LIST[@]}"; do
    case "$ABI" in
        arm64-v8a) TARGET=aarch64-linux-android ;;
        x86_64) TARGET=x86_64-linux-android ;;
        *) echo "unsupported ABI: $ABI (expected arm64-v8a or x86_64)" >&2; exit 2 ;;
    esac
    rustup target add "$TARGET" >/dev/null
    cargo build --release --target "$TARGET" --lib
    mkdir -p "android/app/src/main/jniLibs/$ABI"
    cp "target/$TARGET/release/libwunder_android.so" "android/app/src/main/jniLibs/$ABI/"
done

GRADLE="${GRADLE:-$HOME/gradle/gradle-8.13/bin/gradle}"
cd android
"$GRADLE" --no-daemon "-PwunderAbis=$ABIS" assembleDebug

VERSION="$(python3 - "$ROOT/config/app_version.json" <<'PY'
import json, sys
print(json.load(open(sys.argv[1], encoding="utf-8"))["version"])
PY
)"
if [[ "${#ABI_LIST[@]}" -eq 1 ]]; then
    case "${ABI_LIST[0]}" in
        arm64-v8a) ARCH_LABEL=arm64 ;;
        x86_64) ARCH_LABEL=x64 ;;
        *) echo "unsupported ABI: ${ABI_LIST[0]}" >&2; exit 2 ;;
    esac
else
    ARCH_LABEL=universal
fi
OUT_DIR="$ROOT/target/android/dist/android-$ARCH_LABEL"
mkdir -p "$OUT_DIR"
APK_OUT="$OUT_DIR/wunder-android-$VERSION-$ARCH_LABEL.apk"
cp -f "$CRATE/android/app/build/outputs/apk/debug/app-debug.apk" "$APK_OUT"

echo "APK: $APK_OUT"
