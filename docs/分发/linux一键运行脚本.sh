#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ "${1:-}" != "" ] && [ -d "$1" ]; then
  TARGET_DIR="$1"
  shift
else
  TARGET_DIR="$SCRIPT_DIR"
fi

EXTRA_DIR=""
if [ -n "${WUNDER_EXTRA_DIR:-}" ] && [ -d "${WUNDER_EXTRA_DIR}" ]; then
  EXTRA_DIR="${WUNDER_EXTRA_DIR}"
fi
if [ -z "${EXTRA_DIR}" ]; then
  for candidate in "${TARGET_DIR}/wunder补充包" "${TARGET_DIR}/wunder-extra" "${TARGET_DIR}/wunder-python"; do
    if [ -d "${candidate}" ]; then
      EXTRA_DIR="${candidate}"
      break
    fi
  done
fi

detect_arch() {
  local arch
  arch="$(uname -m 2>/dev/null || true)"
  case "${arch}" in
    aarch64|arm64)
      echo "arm64"
      ;;
    x86_64|amd64)
      echo "x86_64"
      ;;
    *)
      echo "${arch}"
      ;;
  esac
}

ARCH="$(detect_arch)"
ARCH_CANDIDATES=("${ARCH}")
case "${ARCH}" in
  arm64)
    ARCH_CANDIDATES+=("aarch64")
    ;;
  aarch64)
    ARCH_CANDIDATES+=("arm64")
    ;;
  x86_64)
    ARCH_CANDIDATES+=("amd64" "x64")
    ;;
  amd64)
    ARCH_CANDIDATES+=("x86_64" "x64")
    ;;
esac

find_appimage() {
  if [ -n "${WUNDER_APPIMAGE:-}" ] && [ -f "${WUNDER_APPIMAGE}" ]; then
    echo "${WUNDER_APPIMAGE}"
    return 0
  fi
  for arch in "${ARCH_CANDIDATES[@]}"; do
    local candidate="${TARGET_DIR}/wunder-desktop-${arch}-sidecar.AppImage"
    if [ -f "${candidate}" ]; then
      echo "${candidate}"
      return 0
    fi
  done
  shopt -s nullglob
  local matches=("${TARGET_DIR}"/wunder-desktop-*-sidecar.AppImage)
  shopt -u nullglob
  if [ "${#matches[@]}" -gt 0 ]; then
    ls -1t -- "${matches[@]}" | head -n 1
    return 0
  fi
  return 1
}

find_tarball() {
  if [ -n "${WUNDER_EXTRA_TARBALL:-}" ] && [ -f "${WUNDER_EXTRA_TARBALL}" ]; then
    echo "${WUNDER_EXTRA_TARBALL}"
    return 0
  fi
  if [ -n "${WUNDER_PYTHON_TARBALL:-}" ] && [ -f "${WUNDER_PYTHON_TARBALL}" ]; then
    echo "${WUNDER_PYTHON_TARBALL}"
    return 0
  fi
  for arch in "${ARCH_CANDIDATES[@]}"; do
    local candidate_extra_zst="${TARGET_DIR}/wunder补充包-${arch}.tar.zst"
    local candidate_extra_gz="${TARGET_DIR}/wunder补充包-${arch}.tar.gz"
    local candidate_extra_legacy="${TARGET_DIR}/wunder-extra-${arch}.tar.gz"
    local candidate_zst="${TARGET_DIR}/wunder-python-${arch}.tar.zst"
    local candidate_gz="${TARGET_DIR}/wunder-python-${arch}.tar.gz"
    if [ -f "${candidate_extra_zst}" ]; then
      echo "${candidate_extra_zst}"
      return 0
    fi
    if [ -f "${candidate_extra_gz}" ]; then
      echo "${candidate_extra_gz}"
      return 0
    fi
    if [ -f "${candidate_extra_legacy}" ]; then
      echo "${candidate_extra_legacy}"
      return 0
    fi
    if [ -f "${candidate_zst}" ]; then
      echo "${candidate_zst}"
      return 0
    fi
    if [ -f "${candidate_gz}" ]; then
      echo "${candidate_gz}"
      return 0
    fi
  done
  shopt -s nullglob
  local matches=("${TARGET_DIR}"/wunder补充包-*.tar.* "${TARGET_DIR}"/wunder-extra-*.tar.* "${TARGET_DIR}"/wunder-python-*.tar.*)
  shopt -u nullglob
  if [ "${#matches[@]}" -gt 0 ]; then
    ls -1t -- "${matches[@]}" | head -n 1
    return 0
  fi
  return 1
}

APPIMAGE="$(find_appimage || true)"
if [ -z "${APPIMAGE}" ] || [ ! -f "${APPIMAGE}" ]; then
  echo "Sidecar AppImage not found under: ${TARGET_DIR}" >&2
  exit 1
fi

if [ -n "${EXTRA_DIR}" ]; then
  echo "Extra sidecar already extracted: ${EXTRA_DIR}"
else
  TARBALL="$(find_tarball || true)"
  if [ -z "${TARBALL}" ] || [ ! -f "${TARBALL}" ]; then
    echo "Extra sidecar tarball not found under: ${TARGET_DIR}" >&2
    exit 1
  fi
  echo "Extracting extra sidecar..."
  case "${TARBALL}" in
    *.tar.zst)
      if command -v zstd >/dev/null 2>&1; then
        tar -I zstd -xf "${TARBALL}" -C "${TARGET_DIR}"
      else
        echo "zstd not found; cannot extract ${TARBALL}" >&2
        exit 1
      fi
      ;;
    *.tar.gz|*.tgz)
      tar -xzf "${TARBALL}" -C "${TARGET_DIR}"
      ;;
    *)
      echo "Unsupported tarball format: ${TARBALL}" >&2
      exit 1
      ;;
  esac
  for candidate in "${TARGET_DIR}/wunder补充包" "${TARGET_DIR}/wunder-extra" "${TARGET_DIR}/wunder-python"; do
    if [ -d "${candidate}" ]; then
      EXTRA_DIR="${candidate}"
      break
    fi
  done
  if [ -z "${EXTRA_DIR}" ]; then
    echo "Extra sidecar directory not found after extraction under: ${TARGET_DIR}" >&2
    exit 1
  fi
fi

chmod -R 777 "${EXTRA_DIR}"
chmod 777 "${APPIMAGE}"

echo "Launching AppImage..."
exec "${APPIMAGE}" "$@"
