#!/usr/bin/env bash
# 服务版 Blender 运行时包生成器·源码编译版（在容器内执行）。
# 由 scripts/prepare-blender-runtime.sh --from-source 调用。
#
# 官方与发行版二进制均不可用于运行镜像基线（官方 tar 无 arm64；trixie 4.x 需
# glibc 2.38+ 而镜像为 bookworm 2.36），现代版本只能源码编译。按 CPU-only 裁剪：
# 无 CUDA/OptiX/HIP、无 USD/MaterialX/OSL(LLVM)/OpenImageDenoise。
set -euo pipefail

BLENDER_VERSION="${BLENDER_VERSION:-4.5.15}"
NCPU="${NCPU:-$(nproc)}"

WORK="$PWD/blender-build"
SRC="$WORK/src"
mkdir -p "$WORK"

# ---------- 系统构建依赖 ----------
DEBIAN_FRONTEND=noninteractive
cat > /etc/apt/sources.list <<'SOURCES'
deb [trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/debian bookworm main contrib non-free non-free-firmware
deb [trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/debian bookworm-updates main contrib non-free non-free-firmware
deb [trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/debian-security bookworm-security main contrib non-free-firmware
SOURCES
apt_get() { apt-get -o Acquire::Retries=5 "$@"; }
apt_get update >/dev/null
apt_get install -y --no-install-recommends \
  build-essential cmake ninja-build git curl xz-utils patch pkg-config bison flex \
  autoconf automake libtool tcl yasm patchelf meson python3-mako \
  python3-dev python3-numpy python3-requests python3-pip \
  libx11-dev libxi-dev libxxf86vm-dev libxfixes-dev libxrender-dev libxrandr-dev \
  libgl-dev libegl-dev libxkbcommon-dev libsm-dev libice-dev libdbus-1-dev \
  libglib2.0-dev libfontconfig1-dev libharfbuzz-dev >/dev/null

# ---------- 源码（输出卷缓存，失败重跑不重下） ----------
cache_dir="$OUT_DIR/.blender-cache"
mkdir -p "$cache_dir"
tarball="$cache_dir/blender-$BLENDER_VERSION.tar.xz"
if [ ! -s "$tarball" ]; then
  printf '%s\n' "[runtime] downloading blender-$BLENDER_VERSION source"
  curl -fsSL "https://download.blender.org/source/blender-$BLENDER_VERSION.tar.xz" -o "$tarball"
else
  printf '%s\n' "[runtime] reusing cached source tarball"
fi
[ -d "$SRC/.git" ] || [ -f "$SRC/Makefile" ] || {
  mkdir -p "$SRC"
  tar -xJf "$tarball" -C "$SRC" --strip-components=1
}

# ---------- 依赖与主体 ----------
# make 必须在源码目录内执行，构建目录出现在源码旁边（build_linux_<arch>）。
# build_environment 的 superbuild 无按需开关（options.cmake 不含 BUILD_USD 等），
# 裁剪要直接注释 include：USD/MaterialX/LLVM/OSL/OpenXR 与 Intel/AMD GPU 编译器栈
# （dpcpp/hiprt/igc/gmmlib/ocloc 在 x64 会全量编译，数小时且我们用不到）。
env_cmake="$SRC/build_files/build_environment/CMakeLists.txt"
dl_cmake="$SRC/build_files/build_environment/cmake/download.cmake"
sed -i \
  -e 's|^\(include(cmake/usd.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/materialx.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/llvm.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/osl.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/xr_openxr.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/hiprt.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/dpcpp.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/dpcpp_deps.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/igc.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/gmmlib.cmake)\)|# \1 (wunder: cut)|' \
  -e 's|^\(include(cmake/ocloc.cmake)\)|# \1 (wunder: cut)|' \
  "$env_cmake"
# download.cmake 在 include 时执行全量 download_source 清单，与上面 include 的裁剪
# 必须同步，否则照旧下载/校验 USD 等已裁剪组件；Wayland/Mesa/LIBGLU 对无头渲染无用。
sed -i \
  -e 's|^\(download_source(USD)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(MATERIALX)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(OIDN)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(LLVM)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(OSL)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(XR_OPENXR_SDK)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(LEVEL_ZERO)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(DPCPP)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(VCINTRINSICS)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(OPENCLHEADERS)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(WL_PROTOCOLS)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(WAYLAND)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(WAYLAND_LIBDECOR)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(WAYLAND_WESTON)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(MESA)\)|# \1 (wunder: cut)|' \
  -e 's|^\(download_source(LIBGLU)\)|# \1 (wunder: cut)|' \
  "$dl_cmake"
APP_ARGS="-DWITH_USD=OFF -DWITH_MATERIALX=OFF -DWITH_OPENIMAGEDENOISE=OFF -DWITH_CYCLES_OSL=OFF -DWITH_CYCLES_CUDA_BINARIES=OFF -DWITH_CYCLES_DEVICE_OPTIX=OFF -DWITH_XR_OPENXR=OFF -DWITH_GHOST_WAYLAND=OFF -DWITH_INSTALL_PORTABLE=ON -DWITH_PYTHON_INSTALL=ON"

# gmplib.org 在部分网络不可达：改用 Debian 的 +dfsg 重打包（仅去 GFDL 文档），
# 恢复上游目录布局 gmp-6.3.0/，并同步改写 versions.cmake 锁定的 MD5。
if [ ! -s "$cache_dir/gmp-6.3.0.tar.xz" ]; then
  curl -fsSL "https://mirrors.tuna.tsinghua.edu.cn/debian/pool/main/g/gmp/gmp_6.3.0+dfsg.orig.tar.xz" -o /tmp/gmp-dfsg.tar.xz
  rm -rf /tmp/gmp-repack && mkdir -p /tmp/gmp-repack
  (cd /tmp/gmp-repack && tar -xJf /tmp/gmp-dfsg.tar.xz && mv gmp-6.3.0+dfsg gmp-6.3.0 &&
    tar -cJf "$cache_dir/gmp-6.3.0.tar.xz" gmp-6.3.0)
fi
gmp_md5="$(md5sum "$cache_dir/gmp-6.3.0.tar.xz" | cut -d" " -f1)"
sed -i "s/set(GMP_HASH [0-9a-f]\{32\})/set(GMP_HASH $gmp_md5)/" \
  "$SRC/build_files/build_environment/cmake/versions.cmake"
for pkg_dir in "$WORK"/build_linux/deps_*/packages; do
  [ -d "$pkg_dir" ] && cp -f "$cache_dir/gmp-6.3.0.tar.xz" "$pkg_dir/"
done

if [ ! -f "$WORK/deps.done" ]; then
  printf '%s\n' "[runtime] building deps (-j$NCPU; qemu 下可能十小时级)"
  (cd "$SRC" && make deps -j"$NCPU")
  : > "$WORK/deps.done"
fi

if ! find "$WORK"/build_linux_*/bin -maxdepth 1 -name blender -type f -print -quit 2>/dev/null | grep -q .; then
  printf '%s\n' "[runtime] building blender (-j$NCPU)"
  (cd "$SRC" && make -j"$NCPU" BUILD_CMAKE_ARGS="$APP_ARGS")
fi

BUILD_DIR="$(find "$WORK" -maxdepth 1 -type d -name 'build_linux_*' | head -1)"
[ -n "$BUILD_DIR" ] && [ -d "$BUILD_DIR/bin" ] || {
  printf '%s\n' "[runtime] error: portable install dir not found" >&2
  exit 1
}
printf '%s\n' "[runtime] portable tree: $BUILD_DIR/bin"

# ---------- 打包为运行时布局 ----------
# 便携树 bin/{blender, <ver>/, lib/} 相对布局原样保留（RPATH 有效），
# 真实二进制改名 blender.bin，同名 wrapper 作为统一入口。
stage="$WORK/blender"
rm -rf "$stage"
mkdir -p "$stage/bin"
cp -a "$BUILD_DIR/bin/." "$stage/bin/"
mv "$stage/bin/blender" "$stage/bin/blender.bin"

# ---------- 系统库闭包 ----------
skip_libs='^(linux-vdso|ld-linux|libc\.so|libm\.so|libpthread|libdl\.so|librt\.so|libresolv|libnsl|libutil|libcrypt|libgcc_s|libstdc\+\+|libz\.so|libcuda\.so|libnvidia-|libamdhip64|libze_loader|libOpenCL)'
libdir="$stage/bin/lib"
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
# 统一入口：真实二进制与数据目录保持同级相对布局，仅补包内 lib 优先加载。
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export LD_LIBRARY_PATH="$ROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$ROOT/blender.bin" "$@"
WRAPPER
chmod +x "$stage/bin/blender"

# ---------- 自检 ----------
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
    "BLENDER_VERSION=$BLENDER_VERSION" \
    "BLENDER_SOURCE=source-cpu"
} > "$OUT_DIR/manifest.env.tmp"
mv "$OUT_DIR/manifest.env.tmp" "$OUT_DIR/manifest.env"

printf '%s\n' "[runtime] done"
ls -lh "$OUT_DIR" | awk '{print "  " $9, $5}'
