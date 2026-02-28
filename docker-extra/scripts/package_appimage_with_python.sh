#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
ARCH="${ARCH:-arm64}"
APPIMAGE_DIR="${APPIMAGE_DIR:-${ROOT_DIR}/target/${ARCH}/release/bundle/appimage}"
APPIMAGE_PATH="${APPIMAGE_PATH:-}"
OUTPUT_DIR="${OUTPUT_DIR:-${ROOT_DIR}/dist}"
BUILD_ROOT="${BUILD_ROOT:-${ROOT_DIR}/.build/python}"
APPIMAGE_WORK="${APPIMAGE_WORK:-${BUILD_ROOT}/appimage}"
TOOLS_DIR="${BUILD_ROOT}/tools"
PREFER_PREBUILT_PYTHON="${PREFER_PREBUILT_PYTHON:-1}"
PREBUILT_PYTHON_ROOT="${BUILD_ROOT}/stage/opt/python"

patch_appimage_runtime_magic() {
  local target_file=$1
  dd if=/dev/zero of="${target_file}" bs=1 seek=8 count=3 conv=notrunc >/dev/null 2>&1
}

extract_appimage() {
  local input_path=$1
  local workdir=$2

  rm -rf "${workdir}"
  mkdir -p "${workdir}"
  cp "${input_path}" "${workdir}/app.AppImage"
  chmod +x "${workdir}/app.AppImage"

  pushd "${workdir}" >/dev/null
  if ! ./app.AppImage --appimage-extract >/dev/null 2>&1; then
    echo "Direct AppImage extraction failed; retrying with patched runtime header..." >&2
    cp ./app.AppImage ./app.extract.AppImage
    patch_appimage_runtime_magic ./app.extract.AppImage
    APPIMAGE_EXTRACT_AND_RUN=1 ./app.extract.AppImage --appimage-extract >/dev/null
  fi
  popd >/dev/null
}

resolve_appimagetool_arch() {
  case "${ARCH}" in
    arm64|aarch64)
      echo "aarch64"
      ;;
    armhf)
      echo "armhf"
      ;;
    x86|x86_64|amd64)
      echo "x86_64"
      ;;
    i686|x86-32)
      echo "i686"
      ;;
    *)
      echo "Unsupported ARCH for appimagetool download: ${ARCH}" >&2
      exit 1
      ;;
  esac
}

