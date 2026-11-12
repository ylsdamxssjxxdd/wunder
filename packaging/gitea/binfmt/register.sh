#!/usr/bin/env bash
# 在 aarch64 宿主注册 x86_64 / i386 的 qemu-user binfmt：ARM 套件里的 Win7 安装包任务
# 直接 docker run amd64 的 Wine 镜像，靠这两个解释器把 x86_64 容器与 i386 Wine 跑起来。
#
# 注册用 F（fix binary）标志：解释器在注册时按本目录的绝对路径打开，容器里不需要再挂载。
# 因此套件目录移动后必须重跑本脚本；重复执行是幂等的。
# 不加 P（preserve argv[0]）标志：argv[0] 与目标路径相同（如 --entrypoint /bin/sh）时
# qemu 会误判保留参数，guest shell 多收一个 /bin/sh 操作数，把自己的 ELF 当脚本读，
# 报 Syntax error: "(" unexpected。OF 布局经嵌套 qemu 复现验证。
set -euo pipefail
cd -- "$(dirname -- "$0")"

die() { echo "[binfmt] $*" >&2; exit 2; }

[[ $EUID -eq 0 ]] || die '需要 root 运行：sudo bash binfmt/register.sh'
host_arch="$(uname -m)"
[[ "$host_arch" == aarch64 || "$host_arch" == arm64 ]] || \
    die "这个脚本只用于 ARM64 宿主，当前是 $host_arch"

grep -q ' /proc/sys/fs/binfmt_misc ' /proc/mounts || \
    mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc || die '无法挂载 binfmt_misc'

register_one() {
    local name="$1" magic="$2" mask="$3"
    local interp="$PWD/$name-static"
    [[ -x "$interp" ]] || die "缺少解释器: $interp"
    if [[ -e "/proc/sys/fs/binfmt_misc/$name" ]]; then
        # 一律禁用重建：F 标志在注册时钉住解释器文件，更换二进制后
        # 路径与 flags 比对察觉不到，复用会让旧解释器继续生效。
        echo -1 > "/proc/sys/fs/binfmt_misc/$name" 2>/dev/null || true
    fi
    printf ':%s:M::%s:%s:%s:OF\n' "$name" "$magic" "$mask" "$interp" \
        > /proc/sys/fs/binfmt_misc/register || die "注册 $name 失败"
    echo "[binfmt] 已注册 $name -> $interp"
}

# 与 Ubuntu arm64 qemu-user-static 的 /usr/lib/binfmt.d/qemu-*.conf 同一组 magic/mask。
register_one qemu-i386 \
    '\x7f\x45\x4c\x46\x01\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\x03\x00' \
    '\xff\xff\xff\xff\xff\xfe\xfe\xfc\xff\xff\xff\xff\xff\xff\xff\xff\xfe\xff\xff\xff'
register_one qemu-x86_64 \
    '\x7f\x45\x4c\x46\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\x3e\x00' \
    '\xff\xff\xff\xff\xff\xfe\xfe\xfc\xff\xff\xff\xff\xff\xff\xff\xff\xfe\xff\xff\xff'

for name in qemu-i386 qemu-x86_64; do
    grep -q '^enabled' "/proc/sys/fs/binfmt_misc/$name" || die "$name 未启用"
done
echo '[binfmt] x86_64/i386 模拟已就绪'
