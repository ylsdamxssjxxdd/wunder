#!/usr/bin/env bash
# 汇总离线安卓工具链到套件的 android/ 目录（在联网准备机上执行一次）。
#
# 产物目录由 Gitea 任务容器的 /opt/android 只读挂载使用，内容：
#   sdk/           Android SDK（platform 36、build-tools、NDK 28.2、licenses）
#   jdk/           Temurin 17（必须能在 Ubuntu 18.04 / glibc 2.27 上运行）
#   gradle/        Gradle 8.13
#   gradle-home/   预热过的 ~/.gradle/caches，构建时复制到可写的 /ci-cache 复用
#   assets/        rootfs.tar.xz、packs/{common,browser}.tar.xz、proot/busybox 运行库与 NOTICE
#   keystore/      固定调试签名 debug.keystore（保证多次构建的 APK 可覆盖安装）
#   skia-binaries/ rust-skia 的 aarch64-android 预编译包（离线渲染链必需）
#
# 用法（x86_64 任务机）：
#   packaging/gitea/prepare-android.sh --out /path/to/kit/android \
#     --sdk "$HOME/android-sdk" --jdk /path/to/temurin-17-x64 \
#     --gradle "$HOME/gradle/gradle-8.13" --gradle-home "$HOME/.gradle" \
#     --rust-std /path/to/rust-std-1.92.0-aarch64-linux-android.tar.xz \
#     --offline-sdk /path/to/rust-builder/offline \
#     --skia /path/to/skia-binaries
#
# ARM64 任务机（NDK/build-tools 只有 x86_64 宿主版，需内置 qemu）：
#   把 --jdk 换成 aarch64 Temurin，并追加
#     --host-arch aarch64 --qemu /usr/bin/qemu-x86_64-static --x86-root /path/to/x86_64-root
#   x86_64-root 是最小 glibc 运行库根（lib64/ld-linux-x86-64.so.2 与 lib/x86_64-linux-gnu/*）。
#
# --rust-std 会把 Android std 安装进 --offline-sdk 的 1.92.0 工具链（套件 SDK 卷的源目录）。
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
NDK_VERSION="${WUNDER_NDK_VERSION:-28.2.13676358}"
OUT=""
SDK=""
JDK=""
GRADLE=""
GRADLE_HOME="${HOME}/.gradle"
RUST_STD=""
OFFLINE_SDK=""
SKIA=""
HOST_ARCH="$(uname -m)"
QEMU=""
X86_ROOT=""

fail() { echo "[prepare-android] $*" >&2; exit 2; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --out) OUT="$2"; shift 2 ;;
        --sdk) SDK="$2"; shift 2 ;;
        --jdk) JDK="$2"; shift 2 ;;
        --gradle) GRADLE="$2"; shift 2 ;;
        --gradle-home) GRADLE_HOME="$2"; shift 2 ;;
        --rust-std) RUST_STD="$2"; shift 2 ;;
        --offline-sdk) OFFLINE_SDK="$2"; shift 2 ;;
        --skia) SKIA="$2"; shift 2 ;;
        --host-arch) HOST_ARCH="$2"; shift 2 ;;
        --qemu) QEMU="$2"; shift 2 ;;
        --x86-root) X86_ROOT="$2"; shift 2 ;;
        -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
        *) fail "未知参数: $1" ;;
    esac
done

[[ -n "$OUT" && -n "$SDK" && -n "$JDK" && -n "$GRADLE" ]] || \
    fail "缺少必填参数（--out/--sdk/--jdk/--gradle），见 --help"
[[ -d "$SDK/platforms/android-36" ]] || fail "SDK 缺少 platforms/android-36"
[[ -d "$SDK/ndk/$NDK_VERSION" ]] || fail "SDK 缺少 ndk/$NDK_VERSION"
[[ -x "$JDK/bin/java" ]] || fail "JDK 路径无效: $JDK"
[[ -x "$GRADLE/bin/gradle" ]] || fail "Gradle 路径无效: $GRADLE"

