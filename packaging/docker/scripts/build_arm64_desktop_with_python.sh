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
CACHE_ROOT="${TARGET_DIR}/.cache"
NPM_CACHE_DIR="${NPM_CACHE_DIR:-${CACHE_ROOT}/npm}"
ELECTRON_CACHE_DIR="${ELECTRON_CACHE_DIR:-${CACHE_ROOT}/electron}"
ELECTRON_BUILDER_CACHE_DIR="${ELECTRON_BUILDER_CACHE_DIR:-${CACHE_ROOT}/electron-builder}"
NPM_STAMP_FILE="${NPM_STAMP_FILE:-${NPM_CACHE_DIR}/workspace-install.sha256}"
APPIMAGE_COMP="${APPIMAGE_COMP:-gzip}"
VALIDATE_MODULES="${VALIDATE_MODULES:-matplotlib,cartopy,pyproj,shapely,netCDF4,cftime,h5py,cinrad}"
ALLOW_PYTHON_REBUILD="${ALLOW_PYTHON_REBUILD:-0}"
WUNDER_BUILD_OFFLINE="${WUNDER_BUILD_OFFLINE:-0}"
WUNDER_BUILD_FRONTEND="${WUNDER_BUILD_FRONTEND:-1}"
FORCE_NPM_INSTALL="${FORCE_NPM_INSTALL:-0}"
SKIP_NPM_INSTALL="${SKIP_NPM_INSTALL:-0}"
SKIP_ARM_BRIDGE_BUILD="${SKIP_ARM_BRIDGE_BUILD:-0}"

CONTAINER_NPM_CACHE_DIR="/app/target/arm64-20/.cache/npm"
CONTAINER_ELECTRON_CACHE_DIR="/app/target/arm64-20/.cache/electron"
CONTAINER_ELECTRON_BUILDER_CACHE_DIR="/app/target/arm64-20/.cache/electron-builder"

step() {
  echo "$1"
}

die() {
  echo "$1" >&2
  exit 1
}

run_in_container() {
  docker compose -f "${COMPOSE_FILE}" exec -T     -e WUNDER_BUILD_OFFLINE="${WUNDER_BUILD_OFFLINE}"     -e WUNDER_BUILD_FRONTEND="${WUNDER_BUILD_FRONTEND}"     -e FORCE_NPM_INSTALL="${FORCE_NPM_INSTALL}"     -e SKIP_NPM_INSTALL="${SKIP_NPM_INSTALL}"     -e VALIDATE_MODULES="${VALIDATE_MODULES}"     -e APPIMAGE_COMP="${APPIMAGE_COMP}"     -e FORCE_PYTHON_SYNC="${FORCE_PYTHON_SYNC:-0}"     -e ALLOW_PYTHON_REBUILD="${ALLOW_PYTHON_REBUILD}"     -e npm_config_cache="${CONTAINER_NPM_CACHE_DIR}"     -e NPM_CONFIG_CACHE="${CONTAINER_NPM_CACHE_DIR}"     -e ELECTRON_CACHE="${CONTAINER_ELECTRON_CACHE_DIR}"     -e ELECTRON_BUILDER_CACHE="${CONTAINER_ELECTRON_BUILDER_CACHE_DIR}"     "${SERVICE}" bash -s --
}

check_prerequisites() {
  step "[1/8] Checking prerequisites..."
  command -v docker >/dev/null 2>&1 || die "docker is not installed or not in PATH."

  if ! docker image inspect "${IMAGE_NAME}" >/dev/null 2>&1; then
    die "Required image not found: ${IMAGE_NAME}
Please load/pull/build it first, then rerun."
  fi

  mkdir -p "${CARGO_HOME_DIR}" "${DIST_DIR}" "${BUILD_ROOT}" "${NPM_CACHE_DIR}" "${ELECTRON_CACHE_DIR}" "${ELECTRON_BUILDER_CACHE_DIR}"

  # Offline rebuild only works after the first online build has warmed every cache.
  if [ "${WUNDER_BUILD_OFFLINE}" = "1" ]; then
    [ -d "${CARGO_HOME_DIR}/registry" ] || die "Offline mode requires ${CARGO_HOME_DIR}/registry. Run one online build first."
    [ -d "${NPM_CACHE_DIR}" ] || die "Offline mode requires ${NPM_CACHE_DIR}. Run one online build first."
    [ -d "${ELECTRON_CACHE_DIR}" ] || die "Offline mode requires ${ELECTRON_CACHE_DIR}. Run one online build first."
    [ -d "${ELECTRON_BUILDER_CACHE_DIR}" ] || die "Offline mode requires ${ELECTRON_BUILDER_CACHE_DIR}. Run one online build first."
  fi

  if [ "${WUNDER_BUILD_FRONTEND}" != "1" ] && [ ! -d "${ROOT_DIR}/frontend/dist" ]; then
    die "Missing frontend build output: ${ROOT_DIR}/frontend/dist
Set WUNDER_BUILD_FRONTEND=1 to build it automatically."
  fi

  if [ ! -x "${BUILD_ROOT}/stage/opt/git/bin/git" ]; then
    echo "Prebuilt embedded Git not found at ${BUILD_ROOT}/stage/opt/git/bin/git."
    echo "Will prepare it automatically during AppImage repack."
  fi
}

