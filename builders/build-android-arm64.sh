#!/usr/bin/env bash
# Build 安卓版 (wunder-android): Rust cdylib slice(s) + APK.
#
# Required environment (defaults assume the local dev box):
#   ANDROID_HOME       Android SDK root              (default ~/android-sdk)
#   ANDROID_NDK_ROOT   NDK root                      (default $ANDROID_HOME/ndk/28.2.13676358)
#   JAVA_HOME          JDK 17                        (default: resolved from javac)
#   GRADLE             Gradle 8.13+ launcher          (default ~/gradle/gradle-8.13/bin/gradle)
#   WUNDER_ANDROID_ABIS  Comma-separated ABIs         (default arm64-v8a;
#                        x86_64 is accepted for emulator verification)
#   WUNDER_ANDROID_BUILD_TYPE  release|debug           (default release; debug keeps
#                        the local emulator install path)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE="$ROOT/crates/wunder-android"

export ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-$ANDROID_HOME/ndk/28.2.13676358}"
# skia-bindings' build script keys off this spelling; without it a cache miss
# falls into the source-build path and fails before the prebuilt lookup.
export ANDROID_NDK="${ANDROID_NDK:-$ANDROID_NDK_ROOT}"
export JAVA_HOME="${JAVA_HOME:-$(dirname "$(dirname "$(readlink -f "$(command -v javac)")")")}"

host_arch="$(uname -m)"
if [[ "$host_arch" == aarch64 || "$host_arch" == arm64 ]]; then
    # NDK 与 build-tools 只有 x86_64 宿主版：套件在 android 卷里带 qemu-x86_64-static，
    # 由 android-host-tools.sh 生成的包装器负责编译/链接/aapt2。
    ANDROID_HOST_BIN="${WUNDER_ANDROID_BIN:-$(cd -- "$ANDROID_HOME/.." && pwd -P)/bin}"
    [[ -x "$ANDROID_HOST_BIN/aarch64-linux-android26-clang" ]] || {
        echo "aarch64 宿主缺少 $ANDROID_HOST_BIN/aarch64-linux-android26-clang（先运行 prepare-android.sh --host-arch aarch64）" >&2
        exit 2
    }
    export PATH="$ANDROID_HOST_BIN:$ANDROID_HOME/platform-tools:$PATH"
    export CC_aarch64_linux_android="$ANDROID_HOST_BIN/aarch64-linux-android26-clang"
    export CXX_aarch64_linux_android="$ANDROID_HOST_BIN/aarch64-linux-android26-clang++"
    export AR_aarch64_linux_android="$ANDROID_HOST_BIN/llvm-ar"
    export RANLIB_aarch64_linux_android="$ANDROID_HOST_BIN/llvm-ranlib"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$ANDROID_HOST_BIN/rust-linker"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"
    export WUNDER_AAPT2="$ANDROID_HOST_BIN/aapt2"
    # CMake 的编译器探测会链接可执行文件；模拟环境下走 aws-lc 的 cc 构建链，只编译+打包。
    export AWS_LC_SYS_CMAKE_BUILDER=0
else
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
fi

if [[ "$host_arch" != aarch64 && "$host_arch" != arm64 ]]; then
    export CC_x86_64_linux_android="$TOOLCHAIN/x86_64-linux-android26-clang"
    export CXX_x86_64_linux_android="$TOOLCHAIN/x86_64-linux-android26-clang++"
    export AR_x86_64_linux_android="$TOOLCHAIN/llvm-ar"
    export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$TOOLCHAIN/x86_64-linux-android26-clang"
    export CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"
fi

ABIS="${WUNDER_ANDROID_ABIS:-arm64-v8a}"
IFS=',' read -r -a ABI_LIST <<<"$ABIS"

# Release is the shipping variant; x86_64 emulator runs can still ask for a
# debug APK (installed over adb with the debug key).
BUILD_TYPE="${WUNDER_ANDROID_BUILD_TYPE:-release}"
case "$BUILD_TYPE" in
    release)
        GRADLE_TASK=assembleRelease
        APK_SOURCE=release/app-release.apk
        ;;
    debug)
        GRADLE_TASK=assembleDebug
        APK_SOURCE=debug/app-debug.apk
        ;;
    *) echo "unsupported WUNDER_ANDROID_BUILD_TYPE: $BUILD_TYPE (expected release or debug)" >&2; exit 2 ;;
esac

cd "$CRATE"
for ABI in "${ABI_LIST[@]}"; do
    case "$ABI" in
        arm64-v8a) TARGET=aarch64-linux-android ;;
        x86_64)
            [[ "$host_arch" != aarch64 && "$host_arch" != arm64 ]] || {
                echo "aarch64 宿主只构建 arm64-v8a（x86_64 切片仅用于 x86 模拟器）" >&2
                exit 2
            }
            TARGET=x86_64-linux-android
            ;;
        *) echo "unsupported ABI: $ABI (expected arm64-v8a or x86_64)" >&2; exit 2 ;;
    esac
    if [[ ! -d "$(rustc --print sysroot)/lib/rustlib/$TARGET/lib" ]]; then
        command -v rustup >/dev/null || {
            echo "Rust target $TARGET is not installed and rustup is unavailable (offline build?)" >&2
            exit 2
        }
        rustup target add "$TARGET" >/dev/null
    fi
    cargo build --release --target "$TARGET" --lib
    target_dir="${CARGO_TARGET_DIR:-$PWD/target}"
    mkdir -p "android/app/src/main/jniLibs/$ABI"
    cp "$target_dir/$TARGET/release/libwunder_android.so" "android/app/src/main/jniLibs/$ABI/"
done

GRADLE="${GRADLE:-$HOME/gradle/gradle-8.13/bin/gradle}"
gradle_args=()
if [[ -n "${WUNDER_GRADLE_ARGS:-}" ]]; then
    read -r -a gradle_args <<<"$WUNDER_GRADLE_ARGS"
fi
cd android
if [[ -n "${WUNDER_AAPT2:-}" ]]; then
    # aarch64 宿主：AGP 默认会解包 Maven 里的 x86_64 aapt2，这里换成 qemu 包装器。
    gradle_args+=("-Pandroid.aapt2FromMavenOverride=$WUNDER_AAPT2")
fi
"$GRADLE" --no-daemon "${gradle_args[@]}" "-PwunderAbis=$ABIS" "$GRADLE_TASK"

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
cp -f "$CRATE/android/app/build/outputs/apk/$APK_SOURCE" "$APK_OUT"

echo "APK ($BUILD_TYPE): $APK_OUT"
