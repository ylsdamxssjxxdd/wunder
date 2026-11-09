#!/usr/bin/env bash
# 把 binfmt 注册做成开机自启：binfmt_misc 的注册不跨重启，Docker 里的 amd64 安装包镜像
# 在重启后会失去解释器。本脚本写一个 oneshot systemd 单元，开机重新注册当前目录的解释器。
set -euo pipefail
cd -- "$(dirname -- "$0")"

die() { echo "[binfmt] $*" >&2; exit 2; }
[[ $EUID -eq 0 ]] || die '需要 root 运行：sudo bash binfmt/install-service.sh'
command -v systemctl >/dev/null || die '没有 systemd，开机后手动执行 binfmt/register.sh'

unit=/etc/systemd/system/wunder-binfmt.service
cat > "$unit" <<EOF
[Unit]
Description=wunder offline CI: register qemu x86_64/i386 binfmt
After=local-fs.target

[Service]
Type=oneshot
ExecStart=/bin/bash $PWD/register.sh
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable --now wunder-binfmt.service
echo "[binfmt] 已启用 $unit（套件目录移动后要重跑本脚本）"
