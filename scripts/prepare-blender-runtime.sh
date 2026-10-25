#!/usr/bin/env bash
# 生成服务版（Docker）使用的 Blender 运行时包，产物落在 config/runtime/<平台>/。
# 之后 docker compose 挂载 config/runtime，容器启动时解压到 /opt/wunder-runtime，
# 升级 Blender 只需重跑本脚本并重启容器，不必重建镜像。
#
# 包来自 Debian bookworm 官方源：与运行镜像同一 glibc/Python 基线，
# 无 CUDA/OptiX/HIP，纯 Cycles CPU + OpenGL，x64 与 arm64 版本号一致。
# 上游官方 tar 包仅有 linux x64，且 trixie 的 4.x 与 bookworm 运行时 glibc 不兼容。
#
# 用法：
#   scripts/prepare-blender-runtime.sh                      # linux/amd64 → config/runtime/linux-x64
#   scripts/prepare-blender-runtime.sh --platform linux/arm64
#                                                           # x86 机器借 qemu 生成 ARM 包
#   scripts/prepare-blender-runtime.sh --image debian:bookworm-slim
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PLATFORM="${WUNDER_RUNTIME_PLATFORM:-linux/amd64}"
IMAGE="${WUNDER_RUNTIME_IMAGE:-debian:bookworm-slim}"
FROM_SOURCE=0
BLENDER_VERSION="${BLENDER_VERSION:-4.5.15}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --platform) PLATFORM="$2"; shift 2 ;;
    --image) IMAGE="$2"; shift 2 ;;
    --from-source) FROM_SOURCE=1; shift ;;
    --blender) BLENDER_VERSION="$2"; shift 2 ;;
    -h | --help)
      sed -n '2,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      printf '%s\n' "unknown argument: $1" >&2
      exit 64
      ;;
  esac
done

case "$PLATFORM" in
  linux/amd64 | linux/x86_64) OUT_SUBDIR=linux-x64 ;;
  linux/arm64 | linux/aarch64) OUT_SUBDIR=linux-arm64 ;;
  *)
    printf '%s\n' "unsupported platform: $PLATFORM (仅支持 linux/amd64 与 linux/arm64)" >&2
    exit 64
    ;;
esac

OUT_DIR="$REPO_ROOT/config/runtime/$OUT_SUBDIR"
mkdir -p "$OUT_DIR"

if [ "$FROM_SOURCE" = "1" ]; then
  # 源码编译：官方/发行版二进制都不可用（无 arm64 或 glibc 过新），
  # 编译期间容器内工作目录较大，挂输出卷承接源码缓存与中间产物。
  INNER="$REPO_ROOT/scripts/prepare-blender-src.inner.sh"
  BUILD_DIR_HOST="$OUT_DIR/.blender-build"
  mkdir -p "$BUILD_DIR_HOST"
  printf '%s\n' "[runtime] source build: target=$PLATFORM blender=$BLENDER_VERSION out=$OUT_DIR"
  # Windows 的 Git Bash 会把容器内路径改写成宿主路径，必须关闭该转换。
  MSYS_NO_PATHCONV=1 docker run --rm \
    --platform "$PLATFORM" \
    -e OUT_DIR=/out \
    -e BLENDER_VERSION="$BLENDER_VERSION" \
    -v "$OUT_DIR":/out \
    -v "$BUILD_DIR_HOST":/work \
    -w /work \
    -v "$INNER":/inner.sh:ro \
    "$IMAGE" bash /inner.sh
else
  printf '%s\n' "[runtime] target=$PLATFORM out=$OUT_DIR image=$IMAGE"
  # Windows 的 Git Bash 会把容器内路径改写成宿主路径，必须关闭该转换。
  MSYS_NO_PATHCONV=1 docker run --rm \
    --platform "$PLATFORM" \
    -e OUT_DIR=/out \
    -v "$OUT_DIR":/out \
    -v "$REPO_ROOT/scripts/prepare-blender-runtime.inner.sh":/inner.sh:ro \
    "$IMAGE" bash /inner.sh
fi

printf '%s\n' "[runtime] files:"
ls -lh "$OUT_DIR" | sed 's/^/  /'
printf '%s\n' "[runtime] 重启容器使其生效：docker compose up -d --force-recreate wunder-sandbox"
