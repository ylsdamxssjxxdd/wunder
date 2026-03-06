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
PREFER_PREBUILT_GIT="${PREFER_PREBUILT_GIT:-1}"
PREBUILT_GIT_ROOT="${BUILD_ROOT}/stage/opt/git"
BUNDLE_PLAYWRIGHT_DEPS="${BUNDLE_PLAYWRIGHT_DEPS:-auto}"
PLAYWRIGHT_INSTALL_DEPS="${PLAYWRIGHT_INSTALL_DEPS:-1}"
EMBED_PYTHON="${EMBED_PYTHON:-1}"
APPIMAGE_SUFFIX="${APPIMAGE_SUFFIX:-}"
APPIMAGE_COMP="${APPIMAGE_COMP:-auto}"
VALIDATE_MODULES="${VALIDATE_MODULES:-matplotlib,cartopy,pyproj,shapely,netCDF4,cftime,h5py,cinrad}"

validate_embedded_python_root() {
  local python_root=$1
  local raw_modules=$2
  if [ -z "${raw_modules}" ] || [ ! -x "${python_root}/bin/python3" ]; then
    return 0
  fi
  "${python_root}/bin/python3" -c 'import importlib,sys; raw=sys.argv[1] if len(sys.argv) > 1 else ""; modules=[item.strip() for item in raw.split(",") if item.strip()]; ns={"importlib": importlib}; exec("def _check(name):\n    try:\n        importlib.import_module(name)\n        return None\n    except Exception as exc:\n        return f\"{name} ({type(exc).__name__}: {exc})\"\n", ns); missing=[item for item in (ns["_check"](name) for name in modules) if item]; missing and (_ for _ in ()).throw(SystemExit("missing embedded python modules: " + ", ".join(missing)))' "${raw_modules}"
}

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

bundle_playwright_deps() {
  local appdir=$1
  local pw_dir="${PLAYWRIGHT_SOURCE_DIR:-${appdir}/opt/python/playwright}"
  local bundle_dir="${appdir}/usr/lib/wunder-playwright"

  if [ ! -d "${pw_dir}" ]; then
    return 0
  fi
  if ! command -v ldd >/dev/null 2>&1; then
    echo "ldd not found; skipping Playwright dependency bundling." >&2
    return 0
  fi

  if [ "${PLAYWRIGHT_INSTALL_DEPS}" = "1" ] && [ -x "${PREBUILT_PYTHON_ROOT}/bin/python3" ]; then
    echo "Installing Playwright system dependencies (chromium) inside build container..."
    "${PREBUILT_PYTHON_ROOT}/bin/python3" -m playwright install-deps chromium || true
  fi

  mkdir -p "${bundle_dir}"
  local -a queue=()
  while IFS= read -r -d '' bin; do
    queue+=("${bin}")
  done < <(
    find "${pw_dir}" -type f \( \
      -name chrome -o -name chrome_sandbox -o -name chrome_crashpad_handler -o \
      -name headless_shell -o -name ffmpeg-linux \
    \) -print0
  )

  if [ "${#queue[@]}" -eq 0 ]; then
    return 0
  fi

  declare -A seen
  declare -A copied

  while [ "${#queue[@]}" -gt 0 ]; do
    local item="${queue[0]}"
    queue=("${queue[@]:1}")
    if [ -z "${item}" ] || [ ! -e "${item}" ]; then
      continue
    fi
    if [[ -n "${seen[${item}]+x}" ]]; then
      continue
    fi
    seen["${item}"]=1

    local ldd_output
    ldd_output=$(ldd "${item}" 2>/dev/null || true)
    if [ -z "${ldd_output}" ]; then
      continue
    fi

    while IFS= read -r lib; do
      if [ -z "${lib}" ]; then
        continue
      fi
      case "${lib}" in
        linux-vdso.so.1) continue ;;
        /lib/ld-linux*|/lib64/ld-linux*|/usr/lib/ld-linux*|/lib/aarch64-linux-gnu/ld-linux*)
          continue
          ;;
        */libc.so.*|*/libm.so.*|*/librt.so.*|*/libpthread.so.*|*/libdl.so.*)
          continue
          ;;
      esac
      if [ -e "${lib}" ]; then
        if [[ -z "${copied[${lib}]+x}" ]]; then
          cp -a "${lib}" "${bundle_dir}/" || true
          copied["${lib}"]=1
        fi
        if [ -L "${lib}" ]; then
          local real
          real=$(readlink -f "${lib}" || true)
          if [ -n "${real}" ] && [ -e "${real}" ] && [[ -z "${copied[${real}]+x}" ]]; then
            cp -a "${real}" "${bundle_dir}/" || true
            copied["${real}"]=1
          fi
        fi
        if [[ -z "${seen[${lib}]+x}" ]]; then
          queue+=("${lib}")
        fi
      fi
    done < <(
      printf '%s\n' "${ldd_output}" \
        | awk '{ if ($1=="linux-vdso.so.1") next; if (NF>=3 && $2=="=>") print $3; else if ($1 ~ /^\\//) print $1; }' \
        | sort -u
    )
  done

  echo "Bundled Playwright runtime libs into ${bundle_dir}."
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