start_container() {
  step "[2/8] Starting arm build container (no image rebuild)..."
  docker compose -f "${COMPOSE_FILE}" --profile arm up -d --no-build
}

prepare_workspace_and_frontend() {
  step "[3/8] Preparing workspace dependencies and frontend dist..."
  run_in_container <<'EOF'
set -euo pipefail

export PATH=/usr/local/cargo/bin:$PATH
export CARGO_HOME=/app/.cargo/arm64-20
export CARGO_TARGET_DIR=/app/target/arm64-20

mkdir -p "$npm_config_cache" "$ELECTRON_CACHE" "$ELECTRON_BUILDER_CACHE"

lock_hash="$(sha256sum /app/package-lock.json | awk '{print $1}')"
stamp_file="/app/target/arm64-20/.cache/npm/workspace-install.sha256"
need_install=0

# Keep installs deterministic but do not force a re-install on every rebuild.
if [ ! -d /app/node_modules ]   || [ ! -d /app/node_modules/electron ]   || [ ! -d /app/node_modules/electron-builder ]   || [ ! -d /app/node_modules/vite ]; then
  need_install=1
elif [ ! -f "$stamp_file" ] || [ "$(cat "$stamp_file")" != "$lock_hash" ]; then
  need_install=1
fi

if [ "${SKIP_NPM_INSTALL:-0}" = "1" ] && [ "$need_install" = "1" ]; then
  echo "Root node_modules or npm cache is not ready for skip mode." >&2
  echo "Run one online build first, or unset SKIP_NPM_INSTALL." >&2
  exit 1
fi

if [ "${FORCE_NPM_INSTALL:-0}" = "1" ] || [ "$need_install" = "1" ]; then
  echo "[js] Installing workspace dependencies into /app/node_modules ..."
  cd /app
  if [ "${WUNDER_BUILD_OFFLINE:-0}" = "1" ]; then
    npm install --offline --prefer-offline --no-audit --no-fund --include-workspace-root=false       --workspace wunder-frontend       --workspace wunder-desktop-electron
  else
    npm install --prefer-offline --no-audit --no-fund --include-workspace-root=false       --workspace wunder-frontend       --workspace wunder-desktop-electron
  fi
  printf '%s
' "$lock_hash" > "$stamp_file"
else
  echo "[js] Reusing existing /app/node_modules and warmed npm cache."
fi

if [ "${WUNDER_BUILD_FRONTEND:-1}" = "1" ]; then
  echo "[js] Building frontend/dist from current source ..."
  cd /app
  npm run build --workspace wunder-frontend
elif [ ! -d /app/frontend/dist ]; then
  echo "frontend/dist is missing and WUNDER_BUILD_FRONTEND=0." >&2
  exit 1
else
  echo "[js] Reusing existing frontend/dist."
fi
EOF
}

build_bridge() {
  if [ "${SKIP_ARM_BRIDGE_BUILD}" = "1" ]; then
    [ -x "${TARGET_DIR}/release/wunder-desktop-bridge" ] || die "SKIP_ARM_BRIDGE_BUILD=1 but ${TARGET_DIR}/release/wunder-desktop-bridge is missing."
    step "[4/8] Reusing existing arm64 bridge binary."
    return
  fi

  step "[4/8] Building arm64 bridge with arm64-20 cache/target..."
  run_in_container <<'EOF'
set -euo pipefail

export PATH=/usr/local/cargo/bin:$PATH
export CARGO_HOME=/app/.cargo/arm64-20
export CARGO_TARGET_DIR=/app/target/arm64-20
if [ "${WUNDER_BUILD_OFFLINE:-0}" = "1" ]; then
  export CARGO_NET_OFFLINE=true
fi
cargo build --release --locked --bin wunder-desktop-bridge
EOF
}

package_electron() {
  step "[5/8] Packaging Electron arm64 AppImage..."
  run_in_container <<'EOF'
set -euo pipefail

cd /app/desktop/electron
WUNDER_BRIDGE_BIN=/app/target/arm64-20/release/wunder-desktop-bridge WUNDER_FRONTEND_DIST=/app/frontend/dist   npm run build:linux:arm64 -- --config.directories.output=/app/target/arm64-20/dist
EOF
}

