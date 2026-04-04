#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
ARCH="${ARCH:-$(uname -m 2>/dev/null || true)}"
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
  echo "rg executable not found in PATH" >&2
  exit 1
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

"${RG_ROOT}/bin/rg" --version | awk 'NR==1 {print $2}' > "${RG_ROOT}/.wunder-rg-version"

echo "Embedded ripgrep prepared at: ${RG_ROOT}"
