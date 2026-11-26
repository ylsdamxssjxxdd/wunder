#!/usr/bin/env bash
# Build 安卓版 (wunder-android): Rust cdylib slice(s) + APK.
#
# Required environment (defaults assume the local dev box):
#   ANDROID_HOME       Android SDK root              (default ~/android-sdk)
#   ANDROID_NDK_ROOT   NDK root                      (default $ANDROID_HOME/ndk/30.0.16248370)
#   JAVA_HOME          JDK 17                        (default: resolved from javac)
#   GRADLE             Gradle 8.13+ launcher          (default ~/gradle/gradle-8.13/bin/gradle)
#   WUNDER_ANDROID_ABIS  Comma-separated ABIs         (default arm64-v8a;
#                        x86_64 is accepted for emulator verification)
#   WUNDER_ANDROID_BUILD_TYPE  release|debug           (default release; debug keeps
#                        the local emulator install path)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE="$ROOT/crates/wunder-android"
cargo_config=()
if [[ -n "${WUNDER_CARGO_VENDOR:-}" ]]; then
    cargo_config=(--config 'source.crates-io.replace-with="vendored-sources"'
        --config "source.vendored-sources.directory=\"$WUNDER_CARGO_VENDOR\"")
fi

export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/android-sdk}}"
# Android NDK 要与随包 libc++ 同源。优先级：显式 WUNDER_ANDROID_NDK > 固定版本
# 目录 > runner 的 latest > 已显式设置的 ANDROID_NDK_ROOT（离线套件只有一套 NDK，
# ARM 侧的 qemu 包装器也按它生成）> $ANDROID_HOME/ndk 下最新的一个。
# 不能直接 `ANDROID_NDK_ROOT:-...`：GitHub runner 会预设成镜像自带的旧 NDK（27.x）。
WUNDER_ANDROID_NDK_VERSION="${WUNDER_ANDROID_NDK_VERSION:-30.0.16248370}"
resolve_ndk() {
    if [[ -n "${WUNDER_ANDROID_NDK:-}" ]]; then echo "$WUNDER_ANDROID_NDK"; return; fi
    if [[ -d "$ANDROID_HOME/ndk/$WUNDER_ANDROID_NDK_VERSION" ]]; then echo "$ANDROID_HOME/ndk/$WUNDER_ANDROID_NDK_VERSION"; return; fi
    if [[ -n "${ANDROID_NDK_LATEST_HOME:-}" && -d "${ANDROID_NDK_LATEST_HOME}" ]]; then echo "$ANDROID_NDK_LATEST_HOME"; return; fi
    if [[ -n "${ANDROID_NDK_ROOT:-}" && -d "${ANDROID_NDK_ROOT}" ]]; then echo "$ANDROID_NDK_ROOT"; return; fi
    if [[ -d "$ANDROID_HOME/ndk" ]]; then
        find "$ANDROID_HOME/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1
    fi
}
export ANDROID_NDK_ROOT="$(resolve_ndk)"
[[ -n "$ANDROID_NDK_ROOT" && -d "$ANDROID_NDK_ROOT" ]] || {
    echo "未找到 Android NDK（期望 $ANDROID_HOME/ndk/$WUNDER_ANDROID_NDK_VERSION）；请安装，或用 WUNDER_ANDROID_NDK 指定一套" >&2
    exit 2
}
# skia-bindings 的 build script 读这两个拼写；缺了会落到源码编译路径。
export ANDROID_NDK="$ANDROID_NDK_ROOT"
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
    # skia-bindings 的产物缓存按 feature key 命中，里面不含 NDK 版本：换过 NDK
    # 之后它仍会复用旧 NDK 编出来的 Skia，链接照样通过，装到手机上却死在 dlopen。
    # 这里记住每套 NDK 的 revision，变更时先丢掉旧的 C++ 产物再编。
    ndk_revision="$(sed -n 's/^Pkg.Revision *= *//p' "$ANDROID_NDK_ROOT/source.properties" | head -1)"
    ndk_stamp="$PWD/target/.ndk-revision-$TARGET"
    if [[ -n "$ndk_revision" && -f "$ndk_stamp" && "$(cat "$ndk_stamp")" != "$ndk_revision" ]]; then
        echo "NDK 从 $(cat "$ndk_stamp") 变为 $ndk_revision，重编 C++ 产物（skia-bindings）" >&2
        cargo "${cargo_config[@]}" clean -p skia-bindings --target "$TARGET"
    fi
    cargo "${cargo_config[@]}" build --release --target "$TARGET" --lib
    if [[ -n "$ndk_revision" ]]; then
        mkdir -p "$PWD/target"
        printf '%s' "$ndk_revision" > "$ndk_stamp"
    fi
    target_dir="${CARGO_TARGET_DIR:-$PWD/target}"
    mkdir -p "android/app/src/main/jniLibs/$ABI"
    cp "$target_dir/$TARGET/release/libwunder_android.so" "android/app/src/main/jniLibs/$ABI/"
    # 随包 libc++ 必须导出 cdylib 需要的全部符号：Skia 是上游预编译产物（用较新
    # libc++ 编），旧 NDK 自带的运行时可能缺符号。候选按优先级逐个验证，取第一个
    # 能自证的：① WUNDER_ANDROID_LIBCXX ② 离线套件的 $ANDROID_HOME/ndk-libcxx
    # ③ 当前 NDK 的 sysroot 副本。都不行就让构建失败，不发出必然闪退的包。
    ndk_libcxx="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/$TARGET/libc++_shared.so"
    [[ -f "$ndk_libcxx" ]] || ndk_libcxx="$(find "$ANDROID_NDK_ROOT" -path "*/sysroot/usr/lib/$TARGET/libc++_shared.so" -print -quit)"
    candidates=()
    [[ -n "${WUNDER_ANDROID_LIBCXX:-}" ]] && candidates+=("$WUNDER_ANDROID_LIBCXX")
    candidates+=("$ANDROID_HOME/ndk-libcxx/$ABI/libc++_shared.so")
    candidates+=("$ndk_libcxx")
    nm_args=()
    if [[ "$host_arch" != aarch64 && "$host_arch" != arm64 && -x "$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-nm" ]]; then
        nm_args=(--nm "$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-nm")
    fi
    libcxx_shared=""
    check_output=""
    for candidate in "${candidates[@]}"; do
        [[ -f "$candidate" ]] || continue
        if check_output="$(python3 "$CRATE/scripts/check-native-runtime.py" \
                --so "android/app/src/main/jniLibs/$ABI/libwunder_android.so" \
                --libcxx "$candidate" "${nm_args[@]}" 2>&1)"; then
            libcxx_shared="$candidate"
            echo "$check_output"
            break
        fi
    done
    if [[ -z "$libcxx_shared" ]]; then
        echo "$check_output" >&2
        echo "没有与 C++ 产物匹配的 libc++：请安装较新的 NDK（$WUNDER_ANDROID_NDK_VERSION），或用 WUNDER_ANDROID_LIBCXX 指定一份" >&2
        exit 2
    fi
    cp -f "$libcxx_shared" "android/app/src/main/jniLibs/$ABI/"
    echo "libc++ ($ABI): $libcxx_shared"
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
