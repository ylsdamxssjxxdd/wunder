#!/bin/bash
# 为 4.19.90-4k 内核准备外部模块构建环境并重建 DKMS 模块（昇腾/流量监控等）。
# 用法：kernel-devel rpm 与本脚本同目录，root 运行；在重启进 4K 内核之前执行即可，
# 旧内核下就能把模块编好，编不过也不影响当前系统。
# 依赖服务器本机的 gcc/make 与 dkms（官方 kernel-devel 里的宿主工具是目标架构二进制，
# 本包不含，首次使用时在服务器上原生编译一次）。
set -euo pipefail
cd -- "$(dirname -- "$0")"
[[ $EUID -eq 0 ]] || { echo '需要 root 运行' >&2; exit 2; }

KDIR=/lib/modules/4.19.90-4k/build
if [[ ! -d "$KDIR" ]]; then
    RPM="$(ls kernel-devel-4.19.90*.rpm 2>/dev/null | head -1)"
    [[ -n "$RPM" ]] || { echo '同目录缺少 kernel-devel-4.19.90*.rpm' >&2; exit 2; }
    rpm -ivh --nodeps "$RPM"
fi
[[ -d "$KDIR" ]] || { echo "kernel-devel 未安装：$KDIR 不存在" >&2; exit 2; }
command -v gcc >/dev/null && command -v make >/dev/null && command -v dkms >/dev/null || {
    echo '缺少 gcc/make/dkms，先用系统包管理器装齐再跑本脚本' >&2; exit 2; }

echo '== 编译内核宿主工具（本机原生架构）'
make -C "$KDIR" -k scripts -j"$(nproc)" || echo 'WARN: 个别目标失败（-k 已继续），只要求下面三个工具存在'
for t in basic/fixdep mod/modpost recordmcount; do
    [[ -x "$KDIR/scripts/$t" ]] || { echo "缺少宿主工具 scripts/$t，中止" >&2; exit 2; }
done

echo '== DKMS 重建 4.19.90-4k 模块'
grep -q '^4.19.90-4k$' <(ls /lib/modules) || { echo '/lib/modules 下没有 4.19.90-4k，先装 4K 内核 rpm' >&2; exit 2; }
dkms autoinstall -k 4.19.90-4k

echo '== 结果'
dkms status
echo '以上模块 built/installed 指向 4.19.90-4k 即可 reboot 进新内核。'
