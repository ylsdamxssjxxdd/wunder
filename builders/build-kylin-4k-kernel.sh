#!/bin/bash
# 交叉编译麒麟 V10 同系（4.19.90）4K 页内核 rpm，供 64K 页鲲鹏服务器切换。
#
# 背景：64K 页 + 老内核宿主上 qemu-user 全版本无法加载 x86 guest（4K 对齐文件映射
# EFAULT/SIGBUS，见 docs/经验教训.md 2026-11-12），社区答案是换 4K 内核。
# 麒麟不公开内核源码，用 openEuler 20.03 LTS SP3 同系源码 + 麒麟 kernel-devel 自带
# .config 作基线，只改页大小与本地版本后缀，驱动配置保持麒麟原样。
#
# 用法：bash builders/build-kylin-4k-kernel.sh --sdk-root <kit-x86>/rust-builder/offline --out <dir>
# 产物：<out>/kernel-*.aarch64.rpm；服务器侧用 grubby 设默认后重启（旧内核保留兜底）。
set -euo pipefail

SDK_ROOT="" OUT=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --sdk-root) SDK_ROOT="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        *) echo "未知参数: $1" >&2; exit 2 ;;
    esac
done
[[ -n "$SDK_ROOT" && -n "$OUT" ]] || { sed -n '2,12p' "$0"; exit 2; }
HOST_TOOLS="$SDK_ROOT/host-tools-amd64/root"
[[ -x "$HOST_TOOLS/usr/bin/aarch64-linux-gnu-gcc-7" ]] || { echo "缺少 x86 宿主 aarch64 交叉 gcc: $HOST_TOOLS" >&2; exit 2; }
command -v aarch64-linux-gnu-ld >/dev/null || { echo '先装 binutils-aarch64-linux-gnu（SDK 自带 2.30 链不动 arm64 PIE）' >&2; exit 2; }

WORK="$(mktemp -d /tmp/kylin-4k-kernel.XXXXXXXX)"
trap 'rm -rf -- "$WORK"' EXIT
mkdir -p "$OUT"

OE_SRC="https://repo.openeuler.org/openEuler-20.03-LTS-SP3/source/Packages/kernel-4.19.90-2112.8.0.0131.oe1.src.rpm"
KY_DEVEL="https://update.cs2c.com.cn/NS/V10/V10SP3/os/adv/lic/base/aarch64/Packages/kernel-devel-4.19.90-52.22.v2207.ky10.aarch64.rpm"
for u in "$OE_SRC" "$KY_DEVEL"; do
    f="$WORK/$(basename "$u")"
    [[ -s "$f" ]] || curl -sL --retry 3 -o "$f" "$u"
done
(cd "$WORK" && rpm2cpio "$(basename "$OE_SRC")" | cpio -idm --quiet && tar -xzf kernel.tar.gz)
(cd "$WORK" && rpm2cpio "$(basename "$KY_DEVEL")" | cpio -idm --quiet)
KY_CONFIG="$(find "$WORK/usr/src/kernels" -name .config | head -1)"
[[ -n "$KY_CONFIG" ]] || { echo "kernel-devel 里没有 .config" >&2; exit 2; }

# 交叉环境只注入交叉工具包装器：GCC_EXEC_PREFIX/LD_LIBRARY_PATH 若全局导出，
# 会污染宿主 gcc/binutils（同 soname 不同版本的 libmpfr 等会串库）。
# gcc 用 SDK 的 7（与麒麟内核同代）；binutils 用系统新版（≥2.35）。
# 注意：4.19 的 __crc(MODVERSIONS)/__jump_table/__mcount_loc 重定位在 -shared
# 链接下 2.30/2.42 都报错（上游 PREL32 修复 5.x 才进），所以配置里必须关
# RELOCATABLE（连带 KASLR），按普通 ET_EXEC 链接；CI 构建机不需要 KASLR。
mkdir -p "$WORK/tc/bin"
for t in gcc ld as ar nm objcopy objdump strip ranlib; do
    if [[ "$t" == gcc ]]; then
        real="$HOST_TOOLS/usr/bin/aarch64-linux-gnu-gcc-7"
    else
        real="$(command -v "aarch64-linux-gnu-$t" || true)"
        [[ -n "$real" ]] || real="$(ls "$HOST_TOOLS/usr/bin/aarch64-linux-gnu-$t" "$HOST_TOOLS/usr/bin/aarch64-linux-gnu-$t-7" 2>/dev/null | head -1)"
    fi
    [[ -n "$real" ]] || continue
    {
        echo '#!/bin/bash'
        if [[ "$t" == gcc ]]; then
            echo "R=\"$HOST_TOOLS\""
            echo 'export LD_LIBRARY_PATH="$R/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"'
            echo 'export GCC_EXEC_PREFIX="$R/usr/lib/gcc-cross/"'
        fi
        echo "exec \"$real\" \"\$@\""
    } > "$WORK/tc/bin/aarch64-linux-gnu-$t"
    chmod +x "$WORK/tc/bin/aarch64-linux-gnu-$t"
done

export PATH="$WORK/tc/bin:$PATH" ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu-
cd "$WORK/kernel"
cp "$KY_CONFIG" .config
make olddefconfig
scripts/config --disable ARM64_64K_PAGES --disable ARM64_16K_PAGES --enable ARM64_4K_PAGES
scripts/config --set-str LOCALVERSION "-4k" --disable LOCALVERSION_AUTO
scripts/config --disable DEBUG_INFO --disable DEBUG_INFO_BTF
scripts/config --set-str SYSTEM_TRUSTED_KEYS ""
scripts/config --disable MODULE_SIG --disable MODULE_SIG_ALL
scripts/config --disable RELOCATABLE --disable RANDOMIZE_BASE
make olddefconfig
grep -E 'CONFIG_ARM64_(4K|16K|64K)_PAGES|CONFIG_LOCALVERSION=' .config
make binrpm-pkg -j"$(nproc)"
# binrpm-pkg 还会产 devel/headers 包，服务器只需要内核本体。
find "$HOME/rpmbuild/RPMS/aarch64" -maxdepth 1 -name 'kernel-4.19.90*.rpm' \
    ! -name '*-devel-*' ! -name '*-headers-*' -exec cp -f -- {} "$OUT/" \;
echo "4K kernel rpm ready: $OUT"