#!/usr/bin/env bash
# 在银河麒麟高级服务器 V10（x86_64 / aarch64）上把 Docker Engine 升级为官方静态二进制包。
#
# 用法：
#   sudo bash upgrade-docker.sh --package /root/docker-24.0.9.tgz          # 升级引擎（自动带同目录的 compose 插件）
#   sudo bash upgrade-docker.sh --package /root/docker-24.0.9.tgz --check  # 只体检，不动系统
#   sudo bash upgrade-docker.sh --package /root/docker-24.0.9.tgz --plugins /root/docker-cli-plugins-x86_64.tgz
#   sudo bash upgrade-docker.sh --rollback /var/backups/docker-upgrade-20261028-1930
#
# 设计前提：
#   * 离线环境，所有依赖必须已在本地（脚本不会联网）。
#   * 只替换 /usr/bin 下的二进制与 systemd 单元，保留 /etc/docker/daemon.json 和 /var/lib/docker。
#   * 失败自动回滚到升级前的二进制与单元文件。
set -euo pipefail

DEFAULT_VERSION=24.0.9
BACKUP_ROOT=/var/backups
PLUGIN_DIR=/usr/local/lib/docker/cli-plugins
TS="$(date +%Y%m%d-%H%M%S)"
PKG=""
PLUGINS_PKG=""
SKIP_PLUGINS=0
MODE=upgrade
CHECK_ONLY=0
ASSUME_YES=0
ROLLBACK_DIR=""

log()  { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
warn() { printf '[warn] %s\n' "$*" >&2; }
die()  { printf '[fail] %s\n' "$*" >&2; exit 1; }

usage() {
    sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --package)  PKG="$2"; shift 2 ;;
        --plugins)  PLUGINS_PKG="$2"; shift 2 ;;        --check)    CHECK_ONLY=1; shift ;;
        --yes|-y)   ASSUME_YES=1; shift ;;
        --rollback) MODE=rollback; ROLLBACK_DIR="$2"; shift 2 ;;
        --version)  DEFAULT_VERSION="$2"; shift 2 ;;
        -h|--help)  usage 0 ;;
        *) die "未知参数：$1（--help 查看用法）" ;;
    esac
done

[[ $EUID -eq 0 ]] || die '需要 root 权限运行（sudo bash upgrade-docker.sh ...）'

