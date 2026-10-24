#!/usr/bin/env bash
# 在无网服务器上导入离线部署包（由 scripts/prepare-offline-bundle.sh 生成）。
#
# 用法：
#   scripts/load-offline-bundle.sh dist/offline
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE="${1:-$REPO_ROOT/dist/offline}"

cd "$REPO_ROOT"

if [ -f "$BUNDLE/images.tar" ]; then
  printf '%s\n' "[offline] 导入镜像（首次或镜像版本变化时）"
  docker load -i "$BUNDLE/images.tar"
else
  printf '%s\n' "[offline] 未找到 images.tar，跳过镜像导入"
fi

if [ -f "$BUNDLE/runtime.tar.gz" ]; then
  printf '%s\n' "[offline] 释放 Node/Remotion 运行时到 config/runtime"
  tar -xzf "$BUNDLE/runtime.tar.gz" -C "$REPO_ROOT"
fi

DEPS_ARCHIVE=""
for candidate in "$BUNDLE"/frontend-deps.tar.gz; do
  [ -f "$candidate" ] && DEPS_ARCHIVE="$candidate"
done
if [ -n "$DEPS_ARCHIVE" ]; then
  printf '%s\n' "[offline] 释放前端依赖（复用挂载的 node_modules-linux-*，避免离线 npm ci）"
  tar -xzf "$DEPS_ARCHIVE" -C "$REPO_ROOT"
fi

printf '%s\n' "[offline] 启动：COMPOSE_FILE=${COMPOSE_FILE:-docker-compose-x86.yml}"
printf '%s\n' "  docker compose -f ${COMPOSE_FILE:-docker-compose-x86.yml} up -d"
printf '%s\n' "[offline] 校验：docker compose exec wunder-sandbox bash -lc 'command -v remotion && node -v'"
