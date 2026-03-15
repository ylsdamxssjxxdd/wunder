#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
TARGET_DIR="${ROOT_DIR}/target/arm64-20"
DIST_DIR="${TARGET_DIR}/dist"
CACHE_ROOT="${TARGET_DIR}/.cache"
OUTPUT_PATH="${OUTPUT_PATH:-${DIST_DIR}/wunder-arm64-offline-bundle.tar.gz}"
INCLUDE_NODE_MODULES="${INCLUDE_NODE_MODULES:-1}"
INCLUDE_FRONTEND_DIST="${INCLUDE_FRONTEND_DIST:-1}"
INCLUDE_BRIDGE_BIN="${INCLUDE_BRIDGE_BIN:-1}"
MANIFEST_PATH="${CACHE_ROOT}/offline-bundle.manifest.txt"

entries=(
  ".cargo/arm64-20"
  "target/arm64-20/.cache"
  "target/arm64-20/.build/python"
)

for required in   ".cargo/arm64-20"   "target/arm64-20/.cache"   "target/arm64-20/.build/python"; do
  [ -e "${ROOT_DIR}/${required}" ] || {
    echo "Missing required offline asset: ${ROOT_DIR}/${required}" >&2
    exit 1
  }
done

if [ "${INCLUDE_NODE_MODULES}" = "1" ]; then
  [ -d "${ROOT_DIR}/node_modules" ] || {
    echo "Missing ${ROOT_DIR}/node_modules; run one online build first or set INCLUDE_NODE_MODULES=0." >&2
    exit 1
  }
  entries+=("node_modules")
fi

if [ "${INCLUDE_FRONTEND_DIST}" = "1" ]; then
  [ -d "${ROOT_DIR}/frontend/dist" ] || {
    echo "Missing ${ROOT_DIR}/frontend/dist; run one online build first or set INCLUDE_FRONTEND_DIST=0." >&2
    exit 1
  }
  entries+=("frontend/dist")
fi

if [ "${INCLUDE_BRIDGE_BIN}" = "1" ] && [ -f "${ROOT_DIR}/target/arm64-20/release/wunder-desktop-bridge" ]; then
  entries+=("target/arm64-20/release/wunder-desktop-bridge")
fi

mkdir -p "${DIST_DIR}" "${CACHE_ROOT}"
{
  echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "Root: ${ROOT_DIR}"
  echo "Entries:"
  for item in "${entries[@]}"; do
    echo "- ${item}"
  done
} > "${MANIFEST_PATH}"
entries+=("target/arm64-20/.cache/offline-bundle.manifest.txt")

tar -czf "${OUTPUT_PATH}" -C "${ROOT_DIR}" "${entries[@]}"

echo "Exported ARM offline bundle: ${OUTPUT_PATH}"
echo "Import on intranet host with: tar -xzf ${OUTPUT_PATH##*/} -C /path/to/wunder"
