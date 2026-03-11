#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
COMPOSE_FILE="${ROOT_DIR}/packaging/docker/docker-compose-ubuntu20.yml"
SERVICE="wunder-build-arm"
IMAGE_NAME="${IMAGE_NAME:-wunder-arm-20:latest}"

CARGO_HOME_DIR="${ROOT_DIR}/.cargo/arm64-20"
TARGET_DIR="${ROOT_DIR}/target/arm64-20"
DIST_DIR="${TARGET_DIR}/dist"
BUILD_ROOT="${TARGET_DIR}/.build/python"
APPIMAGE_COMP="${APPIMAGE_COMP:-gzip}"
VALIDATE_MODULES="${VALIDATE_MODULES:-matplotlib,cartopy,pyproj,shapely,netCDF4,cftime,h5py,cinrad}"
ALLOW_PYTHON_REBUILD="${ALLOW_PYTHON_REBUILD:-0}"

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
  cd /app
  npm install --prefer-offline --no-audit --no-fund --workspace wunder-desktop-electron
  cd /app/desktop/electron
  WUNDER_BRIDGE_BIN=/app/target/arm64-20/release/wunder-desktop-bridge \
    npm run build:linux:arm64 -- --config.directories.output=/app/target/arm64-20/dist
"

if docker compose -f "${COMPOSE_FILE}" exec -T \
  -e VALIDATE_MODULES="${VALIDATE_MODULES}" \
  "${SERVICE}" bash -lc '
    set -euo pipefail
    python_bin=/app/target/arm64-20/.build/python/stage/opt/python/bin/python3
    [ -x "${python_bin}" ] || exit 1
    "${python_bin}" - <<'"'"'PY'"'"'
import importlib
import os

modules = [item.strip() for item in os.environ.get("VALIDATE_MODULES", "").split(",") if item.strip()]
for name in modules:
    importlib.import_module(name)
PY
  '; then
  echo "[5/8] Embedded Python runtime already ready; skipping rebuild."
elif [ "${FORCE_PYTHON_SYNC:-0}" = "1" ] || [ "${ALLOW_PYTHON_REBUILD}" = "1" ]; then
  echo "[5/8] Rebuilding embedded Python runtime by explicit request..."
  docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc "
    set -euo pipefail
    BUILD_ROOT=/app/target/arm64-20/.build/python \
      bash /app/packaging/docker/scripts/build_embedded_python.sh
  "
else
  echo "[5/8] Embedded Python runtime missing or invalid." >&2
  echo "Expected prebuilt runtime under: ${BUILD_ROOT}/stage/opt" >&2
  echo "Required files:" >&2
  echo "  - ${BUILD_ROOT}/stage/opt/python/bin/python3" >&2
  echo "  - ${BUILD_ROOT}/stage/opt/git/bin/git" >&2
  echo "Unpack your sidecar backup so the extracted root contains opt/python and opt/git." >&2
  echo "Python rebuild is disabled by default. Set ALLOW_PYTHON_REBUILD=1 or FORCE_PYTHON_SYNC=1 to rebuild explicitly." >&2
  exit 1
fi

echo "[6/8] Packaging extra sidecar archive..."
docker compose -f "${COMPOSE_FILE}" exec -T "${SERVICE}" bash -lc "
  set -euo pipefail
  BUILD_ROOT=/app/target/arm64-20/.build/python \
  OUTPUT_DIR=/app/target/arm64-20/dist \
    bash /app/packaging/docker/scripts/package_sidecar_python.sh
"

echo "[7/8] Repacking AppImage for sidecar Python/Git runtime (qemu may take 10-30 min)..."
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
  EMBED_GIT=0 \
  BUNDLE_PLAYWRIGHT_DEPS=0 \
  PLAYWRIGHT_INSTALL_DEPS=0 \
  APPIMAGE_COMP="${APPIMAGE_COMP}" \
    bash /app/packaging/docker/scripts/package_appimage_with_python.sh
'

echo "[8/8] Done. Artifacts:"
echo "  - ${TARGET_DIR}/release/wunder-desktop-bridge"
echo "  - ${DIST_DIR}/wunder-desktop-arm64.AppImage"
echo "  - ${DIST_DIR}/wunder-desktop-arm64-sidecar.AppImage (sidecar Python/Git)"
SIDECAR_ARCHIVE=$(find "${DIST_DIR}" -maxdepth 1 -type f -name '*.tar.gz' | head -n 1 || true)
if [ -n "${SIDECAR_ARCHIVE}" ]; then
  echo "  - ${SIDECAR_ARCHIVE} (sidecar extra package)"
else
  echo "  - ${DIST_DIR}/*.tar.gz (sidecar extra package)"
fi