supports_appimage_compression_flag() {
  local tool=$1
  if [[ "${tool}" == *.AppImage ]]; then
    APPIMAGE_EXTRACT_AND_RUN=1 "${tool}" --help 2>&1 | grep -q -- '--comp'
    return $?
  fi
  "${tool}" --help 2>&1 | grep -q -- '--comp'
}

resolve_supported_appimage_comp() {
  local requested=${1:-auto}
  local squashfs_help
  squashfs_help=$(mksquashfs -help 2>&1 || true)

  supports_comp() {
    local name=$1
    printf '%s\n' "${squashfs_help}" | grep -Eiq "(^|[^[:alpha:]])${name}([^[:alpha:]]|$)"
  }

  if [ "${requested}" != "auto" ]; then
    echo "${requested}"
    return 0
  fi

  if supports_comp zstd; then
    echo "zstd"
    return 0
  fi
  if supports_comp gzip; then
    echo "gzip"
    return 0
  fi
  echo "xz"
}

if [ -z "${APPIMAGE_PATH}" ]; then
  APPIMAGE_PATH=$(ls -1 "${APPIMAGE_DIR}"/*.AppImage 2>/dev/null | head -n 1 || true)
fi

if [ -z "${APPIMAGE_PATH}" ] || [ ! -f "${APPIMAGE_PATH}" ]; then
  echo "AppImage not found under ${APPIMAGE_DIR}." >&2
  exit 1
fi

if [ "${EMBED_PYTHON}" = "1" ]; then
  if [ "${PREFER_PREBUILT_PYTHON}" = "1" ] && [ -x "${PREBUILT_PYTHON_ROOT}/bin/python3" ]; then
    echo "Using prebuilt embedded Python under ${PREBUILT_PYTHON_ROOT}."
  else
    "${ROOT_DIR}/docker-extra/scripts/build_embedded_python.sh"
  fi
  validate_embedded_python_root "${PREBUILT_PYTHON_ROOT}" "${VALIDATE_MODULES}"
fi

if [ "${PREFER_PREBUILT_GIT}" = "1" ] && [ -x "${PREBUILT_GIT_ROOT}/bin/git" ]; then
  echo "Using prebuilt embedded Git under ${PREBUILT_GIT_ROOT}."
else
  "${ROOT_DIR}/docker-extra/scripts/build_embedded_git.sh"
fi

extract_appimage "${APPIMAGE_PATH}" "${APPIMAGE_WORK}"

APPDIR="${APPIMAGE_WORK}/squashfs-root"
if [ ! -d "${APPDIR}" ]; then
  echo "Extracted AppDir not found at ${APPDIR}." >&2
  exit 1
fi
if [ ! -x "${PREBUILT_GIT_ROOT}/bin/git" ]; then
  echo "Embedded Git not found under ${PREBUILT_GIT_ROOT}." >&2
  exit 1
fi

mkdir -p "${APPDIR}/opt"
rm -rf "${APPDIR}/opt/python"
if [ "${EMBED_PYTHON}" = "1" ]; then
  if [ ! -x "${PREBUILT_PYTHON_ROOT}/bin/python3" ]; then
    echo "Embedded Python not found under ${PREBUILT_PYTHON_ROOT}." >&2
    exit 1
  fi
  cp -a "${PREBUILT_PYTHON_ROOT}" "${APPDIR}/opt/"
fi
rm -rf "${APPDIR}/opt/git"
cp -a "${PREBUILT_GIT_ROOT}" "${APPDIR}/opt/"
if [ "${EMBED_PYTHON}" = "1" ]; then
  if [ ! -e "${APPDIR}/opt/python/bin/python" ] && [ -x "${APPDIR}/opt/python/bin/python3" ]; then
    ln -s python3 "${APPDIR}/opt/python/bin/python"
  fi
  if [ ! -e "${APPDIR}/opt/python/bin/pip" ] && [ -x "${APPDIR}/opt/python/bin/pip3" ]; then
    ln -s pip3 "${APPDIR}/opt/python/bin/pip"
  fi
fi

if [ -f "${APPDIR}/AppRun" ]; then
  mv "${APPDIR}/AppRun" "${APPDIR}/AppRun.orig"
cat > "${APPDIR}/AppRun" <<'EOF'
#!/usr/bin/env bash
set -e
HERE="$(dirname "$(readlink -f "$0")")"
export APPDIR="$HERE"
APPIMAGE_DIR=""
if [ -n "${APPIMAGE:-}" ]; then
  APPIMAGE_DIR="$(dirname "$APPIMAGE")"
fi
EXTRA_ROOT=""
if [ -n "${WUNDER_EXTRA_DIR:-}" ] && [ -d "${WUNDER_EXTRA_DIR}" ]; then
  EXTRA_ROOT="${WUNDER_EXTRA_DIR}"
fi
if [ -z "$EXTRA_ROOT" ] && [ -n "$APPIMAGE_DIR" ]; then
  for candidate in "$APPIMAGE_DIR/wunder补充包" "$APPIMAGE_DIR/wunder-extra" "$APPIMAGE_DIR/wunder-python"; do
    if [ -d "$candidate" ]; then
      EXTRA_ROOT="$candidate"
      break
    fi
  done
fi
PYTHON_ROOT=""
if [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/opt/python" ]; then
  PYTHON_ROOT="$EXTRA_ROOT/opt/python"
elif [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/python" ]; then
  PYTHON_ROOT="$EXTRA_ROOT/python"
elif [ -d "$APPDIR/opt/python" ]; then
  PYTHON_ROOT="$APPDIR/opt/python"
fi
GIT_ROOT=""
if [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/opt/git" ]; then
  GIT_ROOT="$EXTRA_ROOT/opt/git"
elif [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/git" ]; then
  GIT_ROOT="$EXTRA_ROOT/git"
elif [ -d "$APPDIR/opt/git" ]; then
  GIT_ROOT="$APPDIR/opt/git"
fi
PY_VER="3.11"
if [ -n "$PYTHON_ROOT" ] && [ -f "$PYTHON_ROOT/.wunder-python-version" ]; then
  PY_VER="$(cat "$PYTHON_ROOT/.wunder-python-version" 2>/dev/null || echo "3.11")"
fi
PYTHON_LD=""
PATH_PREFIX=""
if [ -n "$GIT_ROOT" ] && [ -d "$GIT_ROOT/bin" ]; then
  PATH_PREFIX="$GIT_ROOT/bin"
fi
if [ -n "$PYTHON_ROOT" ]; then
  export PYTHONHOME="$PYTHON_ROOT"
  export PYTHONPATH="$PYTHON_ROOT/lib/python${PY_VER}/site-packages${PYTHONPATH:+:$PYTHONPATH}"
  export WUNDER_PYTHON_BIN="$PYTHON_ROOT/bin/python3"
  export PATH="${PATH_PREFIX:+$PATH_PREFIX:}$PYTHON_ROOT/bin:${PATH:-}"
  PYTHON_LD="$PYTHON_ROOT/lib:"
  export PYTHONNOUSERSITE=1
  export PIP_NO_INDEX=1
  if [ -f "$PYTHON_ROOT/lib/python${PY_VER}/site-packages/certifi/cacert.pem" ]; then
    export SSL_CERT_FILE="$PYTHON_ROOT/lib/python${PY_VER}/site-packages/certifi/cacert.pem"
  fi
  if [ -d "$PYTHON_ROOT/share/cartopy" ]; then
    export CARTOPY_DATA_DIR="$PYTHON_ROOT/share/cartopy"
  fi
else
  export PATH="${PATH_PREFIX:+$PATH_PREFIX:}${PATH:-}"
fi
GIT_LD=""
if [ -n "$GIT_ROOT" ] && [ -d "$GIT_ROOT/lib" ]; then
  GIT_LD="$GIT_ROOT/lib:"
fi
export LD_LIBRARY_PATH="$APPDIR/usr/lib/wunder-playwright:${GIT_LD}${PYTHON_LD}$APPDIR/usr/lib:${LD_LIBRARY_PATH:-}"
PLAYWRIGHT_DIR=""
if [ -n "$APPIMAGE_DIR" ] && [ -d "$APPIMAGE_DIR/wunder-playwright" ]; then
  PLAYWRIGHT_DIR="$APPIMAGE_DIR/wunder-playwright"
elif [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/playwright" ]; then
  PLAYWRIGHT_DIR="$EXTRA_ROOT/playwright"
elif [ -n "$PYTHON_ROOT" ] && [ -d "$PYTHON_ROOT/playwright" ]; then
  PLAYWRIGHT_DIR="$PYTHON_ROOT/playwright"
fi
if [ -n "$PLAYWRIGHT_DIR" ]; then
  export PLAYWRIGHT_BROWSERS_PATH="$PLAYWRIGHT_DIR"
fi
if [ -n "$GIT_ROOT" ]; then
  if [ -d "$GIT_ROOT/libexec/git-core" ]; then
    export GIT_EXEC_PATH="$GIT_ROOT/libexec/git-core"
  fi
  if [ -d "$GIT_ROOT/share/git-core/templates" ]; then
    export GIT_TEMPLATE_DIR="$GIT_ROOT/share/git-core/templates"
  fi
  if [ -x "$GIT_ROOT/bin/git" ]; then
    export WUNDER_GIT_BIN="$GIT_ROOT/bin/git"
  fi
fi
exec "$APPDIR/AppRun.orig" "$@"
EOF
  chmod +x "${APPDIR}/AppRun" "${APPDIR}/AppRun.orig"
else
cat > "${APPDIR}/AppRun" <<'EOF'
#!/usr/bin/env bash
set -e
HERE="$(dirname "$(readlink -f "$0")")"
export APPDIR="$HERE"
APPIMAGE_DIR=""
if [ -n "${APPIMAGE:-}" ]; then
  APPIMAGE_DIR="$(dirname "$APPIMAGE")"
fi
EXTRA_ROOT=""
if [ -n "${WUNDER_EXTRA_DIR:-}" ] && [ -d "${WUNDER_EXTRA_DIR}" ]; then
  EXTRA_ROOT="${WUNDER_EXTRA_DIR}"
fi
if [ -z "$EXTRA_ROOT" ] && [ -n "$APPIMAGE_DIR" ]; then
  for candidate in "$APPIMAGE_DIR/wunder补充包" "$APPIMAGE_DIR/wunder-extra" "$APPIMAGE_DIR/wunder-python"; do
    if [ -d "$candidate" ]; then
      EXTRA_ROOT="$candidate"
      break
    fi
  done
fi
PYTHON_ROOT=""
if [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/opt/python" ]; then
  PYTHON_ROOT="$EXTRA_ROOT/opt/python"
elif [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/python" ]; then
  PYTHON_ROOT="$EXTRA_ROOT/python"
elif [ -d "$APPDIR/opt/python" ]; then
  PYTHON_ROOT="$APPDIR/opt/python"
fi
GIT_ROOT=""
if [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/opt/git" ]; then
  GIT_ROOT="$EXTRA_ROOT/opt/git"
elif [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/git" ]; then
  GIT_ROOT="$EXTRA_ROOT/git"
elif [ -d "$APPDIR/opt/git" ]; then
  GIT_ROOT="$APPDIR/opt/git"
fi
PY_VER="3.11"
if [ -n "$PYTHON_ROOT" ] && [ -f "$PYTHON_ROOT/.wunder-python-version" ]; then
  PY_VER="$(cat "$PYTHON_ROOT/.wunder-python-version" 2>/dev/null || echo "3.11")"
fi
PYTHON_LD=""
PATH_PREFIX=""
if [ -n "$GIT_ROOT" ] && [ -d "$GIT_ROOT/bin" ]; then
  PATH_PREFIX="$GIT_ROOT/bin"
fi
if [ -n "$PYTHON_ROOT" ]; then
  export PYTHONHOME="$PYTHON_ROOT"
  export PYTHONPATH="$PYTHON_ROOT/lib/python${PY_VER}/site-packages${PYTHONPATH:+:$PYTHONPATH}"
  export WUNDER_PYTHON_BIN="$PYTHON_ROOT/bin/python3"
  export PATH="${PATH_PREFIX:+$PATH_PREFIX:}$PYTHON_ROOT/bin:${PATH:-}"
  PYTHON_LD="$PYTHON_ROOT/lib:"
  export PYTHONNOUSERSITE=1
  export PIP_NO_INDEX=1
  if [ -f "$PYTHON_ROOT/lib/python${PY_VER}/site-packages/certifi/cacert.pem" ]; then
    export SSL_CERT_FILE="$PYTHON_ROOT/lib/python${PY_VER}/site-packages/certifi/cacert.pem"
  fi
  if [ -d "$PYTHON_ROOT/share/cartopy" ]; then
    export CARTOPY_DATA_DIR="$PYTHON_ROOT/share/cartopy"
  fi
else
  export PATH="${PATH_PREFIX:+$PATH_PREFIX:}${PATH:-}"
fi
GIT_LD=""
if [ -n "$GIT_ROOT" ] && [ -d "$GIT_ROOT/lib" ]; then
  GIT_LD="$GIT_ROOT/lib:"
fi
export LD_LIBRARY_PATH="$APPDIR/usr/lib/wunder-playwright:${GIT_LD}${PYTHON_LD}$APPDIR/usr/lib:${LD_LIBRARY_PATH:-}"
PLAYWRIGHT_DIR=""
if [ -n "$APPIMAGE_DIR" ] && [ -d "$APPIMAGE_DIR/wunder-playwright" ]; then
  PLAYWRIGHT_DIR="$APPIMAGE_DIR/wunder-playwright"
elif [ -n "$EXTRA_ROOT" ] && [ -d "$EXTRA_ROOT/playwright" ]; then
  PLAYWRIGHT_DIR="$EXTRA_ROOT/playwright"
elif [ -n "$PYTHON_ROOT" ] && [ -d "$PYTHON_ROOT/playwright" ]; then
  PLAYWRIGHT_DIR="$PYTHON_ROOT/playwright"
fi
if [ -n "$PLAYWRIGHT_DIR" ]; then
  export PLAYWRIGHT_BROWSERS_PATH="$PLAYWRIGHT_DIR"
fi
if [ -n "$GIT_ROOT" ]; then
  if [ -d "$GIT_ROOT/libexec/git-core" ]; then
    export GIT_EXEC_PATH="$GIT_ROOT/libexec/git-core"
  fi
  if [ -d "$GIT_ROOT/share/git-core/templates" ]; then
    export GIT_TEMPLATE_DIR="$GIT_ROOT/share/git-core/templates"
  fi
  if [ -x "$GIT_ROOT/bin/git" ]; then
    export WUNDER_GIT_BIN="$GIT_ROOT/bin/git"
  fi
fi
exec "$APPDIR/usr/bin/wunder-desktop" "$@"
EOF
  chmod +x "${APPDIR}/AppRun"
fi

if [ -z "${APPIMAGE_SUFFIX}" ]; then
  if [ "${EMBED_PYTHON}" = "1" ]; then
    APPIMAGE_SUFFIX="python"
  else
    APPIMAGE_SUFFIX="sidecar"
  fi
fi

if [ "${BUNDLE_PLAYWRIGHT_DEPS}" = "1" ] || \
   { [ "${BUNDLE_PLAYWRIGHT_DEPS}" = "auto" ] && [ -d "${APPDIR}/opt/python/playwright" ]; }; then
  bundle_playwright_deps "${APPDIR}"
elif [ "${BUNDLE_PLAYWRIGHT_DEPS}" = "1" ] && [ "${EMBED_PYTHON}" = "0" ] && [ -d "${PREBUILT_PYTHON_ROOT}/playwright" ]; then
  PLAYWRIGHT_SOURCE_DIR="${PREBUILT_PYTHON_ROOT}/playwright" bundle_playwright_deps "${APPDIR}"
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
OUT_NAME="${OUT_NAME%.AppImage}-${APPIMAGE_SUFFIX}.AppImage"
OUT_PATH="${OUTPUT_DIR}/${OUT_NAME}"

APPIMAGE_COMP=$(resolve_supported_appimage_comp "${APPIMAGE_COMP}")
APPIMAGE_TOOL_ARGS=("${APPDIR}" "${OUT_PATH}")
if supports_appimage_compression_flag "${APPIMAGETOOL_RUNNER}"; then
  APPIMAGE_TOOL_ARGS=(--comp "${APPIMAGE_COMP}" "${APPDIR}" "${OUT_PATH}")
  echo "Using AppImage squashfs compression: ${APPIMAGE_COMP}"
fi

if [[ "${APPIMAGETOOL_RUNNER}" == *.AppImage ]]; then
  APPIMAGE_EXTRACT_AND_RUN=1 "${APPIMAGETOOL_RUNNER}" "${APPIMAGE_TOOL_ARGS[@]}"
else
  "${APPIMAGETOOL_RUNNER}" "${APPIMAGE_TOOL_ARGS[@]}"
fi

if [ "${EMBED_PYTHON}" = "1" ]; then
  echo "AppImage with embedded Python and Git: ${OUT_PATH}"
else
  echo "AppImage with sidecar Python and embedded Git: ${OUT_PATH}"
fi
