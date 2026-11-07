#!/usr/bin/env bash
set -euo pipefail

# 在套件的任务容器里离线构建安卓 arm64 APK。
# 工具链与 APK 内嵌资产来自套件挂载的 /opt/android（SDK/NDK/JDK/Gradle/rootfs/补充包），
# 依赖从只读 SDK 的 vendored crate 源解析；全程不访问网络。
ci_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$ci_dir/../../.." && pwd -P)"
android_root="${WUNDER_ANDROID_ROOT:-/opt/android}"
offline_root="${WUNDER_OFFLINE_ROOT:-/opt/offline-sdk}"

host_arch="$(uname -m)"
case "$host_arch" in
    x86_64|aarch64|arm64) ;;
    *)
        echo "SKIP: 安卓 arm64 需要 x86_64 或 aarch64 worker，当前 worker 是 $host_arch。"
        mkdir -p release-assets
        find release-assets -maxdepth 1 -type f -delete
        exit 0
        ;;
esac

for path in "$android_root/sdk" "$android_root/jdk" "$android_root/gradle" \
    "$android_root/skia-binaries" "$android_root/assets/rootfs.tar.xz" \
    "$android_root/assets/packs/common.tar.xz" "$offline_root/cargo-vendor-slint"; do
    [[ -e "$path" ]] || { echo "离线安卓输入缺失: $path（先按 README 准备 kit 的 android/ 目录）" >&2; exit 2; }
done

host_toolchain=1.92.0-aarch64-unknown-linux-gnu
[[ "$host_arch" == x86_64 ]] && host_toolchain=1.92.0-x86_64-unknown-linux-gnu
[[ -d "$offline_root/rust/toolchains/$host_toolchain/lib/rustlib/aarch64-linux-android/lib" ]] || {
    echo "SDK 的 $host_toolchain 缺少 aarch64-linux-android 标准库（准备机先按 README 安装 rust-std）" >&2
    exit 2
}

if [[ "$host_arch" == aarch64 || "$host_arch" == arm64 ]]; then
    # NDK/build-tools 只有 x86_64 宿主版：卷里的 qemu 包装器负责模拟。
    for path in "$android_root/bin/rust-linker" "$android_root/bin/aapt2" \
        "$android_root/qemu/qemu-x86_64-static" "$android_root/x86_64-root/lib64/ld-linux-x86-64.so.2"; do
        [[ -e "$path" ]] || { echo "ARM 安卓模拟输入缺失: $path" >&2; exit 2; }
    done
    export WUNDER_ANDROID_BIN="$android_root/bin"
    export PATH="$WUNDER_ANDROID_BIN:$PATH"
fi

cache_key="$(printf '%s' "${CI_REPOSITORY:?CI_REPOSITORY is required}" | sha256sum | cut -c 1-32)"
cache="/ci-cache/$cache_key/android"
mkdir -p "$cache" release-assets
find release-assets -maxdepth 1 -type f -delete

# 分支头已推进到别的提交（退出码 10）才让路；判据脚本自身出错时照常构建。
supersede=0
python3 "$ci_dir/is-latest.py" || supersede=$?
if [[ "$supersede" -eq 10 ]]; then
    echo "SKIP: 本次运行已被更新的提交取代，不再构建产物。"
    exit 0
elif [[ "$supersede" -ne 0 ]]; then
    echo "WARN: is-latest.py 以退出码 $supersede 结束，判不出新旧，照常构建。"
fi

# Gradle 用户目录必须可写：首次运行从套件的只读种子复制一次，之后复用。
if [[ ! -f "$cache/gradle-home/.seeded" ]]; then
    rm -rf "$cache/gradle-home"
    mkdir -p "$cache/gradle-home"
    cp -a "$android_root/gradle-home/caches" "$cache/gradle-home/" 2>/dev/null || true
    touch "$cache/gradle-home/.seeded"
fi

# 固定调试签名：沿用套件里与本地构建同一份 keystore，保证多次 CI 出的 APK 可覆盖安装。
mkdir -p "$HOME/.android"
if [[ -f "$android_root/keystore/debug.keystore" ]]; then
    cp -f "$android_root/keystore/debug.keystore" "$HOME/.android/debug.keystore"
fi

# 把 rootfs、离线工具补充包与 proot 运行库放进应用源码树，构建脚本会把它们打进 APK。
app_src="$repo_root/crates/wunder-android/android/app/src/main"
mkdir -p "$app_src/assets/ubuntu/packs" "$app_src/jniLibs/arm64-v8a"
cp -f "$android_root/assets/rootfs.tar.xz" "$app_src/assets/ubuntu/"
cp -f "$android_root/assets/NOTICE.txt" "$app_src/assets/ubuntu/"
cp -f "$android_root/assets/packs/common.tar.xz" "$app_src/assets/ubuntu/packs/"
cp -f "$android_root/assets/jniLibs/arm64-v8a/"*.so "$app_src/jniLibs/arm64-v8a/"

export ANDROID_HOME="$android_root/sdk"
export ANDROID_NDK_ROOT="$ANDROID_HOME/ndk/28.2.13676358"
export JAVA_HOME="$android_root/jdk"
export GRADLE="$android_root/gradle/bin/gradle"
export GRADLE_USER_HOME="$cache/gradle-home"
if [[ "$host_arch" == x86_64 ]]; then
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"
else
    # NDK bin 里是 x86_64 的 ld；放进 ARM 的 PATH 会让宿主 gcc 的 collect2 找错链接器。
    export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH"
fi
export CARGO_HOME="$cache/cargo-home"
export CARGO_TARGET_DIR="$cache/target"
export CARGO_NET_OFFLINE=true
# Slint 的安卓渲染链依赖 rust-skia 的预编译包；离线端只能用套件里的副本。
export SKIA_BINARIES_URL="file://$android_root/skia-binaries/{tag}/skia-binaries-{key}.tar.gz"
export WUNDER_GRADLE_ARGS="--offline"
export WUNDER_ANDROID_ABIS=arm64-v8a

mkdir -p "$CARGO_HOME"
cat > "$CARGO_HOME/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$offline_root/cargo-vendor-slint"
[net]
offline = true
EOF

bash "$repo_root/builders/build-android-arm64.sh"

apk="$(find "$repo_root/target/android/dist" -type f -name 'wunder-android-*-arm64.apk' -print -quit)"
[[ -n "$apk" ]] || { echo "APK 未产出" >&2; exit 2; }
cp -f "$apk" release-assets/

python3 "$ci_dir/verify.py" release-assets android-arm64
(cd release-assets; find . -maxdepth 1 -type f ! -name 'SHA256SUMS-*' -print0 | sort -z | xargs -0 sha256sum > "SHA256SUMS-android-arm64.txt")
printf '\nArtifacts ready for android-arm64 (host %s)\n' "$host_arch"
