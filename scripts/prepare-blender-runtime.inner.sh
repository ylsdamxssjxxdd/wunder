#!/usr/bin/env bash
# 服务版 Blender 运行时包生成器（在容器内执行，跨平台一致）。
# 由 scripts/prepare-blender-runtime.sh 调用，参数全部通过环境变量传入。
#
# 包来自 Debian bookworm 官方源（与运行镜像同一 glibc/Python 基线，
# 上游 trixie 的 4.x 需要 glibc 2.38+，bookworm 运行时跑不起来）。
# Debian 主仓库无 CUDA/OptiX/HIP，纯 Cycles CPU + OpenGL，天然满足精简要求；
# Python 标准库与 numpy 一并打包，运行镜像无需装任何东西。
set -euo pipefail

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

DEBIAN_FRONTEND=noninteractive
# 与仓库其他构建一致换清华镜像；http + trusted=yes 规避证书依赖。
cat > /etc/apt/sources.list <<'SOURCES'
deb [trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/debian bookworm main contrib non-free non-free-firmware
deb [trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/debian bookworm-updates main contrib non-free non-free-firmware
deb [trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/debian-security bookworm-security main contrib non-free-firmware
SOURCES

apt_get() {
  apt-get -o Acquire::Retries=5 "$@"
}
apt_get update >/dev/null
# blender 依赖已覆盖全部渲染核心库；额外补 numpy/requests 供脚本与插件使用。
apt_get install -y --no-install-recommends blender python3-numpy python3-requests >/dev/null

blender_version="$(dpkg-query -W -f '${Version}' blender | sed 's/+dfsg.*//')"
printf '%s\n' "[runtime] debian blender $blender_version ($(dpkg --print-architecture))"

# ---------- 收集包体 ----------
# Debian 布局：二进制在 /usr/bin/blender，数据在 /usr/share/blender/{datafiles,scripts}。
# 包内保持 <root>/bin 与 <root>/share 的相对布局，wrapper 再用 BLENDER_SYSTEM_* 显式指路。
stage="$WORK/blender"
mkdir -p "$stage/bin"
cp /usr/bin/blender "$stage/bin/blender.bin"
mkdir -p "$stage/share"
cp -a /usr/share/blender "$stage/share/blender"
rm -rf "$stage/share/blender/datafiles/locale"

# Python：blender 内嵌 libpython3.11，标准库与第三方模块也打包进去，
# 用 PYTHONHOME 重定向，运行镜像有没有 python 都不影响。
py_ver="$(ls /usr/lib | grep -E '^python3\.[0-9]+$' | head -1)"
[ -n "$py_ver" ] || {
  printf '%s\n' "[runtime] error: python3 stdlib not found in build image" >&2
  exit 1
}
mkdir -p "$stage/python/lib" "$stage/python/lib/$py_ver/site-packages"
# 目标目录已存在，必须复制“内容”而不是整个目录，否则会多嵌套一层 python3.11/。
cp -a "/usr/lib/$py_ver/." "$stage/python/lib/$py_ver/"
[ -d "$stage/python/lib/$py_ver/encodings" ] || {
  printf '%s\n' "[runtime] error: python stdlib copy incomplete" >&2
  exit 1
}
for pkg_dir in /usr/lib/python3/dist-packages /usr/local/lib/python3*/dist-packages; do
  [ -d "$pkg_dir" ] || continue
  cp -a "$pkg_dir/." "$stage/python/lib/$py_ver/site-packages/" 2>/dev/null || true
done

# ---------- 兼容库闭包 ----------
# 把 ldd 解析到系统路径的非核心库收进包内 lib/（wrapper 设 LD_LIBRARY_PATH），包即自包含。
# 排除：glibc/gcc 核心运行时（运行环境必有，捆绑反而危险）；
# GPU 驱动库（CUDA/HIP/oneAPI/OpenCL，宿主机驱动运行时 dlopen 的可选后端）。
skip_libs='^(linux-vdso|ld-linux|libc\.so|libm\.so|libpthread|libdl\.so|librt\.so|libresolv|libnsl|libutil|libcrypt|libgcc_s|libstdc\+\+|libz\.so|libcuda\.so|libnvidia-|libamdhip64|libze_loader|libOpenCL)'
libdir="$stage/lib"
mkdir -p "$libdir"
bundle_system_libs() {
  local obj name target new=0
  for obj in "$stage/bin/blender.bin" "$libdir"/*.so*; do
    [ -e "$obj" ] || continue
    while read -r name _arrow target _rest; do
      [ -n "${target:-}" ] || continue
      case "$name" in $skip_libs) continue ;; esac
      case "$target" in "$libdir"/*) continue ;; esac
      if [ -e "$target" ] && [ ! -e "$libdir/$name" ]; then
        cp -L "$target" "$libdir/$name"
        new=1
      fi
    done < <(ldd "$obj" 2>/dev/null)
  done
  [ "$new" -eq 1 ]
}

pass=0
while bundle_system_libs && [ "$pass" -lt 6 ]; do
  pass=$((pass + 1))
done
printf '%s\n' "[runtime] bundled $(ls "$libdir" | wc -l) compat libs"

remaining="$(for obj in "$stage/bin/blender.bin" "$libdir"/*.so*; do
  [ -e "$obj" ] || continue
  ldd "$obj" 2>/dev/null || true
done | awk '/not found/{print $1}' | sort -u | grep -Ev "$skip_libs" || true)"
if [ -n "$remaining" ]; then
  printf '%s\n' "[runtime] error: unresolved libraries: $(printf '%s' "$remaining" | tr '\n' ' ')" >&2
  exit 1
fi

# ---------- 统一入口 ----------
cat > "$stage/bin/blender" <<'WRAPPER'
#!/bin/sh
# 统一入口：指向运行时内的真实二进制，并优先加载捆绑的兼容库与 Python，
# 数据与脚本显式指向包内 share/blender，不依赖安装路径。
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
export LD_LIBRARY_PATH="$ROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BLENDER_SYSTEM_DATAFILES="$ROOT/share/blender/datafiles"
export BLENDER_SYSTEM_SCRIPTS="$ROOT/share/blender/scripts"
if [ -d "$ROOT/python/lib" ]; then
  export PYTHONHOME="$ROOT/python"
  for py_lib in "$ROOT"/python/lib/python3.*; do
    [ -d "$py_lib" ] && export PYTHONPATH="$py_lib/site-packages${PYTHONPATH:+:$PYTHONPATH}"
  done
fi
exec "$ROOT/bin/blender.bin" "$@"
WRAPPER
chmod +x "$stage/bin/blender"

# ---------- 打包前自检 ----------
cat > "$WORK/smoke.py" <<'SMOKE'
import os
import sys

import bpy

out = sys.argv[sys.argv.index("--") + 1]
os.makedirs(out, exist_ok=True)
scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.device = "CPU"
scene.cycles.samples = 1
scene.cycles.use_denoising = False
scene.render.resolution_x = 64
scene.render.resolution_y = 64
scene.render.filepath = os.path.join(out, "smoke.png")
bpy.ops.render.render(write_still=True)
SMOKE
printf '%s\n' "[runtime] smoke render"
rm -rf /tmp/wunder-blender-smoke
"$stage/bin/blender" --background --factory-startup --python "$WORK/smoke.py" -- /tmp/wunder-blender-smoke >/dev/null
[ -s /tmp/wunder-blender-smoke/smoke.png ] || {
  printf '%s\n' "[runtime] error: smoke render produced no output" >&2
  exit 1
}
rm -rf /tmp/wunder-blender-smoke

# ---------- 打包与清单 ----------
printf '%s\n' "[runtime] packing blender"
tar -czf "$OUT_DIR/blender.tar.gz" -C "$WORK" blender

{
  if [ -f "$OUT_DIR/manifest.env" ]; then
    grep -v '^BLENDER_' "$OUT_DIR/manifest.env" || true
  fi
  printf '%s\n' \
    "BLENDER_VERSION=$blender_version" \
    "BLENDER_SOURCE=debian-bookworm"
} > "$OUT_DIR/manifest.env.tmp"
mv "$OUT_DIR/manifest.env.tmp" "$OUT_DIR/manifest.env"

printf '%s\n' "[runtime] done"
ls -lh "$OUT_DIR" | awk '{print "  " $9, $5}'
