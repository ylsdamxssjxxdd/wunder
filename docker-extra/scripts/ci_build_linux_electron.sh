#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${ROOT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
ARCH="${ARCH:-x64}"
OUTPUT_DIR="${OUTPUT_DIR:-${ROOT_DIR}/target/nightly/linux-${ARCH}}"
CARGO_HOME="${CARGO_HOME:-${ROOT_DIR}/.cargo/ci-linux-${ARCH}}"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT_DIR}/target/ci-linux-${ARCH}}"
NPM_CACHE_DIR="${NPM_CACHE_DIR:-${ROOT_DIR}/.npm-cache/linux-${ARCH}}"
NIGHTLY_VERSION="${NIGHTLY_VERSION:-}"

case "${ARCH}" in
  x64)
    BUILD_ARCH_ARG="--x64"
    ;;
  arm64)
    BUILD_ARCH_ARG="--arm64"
    ;;
  *)
    echo "Unsupported ARCH: ${ARCH}" >&2
    exit 1
    ;;
esac

export PATH="/usr/local/cargo/bin:${PATH}"
export CARGO_HOME
export CARGO_TARGET_DIR
export NPM_CONFIG_CACHE="${NPM_CONFIG_CACHE:-${NPM_CACHE_DIR}}"

rm -rf "${OUTPUT_DIR}"
mkdir -p "${OUTPUT_DIR}" "${CARGO_HOME}" "${CARGO_TARGET_DIR}" "${NPM_CONFIG_CACHE}"

if [ ! -d "${ROOT_DIR}/frontend/dist" ]; then
  echo "frontend/dist is missing, building frontend..."
  pushd "${ROOT_DIR}/frontend" >/dev/null
  npm ci
  npm run build
  popd >/dev/null
else
  echo "Using existing frontend/dist."
fi

echo "Building bridge binary..."
cargo build --release --bin wunder-desktop-bridge
BRIDGE_BIN="${CARGO_TARGET_DIR}/release/wunder-desktop-bridge"
if [ ! -x "${BRIDGE_BIN}" ]; then
  echo "Bridge binary not found: ${BRIDGE_BIN}" >&2
  exit 1
fi

echo "Building Electron AppImage (${ARCH})..."
pushd "${ROOT_DIR}/wunder-desktop-electron" >/dev/null
npm ci
WUNDER_BRIDGE_BIN="${BRIDGE_BIN}" npm run prepare:resources
extra_metadata_args=()
if [ -n "${NIGHTLY_VERSION}" ]; then
  extra_metadata_args+=(--config.extraMetadata.version="${NIGHTLY_VERSION}")
fi
npx electron-builder --linux "${BUILD_ARCH_ARG}" --publish=never --config.directories.output="${OUTPUT_DIR}" "${extra_metadata_args[@]}"
popd >/dev/null

echo "Linux Electron build completed. Output: ${OUTPUT_DIR}"
