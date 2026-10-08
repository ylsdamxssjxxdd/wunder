#!/usr/bin/env bash
set -euo pipefail

CUSTOM_FONT_DIR="${ONLYOFFICE_CUSTOM_FONT_DIR:-/var/www/onlyoffice/Data/custom-fonts}"
SYSTEM_FONT_DIR="${ONLYOFFICE_WUNDER_SYSTEM_FONT_DIR:-/usr/share/fonts/truetype/wunder}"
GENERATE_FONTS="${ONLYOFFICE_GENERATE_WUNDER_FONTS:-true}"
LEGACY_STATE_DIR="${ONLYOFFICE_WUNDER_STATE_DIR:-/var/lib/onlyoffice/.wunder}"
AI_PLUGIN_ENABLED="${ONLYOFFICE_WUNDER_AI_PLUGIN_ENABLED:-true}"
AI_PLUGIN_GUID="${ONLYOFFICE_WUNDER_AI_PLUGIN_GUID:-asc.{9DC93CDB-B576-4F0C-B55E-FCC9C48DD007}}"
AI_PLUGIN_DIR="${ONLYOFFICE_WUNDER_AI_PLUGIN_DIR:-/var/www/onlyoffice/documentserver/sdkjs-plugins/{9DC93CDB-B576-4F0C-B55E-FCC9C48DD007}}"
LOCAL_CONFIG="${ONLYOFFICE_WUNDER_LOCAL_CONFIG:-/etc/onlyoffice/documentserver/local.json}"

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

configure_ai_plugin_autostart() {
  if [ "${AI_PLUGIN_ENABLED}" != "true" ]; then
    return
  fi
  if [ ! -f "${AI_PLUGIN_DIR}/config.json" ]; then
    echo "[wunder-onlyoffice] AI plugin config not found at ${AI_PLUGIN_DIR}/config.json; skipping autostart"
    return
  fi

  echo "[wunder-onlyoffice] ensuring AI plugin autostart in ${LOCAL_CONFIG}"
  python3 - "${LOCAL_CONFIG}" "${AI_PLUGIN_GUID}" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
guid = sys.argv[2]

if path.exists():
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        data = {}
else:
    data = {}

services = data.setdefault("services", {})
coauthoring = services.setdefault("CoAuthoring", {})
plugins = coauthoring.setdefault("plugins", {})
plugins.setdefault("uri", "/sdkjs-plugins")
autostart = plugins.setdefault("autostart", [])

if not isinstance(autostart, list):
    autostart = []
    plugins["autostart"] = autostart

if guid not in autostart:
    autostart.append(guid)

path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
PY
}

if [ "${GENERATE_FONTS}" = "true" ]; then
  echo "[wunder-onlyoffice] rebuilding font indexes without cache"
  remove_legacy_font_cache
  refresh_font_indexes
fi

configure_ai_plugin_autostart

exec env GENERATE_FONTS=false /app/ds/run-document-server.sh
