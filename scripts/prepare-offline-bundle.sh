#!/usr/bin/env bash
# 在有网络的机器上准备离线部署包：镜像、Node/Remotion 运行时、前端依赖。
# 产物拷到无网服务器后用 scripts/load-offline-bundle.sh 导入即可。
#
# 用法：
#   scripts/prepare-offline-bundle.sh                       # 默认 docker-compose-x86.yml → dist/offline
#   COMPOSE_FILE=docker-compose-arm.yml scripts/prepare-offline-bundle.sh
#   OFFLINE_OUT=/data/offline scripts/prepare-offline-bundle.sh
#   SKIP_IMAGES=1 scripts/prepare-offline-bundle.sh          # 只收集运行时与前端依赖
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="${COMPOSE_FILE:-docker-compose-x86.yml}"
OUT="${OFFLINE_OUT:-dist/offline}"
SKIP_IMAGES="${SKIP_IMAGES:-0}"

cd "$REPO_ROOT"
mkdir -p "$OUT"

if [ ! -d config/runtime ] || [ -z "$(find config/runtime -name '*.tar.gz' -print -quit 2>/dev/null)" ]; then
  printf '%s\n' "[offline] config/runtime 为空，先运行 scripts/prepare-node-runtime.sh" >&2
  exit 1
fi

printf '%s\n' "[offline] 收集 Node/Remotion 运行时"
tar -czf "$OUT/runtime.tar.gz" -C "$REPO_ROOT" config/runtime

case "$COMPOSE_FILE" in
  *arm*) DEPS_DIR=node_modules-linux-arm ;;
  *) DEPS_DIR=node_modules-linux-x86 ;;
esac
if [ -d "$DEPS_DIR" ]; then
  printf '%s\n' "[offline] 收集前端依赖 $DEPS_DIR（离线构建蜂巢必需）"
  tar -czf "$OUT/frontend-deps.tar.gz" -C "$REPO_ROOT" "$DEPS_DIR"
else
  printf '%s\n' "[offline] 跳过前端依赖：未找到 $DEPS_DIR" >&2
fi

if [ "$SKIP_IMAGES" = "1" ]; then
  printf '%s\n' "[offline] 跳过镜像导出"
else
  printf '%s\n' "[offline] 解析 $COMPOSE_FILE 需要的镜像"
  images="$(docker compose -f "$COMPOSE_FILE" config --images | sort -u)"
  printf '%s\n' "$images" | sed 's/^/  /'
  printf '%s\n' "$images" > "$OUT/images.txt"
  # shellcheck disable=SC2086
  docker save $images -o "$OUT/images.tar"
  printf '%s\n' "[offline] 镜像已导出：$OUT/images.tar ($(du -h "$OUT/images.tar" | cut -f1))"
fi

printf '%s\n' \
  "created_at=$(date -Iseconds)" \
  "compose_file=$COMPOSE_FILE" \
  "host_platform=$(uname -s)/$(uname -m)" \
  > "$OUT/bundle.env"

printf '%s\n' "[offline] 完成，产物在 $OUT："
ls -lh "$OUT" | sed 's/^/  /'
printf '%s\n' "[offline] 把整个目录拷到服务器后执行 scripts/load-offline-bundle.sh"
