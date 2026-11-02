#!/usr/bin/env bash
set -euo pipefail

# 复用套件里预制的补充包：离线端不构建 opt/python、opt/git、opt/rg、opt/ffmpeg、opt/browser，
# 只做完整性校验、落到发布目录并交给 publish.py。
# 补充包在联网准备机上用 cache/offline_build.sh <arch> 产出，随套件导出（见 prepare-bundle.py）。
case "${ARCH:?ARCH is required}" in
    arm64|amd64) ;;
    *) echo 'Only arm64 and amd64 supplements are supported.' >&2; exit 2 ;;
esac
sdk="${WUNDER_OFFLINE_ROOT:-/opt/offline-sdk}"
prebuilt="$sdk/supplement-prebuilt/$ARCH"
pack_name="wunder-supplement-linux-$ARCH.tar.gz"
[[ -s "$prebuilt/$pack_name" && -s "$prebuilt/$pack_name.sha256" ]] || {
    echo "预制补充包缺失：$prebuilt/$pack_name（或它的 .sha256）" >&2
    echo '在联网准备机上构建补充包后用 prepare-bundle.py --supplement-prebuilt 重新导出套件。' >&2
    exit 2
}
mkdir -p supplement-dist
cp -f "$prebuilt/$pack_name" "supplement-dist/$pack_name"
(cd "$prebuilt" && sha256sum --check --strict "$pack_name.sha256")
tar -tzf "supplement-dist/$pack_name" > /tmp/supplement-members.txt
for component in opt/python/bin opt/python/lib/python3.8/site-packages \
    opt/git/bin/git opt/rg/bin/rg opt/ffmpeg opt/browser; do
    grep -q "^$component" /tmp/supplement-members.txt \
        || { echo "预制补充包内缺少 $component" >&2; exit 2; }
done
cp -f "$prebuilt/$pack_name.sha256" "supplement-dist/$pack_name.sha256"
printf '\nSupplement reused from the bundled prebuilt pack: %s (%s bytes)\n' \
    "$pack_name" "$(stat -c '%s' "supplement-dist/$pack_name")"
