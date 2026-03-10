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
APPIMAGE_COMP="${APPIMAGE_COMP:-auto}"

echo "[1/8] Checking prerequisites..."
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
if [ ! -x "${BUILD_ROOT}/stage/opt/git/bin/git" ]; then
  echo "Prebuilt embedded Git not found at ${BUILD_ROOT}/stage/opt/git/bin/git."
  echo "Will prepare it automatically during AppImage repack."
fi

mkdir -p "${CARGO_HOME_DIR}" "${TARGET_DIR}" "${DIST_DIR}"

echo "[2/8] Starting arm build container (no image rebuild)..."
docker compose -f "${COMPOSE_FILE}" --profile arm up -d --no-build

echo "[3/8] Building arm64 bridge with arm64-20 cache/target..."
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc "
  set -euo pipefail
  export PATH=/usr/local/cargo/bin:\$PATH
  export CARGO_HOME=/app/.cargo/arm64-20
  export CARGO_TARGET_DIR=/app/target/arm64-20
  cargo build --release --bin wunder-desktop-bridge
"

echo "[4/8] Packaging Electron arm64 AppImage..."
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc "
  set -euo pipefail
  cd /app/wunder-desktop-electron
  npm install
  WUNDER_BRIDGE_BIN=/app/target/arm64-20/release/wunder-desktop-bridge \
    npm run build:linux:arm64 -- --config.directories.output=/app/target/arm64-20/dist
"

echo "[5/8] Syncing embedded Python runtime..."
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc "
  set -euo pipefail
  BUILD_ROOT=/app/target/arm64-20/.build/python \
    bash /app/docker-extra/scripts/build_embedded_python.sh
"

echo "[6/8] Packaging extra sidecar archive..."
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc "
  set -euo pipefail
  BUILD_ROOT=/app/target/arm64-20/.build/python \
  OUTPUT_DIR=/app/target/arm64-20/dist \
    bash /app/docker-extra/scripts/package_sidecar_python.sh
"

echo "[7/8] Repacking AppImage with sidecar Python + embedded Git (qemu may take 10-30 min)..."
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc '
  set -euo pipefail
  output_dir=/app/target/arm64-20/dist
  base_appimage="${output_dir}/wunder-desktop-arm64.AppImage"
  src_appimage="$(ls -1t "${output_dir}"/*.AppImage 2>/dev/null \
    | grep -v "python" \
    | grep -v "sidecar" \
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
  PREFER_PREBUILT_GIT=1 \
  EMBED_PYTHON=0 \
  BUNDLE_PLAYWRIGHT_DEPS=0 \
  PLAYWRIGHT_INSTALL_DEPS=0 \
  APPIMAGE_COMP="${APPIMAGE_COMP}" \
    bash /app/docker-extra/scripts/package_appimage_with_python.sh
'

echo "[8/8] Done. Artifacts:"
echo "  - ${TARGET_DIR}/release/wunder-desktop-bridge"
echo "  - ${DIST_DIR}/wunder-desktop-arm64.AppImage"
echo "  - ${DIST_DIR}/wunder-desktop-arm64-sidecar.AppImage (sidecar Python)"
SIDECAR_ARCHIVE=$(find "${DIST_DIR}" -maxdepth 1 -type f -name '*.tar.gz' | head -n 1 || true)
if [ -n "${SIDECAR_ARCHIVE}" ]; then
  echo "  - ${SIDECAR_ARCHIVE} (sidecar extra package)"
else
  echo "  - ${DIST_DIR}/*.tar.gz (sidecar extra package)"
fi
