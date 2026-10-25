#!/bin/bash
# 交叉编译 Remotion compositor 的 32 位 Windows 版本（ia32）。
#
# 官方只为 win32-x64 发布 @remotion/compositor-win32-x64-msvc，32 位要自己编。
# 全部步骤在 Linux 容器内完成，产物可直接注入 win7 补充包。
#
# 工具链与 wunder 主程序保持一致（builders/build-win7-cli.ps1）：
#   host   nightly-2026-03-14-x86_64（64 位 host，规避 LLVM 32 位地址空间限制）
#   target i686-win7-windows-gnu + -Z build-std=std,panic_abort
# 因此 compositor 的 std 也不引入 Win10 API，和 wunder 的 Win7 承诺一致。
#
# 用法（仓库根目录）：
#   scripts/win-ia32-compositor/build.sh
#   SRC_COMPOSITOR=/path/to/remotion/packages/compositor scripts/win-ia32-compositor/build.sh
#
# 前置：Docker；可访问 codeload.github.com、crates.io、static.rust-lang.org、api.github.com。
# 产物：config/runtime/win-ia32/compositor-win32-ia32/
set -euo pipefail

# Windows 的 Git Bash 会把 docker exec 里的 /b/... 改写成宿主路径，必须关闭转换
export MSYS_NO_PATHCONV=1

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"
WORK="$REPO_ROOT/temp/win-ia32-compositor"
IMAGE="${IMAGE:-wunder-x86:latest}"
RUST_TOOLCHAIN="${RUST_TOOLCHAIN:-nightly-2026-03-14}"
RUST_TARGET="${RUST_TARGET:-i686-win7-windows-gnu}"
REMOTION_VERSION="${REMOTION_VERSION:-4.0.538}"
SRC_COMPOSITOR="${SRC_COMPOSITOR:-D:/proj/参考/remotion-main/packages/compositor}"
OUT="$REPO_ROOT/config/runtime/win-ia32/compositor-win32-ia32"

# CI 与干净的 Linux 机器上没有本地 Remotion 源码：从发布 tag 拉一份，只取
# packages/compositor。仓库很大，但 codeload 只传单个 tag 的 tar 包。
if [ ! -f "$SRC_COMPOSITOR/Cargo.toml" ]; then
  printf '%s\n' "[ia32] 本地没有 compositor 源码，从 v$REMOTION_VERSION 拉取"
  mkdir -p "$WORK/dl"
  TARBALL="$WORK/dl/remotion-v$REMOTION_VERSION.tar.gz"
  [ -s "$TARBALL" ] || curl -fsSL --http1.1 --retry 3 \
    "https://codeload.github.com/remotion-dev/remotion/tar.gz/refs/tags/v$REMOTION_VERSION" -o "$TARBALL"
  mkdir -p "$WORK/src"
  rm -rf "$WORK/src/compositor"
  tar -xzf "$TARBALL" -C "$WORK/src" --strip-components=1 \
    --wildcards "*/packages/compositor" 2>/dev/null ||
    tar -xzf "$TARBALL" -C "$WORK/src" --strip-components=2 "*/packages/compositor"
  SRC_COMPOSITOR="$WORK/src/packages/compositor"
  [ -f "$SRC_COMPOSITOR/Cargo.toml" ] || {
    printf '%s\n' "拉取的包里没有 packages/compositor（tag v$REMOTION_VERSION）" >&2
    exit 2
  }
fi

mkdir -p "$WORK/src"
[ -d "$WORK/src/compositor" ] || cp -r "$SRC_COMPOSITOR" "$WORK/src/compositor"
rm -rf "$WORK/src/compositor/target" "$WORK/src/compositor/toolchains"
rm -rf "$WORK/src/bindgen-gen" && cp -r "$HERE/bindgen-gen" "$WORK/src/"
cp "$HERE/fetch-sys.py" "$HERE/fix-bindings.py" "$HERE/patch-compositor.py" "$HERE/inner.sh" "$WORK/"

# rust-ffmpeg-sys 与 rust-ffmpeg 的源码已 vendor 在仓库里：前者仓库有 3.5GB
# （zips/ 是各平台 FFmpeg 二进制），逐文件拉 GitHub API 还会撞 60 次/小时的限流；
# 后者是 cargo 的 git 依赖，而容器里访问不到 github.com（只有 codeload/api 通）。
# 预置后构建不再依赖 GitHub。只覆盖同名文件，保留容器侧生成的 zips/ 与 bindings/。
if [ -d "$HERE/vendor" ]; then
  mkdir -p "$WORK/vendor"
  cp -r "$HERE/vendor/." "$WORK/vendor/"
fi

