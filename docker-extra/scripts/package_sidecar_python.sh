#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
ARCH="${ARCH:-arm64}"
BUILD_ROOT="${BUILD_ROOT:-${ROOT_DIR}/target/${ARCH}/.build/python}"
STAGE_DIR="${STAGE_DIR:-${BUILD_ROOT}/stage}"
PYTHON_ROOT="${PYTHON_ROOT:-${STAGE_DIR}/opt/python}"
GIT_ROOT="${GIT_ROOT:-${STAGE_DIR}/opt/git}"
OUTPUT_DIR="${OUTPUT_DIR:-${ROOT_DIR}/target/${ARCH}/dist}"
PACKAGE_DIR_NAME="${PACKAGE_DIR_NAME:-wunder补充包}"
OUT_NAME="${OUT_NAME:-${PACKAGE_DIR_NAME}-${ARCH}.tar.zst}"
INCLUDE_GIT="${INCLUDE_GIT:-1}"

if [ ! -d "${PYTHON_ROOT}" ]; then
  echo "Embedded Python root not found: ${PYTHON_ROOT}" >&2
  exit 1
fi

mkdir -p "${OUTPUT_DIR}"

if [ ! -d "${STAGE_DIR}" ]; then
  echo "Embedded stage root not found: ${STAGE_DIR}" >&2
  exit 1
fi

ITEMS=("opt/python")
if [ "${INCLUDE_GIT}" = "1" ]; then
  if [ ! -d "${GIT_ROOT}" ]; then
    "${ROOT_DIR}/docker-extra/scripts/build_embedded_git.sh"
  fi
  if [ -d "${GIT_ROOT}" ]; then
    ITEMS+=("opt/git")
  else
    echo "Embedded Git root not found: ${GIT_ROOT}" >&2
    exit 1
  fi
fi

if command -v zstd >/dev/null 2>&1; then
  tar -C "${STAGE_DIR}" --transform "s,^opt,${PACKAGE_DIR_NAME}/opt," \
    -I 'zstd -19 -T0' -cf "${OUTPUT_DIR}/${OUT_NAME}" "${ITEMS[@]}"
else
  OUT_NAME="${OUT_NAME%.tar.zst}.tar.gz"
  tar -C "${STAGE_DIR}" --transform "s,^opt,${PACKAGE_DIR_NAME}/opt," \
    -czf "${OUTPUT_DIR}/${OUT_NAME}" "${ITEMS[@]}"
fi

echo "Sidecar extra package: ${OUTPUT_DIR}/${OUT_NAME}"
