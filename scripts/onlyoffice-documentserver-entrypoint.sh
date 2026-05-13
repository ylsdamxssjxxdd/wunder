#!/usr/bin/env bash
set -euo pipefail

CUSTOM_FONT_DIR="${ONLYOFFICE_CUSTOM_FONT_DIR:-/var/www/onlyoffice/Data/custom-fonts}"
SYSTEM_FONT_DIR="${ONLYOFFICE_WUNDER_SYSTEM_FONT_DIR:-/usr/share/fonts/truetype/wunder}"
GENERATE_FONTS="${ONLYOFFICE_GENERATE_WUNDER_FONTS:-true}"
LEGACY_STATE_DIR="${ONLYOFFICE_WUNDER_STATE_DIR:-/var/lib/onlyoffice/.wunder}"

remove_legacy_font_cache() {
  rm -rf "${LEGACY_STATE_DIR}/font-index-cache" "${LEGACY_STATE_DIR}/font-index-state.sha256"
}

refresh_font_indexes() {
  echo "[wunder-onlyoffice] syncing custom fonts into ${SYSTEM_FONT_DIR}"
  rm -rf "${SYSTEM_FONT_DIR}"
  mkdir -p "${SYSTEM_FONT_DIR}"

  if [ -d "${CUSTOM_FONT_DIR}" ]; then
    find "${CUSTOM_FONT_DIR}" -type f \( \
      -iname '*.ttf' -o \
      -iname '*.ttc' -o \
      -iname '*.otf' -o \
      -iname '*.woff' -o \
      -iname '*.woff2' \
    \) -exec cp -f {} "${SYSTEM_FONT_DIR}/" \;
  fi

  echo "[wunder-onlyoffice] refreshing fontconfig cache from ${CUSTOM_FONT_DIR}"
  fc-cache -f "${CUSTOM_FONT_DIR}" "${SYSTEM_FONT_DIR}" || fc-cache -f || true

  echo "[wunder-onlyoffice] generating OnlyOffice font indexes"
  /usr/bin/documentserver-generate-allfonts.sh true
}

if [ "${GENERATE_FONTS}" = "true" ]; then
  echo "[wunder-onlyoffice] rebuilding font indexes without cache"
  remove_legacy_font_cache
  refresh_font_indexes
fi

exec env GENERATE_FONTS=false /app/ds/run-document-server.sh