# wunder-x86 是本地用根 Dockerfile 构建的开发镜像（rust:1.92-slim-bookworm 基座），
# CI 和新机器上没有。缺省镜像本地不存在时退回同基座的官方 rust 镜像即可：
# mingw、nasm、libclang、python3 等依赖由下面的 apt 步骤补齐，rustup 装工具链
# 也在容器内完成。显式指定 IMAGE 时不在回退之列，让它按 docker 语义报错。
if [ "$IMAGE" = "wunder-x86:latest" ] && ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  printf '%s\n' "[ia32] 本地没有 $IMAGE，退回官方 rust:1.92-slim-bookworm 基座"
  IMAGE="rust:1.92-slim-bookworm"
fi

docker rm -f xw7build >/dev/null 2>&1 || true
MSYS_NO_PATHCONV=1 docker run -d --name xw7build --shm-size=2gb -w /b \
  -v "$WORK:/b" -v "$WORK/src/compositor:/b/crate" \
  "$IMAGE" sleep infinity >/dev/null

printf '%s\n' "[ia32] 准备容器依赖"
docker exec xw7build bash -lc '
set -e
export DEBIAN_FRONTEND=noninteractive
command -v i686-w64-mingw32-gcc >/dev/null 2>&1 || {
  apt-get update -qq
  apt-get install -y -qq --no-install-recommends \
    mingw-w64 nasm yasm pkg-config xz-utils libclang-dev curl make ca-certificates python3 >/dev/null
}
'

printf '%s\n' "[ia32] 工具链 $RUST_TOOLCHAIN / $RUST_TARGET"
docker exec -e RUST_TOOLCHAIN="$RUST_TOOLCHAIN" -e RUST_TARGET="$RUST_TARGET" \
  xw7build bash -c '
set -e
# rustup/cargo 只在镜像 ENV PATH（/usr/local/cargo/bin）里，bash -l 会被
# /etc/profile 重置 PATH 导致 rustup not found；用非登录 shell 并显式补上。
export PATH="/usr/local/cargo/bin:$PATH"
# win7 target 是 tier 3，没有预编译 std；不装 --target，靠 rust-src + build-std 现编
rustup toolchain list | grep -q "$RUST_TOOLCHAIN" ||
  rustup toolchain install "$RUST_TOOLCHAIN" --profile minimal
rustup +"$RUST_TOOLCHAIN" component list --installed | grep -q rust-src ||
  rustup +"$RUST_TOOLCHAIN" component add rust-src
rustc +"$RUST_TOOLCHAIN" -V
'

printf '%s\n' "[ia32] 开始构建（x264 → FFmpeg 7.1 → bindings → compositor）"
docker exec -e RUST_TOOLCHAIN="$RUST_TOOLCHAIN" -e RUST_TARGET="$RUST_TARGET" \
  xw7build bash /b/inner.sh

mkdir -p "$OUT"
cp -f "$WORK/out/compositor-win32-ia32/"* "$OUT/"

# Win7 承诺校验：必须是 32 位 PE，且不允许 Win10 之后才有的导入。
# 注意 objdump -p 不打印 "file format" 行，架构要用 -f 看。
PE_FMT="$(docker exec xw7build bash -lc "i686-w64-mingw32-objdump -f /b/out/compositor-win32-ia32/remotion.exe")"
PE_REPORT="$(docker exec xw7build bash -lc "i686-w64-mingw32-objdump -p /b/out/compositor-win32-ia32/remotion.exe")"
docker rm -f xw7build >/dev/null 2>&1 || true

printf '%s\n' "$PE_FMT" | grep -q 'pei-i386' || {
  printf '%s\n' "[ia32] 产物不是 32 位 PE：" >&2
  printf '%s\n' "$PE_FMT" | grep -i 'file format' >&2
  exit 1
}
if printf '%s\n' "$PE_REPORT" | grep -qiE 'combase\.dll|api-ms-win-|ext-ms-win-|GetDpiForWindow|WaitOnAddress|WakeByAddress|SetThreadDescription|bcryptprimitives'; then
  printf '%s\n' "[ia32] 产物引入了 Win10+ API，Win7 无法加载" >&2
  exit 1
fi
if printf '%s\n' "$PE_REPORT" | grep -qi 'large address aware'; then
  printf '%s\n' "[ia32] PE ok: i386, 无 Win10 导入, LARGE_ADDRESS_AWARE"
else
  printf '%s\n' "[ia32] 警告：未标记 LARGE_ADDRESS_AWARE，32 位进程只有 2GB 地址空间" >&2
fi

printf '%s\n' "[ia32] 产物：$OUT"
printf '%s\n' "[ia32] 注入补充包："
printf '%s\n' "  scripts/patch-win7-remotion-ia32.sh --root <package-root> --bin $OUT \\"
printf '%s\n' "    --esbuild-tgz config/runtime/win-ia32/esbuild-win32-ia32-0.28.1.tgz"
