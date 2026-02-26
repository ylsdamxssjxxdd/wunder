#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PYTHON_VERSION="${PYTHON_VERSION:-3.11.5}"
PYTHON_PREFIX="${PYTHON_PREFIX:-/opt/python}"
BUILD_ROOT="${BUILD_ROOT:-${ROOT_DIR}/.build/python}"
SRC_DIR="${BUILD_ROOT}/src"
STAGE_DIR="${BUILD_ROOT}/stage"
PYTHON_ROOT="${STAGE_DIR}${PYTHON_PREFIX}"
REQ_FILE="${REQ_FILE:-${ROOT_DIR}/packaging/python/requirements-full.txt}"
WHEELHOUSE_DIR="${WHEELHOUSE_DIR:-${BUILD_ROOT}/wheelhouse}"
# Allow source fallback for a small set of pure-python packages without wheels on arm64.
SOURCE_FALLBACK_PACKAGES="${SOURCE_FALLBACK_PACKAGES:-odfpy}"

mkdir -p "${SRC_DIR}" "${STAGE_DIR}" "${WHEELHOUSE_DIR}"

if [ ! -x "${PYTHON_ROOT}/bin/python3" ]; then
  TARBALL="${SRC_DIR}/Python-${PYTHON_VERSION}.tgz"
  if [ ! -f "${TARBALL}" ]; then
    curl -fsSL "https://www.python.org/ftp/python/${PYTHON_VERSION}/Python-${PYTHON_VERSION}.tgz" -o "${TARBALL}"
  fi
  rm -rf "${SRC_DIR}/Python-${PYTHON_VERSION}"
  tar -xzf "${TARBALL}" -C "${SRC_DIR}"
  pushd "${SRC_DIR}/Python-${PYTHON_VERSION}" >/dev/null
  ./configure \
    --prefix="${PYTHON_PREFIX}" \
    --enable-shared \
    --with-ensurepip=install
  make -j"$(nproc)"
  make install DESTDIR="${STAGE_DIR}"
  popd >/dev/null
  if command -v patchelf >/dev/null 2>&1; then
    patchelf --set-rpath '$ORIGIN/../lib' "${PYTHON_ROOT}/bin/python3" || true
  fi
fi

export PYTHONHOME="${PYTHON_ROOT}"
export LD_LIBRARY_PATH="${PYTHON_ROOT}/lib:${LD_LIBRARY_PATH:-}"

"${PYTHON_ROOT}/bin/python3" -m pip install --upgrade pip setuptools wheel
"${PYTHON_ROOT}/bin/python3" -m pip download setuptools wheel -d "${WHEELHOUSE_DIR}" --only-binary=:all:
if [ -n "${SOURCE_FALLBACK_PACKAGES}" ]; then
  "${PYTHON_ROOT}/bin/python3" -m pip download -r "${REQ_FILE}" -d "${WHEELHOUSE_DIR}" \
    --only-binary=:all: \
    --no-binary "${SOURCE_FALLBACK_PACKAGES}"
else
  "${PYTHON_ROOT}/bin/python3" -m pip download -r "${REQ_FILE}" -d "${WHEELHOUSE_DIR}" --only-binary=:all:
fi
"${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation -r "${REQ_FILE}"

PY_VER=$("${PYTHON_ROOT}/bin/python3" - <<'PY'
import sys
print(f"{sys.version_info.major}.{sys.version_info.minor}")
PY
)
echo "${PY_VER}" > "${PYTHON_ROOT}/.wunder-python-version"

find "${PYTHON_ROOT}" -type d -name '__pycache__' -prune -exec rm -rf {} +
find "${PYTHON_ROOT}" -type f -name '*.pyc' -delete
