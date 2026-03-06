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
SOURCE_FALLBACK_PACKAGES="${SOURCE_FALLBACK_PACKAGES:-odfpy,cinrad}"
EXTRA_REQUIREMENTS="${EXTRA_REQUIREMENTS:-}"
INCLUDE_PLAYWRIGHT="${INCLUDE_PLAYWRIGHT:-0}"
PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH:-${PYTHON_ROOT}/playwright}"
CARTOPY_DATA_DIR="${CARTOPY_DATA_DIR:-${PYTHON_ROOT}/share/cartopy}"
CARTOPY_DATA_LEVELS="${CARTOPY_DATA_LEVELS:-110m,50m,10m}"
CARTOPY_FEATURES="${CARTOPY_FEATURES:-coastline,land,ocean,lakes,rivers_lake_centerlines,admin_0_boundary_lines_land,admin_0_countries}"
CARTOPY_DOWNLOAD="${CARTOPY_DOWNLOAD:-1}"

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
"${PYTHON_ROOT}/bin/python3" -m pip download numpy -d "${WHEELHOUSE_DIR}" --only-binary=:all:
"${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation numpy
"${PYTHON_ROOT}/bin/python3" -m pip download Cython -d "${WHEELHOUSE_DIR}" --only-binary=:all:
"${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation Cython
if [ -n "${SOURCE_FALLBACK_PACKAGES}" ]; then
  "${PYTHON_ROOT}/bin/python3" -m pip download -r "${REQ_FILE}" -d "${WHEELHOUSE_DIR}" \
    --only-binary=:all: \
    --no-binary "${SOURCE_FALLBACK_PACKAGES}" \
    --no-build-isolation
else
  "${PYTHON_ROOT}/bin/python3" -m pip download -r "${REQ_FILE}" -d "${WHEELHOUSE_DIR}" --only-binary=:all: --no-build-isolation
fi
"${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation -r "${REQ_FILE}"

if [ "${INCLUDE_PLAYWRIGHT}" = "1" ]; then
  EXTRA_REQUIREMENTS="${EXTRA_REQUIREMENTS} playwright"
fi

if [ -n "${EXTRA_REQUIREMENTS}" ]; then
  "${PYTHON_ROOT}/bin/python3" -m pip download ${EXTRA_REQUIREMENTS} -d "${WHEELHOUSE_DIR}" --only-binary=:all:
  "${PYTHON_ROOT}/bin/python3" -m pip install --no-index --find-links "${WHEELHOUSE_DIR}" --no-build-isolation ${EXTRA_REQUIREMENTS}
fi

if [ "${CARTOPY_DOWNLOAD}" = "1" ]; then
  export CARTOPY_DATA_DIR
  export CARTOPY_DATA_LEVELS
  export CARTOPY_FEATURES
  "${PYTHON_ROOT}/bin/python3" - <<'PY'
import os
import sys

data_dir = os.environ.get("CARTOPY_DATA_DIR")
levels = os.environ.get("CARTOPY_DATA_LEVELS", "")
features = os.environ.get("CARTOPY_FEATURES", "")

if not data_dir:
    sys.exit(0)

os.makedirs(data_dir, exist_ok=True)

try:
    import cartopy
    from cartopy import config as cartopy_config
    from cartopy.io import shapereader
except Exception as exc:
    print(f"[cartopy] not available: {exc}", file=sys.stderr)
    sys.exit(1)

cartopy_config["data_dir"] = data_dir

levels_list = [x.strip() for x in levels.split(",") if x.strip()]
features_list = [x.strip() for x in features.split(",") if x.strip()]

if not levels_list or not features_list:
    sys.exit(0)

def category_for(name: str) -> str:
    if name.startswith("admin_") or name.endswith("_countries") or name.endswith("_states_provinces"):
        return "cultural"
    return "physical"

errors = []
for level in levels_list:
    for name in features_list:
        category = category_for(name)
        try:
            shapereader.natural_earth(resolution=level, category=category, name=name)
        except Exception as exc:
            errors.append(f"{level}/{category}/{name}: {exc}")

if errors:
    print("[cartopy] download failed:", file=sys.stderr)
    for item in errors:
        print(f"  - {item}", file=sys.stderr)
    sys.exit(1)
PY
fi

if [ "${INCLUDE_PLAYWRIGHT}" = "1" ]; then
  export PLAYWRIGHT_BROWSERS_PATH
  mkdir -p "${PLAYWRIGHT_BROWSERS_PATH}"
  for cache_dir in \
    "${BUILD_ROOT}/playwright-cache/ms-playwright" \
    "${BUILD_ROOT}/playwright-test/ms-playwright" \
    "${BUILD_ROOT}/ms-playwright"; do
    if [ -d "${cache_dir}" ]; then
      cp -a "${cache_dir}/." "${PLAYWRIGHT_BROWSERS_PATH}/"
      break
    fi
  done
  "${PYTHON_ROOT}/bin/python3" -m playwright install chromium
fi

PY_VER=$("${PYTHON_ROOT}/bin/python3" - <<'PY'
import sys
print(f"{sys.version_info.major}.{sys.version_info.minor}")
PY
)
echo "${PY_VER}" > "${PYTHON_ROOT}/.wunder-python-version"

find "${PYTHON_ROOT}" -type d -name '__pycache__' -prune -exec rm -rf {} +
find "${PYTHON_ROOT}" -type f -name '*.pyc' -delete