mkdir -p "$OUT"/{sdk/platforms,sdk/build-tools,sdk/ndk,gradle,gradle-home,assets/packs,assets/jniLibs/arm64-v8a,keystore}
rsync -a "$SDK/platforms/android-36" "$OUT/sdk/platforms/"
for tools in "$SDK"/build-tools/*/; do
    [[ -d "$tools" ]] || continue
    rsync -a "$tools" "$OUT/sdk/build-tools/"
done
rsync -a "$SDK/ndk/$NDK_VERSION" "$OUT/sdk/ndk/"
[[ -d "$SDK/licenses" ]] && rsync -a "$SDK/licenses" "$OUT/sdk/"
[[ -d "$SDK/platform-tools" ]] && rsync -a "$SDK/platform-tools" "$OUT/sdk/"
rsync -a "$JDK/" "$OUT/jdk/"
rsync -a "$GRADLE/" "$OUT/gradle/"
[[ -d "$GRADLE_HOME/caches" ]] || fail "Gradle 缓存目录不存在: $GRADLE_HOME/caches"
rsync -a "$GRADLE_HOME/caches" "$OUT/gradle-home/"

app="$ROOT/crates/wunder-android/android/app/src/main"
for pack in common browser; do
    [[ -f "$app/assets/ubuntu/packs/$pack.tar.xz" ]] || \
        fail "缺少 android 资产 assets/ubuntu/packs/$pack.tar.xz（先构建对应补充包）"
done
rsync -a "$app/assets/ubuntu/rootfs.tar.xz" "$OUT/assets/"
rsync -a "$app/assets/ubuntu/NOTICE.txt" "$OUT/assets/"
rsync -a "$app/assets/ubuntu/packs/common.tar.xz" "$OUT/assets/packs/"
rsync -a "$app/assets/ubuntu/packs/browser.tar.xz" "$OUT/assets/packs/"
rsync -a "$app/jniLibs/arm64-v8a/liboperit_proot.so" \
         "$app/jniLibs/arm64-v8a/liboperit_loader.so" \
         "$app/jniLibs/arm64-v8a/libbusybox.so" "$OUT/assets/jniLibs/arm64-v8a/"
if [[ -n "$SKIA" ]]; then
    [[ -d "$SKIA" ]] || fail "Skia 预编译目录不存在: $SKIA"
    mkdir -p "$OUT/skia-binaries"
    rsync -a "$SKIA/" "$OUT/skia-binaries/"
fi

if [[ "$HOST_ARCH" == aarch64 || "$HOST_ARCH" == arm64 ]]; then
    [[ -n "$QEMU" && -n "$X86_ROOT" ]] || fail "aarch64 宿主需要 --qemu 与 --x86-root（NDK/build-tools 只有 x86_64 版）"
    [[ -x "$QEMU" ]] || fail "qemu-x86_64-static 不可执行: $QEMU"
    [[ -e "$X86_ROOT/lib64/ld-linux-x86-64.so.2" ]] || fail "x86_64 运行库根缺少 lib64/ld-linux-x86-64.so.2: $X86_ROOT"
    mkdir -p "$OUT/qemu"
    rsync -a "$QEMU" "$OUT/qemu/qemu-x86_64-static"
    chmod +x "$OUT/qemu/qemu-x86_64-static"
    rsync -a "$X86_ROOT/" "$OUT/x86_64-root/"
    bash "$ROOT/packaging/gitea/android-host-tools.sh" "$OUT" "$NDK_VERSION"
fi
if [[ -f "$HOME/.android/debug.keystore" ]]; then
    rsync -a "$HOME/.android/debug.keystore" "$OUT/keystore/"
else
    echo "[prepare-android] WARN: 没有 $HOME/.android/debug.keystore，CI 会生成随机调试签名，APK 将无法覆盖旧安装" >&2
fi

if [[ -n "$RUST_STD" ]]; then
    [[ -n "$OFFLINE_SDK" ]] || fail "--rust-std 需要同时给 --offline-sdk（套件 SDK 卷的源目录）"
    toolchain="$OFFLINE_SDK/rust/toolchains/1.92.0-x86_64-unknown-linux-gnu"
    [[ -d "$toolchain" ]] || fail "SDK 工具链不存在: $toolchain"
    staging="$(mktemp -d)"
    trap 'rm -rf "$staging"' EXIT
    tar -xJf "$RUST_STD" -C "$staging"
    std_dir="$staging/rust-std-1.92.0-aarch64-linux-android/lib/rustlib"
    [[ -d "$std_dir/aarch64-linux-android" ]] || fail "rust-std 归档结构不符合预期"
    cp -a "$std_dir/aarch64-linux-android" "$toolchain/lib/rustlib/"
    [[ -f "$std_dir/manifest-rust-std-aarch64-linux-android" ]] && \
        cp -a "$std_dir/manifest-rust-std-aarch64-linux-android" "$toolchain/lib/rustlib/"
else
    echo "[prepare-android] WARN: 未提供 --rust-std；请确认 SDK 的 1.92.0 工具链已含 aarch64-linux-android 标准库，" >&2
    echo "                否则下载 https://static.rust-lang.org/dist/rust-std-1.92.0-aarch64-linux-android.tar.xz 后重跑。" >&2
fi

required=(sdk/platforms/android-36 jdk/bin/java gradle/bin/gradle \
    assets/rootfs.tar.xz assets/packs/common.tar.xz assets/packs/browser.tar.xz keystore/debug.keystore \
    skia-binaries)
if [[ "$HOST_ARCH" == aarch64 || "$HOST_ARCH" == arm64 ]]; then
    required+=(bin/rust-linker bin/aapt2 qemu/qemu-x86_64-static x86_64-root/lib64/ld-linux-x86-64.so.2)
fi
for entry in "${required[@]}"; do
    [[ -e "$OUT/$entry" ]] || fail "产物缺少: $entry"
done
du -sh "$OUT"
echo "[prepare-android] done: $OUT"
