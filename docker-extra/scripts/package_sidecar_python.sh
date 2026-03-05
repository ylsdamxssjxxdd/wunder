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
FONTS_DIR="${FONTS_DIR:-${ROOT_DIR}/fonts}"
FONT_LIST="${FONT_LIST:-}"

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

PY_VER=""
if [ -f "${PYTHON_ROOT}/.wunder-python-version" ]; then
  PY_VER="$(cat "${PYTHON_ROOT}/.wunder-python-version" | tr -d '\r\n')"
fi
if [ -z "${PY_VER}" ]; then
  PY_VER=$(ls -1d "${PYTHON_ROOT}/lib/python3"* 2>/dev/null | head -n 1 | xargs -I{} basename {} | sed 's/^python//')
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

if command -v zstd >/dev/null 2>&1; then
  tar -C "${STAGE_DIR}" --transform "s,^opt,${PACKAGE_DIR_NAME}/opt," \
    -I 'zstd -19 -T0' -cf "${OUTPUT_DIR}/${OUT_NAME}" "${ITEMS[@]}"
else
  OUT_NAME="${OUT_NAME%.tar.zst}.tar.gz"
  tar -C "${STAGE_DIR}" --transform "s,^opt,${PACKAGE_DIR_NAME}/opt," \
    -czf "${OUTPUT_DIR}/${OUT_NAME}" "${ITEMS[@]}"
fi

echo "Sidecar extra package: ${OUTPUT_DIR}/${OUT_NAME}"
