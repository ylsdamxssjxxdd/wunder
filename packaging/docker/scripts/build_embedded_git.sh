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
GIT_PREFIX="${GIT_PREFIX:-/opt/git}"
STAGE_DIR="${BUILD_ROOT}/stage"
GIT_ROOT="${STAGE_DIR}${GIT_PREFIX}"

GIT_BIN="${GIT_BIN:-$(command -v git || true)}"
if [ -z "${GIT_BIN}" ] || [ ! -x "${GIT_BIN}" ]; then
  echo "git executable not found in PATH" >&2
  exit 1
fi

GIT_EXEC_PATH="${GIT_EXEC_PATH_OVERRIDE:-$(git --exec-path)}"
if [ ! -d "${GIT_EXEC_PATH}" ]; then
  echo "git exec path not found: ${GIT_EXEC_PATH}" >&2
  exit 1
fi

GIT_SHARE_PATH="${GIT_SHARE_PATH:-/usr/share/git-core}"
if [ ! -d "${GIT_SHARE_PATH}" ]; then
  GIT_SHARE_PATH=""
fi

mkdir -p "${GIT_ROOT}/bin" "${GIT_ROOT}/libexec" "${GIT_ROOT}/share" "${GIT_ROOT}/lib"

cp -f "${GIT_BIN}" "${GIT_ROOT}/bin/git"
chmod +x "${GIT_ROOT}/bin/git"

rm -rf "${GIT_ROOT}/libexec/git-core"
cp -a "${GIT_EXEC_PATH}" "${GIT_ROOT}/libexec/git-core"

if [ -n "${GIT_SHARE_PATH}" ]; then
  rm -rf "${GIT_ROOT}/share/git-core"
  cp -a "${GIT_SHARE_PATH}" "${GIT_ROOT}/share/git-core"
fi

tmp_bins=$(mktemp)
{
  echo "${GIT_ROOT}/bin/git"
  find "${GIT_ROOT}/libexec/git-core" -maxdepth 1 -type f -perm -u+x
} | sort -u > "${tmp_bins}"

tmp_libs=$(mktemp)
while IFS= read -r bin_file; do
  if [ ! -x "${bin_file}" ]; then
    continue
  fi
  ldd "${bin_file}" 2>/dev/null \
    | awk '/=> \// {print $3} /^\/lib/ {print $1}' \
    | sed '/^$/d' >> "${tmp_libs}" || true
done < "${tmp_bins}"

sort -u "${tmp_libs}" | while IFS= read -r lib_file; do
  if [ -f "${lib_file}" ]; then
    cp -L "${lib_file}" "${GIT_ROOT}/lib/"
  fi
done

rm -f "${tmp_bins}" "${tmp_libs}"

git --version | awk '{print $3}' > "${GIT_ROOT}/.wunder-git-version"

find "${GIT_ROOT}" -type d -name '__pycache__' -prune -exec rm -rf {} + 2>/dev/null || true

echo "Embedded git prepared at: ${GIT_ROOT}"