# ---------------------------------------------------------------- 回滚模式
if [[ $MODE == rollback ]]; then
    [[ -d "$ROLLBACK_DIR" ]] || die "备份目录不存在：$ROLLBACK_DIR"
    [[ -x "$ROLLBACK_DIR/bin/dockerd" ]] || die "备份里没有 dockerd：$ROLLBACK_DIR/bin"
    log "回滚到 $ROLLBACK_DIR"
    systemctl stop docker docker.socket 2>/dev/null || true
    systemctl stop containerd 2>/dev/null || true
    install -m 0755 "$ROLLBACK_DIR"/bin/* /usr/bin/
    for unit in docker.service containerd.service docker.socket; do
        if [[ -f "$ROLLBACK_DIR/units/$unit" ]]; then
            cp -f "$ROLLBACK_DIR/units/$unit" "/etc/systemd/system/$unit"
        else
            rm -f "/etc/systemd/system/$unit"
        fi
    done
    [[ -f "$ROLLBACK_DIR/daemon.json" ]] && cp -f "$ROLLBACK_DIR/daemon.json" /etc/docker/daemon.json
    # 插件：先还原被覆盖的，再删掉本次新建的
    if [[ -d "$ROLLBACK_DIR/plugins" ]]; then
        mkdir -p "$PLUGIN_DIR"
        cp -af "$ROLLBACK_DIR/plugins/." "$PLUGIN_DIR/" 2>/dev/null || true
    fi
    if [[ -f "$ROLLBACK_DIR/plugins-created.txt" ]]; then
        while IFS= read -r name; do
            [[ -n "$name" ]] && rm -f "$PLUGIN_DIR/$name"
        done < "$ROLLBACK_DIR/plugins-created.txt"
    fi
    systemctl daemon-reload
    [[ -f /etc/systemd/system/docker.socket ]] && systemctl enable docker.socket >/dev/null 2>&1
    systemctl start containerd 2>/dev/null || true
    systemctl start docker 2>/dev/null || true
    sleep 3
    docker version --format '回滚完成：server {{.Server.Version}} / API {{.Server.APIVersion}}' || warn 'Docker 未能启动，请查看 journalctl -u docker -n 80'
    exit 0
fi

# ---------------------------------------------------------------- 架构与包
arch="$(uname -m)"
case "$arch" in
    x86_64|amd64)  DOCKER_ARCH=x86_64 ;;
    aarch64|arm64) DOCKER_ARCH=aarch64 ;;
    *) die "不支持的架构：$arch" ;;
esac

if [[ -z "$PKG" ]]; then
    for candidate in "./docker-${DEFAULT_VERSION}-${DOCKER_ARCH}.tgz" "./docker-${DEFAULT_VERSION}.tgz" \
                     "./docker-static/docker-${DEFAULT_VERSION}.tgz" "/root/docker-${DEFAULT_VERSION}-${DOCKER_ARCH}.tgz" \
                     "/root/docker-${DEFAULT_VERSION}.tgz" "/opt/docker-${DEFAULT_VERSION}.tgz"; do
        [[ -f "$candidate" ]] && { PKG="$candidate"; break; }
    done
fi
[[ -n "$PKG" && -f "$PKG" ]] || die "找不到静态包。请指定 --package /path/docker-${DEFAULT_VERSION}.tgz（架构 $DOCKER_ARCH）"
PKG="$(readlink -f "$PKG")"

if [[ "$PLUGINS_PKG" == none ]]; then
    SKIP_PLUGINS=1
    PLUGINS_PKG=""
fi

# 插件包（docker compose / buildx）不指定时，自动找引擎包同目录下的同名架构包。
if [[ -z "$PLUGINS_PKG" && $SKIP_PLUGINS -eq 0 ]]; then
    for candidate in "$(dirname -- "$PKG")/docker-cli-plugins-${DOCKER_ARCH}.tgz" \
                     "./docker-cli-plugins-${DOCKER_ARCH}.tgz"; do
        [[ -f "$candidate" ]] && { PLUGINS_PKG="$candidate"; break; }
    done
fi
if [[ -n "$PLUGINS_PKG" ]]; then
    [[ -f "$PLUGINS_PKG" ]] || die "插件包不存在：$PLUGINS_PKG"
    PLUGINS_PKG="$(readlink -f "$PLUGINS_PKG")"
fi

log "系统：$(. /etc/os-release 2>/dev/null && echo "${PRETTY_NAME:-unknown}") / 内核 $(uname -r) / $arch"
log "静态包：$PKG"

# ---------------------------------------------------------------- 体检
glibc="$(ldd --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+$' || true)"
if [[ -n "$glibc" ]]; then
    glibc_num="$(awk -F. '{ printf "%d%03d", $1, $2 }' <<< "$glibc")"
    log "glibc：$glibc"
    (( glibc_num >= 2017 )) || die "glibc $glibc 过低，静态包至少需要 2.17"
    (( glibc_num >= 2028 )) || warn "glibc 低于 2.28，Docker 24 未在官方支持矩阵内，建议先小范围验证"
else
    warn '无法识别 glibc 版本'
fi

kernel_num="$(uname -r | awk -F. '{ printf "%d%03d", $1, $2 }')"
(( kernel_num >= 3010 )) || die "内核 $(uname -r) 过低，Docker 24 需要 3.10+"
log "内核满足要求（4.19.x 是麒麟 V10 的常见版本，长期验证可用）"

# 24.0.9 的静态二进制是完全静态链接的，libseccomp 不再是硬依赖；解包后会实测一次。

command -v iptables >/dev/null || warn '未找到 iptables，端口映射会失败'
iptables -V 2>/dev/null | sed 's/^/      /' || true
[[ -e /proc/sys/net/bridge/bridge-nf-call-iptables ]] || warn 'br_netfilter 未加载（脚本会自动加载）'

if command -v getenforce >/dev/null && [[ "$(getenforce)" == Enforcing ]]; then
    warn 'SELinux 处于 Enforcing，静态安装的 Docker 可能因缺少 container-selinux 策略而无法启动容器'
    warn '可临时设为宽容模式验证：setenforce 0（写入 /etc/selinux/config 才会持久）'
fi
systemctl is-active --quiet firewalld 2>/dev/null && \
    warn 'firewalld 正在运行：它 reload 时会清掉 Docker 的 iptables 规则，建议把 docker0 加入 trusted 区'

if [[ -f /etc/docker/daemon.json ]]; then
    log '现有 /etc/docker/daemon.json：'
    sed 's/^/      /' /etc/docker/daemon.json | head -20
    if grep -qE '"runtimes"|nvidia|cgroupfs.*systemd|"exec-opts"' /etc/docker/daemon.json; then
        warn 'daemon.json 里包含 exec-opts/runtimes，升级后可能不兼容，请先人工确认'
    fi
fi

cur_ver="$(docker version --format '{{.Server.Version}}' 2>/dev/null || echo '未运行')"
cur_api="$(docker version --format '{{.Server.APIVersion}}' 2>/dev/null || echo '-')"
cur_driver="$(docker info --format '{{.Driver}}' 2>/dev/null || true)"
log "当前 Docker：$cur_ver / API $cur_api / 存储驱动 ${cur_driver:-未运行}"

case "$cur_driver" in
    overlay2|'') : ;;
    *) warn "当前存储驱动是 $cur_driver，换成 24.0 后可能无法读取旧镜像层；建议先迁移到 overlay2" ;;
esac

running="$(docker ps -q 2>/dev/null | wc -l | tr -d ' ' || true)"
containers="$(docker ps -aq 2>/dev/null | wc -l | tr -d ' ' || true)"
log "现有容器：${containers:-0} 个（${running:-0} 个运行中）"

free_mb="$(df -Pm /usr/bin | awk 'NR==2{print $4}')"
(( free_mb > 100 )) || die "/usr/bin 所在分区剩余空间不足（${free_mb}MB）"

# overlay2 在 xfs 上要求 ftype=1，麒麟默认 ext4 无此问题，但定制分区可能是 xfs。
if [[ -d /var/lib/docker ]] && command -v xfs_info >/dev/null; then
    fs_type="$(df -PT /var/lib/docker | awk 'NR==2{print $2}')"
    if [[ "$fs_type" == xfs ]]; then
        xfs_info /var/lib/docker 2>/dev/null | grep -q 'ftype=1' || \
            warn '/var/lib/docker 位于 xfs 且未启用 ftype=1，overlay2 无法使用；需改用 ext4 或重建分区'
    fi
fi

# 解包检查
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
tar -xzf "$PKG" -C "$tmp" || die "解包失败：$PKG"
BIN_SRC="$tmp/docker"
[[ -x "$BIN_SRC/dockerd" ]] || die '包结构异常：没有 docker/dockerd（确认下的是 static/stable 而不是 rpms）'

# 实测依赖：24.0.9 官方静态包是全静态链接，因此在最小化系统上也不需要 libseccomp。
ldd_out="$(ldd "$BIN_SRC/dockerd" 2>&1 || true)"
if [[ "$ldd_out" == *"not a dynamic executable"* || "$ldd_out" == *"statically linked"* ]]; then
    log '二进制自检：dockerd 为静态链接，不依赖 libseccomp/glibc'
else
    log '二进制自检：dockerd 为动态链接'
    ldconfig -p 2>/dev/null | grep -q 'libseccomp\.so\.2' || \
        warn '缺少 libseccomp.so.2，dockerd 可能无法启动（仅老版本静态包需要）'
fi

# 校验包内二进制的架构，避免 x86 包被装到 ARM 机器（或反过来）导致满屏 exec format error。
if command -v od >/dev/null; then
    machine="$(od -An -j18 -N2 -tx1 "$BIN_SRC/dockerd" 2>/dev/null | tr -d ' \n')"
    case "$machine" in
        3e00) pkg_arch=x86_64 ;;
        b700) pkg_arch=aarch64 ;;
        *)    pkg_arch=unknown ;;
    esac
    log "包内架构：$pkg_arch"
    [[ "$pkg_arch" == "$DOCKER_ARCH" ]] || die "包架构是 $pkg_arch，本机是 $DOCKER_ARCH，请换对应的包"
else
    warn '缺少 od，跳过包架构校验'
fi

pkg_ver="$("$BIN_SRC/docker" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
log "包内版本：${pkg_ver:-未知}（$DOCKER_ARCH）"
[[ "$pkg_ver" == "$DEFAULT_VERSION" ]] || warn "包版本与预期 $DEFAULT_VERSION 不一致，继续使用包内版本"
if [[ -n "$pkg_ver" ]]; then
    pv="$(awk -F. '{ printf "%d%03d", $1, $2 }' <<< "$pkg_ver")"
    (( pv >= 1903 )) || die "包版本 $pkg_ver 低于 19.03，无法满足 Runner 5.x 的 API 1.40 要求"
fi

# ---------------------------------------------------------------- 插件包（compose / buildx）
PLUGIN_SRC=""
if [[ -n "$PLUGINS_PKG" ]]; then
    mkdir -p "$tmp/pluginsrc"
    tar -xzf "$PLUGINS_PKG" -C "$tmp/pluginsrc" || die "插件包解包失败：$PLUGINS_PKG"
    PLUGIN_SRC="$tmp/pluginsrc/plugins"
    [[ -x "$PLUGIN_SRC/docker-compose" ]] || die '插件包结构异常：没有 plugins/docker-compose'
    if command -v od >/dev/null; then
        plugin_machine="$(od -An -j18 -N2 -tx1 "$PLUGIN_SRC/docker-compose" 2>/dev/null | tr -d ' \n')"
        case "$plugin_machine" in
            3e00) plugin_arch=x86_64 ;;
            b700) plugin_arch=aarch64 ;;
            *)    plugin_arch=unknown ;;
        esac
        [[ "$plugin_arch" == "$DOCKER_ARCH" ]] || \
            die "插件包架构是 $plugin_arch，本机是 $DOCKER_ARCH，请换对应的包"
    fi
    compose_ver="$("$PLUGIN_SRC/docker-compose" version --short 2>/dev/null || echo '版本未知')"
    log "插件包：docker compose $compose_ver"
else
    log '插件包：未提供，docker compose 保持原样'
fi

if [[ $CHECK_ONLY -eq 1 ]]; then
    log '体检通过（--check 模式，未做任何修改）'
    exit 0
fi
# ---------------------------------------------------------------- 确认
plugin_note=""
if [[ -n "$PLUGIN_SRC" ]]; then
    plugin_note="
  6. 安装 CLI 插件到 $PLUGIN_DIR（docker compose / buildx，原文件先备份）"
fi
cat <<EOF

即将执行：
  1. 备份二进制与 systemd 单元到 $BACKUP_ROOT/docker-upgrade-$TS
  2. 停止 docker / containerd（$running 个运行中的容器会停机）
  3. 用 $PKG 覆盖 /usr/bin/{dockerd,docker,containerd,runc,ctr,docker-proxy,docker-init}
  4. 重写 docker.service / containerd.service（原文件已备份）
  5. 启动并校验 API 版本${plugin_note}

注意：/var/lib/docker 不做全量备份（可能很大）。升级方向受支持，降级需要恢复数据目录。
      如需回滚二进制：bash $0 --rollback $BACKUP_ROOT/docker-upgrade-$TS
EOF

if [[ $ASSUME_YES -ne 1 ]]; then
    read -r -p '确认继续？输入 yes：' ans
    [[ "$ans" == yes ]] || { log '已取消'; exit 1; }
fi

# ---------------------------------------------------------------- 备份
BK="$BACKUP_ROOT/docker-upgrade-$TS"
mkdir -p "$BK/bin" "$BK/units"
for b in dockerd docker containerd runc ctr docker-proxy docker-init containerd-shim-runc-v2; do
    [[ -f "/usr/bin/$b" ]] && cp -a "/usr/bin/$b" "$BK/bin/" || true
done
for u in docker.service containerd.service docker.socket; do
    for base in /etc/systemd/system /usr/lib/systemd/system /lib/systemd/system; do
        if [[ -f "$base/$u" ]]; then cp -a "$base/$u" "$BK/units/$u"; break; fi
    done
done
[[ -f /etc/docker/daemon.json ]] && cp -a /etc/docker/daemon.json "$BK/daemon.json"
docker info > "$BK/docker-info.txt" 2>&1 || true
docker ps -a > "$BK/docker-ps.txt" 2>&1 || true
docker images > "$BK/docker-images.txt" 2>&1 || true

# 已有插件先备份，并记录本次会新建哪些，供回滚时精确还原
if [[ -n "$PLUGIN_SRC" ]]; then
    mkdir -p "$BK/plugins"
    : > "$BK/plugins-created.txt"
    for f in "$PLUGIN_SRC"/*; do
        name="$(basename "$f")"
        if [[ -f "$PLUGIN_DIR/$name" ]]; then
            cp -a "$PLUGIN_DIR/$name" "$BK/plugins/$name"
        else
            printf '%s\n' "$name" >> "$BK/plugins-created.txt"
        fi
    done
fi
log "已备份到 $BK"

restore_on_failure() {
    warn '启动失败，自动回滚'
    systemctl stop docker docker.socket containerd 2>/dev/null || true
    install -m 0755 "$BK"/bin/* /usr/bin/ 2>/dev/null || true
    for u in docker.service containerd.service docker.socket; do
        [[ -f "$BK/units/$u" ]] && cp -f "$BK/units/$u" "/etc/systemd/system/$u" || rm -f "/etc/systemd/system/$u"
    done
    systemctl daemon-reload
    [[ -f /etc/systemd/system/docker.socket ]] && systemctl enable docker.socket >/dev/null 2>&1
    systemctl start containerd 2>/dev/null || true
    systemctl start docker 2>/dev/null || true
    warn "已回滚。请检查 journalctl -u docker -n 100；原始备份保留在 $BK"
    exit 1
}

# ---------------------------------------------------------------- 停止
log '停止 Docker'
systemctl stop docker.socket 2>/dev/null || true
systemctl stop docker 2>/dev/null || true
systemctl stop containerd 2>/dev/null || true
pkill -x dockerd 2>/dev/null || true
pkill -x containerd 2>/dev/null || true
sleep 2

# ---------------------------------------------------------------- 安装二进制
log '安装二进制'
install -m 0755 "$BIN_SRC"/* /usr/bin/
hash -r
if [[ ! -x /usr/bin/containerd-shim-runc-v2 ]]; then
    warn '包内没有 containerd-shim-runc-v2，容器可能无法启动（检查包是否完整，或从 containerd 静态包补齐）'
fi

# ---------------------------------------------------------------- systemd 单元
log '写入 systemd 单元'
cat > /etc/systemd/system/containerd.service <<'EOF'
[Unit]
Description=containerd container runtime
Documentation=https://containerd.io
After=network.target local-fs.target

[Service]
ExecStartPre=-/sbin/modprobe overlay
ExecStart=/usr/bin/containerd
Type=notify
Delegate=yes
KillMode=process
Restart=always
RestartSec=5
LimitNPROC=infinity
LimitCORE=infinity
TasksMax=infinity
OOMScoreAdjust=-999

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/docker.service <<'EOF'
[Unit]
Description=Docker Application Container Engine
Documentation=https://docs.docker.com
After=network-online.target firewalld.service containerd.service
Wants=network-online.target
Requires=containerd.service

[Service]
Type=notify
EnvironmentFile=-/etc/sysconfig/docker
EnvironmentFile=-/etc/default/docker
ExecStartPre=-/sbin/modprobe overlay
ExecStartPre=-/sbin/modprobe br_netfilter
ExecStart=/usr/bin/dockerd -H unix:///var/run/docker.sock --containerd=/run/containerd/containerd.sock
ExecReload=/bin/kill -s HUP $MAINPID
TimeoutStartSec=0
RestartSec=2
Restart=always
LimitNOFILE=infinity
LimitNPROC=infinity
LimitCORE=infinity
TasksMax=infinity
Delegate=yes
KillMode=process
OOMScoreAdjust=-500

[Install]
WantedBy=multi-user.target
EOF

mkdir -p /etc/modules-load.d /etc/sysctl.d
cat > /etc/modules-load.d/docker.conf <<'EOF'
overlay
br_netfilter
EOF
cat > /etc/sysctl.d/99-docker.conf <<'EOF'
net.bridge.bridge-nf-call-iptables = 1
net.bridge.bridge-nf-call-ip6tables = 1
net.ipv4.ip_forward = 1
EOF
modprobe overlay 2>/dev/null || true
modprobe br_netfilter 2>/dev/null || true
sysctl --system >/dev/null 2>&1 || true

# 旧的 docker.socket 会把 /var/run/docker.sock 先绑定再交给 fd:// 启动的 dockerd，
# 与「dockerd 自己监听 unix socket」冲突（address already in use），必须先摘掉。
if [[ -f /etc/systemd/system/docker.socket ]]; then
    systemctl disable docker.socket >/dev/null 2>&1 || true
    rm -f /etc/systemd/system/docker.socket
    log '移除旧 docker.socket 单元（已备份）'
fi

systemctl daemon-reload
systemctl enable containerd >/dev/null 2>&1 || true
systemctl enable docker >/dev/null 2>&1 || true

# ---------------------------------------------------------------- 启动
log '启动 containerd 与 docker'
systemctl start containerd || restore_on_failure
sleep 2
systemctl start docker || restore_on_failure

for _ in $(seq 1 30); do
    docker info >/dev/null 2>&1 && break
    sleep 2
done
docker info >/dev/null 2>&1 || restore_on_failure

new_ver="$(docker version --format '{{.Server.Version}}')"
new_api="$(docker version --format '{{.Server.APIVersion}}')"
new_driver="$(docker info --format '{{.Driver}}')"
api_num="$(awk -F. '{ printf "%d%03d", $1, $2 }' <<< "$new_api")"

log "升级完成：$cur_ver → $new_ver / API $new_api / 存储驱动 $new_driver"
if (( api_num < 1040 )); then
    warn "API $new_api 低于 1.40，若以后换回 Runner 5.x 会不兼容（当前 0.2.11 不受影响）"
fi

# 引擎已正常运行，再装 CLI 插件；插件失败不影响引擎，不回滚。
if [[ -n "$PLUGIN_SRC" ]]; then
    mkdir -p "$PLUGIN_DIR"
    if install -m 0755 "$PLUGIN_SRC"/* "$PLUGIN_DIR"/; then
        if docker compose version >/dev/null 2>&1; then
            log "docker compose：$(docker compose version --short 2>/dev/null || echo '已安装')"
        else
            warn "docker compose 插件已放入 $PLUGIN_DIR，但 CLI 未识别，请确认 docker 版本"
        fi
        if docker buildx version >/dev/null 2>&1; then
            log "docker buildx：$(docker buildx version 2>/dev/null | awk '{print $2}')"
        else
            warn 'docker buildx 插件已放入但不可用'
        fi
    else
        warn "插件安装失败（$PLUGIN_DIR 不可写？），引擎升级不受影响"
    fi
fi

echo
echo '后续检查：'
echo '  docker version'
echo "  docker ps -a          # 应能看到原有 $(docker ps -aq 2>/dev/null | wc -l | tr -d ' ') 个容器"
echo "  docker images         # 应能看到原有镜像"
if [[ -n "$PLUGIN_SRC" ]]; then
    echo '  docker compose version   # 插件版 compose（旧的 docker-compose 1.x 仍在，未删除）'
    echo '  docker compose up -d     # 起业务；也可继续用 docker-compose -f compose.v1.yaml up -d'
else
    echo '  docker compose up -d     # 或 docker-compose -f compose.v1.yaml up -d'
fi
echo
echo "回滚二进制：sudo bash $0 --rollback $BK"
echo '注意：回滚只恢复二进制与单元，不恢复运行中的数据；升级前建议先 docker compose down。'