validate_or_rebuild_python() {
  if run_in_container <<'EOF'
set -euo pipefail

python_bin=/app/target/arm64-20/.build/python/stage/opt/python/bin/python3
[ -x "$python_bin" ] || exit 1
"$python_bin" - <<'PY'
import importlib
import os

modules = [item.strip() for item in os.environ.get("VALIDATE_MODULES", "").split(",") if item.strip()]
for name in modules:
    importlib.import_module(name)
PY
EOF
  then
    step "[6/8] Embedded Python runtime already ready; skipping rebuild."
    return
  fi

  if [ "${FORCE_PYTHON_SYNC:-0}" = "1" ] || [ "${ALLOW_PYTHON_REBUILD}" = "1" ]; then
    step "[6/8] Rebuilding embedded Python runtime by explicit request..."
    run_in_container <<'EOF'
set -euo pipefail
BUILD_ROOT=/app/target/arm64-20/.build/python   bash /app/packaging/docker/scripts/build_embedded_python.sh
EOF
    return
  fi

  die "[6/8] Embedded Python runtime missing or invalid.
Expected prebuilt runtime under: ${BUILD_ROOT}/stage/opt
Required files:
  - ${BUILD_ROOT}/stage/opt/python/bin/python3
  - ${BUILD_ROOT}/stage/opt/git/bin/git
Unpack your sidecar backup so the extracted root contains opt/python and opt/git.
Python rebuild is disabled by default. Set ALLOW_PYTHON_REBUILD=1 or FORCE_PYTHON_SYNC=1 to rebuild explicitly."
}

package_sidecar_archive() {
  step "[7/8] Packaging extra sidecar archive..."
  run_in_container <<'EOF'
set -euo pipefail
BUILD_ROOT=/app/target/arm64-20/.build/python OUTPUT_DIR=/app/target/arm64-20/dist   bash /app/packaging/docker/scripts/package_sidecar_python.sh
EOF
}

repack_sidecar_appimage() {
  step "[8/8] Repacking AppImage for sidecar Python/Git runtime (qemu may take 10-30 min)..."
  run_in_container <<'EOF'
set -euo pipefail

output_dir=/app/target/arm64-20/dist
base_appimage="${output_dir}/wunder-desktop-arm64.AppImage"
src_appimage="$(ls -1t "${output_dir}"/*.AppImage 2>/dev/null   | grep -v "python"   | grep -v "sidecar"   | grep -v "/wunder-desktop-arm64.AppImage$"   | head -n 1 || true)"
if [ -n "${src_appimage}" ]; then
  cp -f "${src_appimage}" "${base_appimage}"
elif [ ! -f "${base_appimage}" ]; then
  echo "No base AppImage found under ${output_dir}" >&2
  exit 1
fi

ARCH=arm64 APPIMAGE_PATH="${base_appimage}" BUILD_ROOT=/app/target/arm64-20/.build/python APPIMAGE_WORK=/app/target/arm64-20/.build/python/appimage OUTPUT_DIR="${output_dir}" PREFER_PREBUILT_PYTHON=1 PREFER_PREBUILT_GIT=1 EMBED_PYTHON=0 EMBED_GIT=0 BUNDLE_PLAYWRIGHT_DEPS=0 PLAYWRIGHT_INSTALL_DEPS=0 APPIMAGE_COMP="${APPIMAGE_COMP}"   bash /app/packaging/docker/scripts/package_appimage_with_python.sh
EOF
}

print_artifacts() {
  step "[done] Artifacts:"
  echo "  - ${TARGET_DIR}/release/wunder-desktop-bridge"
  echo "  - ${DIST_DIR}/wunder-desktop-arm64.AppImage"
  echo "  - ${DIST_DIR}/wunder-desktop-arm64-sidecar.AppImage (sidecar Python/Git)"
  sidecar_archive=$(find "${DIST_DIR}" -maxdepth 1 -type f -name '*.tar.gz' | head -n 1 || true)
  if [ -n "${sidecar_archive}" ]; then
    echo "  - ${sidecar_archive} (sidecar extra package)"
  else
    echo "  - ${DIST_DIR}/*.tar.gz (sidecar extra package)"
  fi
  echo "  - ${CACHE_ROOT} (repo-local npm/electron/electron-builder cache)"
}

check_prerequisites
start_container
prepare_workspace_and_frontend
build_bridge
package_electron
validate_or_rebuild_python
package_sidecar_archive
repack_sidecar_appimage
print_artifacts
