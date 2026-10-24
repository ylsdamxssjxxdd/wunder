#!/usr/bin/env bash
# 生成服务版（Docker）使用的 Node + Remotion 运行时包，产物落在 config/runtime/<平台>/。
# 之后 docker compose 挂载 config/runtime，容器启动时解压到 /opt/wunder-runtime，
# 升级 Node/Remotion 只需重跑本脚本并重启容器，不必重建镜像。
#
# 用法：
#   scripts/prepare-node-runtime.sh                          # linux/amd64 → config/runtime/linux-x64
#   scripts/prepare-node-runtime.sh --platform linux/arm64   # → config/runtime/linux-arm64
#   scripts/prepare-node-runtime.sh --platform linux/arm64 --image wunder-arm:latest
#                                                           # x86 机器借 qemu 生成 ARM 包（约 11 分钟）
#   scripts/prepare-node-runtime.sh --remotion 4.0.560 --node 22.22.2
#   scripts/prepare-node-runtime.sh --extra-deps "mediabunny @remotion/google-fonts"
#   scripts/prepare-node-runtime.sh --skip-browser           # 不带 Chrome Headless Shell
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PLATFORM="${WUNDER_RUNTIME_PLATFORM:-linux/amd64}"
IMAGE="${WUNDER_RUNTIME_IMAGE:-}"
NODE_VERSION="${NODE_VERSION:-22.22.2}"
REMOTION_VERSION="${REMOTION_VERSION:-4.0.538}"
REACT_VERSION="${REMOTION_REACT_VERSION:-19.2.3}"
REGISTRY="${NPM_CONFIG_REGISTRY:-https://registry.npmmirror.com}"
NODE_BASE="${WUNDER_NODE_MIRROR:-https://registry.npmmirror.com/-/binary/node}"
INSTALL_BROWSER=1
OUT_SUBDIR=""
EXTRA_DEPS="${EXTRA_DEPS:-}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --platform) PLATFORM="$2"; shift 2 ;;
    --image) IMAGE="$2"; shift 2 ;;
    --node) NODE_VERSION="$2"; shift 2 ;;
    --remotion) REMOTION_VERSION="$2"; shift 2 ;;
    --react) REACT_VERSION="$2"; shift 2 ;;
    --out) OUT_SUBDIR="$2"; shift 2 ;;
    --extra-deps) EXTRA_DEPS="$2"; shift 2 ;;
    --registry) REGISTRY="$2"; shift 2 ;;
    --skip-browser) INSTALL_BROWSER=0; shift ;;
    -h | --help)
      sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      printf '%s\n' "unknown argument: $1" >&2
      exit 64
      ;;
  esac
done

if [ -z "$OUT_SUBDIR" ]; then
  case "$PLATFORM" in
    *arm64*) OUT_SUBDIR=linux-arm64 ;;
    *) OUT_SUBDIR=linux-x64 ;;
  esac
fi

# 离线视频制作常用的 Remotion 生态依赖，随缓存一起预取；
# 需要额外包时用 --extra-deps "pkg@version ..." 覆盖，或设 EXTRA_DEPS 环境变量。
if [ -z "$EXTRA_DEPS" ]; then
  EXTRA_DEPS="mediabunny zod \
@remotion/transitions@$REMOTION_VERSION \
@remotion/paths@$REMOTION_VERSION \
@remotion/shapes@$REMOTION_VERSION \
@remotion/layout-utils@$REMOTION_VERSION \
@remotion/noise@$REMOTION_VERSION \
@remotion/zod-types@$REMOTION_VERSION \
@remotion/captions@$REMOTION_VERSION \
@remotion/media-utils@$REMOTION_VERSION \
@remotion/motion-blur@$REMOTION_VERSION"
fi

if [ -z "$IMAGE" ]; then
  case "$OUT_SUBDIR" in
    linux-arm64*) IMAGE=wunder-arm:latest ;;
    *) IMAGE=wunder-x86:latest ;;
  esac
fi

OUT_DIR="$REPO_ROOT/config/runtime/$OUT_SUBDIR"
mkdir -p "$OUT_DIR"

printf '%s\n' "[runtime] target=$PLATFORM out=$OUT_DIR image=$IMAGE"
printf '%s\n' "[runtime] node=$NODE_VERSION remotion=$REMOTION_VERSION react=$REACT_VERSION browser=$INSTALL_BROWSER"

# Windows 的 Git Bash 会把容器内路径改写成宿主路径，必须关闭该转换。
MSYS_NO_PATHCONV=1 docker run --rm \
  --platform "$PLATFORM" \
  -e NODE_VERSION="$NODE_VERSION" \
  -e REMOTION_VERSION="$REMOTION_VERSION" \
  -e REACT_VERSION="$REACT_VERSION" \
  -e NPM_CONFIG_REGISTRY="$REGISTRY" \
  -e NODE_BASE="$NODE_BASE" \
  -e INSTALL_REMOTION_BROWSER="$INSTALL_BROWSER" \
  -e EXTRA_DEPS="$EXTRA_DEPS" \
  -e NPM_CONFIG_FUND=false \
  -e NPM_CONFIG_AUDIT=false \
  -e OUT_DIR=/out \
  -v "$OUT_DIR":/out \
  -v "$REPO_ROOT/scripts/prepare-node-runtime.inner.sh":/inner.sh:ro \
  "$IMAGE" bash /inner.sh

printf '%s\n' "[runtime] files:"
ls -lh "$OUT_DIR" | sed 's/^/  /'
printf '%s\n' "[runtime] 重启容器使其生效：docker compose up -d --force-recreate wunder-sandbox"
