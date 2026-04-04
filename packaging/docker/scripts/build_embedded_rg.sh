#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
ARCH="${ARCH:-$(uname -m 2>/dev/null || true)}"
RG_VERSION="${RG_VERSION:-14.1.1}"
case "${ARCH}" in
  x64|x86_64|amd64|x86)
    TARGET_FLAVOR_DEFAULT="x86-20"
    ;;
  arm64|aarch64)
    TARGET_FLAVOR_DEFAULT="arm64-20"
    ;;
  *)
    TARGET_FLAVOR_DEFAULT="${ARCH}"
    ;;
esac
TARGET_DIR="${TARGET_DIR:-${ROOT_DIR}/target/${TARGET_FLAVOR_DEFAULT}}"
BUILD_ROOT="${BUILD_ROOT:-${TARGET_DIR}/.build/python}"
RG_PREFIX="${RG_PREFIX:-/opt/rg}"
STAGE_DIR="${BUILD_ROOT}/stage"
RG_ROOT="${STAGE_DIR}${RG_PREFIX}"

RG_BIN="${RG_BIN:-$(command -v rg || true)}"
if [ -z "${RG_BIN}" ] || [ ! -x "${RG_BIN}" ]; then
  mkdir -p "${BUILD_ROOT}/downloads" "${BUILD_ROOT}/src"
  case "${ARCH}" in
    arm64|aarch64)
      RG_TARGET="aarch64-unknown-linux-gnu"
      ;;
    x64|x86_64|amd64)
      RG_TARGET="x86_64-unknown-linux-gnu"
      ;;
    *)
      echo "Unsupported ARCH for ripgrep download: ${ARCH}" >&2
      exit 1
      ;;
  esac

  RG_ARCHIVE="ripgrep-${RG_VERSION}-${RG_TARGET}.tar.gz"
  RG_URL="${RG_URL:-https://github.com/BurntSushi/ripgrep/releases/download/${RG_VERSION}/${RG_ARCHIVE}}"
  RG_ARCHIVE_PATH="${BUILD_ROOT}/downloads/${RG_ARCHIVE}"
  RG_SRC_ROOT="${BUILD_ROOT}/src/ripgrep-${RG_VERSION}-${RG_TARGET}"
  if [ ! -f "${RG_ARCHIVE_PATH}" ]; then
    curl -fsSL "${RG_URL}" -o "${RG_ARCHIVE_PATH}"
  fi
  rm -rf "${RG_SRC_ROOT}"
  tar -xzf "${RG_ARCHIVE_PATH}" -C "${BUILD_ROOT}/src"
  RG_BIN="${RG_SRC_ROOT}/rg"
  if [ ! -x "${RG_BIN}" ]; then
    echo "downloaded ripgrep binary missing: ${RG_BIN}" >&2
    exit 1
  fi
fi

mkdir -p "${RG_ROOT}/bin" "${RG_ROOT}/lib"

cp -f "${RG_BIN}" "${RG_ROOT}/bin/rg"
chmod +x "${RG_ROOT}/bin/rg"

ldd "${RG_ROOT}/bin/rg" 2>/dev/null \
  | awk '/=> \// {print $3} /^\/lib/ {print $1}' \
  | sed '/^$/d' \
  | sort -u \
  | while IFS= read -r lib_file; do
      if [ -f "${lib_file}" ]; then
        cp -L "${lib_file}" "${RG_ROOT}/lib/"
      fi
    done

for doc_name in LICENSE-MIT UNLICENSE README.md COPYING LICENSE; do
  if [ -f "$(dirname "${RG_BIN}")/${doc_name}" ]; then
    cp -f "$(dirname "${RG_BIN}")/${doc_name}" "${RG_ROOT}/${doc_name}" || true
  fi
done

"${RG_ROOT}/bin/rg" --version | awk 'NR==1 {print $2}' > "${RG_ROOT}/.wunder-rg-version"

echo "Embedded ripgrep prepared at: ${RG_ROOT}"
