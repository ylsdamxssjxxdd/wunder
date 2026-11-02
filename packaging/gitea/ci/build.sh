#!/usr/bin/env bash
set -euo pipefail

# 在套件的任务容器里离线构建蛹（wunder）的 Linux 桌面/命令行与 Win7 32 位产物。
# 宿主可以是 x86_64 或 ARM64：交叉目标由 SDK 里的交叉工具链完成，不需要 QEMU。
case "${ARCH:?ARCH is required}" in
    arm64|amd64|win7-x86) ;;
    *) echo 'Only arm64, amd64 and win7-x86 are supported.' >&2; exit 2 ;;
esac
host_arch="$(uname -m)"
case "$host_arch" in
    aarch64|arm64) host_toolchain=1.92.0-aarch64-unknown-linux-gnu ;;
    x86_64) host_toolchain=1.92.0-x86_64-unknown-linux-gnu ;;
    *) echo 'A x86_64 or ARM64 Linux build worker is required.' >&2; exit 2 ;;
esac
if [[ "$ARCH" == win7-x86 && "$host_arch" != x86_64 ]]; then
    # 同一条工作流要服务两种架构的 worker：Gitea 表达式里没有 runner 上下文，
    # 架构门禁只能放在这里。ARM64 worker 上 win7-x86 无法构建，跳过而不是失败。
    echo "SKIP: win7-x86 needs an x86_64 worker (kylin-x86 MinGW), this worker is $host_arch."
    mkdir -p release-assets
    find release-assets -maxdepth 1 -type f -delete
    exit 0
fi
export WUNDER_OFFLINE_ROOT="${WUNDER_OFFLINE_ROOT:-/opt/offline-sdk}"
[[ -d "$WUNDER_OFFLINE_ROOT/cargo-vendor-slint" \
    && -x "$WUNDER_OFFLINE_ROOT/rust/toolchains/$host_toolchain/bin/rustc" ]] || {
    echo 'The read-only Rust-builder SDK mount is missing or incomplete. Run prepare-sdk.sh before starting the runner.' >&2
    exit 2
}
cache_key="$(printf '%s' "${CI_REPOSITORY:?CI_REPOSITORY is required}" | sha256sum | cut -c 1-32)"
cache="/ci-cache/$cache_key/$ARCH"
mkdir -p "$cache/desktop" "$cache/cli" "$cache/desktop-home" "$cache/cli-home" release-assets
# 工作区按任务复用且不删未跟踪文件：不清掉上一轮的产物，旧版本会被当作本次产物上传，
# verify 的重名门禁也会误报。
find release-assets -maxdepth 1 -type f -delete
export WUNDER_OUTPUT_DIR="$PWD/release-assets"
export WUNDER_CLI_OUTPUT_DIR="$PWD/release-assets"
export CARGO_BUILD_JOBS="${WUNDER_BUILD_JOBS:-4}"
chmod +x builders/linux_arm64_sysroot_cc.sh builders/linux-amd64-cross-cc.sh 2>/dev/null || true

if [[ "$ARCH" == win7-x86 ]]; then
    # Tier 3 目标：Desktop 与 CLI 都用 build-std 从 SDK 内的 rust-src 重建标准库。
    CARGO_HOME="$cache/desktop-home" CARGO_TARGET_DIR="$cache/desktop" \
        bash builders/build-win7-x86-offline.sh
    CARGO_HOME="$cache/cli-home" CARGO_TARGET_DIR="$cache/cli" \
        bash builders/build-cli-win7-x86-offline.sh
else
    CARGO_HOME="$cache/desktop-home" CARGO_TARGET_DIR="$cache/desktop" \
        bash "builders/build-linux-$ARCH-desktop.sh"
    CARGO_HOME="$cache/cli-home" CARGO_TARGET_DIR="$cache/cli" \
        bash "builders/build-cli-linux-$ARCH-offline.sh"
fi

# win7 构建会把 PE 导入门禁的审计转储写进输出目录；留档但不该进 Release 附件。
mv -f release-assets/imports.txt "$cache/imports-$ARCH.txt" 2>/dev/null || true

python3 "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/verify.py" release-assets "$ARCH"
(cd release-assets; find . -maxdepth 1 -type f ! -name 'SHA256SUMS-*' -print0 | sort -z | xargs -0 sha256sum > "SHA256SUMS-$ARCH.txt")
printf '\nArtifacts ready for %s (host %s)\n' "$ARCH" "$host_arch"
