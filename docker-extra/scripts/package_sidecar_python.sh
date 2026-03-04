#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
ARCH="${ARCH:-arm64}"
BUILD_ROOT="${BUILD_ROOT:-${ROOT_DIR}/target/${ARCH}/.build/python}"
PYTHON_ROOT="${PYTHON_ROOT:-${BUILD_ROOT}/stage/opt/python}"
OUTPUT_DIR="${OUTPUT_DIR:-${ROOT_DIR}/target/${ARCH}/dist}"
PACKAGE_DIR_NAME="${PACKAGE_DIR_NAME:-wunder-python}"
OUT_NAME="${OUT_NAME:-${PACKAGE_DIR_NAME}-${ARCH}.tar.zst}"

if [ ! -d "${PYTHON_ROOT}" ]; then
  echo "Embedded Python root not found: ${PYTHON_ROOT}" >&2
  exit 1
fi

mkdir -p "${OUTPUT_DIR}"

SRC_BASE=$(dirname "${PYTHON_ROOT}")
SRC_NAME=$(basename "${PYTHON_ROOT}")

if command -v zstd >/dev/null 2>&1; then
  tar -C "${SRC_BASE}" --transform "s,^${SRC_NAME},${PACKAGE_DIR_NAME}," \
    -I 'zstd -19 -T0' -cf "${OUTPUT_DIR}/${OUT_NAME}" "${SRC_NAME}"
else
  OUT_NAME="${OUT_NAME%.tar.zst}.tar.gz"
  tar -C "${SRC_BASE}" --transform "s,^${SRC_NAME},${PACKAGE_DIR_NAME}," \
    -czf "${OUTPUT_DIR}/${OUT_NAME}" "${SRC_NAME}"
fi

echo "Sidecar Python package: ${OUTPUT_DIR}/${OUT_NAME}"