if [ -z "${APPIMAGE_PATH}" ]; then
  APPIMAGE_PATH=$(ls -1 "${APPIMAGE_DIR}"/*.AppImage 2>/dev/null | head -n 1 || true)
fi

if [ -z "${APPIMAGE_PATH}" ] || [ ! -f "${APPIMAGE_PATH}" ]; then
  echo "AppImage not found under ${APPIMAGE_DIR}." >&2
  exit 1
fi

if [ "${PREFER_PREBUILT_PYTHON}" = "1" ] && [ -x "${PREBUILT_PYTHON_ROOT}/bin/python3" ]; then
  echo "Using prebuilt embedded Python under ${PREBUILT_PYTHON_ROOT}."
else
  "${ROOT_DIR}/docker-extra/scripts/build_embedded_python.sh"
fi

extract_appimage "${APPIMAGE_PATH}" "${APPIMAGE_WORK}"

APPDIR="${APPIMAGE_WORK}/squashfs-root"
if [ ! -d "${APPDIR}" ]; then
  echo "Extracted AppDir not found at ${APPDIR}." >&2
  exit 1
fi
if [ ! -x "${PREBUILT_PYTHON_ROOT}/bin/python3" ]; then
  echo "Embedded Python not found under ${PREBUILT_PYTHON_ROOT}." >&2
  exit 1
fi

mkdir -p "${APPDIR}/opt"
rm -rf "${APPDIR}/opt/python"
cp -a "${PREBUILT_PYTHON_ROOT}" "${APPDIR}/opt/"

if [ -f "${APPDIR}/AppRun" ]; then
  mv "${APPDIR}/AppRun" "${APPDIR}/AppRun.orig"
  cat > "${APPDIR}/AppRun" <<'EOF'
#!/usr/bin/env bash
set -e
HERE="$(dirname "$(readlink -f "$0")")"
export APPDIR="$HERE"
PY_VER="$(cat "$APPDIR/opt/python/.wunder-python-version" 2>/dev/null || echo "3.11")"
export PYTHONHOME="$APPDIR/opt/python"
export PYTHONPATH="$APPDIR/opt/python/lib/python${PY_VER}/site-packages${PYTHONPATH:+:$PYTHONPATH}"
export LD_LIBRARY_PATH="$APPDIR/opt/python/lib:$APPDIR/usr/lib:${LD_LIBRARY_PATH:-}"
if [ -f "$APPDIR/opt/python/lib/python${PY_VER}/site-packages/certifi/cacert.pem" ]; then
  export SSL_CERT_FILE="$APPDIR/opt/python/lib/python${PY_VER}/site-packages/certifi/cacert.pem"
fi
export PYTHONNOUSERSITE=1
export PIP_NO_INDEX=1
export WUNDER_PYTHON_BIN="$APPDIR/opt/python/bin/python3"
exec "$APPDIR/AppRun.orig" "$@"
EOF
  chmod +x "${APPDIR}/AppRun" "${APPDIR}/AppRun.orig"
else
  cat > "${APPDIR}/AppRun" <<'EOF'
#!/usr/bin/env bash
set -e
HERE="$(dirname "$(readlink -f "$0")")"
export APPDIR="$HERE"
PY_VER="$(cat "$APPDIR/opt/python/.wunder-python-version" 2>/dev/null || echo "3.11")"
export PYTHONHOME="$APPDIR/opt/python"
export PYTHONPATH="$APPDIR/opt/python/lib/python${PY_VER}/site-packages${PYTHONPATH:+:$PYTHONPATH}"
export LD_LIBRARY_PATH="$APPDIR/opt/python/lib:$APPDIR/usr/lib:${LD_LIBRARY_PATH:-}"
if [ -f "$APPDIR/opt/python/lib/python${PY_VER}/site-packages/certifi/cacert.pem" ]; then
  export SSL_CERT_FILE="$APPDIR/opt/python/lib/python${PY_VER}/site-packages/certifi/cacert.pem"
fi
export PYTHONNOUSERSITE=1
export PIP_NO_INDEX=1
export WUNDER_PYTHON_BIN="$APPDIR/opt/python/bin/python3"
exec "$APPDIR/usr/bin/wunder-desktop" "$@"
EOF
  chmod +x "${APPDIR}/AppRun"
fi

APPIMAGETOOL_BIN="${APPIMAGETOOL:-}"
if [ -z "${APPIMAGETOOL_BIN}" ]; then
  APPIMAGETOOL_BIN=$(command -v appimagetool || true)
fi
if [ -z "${APPIMAGETOOL_BIN}" ]; then
  APPIMAGETOOL_BIN=$(find "${HOME}/.cache/tauri" -name appimagetool -type f 2>/dev/null | head -n 1 || true)
fi

APPIMAGETOOL_RUNNER=""
if [ -n "${APPIMAGETOOL_BIN}" ]; then
  APPIMAGETOOL_RUNNER="${APPIMAGETOOL_BIN}"
else
  mkdir -p "${TOOLS_DIR}"
  TOOL_ARCH=$(resolve_appimagetool_arch)
  DOWNLOADED_TOOL="${TOOLS_DIR}/appimagetool-${TOOL_ARCH}.AppImage"
  if [ ! -f "${DOWNLOADED_TOOL}" ]; then
    TOOL_URL="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${TOOL_ARCH}.AppImage"
    curl -Lf "${TOOL_URL}" -o "${DOWNLOADED_TOOL}"
    chmod +x "${DOWNLOADED_TOOL}"
  fi
  APPIMAGETOOL_RUNNER="${APPIMAGE_WORK}/appimagetool.run.AppImage"
  cp "${DOWNLOADED_TOOL}" "${APPIMAGETOOL_RUNNER}"
  patch_appimage_runtime_magic "${APPIMAGETOOL_RUNNER}"
  chmod +x "${APPIMAGETOOL_RUNNER}"
fi

mkdir -p "${OUTPUT_DIR}"
OUT_NAME=$(basename "${APPIMAGE_PATH}")
OUT_NAME="${OUT_NAME%.AppImage}-python.AppImage"
OUT_PATH="${OUTPUT_DIR}/${OUT_NAME}"

if [[ "${APPIMAGETOOL_RUNNER}" == *.AppImage ]]; then
  APPIMAGE_EXTRACT_AND_RUN=1 "${APPIMAGETOOL_RUNNER}" "${APPDIR}" "${OUT_PATH}"
else
  "${APPIMAGETOOL_RUNNER}" "${APPDIR}" "${OUT_PATH}"
fi

echo "AppImage with embedded Python: ${OUT_PATH}"
