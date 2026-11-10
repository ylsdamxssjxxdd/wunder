#!/usr/bin/env bash
set -euo pipefail

# 在带 Wine + Inno Setup 的任务镜像里组 Win7 安装包。桌面与 CLI 的 i386 PE 由 win7 构建任务
# 写进 /ci-cache 的交接目录，补充包取套件预置的 zip；这一步不编译 Rust，也不联网。
host_arch="$(uname -m)"
mkdir -p release-assets
find release-assets -maxdepth 1 -type f -delete
if [[ "$host_arch" != x86_64 ]]; then
    # 这里只防错配：ARM 套件会把本任务指到 amd64 的 Wine 镜像，容器里 uname 仍是 x86_64；
    # 若仍是原生 aarch64 镜像，说明 runner.yaml 的 installer 标签没换成 Wine 镜像。
    echo "SKIP: Win7 安装包需要 x86_64 任务镜像（Wine + ISCC），当前容器报告 $host_arch。" >&2
    echo "      ARM 套件应把 wunder-offline-installer 标签指向 amd64 的 Wine 镜像并注册 binfmt。" >&2
    exit 0
fi
if ! grep -q '^vendor_id' /proc/cpuinfo 2>/dev/null; then
    # uname 是模拟架构（x86_64），/proc/cpuinfo 仍是 ARM：说明在 qemu 下跑。
    echo "NOTE: 检测到 qemu 模拟运行，ISCC 压缩会比原生慢很多；超时可设 WUNDER_INNO_COMPRESSION=zip。"
fi

# 同 build.sh：只有明确判出「已被更新的提交取代」（退出码 10）才让路，脚本自身出错照常组包。
supersede=0
python3 "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/is-latest.py" || supersede=$?
if [[ "$supersede" -eq 10 ]]; then
    echo "SKIP: 本次运行已被更新的提交取代，不再组安装包。"
    exit 0
elif [[ "$supersede" -ne 0 ]]; then
    echo "WARN: is-latest.py 以退出码 $supersede 结束，判不出新旧，照常组包。"
fi

cache_key="$(printf '%s' "${CI_REPOSITORY:?CI_REPOSITORY is required}" | sha256sum | cut -c 1-32)"
cache="/ci-cache/$cache_key/win7-x86"
inputs="$cache/installer-inputs"
export WUNDER_OFFLINE_ROOT="${WUNDER_OFFLINE_ROOT:-/opt/offline-sdk}"

desktop="$(find "$inputs" -maxdepth 1 -name 'wunder-desktop-*-win7-x86.exe' -print -quit 2>/dev/null)"
cli="$(find "$inputs" -maxdepth 1 -name 'wunder-cli-*win7-x86.exe' -print -quit 2>/dev/null)"
pack="$WUNDER_OFFLINE_ROOT/supplement-prebuilt/win7-ia32/wunder-supplement-win7-ia32-common.zip"
[[ -d "$inputs" ]] || { echo "交接目录不存在: $inputs（win7 构建任务未产出或缓存卷被清理）" >&2; exit 2; }
[[ -n "$desktop" ]] || { echo "交接目录缺少桌面 exe: $inputs" >&2; exit 2; }
[[ -n "$cli" ]] || { echo "交接目录缺少 CLI exe: $inputs" >&2; exit 2; }
[[ -s "$pack" && -s "$pack.sha256" ]] || {
    echo "套件预置的 win7 补充包缺失: $pack" >&2
    exit 2
}
( cd "$(dirname "$pack")" && sha256sum --check --strict "$(basename "$pack").sha256" )

command -v wine >/dev/null || { echo "任务镜像里没有 wine" >&2; exit 2; }
[[ -n "${WUNDER_ISCC:-}" ]] || export WUNDER_ISCC='C:\InnoSetup\ISCC.exe'

# -u: 解压补充包与 ISCC 压缩都可能持续很久，无缓冲才能让 CI 实时看到阶段日志。
python3 -u packaging/windows/scripts/build_win7_installer_linux.py \
    --desktop "$desktop" --cli "$cli" --supplement "$pack" \
    --output-dir release-assets --compression "${WUNDER_INNO_COMPRESSION:-lzma2/normal}"

python3 "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/verify.py" release-assets win7-installer
(cd release-assets; find . -maxdepth 1 -type f ! -name 'SHA256SUMS-*' -print0 | sort -z | xargs -0 sha256sum > "SHA256SUMS-win7-x86-setup.txt")
printf '\nWin7 installer ready (host %s)\n' "$host_arch"
ls -l release-assets
