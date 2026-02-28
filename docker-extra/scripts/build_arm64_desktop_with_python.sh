#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
COMPOSE_FILE="${ROOT_DIR}/docker-extra/docker-compose-ubuntu20.yml"
SERVICE="wunder-build-arm"
IMAGE_NAME="${IMAGE_NAME:-wunder-arm-20:latest}"

CARGO_HOME_DIR="${ROOT_DIR}/.cargo/arm64-20"
TARGET_DIR="${ROOT_DIR}/target/arm64-20"
DIST_DIR="${TARGET_DIR}/dist"
BUILD_ROOT="${TARGET_DIR}/.build/python"

echo "[1/6] Checking prerequisites..."
if ! command -v docker >/dev/null 2>&1; then
  echo "docker is not installed or not in PATH." >&2
  exit 1
fi
if ! docker image inspect "${IMAGE_NAME}" >/dev/null 2>&1; then
  echo "Required image not found: ${IMAGE_NAME}" >&2
  echo "Please load/pull/build it first, then rerun." >&2
  exit 1
fi
if [ ! -d "${ROOT_DIR}/frontend/dist" ]; then
  echo "Missing frontend build output: ${ROOT_DIR}/frontend/dist" >&2
  echo "Run frontend build first." >&2
  exit 1
fi
if [ ! -x "${BUILD_ROOT}/stage/opt/python/bin/python3" ]; then
  echo "Missing prebuilt embedded Python: ${BUILD_ROOT}/stage/opt/python/bin/python3" >&2
  echo "Please prepare target/arm64-20/.build/python first." >&2
  exit 1
fi

mkdir -p "${CARGO_HOME_DIR}" "${TARGET_DIR}" "${DIST_DIR}"

echo "[2/6] Starting arm build container (no image rebuild)..."
docker compose -f "${COMPOSE_FILE}" --profile arm up -d --no-build

echo "[3/6] Building arm64 bridge with arm64-20 cache/target..."
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc "
  set -euo pipefail
  export PATH=/usr/local/cargo/bin:\$PATH
  export CARGO_HOME=/app/.cargo/arm64-20
  export CARGO_TARGET_DIR=/app/target/arm64-20
  cargo build --release --bin wunder-desktop-bridge
"

echo "[4/6] Packaging Electron arm64 AppImage..."
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc "
  set -euo pipefail
  cd /app/wunder-desktop-electron
  npm install
  WUNDER_BRIDGE_BIN=/app/target/arm64-20/release/wunder-desktop-bridge \
    npm run build:linux:arm64 -- --config.directories.output=/app/target/arm64-20/dist
"

echo "[5/6] Repacking AppImage with embedded Python from arm64-20 (qemu may take 10-30 min)..."
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc '
  set -euo pipefail
  output_dir=/app/target/arm64-20/dist
  base_appimage="${output_dir}/wunder-desktop-arm64.AppImage"
  src_appimage="$(ls -1t "${output_dir}"/*.AppImage 2>/dev/null \
    | grep -v "python" \
    | grep -v "/wunder-desktop-arm64.AppImage$" \
    | head -n 1 || true)"
  if [ -n "${src_appimage}" ]; then
    cp -f "${src_appimage}" "${base_appimage}"
  elif [ ! -f "${base_appimage}" ]; then
    echo "No base AppImage found under ${output_dir}" >&2
    exit 1
  fi
  ARCH=arm64 \
  APPIMAGE_PATH="${base_appimage}" \
  BUILD_ROOT=/app/target/arm64-20/.build/python \
  APPIMAGE_WORK=/app/target/arm64-20/.build/python/appimage \
  OUTPUT_DIR="${output_dir}" \
  PREFER_PREBUILT_PYTHON=1 \
    bash /app/docker-extra/scripts/package_appimage_with_python.sh
'

echo "[6/6] Done. Artifacts:"
echo "  - ${TARGET_DIR}/release/wunder-desktop-bridge"
echo "  - ${DIST_DIR}/wunder-desktop-arm64.AppImage"
echo "  - ${DIST_DIR}/wunder-desktop-arm64-python.AppImage"
