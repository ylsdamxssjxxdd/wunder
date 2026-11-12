#!/bin/bash
# 安装 4K 页内核 rpm 并设为默认启动项（旧 64K 内核保留在 grub 菜单兜底）。
# 用法：把 build-kylin-4k-kernel.sh 产出的 rpm 与本脚本放同一目录，root 运行；
# 脚本不自动重启，确认默认内核后手动 reboot。
# 重启后：getconf PAGESIZE 应为 4096；binfmt 注册不跨重启，重跑 ./load-images.sh
# （或 binfmt/install-service.sh 的开机单元）再触发工作流。
set -euo pipefail
cd -- "$(dirname -- "$0")"
[[ $EUID -eq 0 ]] || { echo '需要 root 运行' >&2; exit 2; }
RPM="$(ls kernel-4.19.90*.aarch64.rpm 2>/dev/null | grep -v -e -devel -e -headers | head -1)"
[[ -n "$RPM" ]] || { echo '同目录缺少 kernel-4.19.90*.aarch64.rpm（build-kylin-4k-kernel.sh 产物）' >&2; exit 2; }
rpm -ivh --nodeps --force "$RPM"
NEW="$(ls -t /boot/vmlinuz-*4k* 2>/dev/null | head -1)"
[[ -n "$NEW" ]] || { echo '/boot 下未找到 4k 内核' >&2; exit 2; }
command -v grubby >/dev/null || { echo "缺少 grubby，请手动把默认启动项设为 $NEW" >&2; exit 2; }
grubby --set-default "$NEW"
echo "默认内核: $(grubby --default-kernel)"
echo '确认无误后手动 reboot；重启后 getconf PAGESIZE 应为 4096，再重跑 ./load-images.sh。'