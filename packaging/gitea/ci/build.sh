#!/usr/bin/env bash
set -euo pipefail

# 在套件的任务容器里离线构建蛹（wunder）的 Linux 桌面/命令行与 Win7 32 位产物。
# 宿主可以是 x86_64 或 ARM64：交叉目标由 SDK 里的交叉工具链完成，不需要 QEMU。
ci_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

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
if [[ "$ARCH" == win7-x86 && "$host_arch" != x86_64 && "$host_arch" != aarch64 ]]; then
    # 同一条工作流要服务多种 worker：Gitea 表达式里没有 runner 上下文，架构门禁只能放在这里。
    # win7 是 Tier 3 目标，两种宿主都靠 SDK 内的 MinGW 便携包 + build-std 交叉，不需要 QEMU。
    echo "SKIP: win7-x86 需要 x86_64 或 aarch64 worker（SDK 内的 MinGW 便携包），当前 worker 是 $host_arch。"
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

# 分支头已经推进到别的提交（退出码 10）才让路；判据脚本自己出错一律照常构建，
# 否则一个环境问题会变成「整条流水线什么都不产出」，而且看起来像成功。
supersede=0
python3 "$ci_dir/is-latest.py" || supersede=$?
if [[ "$supersede" -eq 10 ]]; then
    echo "SKIP: 本次运行已被更新的提交取代，不再构建产物。"
    exit 0
elif [[ "$supersede" -ne 0 ]]; then
    echo "WARN: is-latest.py 以退出码 $supersede 结束，判不出新旧，照常构建。"
fi

export WUNDER_OUTPUT_DIR="$PWD/release-assets"
export WUNDER_CLI_OUTPUT_DIR="$PWD/release-assets"
export CARGO_BUILD_JOBS="${WUNDER_BUILD_JOBS:-4}"
chmod +x builders/linux_arm64_sysroot_cc.sh builders/linux-amd64-cross-cc.sh 2>/dev/null || true

if [[ "$ARCH" == win7-x86 ]]; then
    # Tier 3 目标：Desktop 与 CLI 都用 build-std 从 SDK 内的 rust-src 重建标准库。
    # 宿主架构决定用哪套 MinGW 便携包与 nightly：x86_64 走 kylin-x86 那套，aarch64 走 arm 便携包，
    # 两条通道的产物名、imports.txt 与 PE 门禁完全一致。
    if [[ "$host_arch" == x86_64 ]]; then
        win7_desktop=build-win7-x86-offline.sh
        win7_cli=build-cli-win7-x86-offline.sh
    else
        win7_desktop=build-win7-arm64-offline.sh
        win7_cli=build-cli-win7-arm64-offline.sh
    fi
    CARGO_HOME="$cache/desktop-home" CARGO_TARGET_DIR="$cache/desktop" \
        bash "builders/$win7_desktop"
    CARGO_HOME="$cache/cli-home" CARGO_TARGET_DIR="$cache/cli" \
        bash "builders/$win7_cli"
else
    CARGO_HOME="$cache/desktop-home" CARGO_TARGET_DIR="$cache/desktop" \
        bash "builders/build-linux-$ARCH-desktop.sh"
    CARGO_HOME="$cache/cli-home" CARGO_TARGET_DIR="$cache/cli" \
        bash "builders/build-cli-linux-$ARCH-offline.sh"
fi

# win7 构建会把 PE 导入门禁的审计转储写进输出目录；留档但不该进 Release 附件。
mv -f release-assets/imports.txt "$cache/imports-$ARCH.txt" 2>/dev/null || true

if [[ "$ARCH" != win7-x86 ]]; then
    # 与 GitHub 发布同形：Linux 每架构交桌面归档 + .portable + .run 三件，
    # 裸 CLI 与组包留下的校验件不进 Release（GitHub 的产物通配也收不到它们）。
    # 补充包是 .run 的必需输入，离线端只复用套件预置包，先复核 SHA256 再用。
    pack="$WUNDER_OFFLINE_ROOT/supplement-prebuilt/$ARCH/wunder-supplement-linux-$ARCH.tar.gz"
    [[ -s "$pack" && -s "$pack.sha256" ]] || {
        echo "套件预置补充包缺失: $pack（在准备机重产并重新导出套件）" >&2
        exit 2
    }
    ( cd "$(dirname "$pack")" && sha256sum --check --strict "$(basename "$pack").sha256" )
    desktop="$(find release-assets -maxdepth 1 -name "wunder-slint-*-linux-$ARCH.tar.gz" -print -quit)"
    cli="$(find release-assets -maxdepth 1 -name "wunder-cli-*-linux-$ARCH" -print -quit)"
    [[ -n "$desktop" && -n "$cli" ]] || { echo "桌面归档或 CLI 缺失: desktop=$desktop cli=$cli" >&2; exit 2; }
    bash builders/pack-portable.sh -a "linux-$ARCH" --desktop-archive "$desktop" --output-dir release-assets
    bash builders/pack.sh -a "linux-$ARCH" --desktop-archive "$desktop" --cli "$cli" \
        --supplement "$pack" --output-dir release-assets
    sample="$cache/wunder-cli-$ARCH.bin"
    mv -f -- "$cli" "$sample"
    # 桌面构建会同时留下裸 ELF 与 AppDir 归档；GitHub 只收归档，松散二进制一并转存缓存卷。
    find release-assets -maxdepth 1 -type f -name "wunder-frontend-slint-*-linux-$ARCH" \
        -exec mv -f -- {} "$cache/" \;
    # 组包脚本会就地留下 .sha256 与 .manifest.json，一并转存到缓存卷，避免混进附件。
    find release-assets -maxdepth 1 -type f \( -name '*.sha256' -o -name '*.manifest.json' \) \
        -exec mv -f -- {} "$cache/" \;
else
    sample="$cache/wunder-cli-win7-x86.exe"
    cli="$(find release-assets -maxdepth 1 -name 'wunder-cli-*-win7-x86.exe' -print -quit)"
    if [[ -n "$cli" ]]; then
        mv -f -- "$cli" "$sample"
    fi
    # 交接给安装包任务：Inno Setup 组包要在同一缓存卷里拿到两个 i386 PE（桌面与 CLI），
    # 那个任务跑在带 Wine 的独立镜像里，不共享本次任务的工作区。
    # 交接目录每次重建，只留本次运行的输入，避免上一轮的版本混进组包。
    rm -rf "$cache/installer-inputs"
    mkdir -p "$cache/installer-inputs"
    find release-assets -maxdepth 1 -name 'wunder-frontend-slint-*-win7-x86.exe' \
        -exec cp -f -- {} "$cache/installer-inputs/" \;
    if [[ -f "$sample" ]]; then
        cp -f -- "$sample" "$cache/installer-inputs/wunder-cli-win7-x86.exe"
    fi
fi

python3 "$ci_dir/verify.py" release-assets "$ARCH" "$sample"
(cd release-assets; find . -maxdepth 1 -type f ! -name 'SHA256SUMS-*' -print0 | sort -z | xargs -0 sha256sum > "SHA256SUMS-$ARCH.txt")
printf '\nArtifacts ready for %s (host %s)\n' "$ARCH" "$host_arch"
