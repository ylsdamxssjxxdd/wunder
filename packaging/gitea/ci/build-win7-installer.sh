#!/usr/bin/env bash
set -euo pipefail

# 在带 Wine + Inno Setup 的任务镜像里组 Win7 安装包。桌面与 CLI 的 i386 PE 由 win7 构建任务
# 写进 /ci-cache 的交接目录，补充包取套件预置的 zip；这一步不编译 Rust，也不联网。
host_arch="$(uname -m)"
mkdir -p release-assets
find release-assets -maxdepth 1 -type f -delete
if [[ "$host_arch" != x86_64 ]]; then
    # Wine 跑 i386 Windows 程序依赖 x86 宿主；ARM worker 上明确跳过而不是失败。
    echo "SKIP: Win7 安装包需要 x86_64 worker（Wine + ISCC），当前宿主是 $host_arch。"
    exit 0
fi

# 同 build.sh：被更新的提交取代就不再占 runner 名额。
if ! python3 "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/is-latest.py"; then
    echo "SKIP: 本次运行已被更新的提交取代，不再组安装包。"
    exit 0
fi

cache_key="$(printf '%s' "${CI_REPOSITORY:?CI_REPOSITORY is required}" | sha256sum | cut -c 1-32)"
cache="/ci-cache/$cache_key/win7-x86"
inputs="$cache/installer-inputs"
export WUNDER_OFFLINE_ROOT="${WUNDER_OFFLINE_ROOT:-/opt/offline-sdk}"

desktop="$(find "$inputs" -maxdepth 1 -name 'wunder-frontend-slint-*-win7-x86.exe' -print -quit 2>/dev/null)"
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

python3 packaging/windows/scripts/build_win7_installer_linux.py \
    --desktop "$desktop" --cli "$cli" --supplement "$pack" \
    --output-dir release-assets --compression "${WUNDER_INNO_COMPRESSION:-lzma2/fast}"

python3 "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/verify.py" release-assets win7-installer
(cd release-assets; find . -maxdepth 1 -type f ! -name 'SHA256SUMS-*' -print0 | sort -z | xargs -0 sha256sum > "SHA256SUMS-win7-x86-setup.txt")
printf '\nWin7 installer ready (host %s)\n' "$host_arch"
ls -l release-assets
