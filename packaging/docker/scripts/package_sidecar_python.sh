#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
ARCH="${ARCH:-arm64}"
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
STAGE_DIR="${STAGE_DIR:-${BUILD_ROOT}/stage}"
PYTHON_ROOT="${PYTHON_ROOT:-${STAGE_DIR}/opt/python}"
GIT_ROOT="${GIT_ROOT:-${STAGE_DIR}/opt/git}"
OUTPUT_DIR="${OUTPUT_DIR:-${TARGET_DIR}/dist}"
PACKAGE_DIR_NAME="${PACKAGE_DIR_NAME:-wunder补充包}"
OUT_NAME="${OUT_NAME:-${PACKAGE_DIR_NAME}-${ARCH}.tar.zst}"
INCLUDE_GIT="${INCLUDE_GIT:-1}"
DEREFERENCE_SYMLINKS="${DEREFERENCE_SYMLINKS:-1}"
FONTS_DIR="${FONTS_DIR:-${ROOT_DIR}/fonts}"
FONT_LIST="${FONT_LIST:-}"
VALIDATE_MODULES="${VALIDATE_MODULES:-matplotlib,cartopy,pyproj,shapely,netCDF4,cftime,h5py,cinrad}"

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
    BUILD_ROOT="${BUILD_ROOT}" TARGET_DIR="${TARGET_DIR}" ARCH="${ARCH}" \
      "${ROOT_DIR}/packaging/docker/scripts/build_embedded_git.sh"
  fi
  if [ -d "${GIT_ROOT}" ]; then
    ITEMS+=("opt/git")
  else
    echo "Embedded Git root not found: ${GIT_ROOT}" >&2
    exit 1
  fi
fi

PY_VER=""
if [ -f "${PYTHON_ROOT}/.wunder-python-version" ]; then
  PY_VER="$(cat "${PYTHON_ROOT}/.wunder-python-version" | tr -d '\r\n')"
fi
if [ -z "${PY_VER}" ]; then
  PY_VER=$(ls -1d "${PYTHON_ROOT}/lib/python3"* 2>/dev/null | head -n 1 | xargs -I{} basename {} | sed 's/^python//')
fi

if [ -n "${VALIDATE_MODULES}" ]; then
  if ! "${PYTHON_ROOT}/bin/python3" - "${VALIDATE_MODULES}" <<'PY'
import importlib
import sys

raw = sys.argv[1] if len(sys.argv) > 1 else ""
modules = [item.strip() for item in raw.split(",") if item.strip()]
missing = []
for name in modules:
    try:
        importlib.import_module(name)
    except Exception as exc:
        missing.append(f"{name} ({type(exc).__name__}: {exc})")
if missing:
    raise SystemExit(f"missing embedded python modules: {', '.join(missing)}")
PY
  then
    echo "Embedded Python is missing required modules; rerun packaging/docker/scripts/build_embedded_python.sh before packaging." >&2
    exit 1
  fi
fi

if [[ ",${VALIDATE_MODULES}," == *",cartopy," ]] && [ ! -d "${PYTHON_ROOT}/share/cartopy" ]; then
  echo "cartopy module is present but offline data directory is missing: ${PYTHON_ROOT}/share/cartopy" >&2
fi

if [ -n "${PY_VER}" ]; then
  MPL_FONTS_DIR="${PYTHON_ROOT}/lib/python${PY_VER}/site-packages/matplotlib/mpl-data/fonts/ttf"
  if [ -d "${MPL_FONTS_DIR}" ] && [ -d "${FONTS_DIR}" ]; then
    mkdir -p "${MPL_FONTS_DIR}"
    if [ -z "${FONT_LIST}" ]; then
      FONT_FILES=(
        "NotoSansSC-VF.ttf"
        "NotoSerifSC-VF.ttf"
        "msyh.ttc"
        "msyhbd.ttc"
        "simsun.ttc"
        "simhei.ttf"
        "arial.ttf"
        "arialbd.ttf"
        "times.ttf"
        "timesbd.ttf"
        "consola.ttf"
      )
    else
      IFS=',' read -r -a FONT_FILES <<< "${FONT_LIST}"
    fi
    for font in "${FONT_FILES[@]}"; do
      font="$(echo "${font}" | xargs)"
      [ -z "${font}" ] && continue
      if [ -f "${FONTS_DIR}/${font}" ]; then
        cp -f "${FONTS_DIR}/${font}" "${MPL_FONTS_DIR}/"
      else
        echo "Font not found: ${FONTS_DIR}/${font}" >&2
      fi
    done
  fi

  MPL_RC_DIR="${PYTHON_ROOT}/etc"
  MPL_RC_FILE="${MPL_RC_DIR}/matplotlibrc"
  mkdir -p "${MPL_RC_DIR}"
  cat > "${MPL_RC_FILE}" <<'EOF'
font.family : sans-serif
font.sans-serif : Noto Sans SC, Noto Serif SC, Microsoft YaHei, SimHei, SimSun, Arial, DejaVu Sans
font.serif : Noto Serif SC, SimSun, Times New Roman, DejaVu Serif
axes.unicode_minus : False
EOF
fi

TAR_LINK_ARGS=()
if [ "${DEREFERENCE_SYMLINKS}" = "1" ]; then
  TAR_LINK_ARGS=(--dereference)
fi

if command -v zstd >/dev/null 2>&1; then
  tar "${TAR_LINK_ARGS[@]}" -C "${STAGE_DIR}" --transform "s,^opt,${PACKAGE_DIR_NAME}/opt," \
    -I 'zstd -19 -T0' -cf "${OUTPUT_DIR}/${OUT_NAME}" "${ITEMS[@]}"
else
  if [[ "${OUT_NAME}" == *.tar.gz ]]; then
    :
  elif [[ "${OUT_NAME}" == *.tar.zst ]]; then
    OUT_NAME="${OUT_NAME%.tar.zst}.tar.gz"
  else
    OUT_NAME="${OUT_NAME}.tar.gz"
  fi
  tar "${TAR_LINK_ARGS[@]}" -C "${STAGE_DIR}" --transform "s,^opt,${PACKAGE_DIR_NAME}/opt," \
    -czf "${OUTPUT_DIR}/${OUT_NAME}" "${ITEMS[@]}"
fi

if [[ "${OUT_NAME}" == *.tar.gz ]] && command -v gzip >/dev/null 2>&1; then
  gzip -t "${OUTPUT_DIR}/${OUT_NAME}"
fi

echo "Sidecar extra package: ${OUTPUT_DIR}/${OUT_NAME}"
