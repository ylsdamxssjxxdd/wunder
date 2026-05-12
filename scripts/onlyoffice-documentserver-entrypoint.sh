#!/usr/bin/env bash
set -euo pipefail

CUSTOM_FONT_DIR="${ONLYOFFICE_CUSTOM_FONT_DIR:-/var/www/onlyoffice/Data/custom-fonts}"
GENERATE_FONTS="${ONLYOFFICE_GENERATE_WUNDER_FONTS:-true}"
FORCE_REBUILD="${ONLYOFFICE_FORCE_FONT_REBUILD:-false}"
STATE_DIR="${ONLYOFFICE_WUNDER_STATE_DIR:-/var/lib/onlyoffice/.wunder}"
STATE_FILE="${STATE_DIR}/font-index-state.sha256"
CACHE_DIR="${STATE_DIR}/font-index-cache"
CACHE_ALL_FONTS_WEB="${CACHE_DIR}/AllFonts.sdkjs.js"
CACHE_ALL_FONTS_BIN="${CACHE_DIR}/AllFonts.converter.js"
CACHE_FONT_SELECTION_BIN="${CACHE_DIR}/font_selection.bin"
SCRIPT_SCHEMA_VERSION="v2"
DOCSERVICE_BIN="${ONLYOFFICE_DOCSERVICE_BIN:-/var/www/onlyoffice/documentserver/server/DocService/docservice}"
ALL_FONTS_WEB="${ONLYOFFICE_ALL_FONTS_WEB:-/var/www/onlyoffice/documentserver/sdkjs/common/AllFonts.js}"
ALL_FONTS_BIN="${ONLYOFFICE_ALL_FONTS_BIN:-/var/www/onlyoffice/documentserver/server/FileConverter/bin/AllFonts.js}"
FONT_SELECTION_BIN="${ONLYOFFICE_FONT_SELECTION_BIN:-/var/www/onlyoffice/documentserver/server/FileConverter/bin/font_selection.bin}"

font_files() {
  [ -d "${CUSTOM_FONT_DIR}" ] || return 1
  find "${CUSTOM_FONT_DIR}" -type f \( \
    -iname '*.ttf' -o \
    -iname '*.ttc' -o \
    -iname '*.otf' -o \
    -iname '*.woff' -o \
    -iname '*.woff2' \
  \)
}

count_custom_fonts() {
  font_files 2>/dev/null | wc -l | tr -d ' '
}

font_outputs_exist() {
  [ -s "${ALL_FONTS_WEB}" ] && [ -s "${ALL_FONTS_BIN}" ] && [ -s "${FONT_SELECTION_BIN}" ]
}

cached_font_outputs_exist() {
  [ -s "${CACHE_ALL_FONTS_WEB}" ] && [ -s "${CACHE_ALL_FONTS_BIN}" ] && [ -s "${CACHE_FONT_SELECTION_BIN}" ]
}

build_font_state() {
  {
    echo "schema=${SCRIPT_SCHEMA_VERSION}"
    echo "release=${OC_FILE_SUFFIX:-unknown}"
    echo "docservice=$(stat -c '%s:%Y' "${DOCSERVICE_BIN}" 2>/dev/null || echo missing)"
    font_files 2>/dev/null \
      | LC_ALL=C sort \
      | while IFS= read -r font_path; do
          stat -c '%n|%s|%Y' "${font_path}"
        done
  } | sha256sum | awk '{print $1}'
}

current_state() {
  [ -f "${STATE_FILE}" ] || return 1
  tr -d '\r\n' < "${STATE_FILE}"
}

write_state() {
  mkdir -p "${STATE_DIR}"
  printf '%s\n' "$1" > "${STATE_FILE}"
}

cache_font_outputs() {
  font_outputs_exist || return 1
  mkdir -p "${CACHE_DIR}"
  cp -f "${ALL_FONTS_WEB}" "${CACHE_ALL_FONTS_WEB}"
  cp -f "${ALL_FONTS_BIN}" "${CACHE_ALL_FONTS_BIN}"
  cp -f "${FONT_SELECTION_BIN}" "${CACHE_FONT_SELECTION_BIN}"
}

restore_cached_font_outputs() {
  cached_font_outputs_exist || return 1
  mkdir -p "$(dirname "${ALL_FONTS_WEB}")" "$(dirname "${ALL_FONTS_BIN}")" "$(dirname "${FONT_SELECTION_BIN}")"
  cp -f "${CACHE_ALL_FONTS_WEB}" "${ALL_FONTS_WEB}"
  cp -f "${CACHE_ALL_FONTS_BIN}" "${ALL_FONTS_BIN}"
  cp -f "${CACHE_FONT_SELECTION_BIN}" "${FONT_SELECTION_BIN}"
}

refresh_font_indexes() {
  echo "[wunder-onlyoffice] refreshing fontconfig cache from ${CUSTOM_FONT_DIR}"
  fc-cache -f "${CUSTOM_FONT_DIR}" || fc-cache -f || true

  echo "[wunder-onlyoffice] generating OnlyOffice font indexes"
  /usr/bin/documentserver-generate-allfonts.sh true
}

if [ "${GENERATE_FONTS}" = "true" ]; then
  custom_font_count="$(count_custom_fonts)"
  desired_state="$(build_font_state)"
  saved_state="$(current_state || true)"
  rebuild_reason=""

  if [ "${FORCE_REBUILD}" != "true" ] &&
    ! font_outputs_exist &&
    [ -n "${saved_state}" ] &&
    [ "${saved_state}" = "${desired_state}" ] &&
    cached_font_outputs_exist; then
    echo "[wunder-onlyoffice] restoring cached font indexes"
    restore_cached_font_outputs || true
  fi

  if [ "${FORCE_REBUILD}" = "true" ]; then
    rebuild_reason="forced"
  elif ! font_outputs_exist; then
    rebuild_reason="missing-index-files"
  elif [ -z "${saved_state}" ] && [ "${custom_font_count}" -gt 0 ]; then
    rebuild_reason="initial-custom-font-build"
  elif [ -n "${saved_state}" ] && [ "${saved_state}" != "${desired_state}" ]; then
    rebuild_reason="font-or-release-changed"
  fi

  if [ -n "${rebuild_reason}" ]; then
    echo "[wunder-onlyoffice] rebuilding font indexes (${rebuild_reason})"
    refresh_font_indexes
    cache_font_outputs || true
    write_state "${desired_state}"
  else
    cache_font_outputs || true
    if [ -z "${saved_state}" ]; then
      write_state "${desired_state}"
    fi
    echo "[wunder-onlyoffice] font indexes unchanged; skipping rebuild"
  fi
fi

exec env GENERATE_FONTS=false /app/ds/run-document-server.sh
