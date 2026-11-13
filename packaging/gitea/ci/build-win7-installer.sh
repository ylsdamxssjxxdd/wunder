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

# 这里不再按「分支头是否推进」让路：编译任务十几分钟就产出并发布了 win7 exe，
# 组包只要两三分钟，跳过只会留下一个缺 setup.exe 的半套 release；按提交分家的交接
# 目录已经不担心新提交覆盖输入，所以只要本次提交有输入就补齐安装包。
cache_key="$(printf '%s' "${CI_REPOSITORY:?CI_REPOSITORY is required}" | sha256sum | cut -c 1-32)"
cache="/ci-cache/$cache_key/win7-x86"
inputs="$cache/installer-inputs/${CI_COMMIT:?CI_COMMIT is required}"
export WUNDER_OFFLINE_ROOT="${WUNDER_OFFLINE_ROOT:-/opt/offline-sdk}"

if [[ ! -d "$inputs" ]]; then
    # 编译任务开头就判出「已被更新的提交取代」时根本不会写交接目录，那一轮也没有 win7 exe 发布，
    # 安装包无事可做。这是正常让路，不是失败。
    echo "SKIP: 没有本次提交（${CI_COMMIT:0:12}）的 win7 构建输入，不组安装包。"
    exit 0
fi

desktop="$(find "$inputs" -maxdepth 1 -name 'wunder-desktop-*-win7-x86.exe' -print -quit 2>/dev/null)"
cli="$(find "$inputs" -maxdepth 1 -name 'wunder-cli-*win7-x86.exe' -print -quit 2>/dev/null)"
pack="$WUNDER_OFFLINE_ROOT/supplement-prebuilt/win7-ia32/wunder-supplement-win7-ia32-common.zip"
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
# 安装包已落地，本次提交的输入不再有用；删掉免得缓存卷被历史提交堆满。
rm -rf -- "$inputs"
