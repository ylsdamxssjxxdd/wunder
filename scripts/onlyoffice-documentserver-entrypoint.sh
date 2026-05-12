#!/usr/bin/env bash
set -euo pipefail

CUSTOM_FONT_DIR="${ONLYOFFICE_CUSTOM_FONT_DIR:-/var/www/onlyoffice/Data/custom-fonts}"
GENERATE_FONTS="${ONLYOFFICE_GENERATE_WUNDER_FONTS:-true}"
FORCE_REBUILD="${ONLYOFFICE_FORCE_FONT_REBUILD:-false}"
PATCH_CURSORS="${ONLYOFFICE_PATCH_WUNDER_CURSORS:-true}"
STATE_DIR="${ONLYOFFICE_WUNDER_STATE_DIR:-/var/lib/onlyoffice/.wunder}"
STATE_FILE="${STATE_DIR}/font-index-state.sha256"
CACHE_DIR="${STATE_DIR}/font-index-cache"
CACHE_ALL_FONTS_WEB="${CACHE_DIR}/AllFonts.sdkjs.js"
CACHE_ALL_FONTS_BIN="${CACHE_DIR}/AllFonts.converter.js"
CACHE_FONT_SELECTION_BIN="${CACHE_DIR}/font_selection.bin"
CACHE_FONT_ASSETS_DIR="${CACHE_DIR}/fonts"
CACHE_FONT_THUMBNAILS_DIR="${CACHE_DIR}/font-thumbnails"
SCRIPT_SCHEMA_VERSION="v4"
DOCSERVICE_BIN="${ONLYOFFICE_DOCSERVICE_BIN:-/var/www/onlyoffice/documentserver/server/DocService/docservice}"
ALL_FONTS_WEB="${ONLYOFFICE_ALL_FONTS_WEB:-/var/www/onlyoffice/documentserver/sdkjs/common/AllFonts.js}"
ALL_FONTS_BIN="${ONLYOFFICE_ALL_FONTS_BIN:-/var/www/onlyoffice/documentserver/server/FileConverter/bin/AllFonts.js}"
FONT_SELECTION_BIN="${ONLYOFFICE_FONT_SELECTION_BIN:-/var/www/onlyoffice/documentserver/server/FileConverter/bin/font_selection.bin}"
FONT_ASSETS_DIR="${ONLYOFFICE_FONT_ASSETS_DIR:-/var/www/onlyoffice/documentserver/fonts}"
FONT_THUMBNAILS_DIR="${ONLYOFFICE_FONT_THUMBNAILS_DIR:-/var/www/onlyoffice/documentserver/sdkjs/common/Images}"
FONT_THUMBNAILS_GLOB="${ONLYOFFICE_FONT_THUMBNAILS_GLOB:-fonts_thumbnail*}"
DOCUMENTSERVER_ROOT="${ONLYOFFICE_DOCUMENTSERVER_ROOT:-/var/www/onlyoffice/documentserver}"
CURSOR_DIR="${ONLYOFFICE_CURSOR_DIR:-${DOCUMENTSERVER_ROOT}/sdkjs/common/Images/cursors}"
WEB_APPS_DIR="${ONLYOFFICE_WEB_APPS_DIR:-${DOCUMENTSERVER_ROOT}/web-apps/apps}"

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

dir_has_files() {
  [ -d "$1" ] && [ -n "$(find "$1" -mindepth 1 -maxdepth 1 -type f -print -quit 2>/dev/null)" ]
}

dir_has_glob_files() {
  [ -d "$1" ] && [ -n "$(find "$1" -mindepth 1 -maxdepth 1 -type f -name "$2" -print -quit 2>/dev/null)" ]
}

font_outputs_exist() {
  [ -s "${ALL_FONTS_WEB}" ] &&
    [ -s "${ALL_FONTS_BIN}" ] &&
    [ -s "${FONT_SELECTION_BIN}" ] &&
    dir_has_files "${FONT_ASSETS_DIR}" &&
    dir_has_glob_files "${FONT_THUMBNAILS_DIR}" "${FONT_THUMBNAILS_GLOB}"
}

cached_font_outputs_exist() {
  [ -s "${CACHE_ALL_FONTS_WEB}" ] &&
    [ -s "${CACHE_ALL_FONTS_BIN}" ] &&
    [ -s "${CACHE_FONT_SELECTION_BIN}" ] &&
    dir_has_files "${CACHE_FONT_ASSETS_DIR}" &&
    dir_has_files "${CACHE_FONT_THUMBNAILS_DIR}"
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
  rm -rf "${CACHE_FONT_ASSETS_DIR}.tmp"
  mkdir -p "${CACHE_FONT_ASSETS_DIR}.tmp"
  cp -a "${FONT_ASSETS_DIR}/." "${CACHE_FONT_ASSETS_DIR}.tmp/"
  rm -rf "${CACHE_FONT_ASSETS_DIR}"
  mv "${CACHE_FONT_ASSETS_DIR}.tmp" "${CACHE_FONT_ASSETS_DIR}"
  rm -rf "${CACHE_FONT_THUMBNAILS_DIR}.tmp"
  mkdir -p "${CACHE_FONT_THUMBNAILS_DIR}.tmp"
  find "${FONT_THUMBNAILS_DIR}" -mindepth 1 -maxdepth 1 -type f -name "${FONT_THUMBNAILS_GLOB}" \
    -exec cp -a {} "${CACHE_FONT_THUMBNAILS_DIR}.tmp/" \;
  rm -rf "${CACHE_FONT_THUMBNAILS_DIR}"
  mv "${CACHE_FONT_THUMBNAILS_DIR}.tmp" "${CACHE_FONT_THUMBNAILS_DIR}"
}

restore_cached_font_outputs() {
  cached_font_outputs_exist || return 1
  mkdir -p "$(dirname "${ALL_FONTS_WEB}")" "$(dirname "${ALL_FONTS_BIN}")" "$(dirname "${FONT_SELECTION_BIN}")" "${FONT_ASSETS_DIR}" "${FONT_THUMBNAILS_DIR}"
  cp -f "${CACHE_ALL_FONTS_WEB}" "${ALL_FONTS_WEB}"
  cp -f "${CACHE_ALL_FONTS_BIN}" "${ALL_FONTS_BIN}"
  cp -f "${CACHE_FONT_SELECTION_BIN}" "${FONT_SELECTION_BIN}"
  find "${FONT_ASSETS_DIR}" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
  cp -a "${CACHE_FONT_ASSETS_DIR}/." "${FONT_ASSETS_DIR}/"
  find "${FONT_THUMBNAILS_DIR}" -mindepth 1 -maxdepth 1 -type f -name "${FONT_THUMBNAILS_GLOB}" -delete
  cp -a "${CACHE_FONT_THUMBNAILS_DIR}/." "${FONT_THUMBNAILS_DIR}/"
}

refresh_font_indexes() {
  echo "[wunder-onlyoffice] refreshing fontconfig cache from ${CUSTOM_FONT_DIR}"
  fc-cache -f "${CUSTOM_FONT_DIR}" || fc-cache -f || true

  echo "[wunder-onlyoffice] generating OnlyOffice font indexes"
  /usr/bin/documentserver-generate-allfonts.sh true
}

patch_onlyoffice_cursors() {
  if ! command -v python3 >/dev/null 2>&1; then
    echo "[wunder-onlyoffice] python3 missing; skipping cursor patch"
    return 0
  fi

  ONLYOFFICE_CURSOR_DIR="${CURSOR_DIR}" \
    ONLYOFFICE_WEB_APPS_DIR="${WEB_APPS_DIR}" \
    python3 - <<'PY'
import gzip
import io
import json
import os
from pathlib import Path

cursor_dir = Path(os.environ["ONLYOFFICE_CURSOR_DIR"])
web_apps_dir = Path(os.environ["ONLYOFFICE_WEB_APPS_DIR"])

if not cursor_dir.is_dir():
    print(f"[wunder-onlyoffice] cursor directory missing; skipping cursor patch: {cursor_dir}")
    raise SystemExit(0)

def gzip_bytes(payload):
    gz_buffer = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=gz_buffer, mtime=0) as gz_file:
        gz_file.write(payload)
    return gz_buffer.getvalue()

def replace_existing_injection(content, replacement):
    marker = '    <script id="wunder-onlyoffice-cursor-patch">'
    start = content.find(marker)
    if start < 0:
        return None
    end = content.find("    </script>", start)
    if end < 0:
        return None
    end += len("    </script>")
    if content.startswith("\r\n", end):
        end += 2
    elif content.startswith("\n", end):
        end += 1
    return content[:start] + replacement + content[end:]

default_svg = "<svg width='24' height='24' viewBox='0 0 24 24' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M4 2L19 16H12L9 22L6 21L9 15H4V2Z' fill='black' stroke='white' stroke-width='4' stroke-linejoin='round'/><path d='M4 2L19 16H12L9 22L6 21L9 15H4V2Z' fill='black' stroke='black' stroke-width='1' stroke-linejoin='round'/></svg>"
text_svg = "<svg width='16' height='24' viewBox='0 0 16 24' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M4 3H12M8 3V21M4 21H12' stroke='white' stroke-width='5' stroke-linecap='square'/><path d='M4 3H12M8 3V21M4 21H12' stroke='black' stroke-width='2' stroke-linecap='square'/></svg>"
crosshair_svg = "<svg width='24' height='24' viewBox='0 0 24 24' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M12 2V22M2 12H22' stroke='white' stroke-width='5' stroke-linecap='square'/><path d='M12 2V22M2 12H22' stroke='black' stroke-width='2' stroke-linecap='square'/></svg>"
plus_svg = "<svg width='18' height='18' viewBox='0 0 18 18' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M7 1H11V7H17V11H11V17H7V11H1V7H7V1Z' fill='black' stroke='white' stroke-width='2' stroke-linejoin='round'/><path d='M7 1H11V7H17V11H11V17H7V11H1V7H7V1Z' fill='black'/></svg>"
copy_svg = "<svg width='28' height='22' viewBox='0 0 28 22' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M2 2L15 15H9L7 20L5 19L7 14H2V2Z' fill='black' stroke='white' stroke-width='3' stroke-linejoin='round'/><path d='M18 5H22V9H26V13H22V17H18V13H14V9H18V5Z' fill='black' stroke='white' stroke-width='2' stroke-linejoin='round'/><path d='M2 2L15 15H9L7 20L5 19L7 14H2V2Z' fill='black'/><path d='M18 5H22V9H26V13H22V17H18V13H14V9H18V5Z' fill='black'/></svg>"
text_copy_svg = "<svg width='24' height='24' viewBox='0 0 24 24' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M3 3H11M7 3V21M3 21H11' stroke='white' stroke-width='5' stroke-linecap='square'/><path d='M3 3H11M7 3V21M3 21H11' stroke='black' stroke-width='2' stroke-linecap='square'/><path d='M16 6H19V10H23V13H19V17H16V13H12V10H16V6Z' fill='black' stroke='white' stroke-width='2' stroke-linejoin='round'/><path d='M16 6H19V10H23V13H19V17H16V13H12V10H16V6Z' fill='black'/></svg>"
move_h_svg = "<svg width='24' height='24' viewBox='0 0 24 24' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M2 12L8 6V10H16V6L22 12L16 18V14H8V18L2 12Z' fill='black' stroke='white' stroke-width='3' stroke-linejoin='round'/><path d='M2 12L8 6V10H16V6L22 12L16 18V14H8V18L2 12Z' fill='black'/></svg>"
move_v_svg = "<svg width='24' height='24' viewBox='0 0 24 24' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M12 2L18 8H14V16H18L12 22L6 16H10V8H6L12 2Z' fill='black' stroke='white' stroke-width='3' stroke-linejoin='round'/><path d='M12 2L18 8H14V16H18L12 22L6 16H10V8H6L12 2Z' fill='black'/></svg>"
table_svg = "<svg width='20' height='20' viewBox='0 0 20 20' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M3 3H17V17H3V3Z' fill='white' stroke='black' stroke-width='3'/><path d='M3 8H17M3 13H17M8 3V17M13 3V17' stroke='black' stroke-width='2'/><path d='M1 1L8 8H4L2 12L1 11L3 7H1V1Z' fill='black' stroke='white' stroke-width='2' stroke-linejoin='round'/></svg>"
tool_svg = "<svg width='22' height='22' viewBox='0 0 22 22' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M15 2L20 7L8 19H3V14L15 2Z' fill='white' stroke='black' stroke-width='3' stroke-linejoin='round'/><path d='M15 2L20 7L8 19H3V14L15 2Z' fill='black'/><path d='M12 5L17 10' stroke='white' stroke-width='2'/></svg>"
hand_svg = "<svg width='20' height='24' viewBox='0 0 20 24' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M7 11V4C7 2 10 2 10 4V10V3C10 1 13 1 13 3V10V5C13 3 16 3 16 5V12V8C16 6 19 6 19 8V13C19 19 15 22 10 22C6 22 3 19 1 14C0 12 2 10 4 12L7 15V11Z' fill='black' stroke='white' stroke-width='3' stroke-linejoin='round'/><path d='M7 11V4C7 2 10 2 10 4V10V3C10 1 13 1 13 3V10V5C13 3 16 3 16 5V12V8C16 6 19 6 19 8V13C19 19 15 22 10 22C6 22 3 19 1 14C0 12 2 10 4 12L7 15V11Z' fill='black'/></svg>"

cursor_svgs = {
    "wunder-default": default_svg,
    "wunder-text": text_svg,
    "wunder-crosshair": crosshair_svg,
    "eyedropper": tool_svg,
    "grab": hand_svg,
    "grabbing": hand_svg,
    "marker-format": text_copy_svg,
    "move-border-horizontally": move_h_svg,
    "move-border-vertically": move_v_svg,
    "plus": plus_svg,
    "plus-copy": copy_svg,
    "select-table-cell": table_svg,
    "select-table-column": table_svg,
    "select-table-content": table_svg,
    "select-table-row": table_svg,
    "shape-copy": copy_svg,
    "table-eraser": tool_svg,
    "table-pen": tool_svg,
    "text-copy": text_copy_svg,
}

hotspots = {
    "wunder-default": "1 1",
    "wunder-text": "8 12",
    "wunder-crosshair": "12 12",
    "eyedropper": "1 20",
    "grab": "8 8",
    "grabbing": "8 8",
    "marker-format": "7 12",
    "move-border-horizontally": "12 12",
    "move-border-vertically": "12 12",
    "plus": "9 9",
    "plus-copy": "1 1",
    "select-table-cell": "1 1",
    "select-table-column": "10 10",
    "select-table-content": "10 10",
    "select-table-row": "10 8",
    "shape-copy": "1 1",
    "table-eraser": "3 18",
    "table-pen": "3 18",
    "text-copy": "8 12",
}

fallbacks = {
    "wunder-default": "default",
    "wunder-text": "text",
    "wunder-crosshair": "crosshair",
    "grab": "grab",
    "grabbing": "grabbing",
    "move-border-horizontally": "ew-resize",
    "move-border-vertically": "ns-resize",
    "text-copy": "text",
}

for name, svg in cursor_svgs.items():
    svg_path = cursor_dir / f"{name}.svg"
    svg_path.write_text(svg + "\n", encoding="utf-8")
    (cursor_dir / f"{name}.svg.gz").write_bytes(gzip_bytes((svg + "\n").encode("utf-8")))

json_path = cursor_dir / "svg.json"
try:
    cursor_json = json.loads(json_path.read_text(encoding="utf-8"))
except Exception:
    cursor_json = {}
cursor_json.update(cursor_svgs)
json_payload = json.dumps(cursor_json, ensure_ascii=False, indent=4, sort_keys=True) + "\n"
json_path.write_text(json_payload, encoding="utf-8")
(cursor_dir / "svg.json.gz").write_bytes(gzip_bytes(json_payload.encode("utf-8")))

js_svgs = json.dumps(cursor_svgs, ensure_ascii=False, separators=(",", ":"))
js_hotspots = json.dumps(hotspots, ensure_ascii=True, separators=(",", ":"))
js_fallbacks = json.dumps(fallbacks, ensure_ascii=True, separators=(",", ":"))
injection = f"""    <script id="wunder-onlyoffice-cursor-patch">
(function () {{
    if (window.__wunderOnlyOfficeCursorPatch) return;
    window.__wunderOnlyOfficeCursorPatch = true;
    var svg = {js_svgs};
    var hotspots = {js_hotspots};
    var fallbacks = {js_fallbacks};
    function makeCursor(name, fallback) {{
        var source = svg[name];
        if (!source) return fallback || "default";
        var hot = hotspots[name] || "1 1";
        return "url(\\"data:image/svg+xml;charset=utf-8," + encodeURIComponent(source) + "\\") " + hot + ", " + (fallback || fallbacks[name] || "default");
    }}
    var custom = {{}};
    Object.keys(svg).forEach(function (name) {{
        custom[name] = makeCursor(name, fallbacks[name]);
    }});
    var nativeCursor = {{
        "auto": custom["wunder-default"],
        "default": custom["wunder-default"],
        "text": custom["wunder-text"],
        "vertical-text": custom["wunder-text"],
        "crosshair": custom["wunder-crosshair"]
    }};
    function normalizeCursor(value) {{
        if (typeof value !== "string") return value;
        var trimmed = value.trim();
        if (!trimmed) return value;
        var lower = trimmed.toLowerCase();
        if (lower.indexOf("data:image/svg+xml") >= 0) return value;
        if (nativeCursor[lower]) return nativeCursor[lower];
        var match = lower.match(/(?:^|\\/)([a-z0-9-]+?)(?:_2x)?\\.(?:cur|png|svg)(?:[\\\"')\\s,]|$)/);
        if (match && custom[match[1]]) return custom[match[1]];
        return value;
    }}
    function normalizeStyleText(value) {{
        if (typeof value !== "string") return value;
        return value.replace(/(^|;)\\s*cursor\\s*:\\s*(auto|default|text|vertical-text|crosshair)\\s*(!important)?\\s*(?=;|$)/gi, function (_, prefix, keyword, important) {{
            return prefix + "cursor:" + normalizeCursor(keyword) + (important ? " !important" : "");
        }});
    }}
    try {{
        var styleProto = window.CSSStyleDeclaration && window.CSSStyleDeclaration.prototype;
        if (styleProto && !styleProto.__wunderOnlyOfficeCursorPatch) {{
            styleProto.__wunderOnlyOfficeCursorPatch = true;
            var originalSetProperty = styleProto.setProperty;
            if (originalSetProperty) {{
                styleProto.setProperty = function (name, value, priority) {{
                    if (name && String(name).toLowerCase() === "cursor") {{
                        value = normalizeCursor(value);
                    }}
                    return originalSetProperty.call(this, name, value, priority);
                }};
            }}
            var cursorDescriptor = Object.getOwnPropertyDescriptor(styleProto, "cursor");
            if (cursorDescriptor && cursorDescriptor.set && cursorDescriptor.get) {{
                Object.defineProperty(styleProto, "cursor", {{
                    configurable: true,
                    enumerable: cursorDescriptor.enumerable,
                    get: function () {{ return cursorDescriptor.get.call(this); }},
                    set: function (value) {{ cursorDescriptor.set.call(this, normalizeCursor(value)); }}
                }});
            }}
            var cssTextDescriptor = Object.getOwnPropertyDescriptor(styleProto, "cssText");
            if (cssTextDescriptor && cssTextDescriptor.set && cssTextDescriptor.get) {{
                Object.defineProperty(styleProto, "cssText", {{
                    configurable: true,
                    enumerable: cssTextDescriptor.enumerable,
                    get: function () {{ return cssTextDescriptor.get.call(this); }},
                    set: function (value) {{ cssTextDescriptor.set.call(this, normalizeStyleText(value)); }}
                }});
            }}
        }}
    }} catch (error) {{
        window.__wunderOnlyOfficeCursorPatchStyleError = String(error && error.message || error);
    }}
    try {{
        var elementProto = window.Element && window.Element.prototype;
        if (elementProto && elementProto.setAttribute && !elementProto.__wunderOnlyOfficeCursorPatch) {{
            elementProto.__wunderOnlyOfficeCursorPatch = true;
            var originalSetAttribute = elementProto.setAttribute;
            elementProto.setAttribute = function (name, value) {{
                if (name && String(name).toLowerCase() === "style") {{
                    value = normalizeStyleText(value);
                }}
                return originalSetAttribute.call(this, name, value);
            }};
        }}
    }} catch (error) {{
        window.__wunderOnlyOfficeCursorPatchElementError = String(error && error.message || error);
    }}
}})();
    </script>
"""

editor_pages = [
    web_apps_dir / "documenteditor/main/index.html",
    web_apps_dir / "spreadsheeteditor/main/index.html",
    web_apps_dir / "presentationeditor/main/index.html",
    web_apps_dir / "pdfeditor/main/index.html",
    web_apps_dir / "visioeditor/main/index.html",
]

patched_pages = 0
updated_pages = 0
for page in editor_pages:
    if not page.is_file():
        continue
    content = page.read_text(encoding="utf-8")
    updated_content = replace_existing_injection(content, injection)
    if updated_content is not None:
        content = updated_content
        page.write_text(content, encoding="utf-8")
        updated_pages += 1
    else:
        marker = "</head>"
        index = content.find(marker)
        if index < 0:
            print(f"[wunder-onlyoffice] no </head> marker in {page}; skipping page injection")
            continue
        content = content[:index] + injection + content[index:]
        page.write_text(content, encoding="utf-8")
        patched_pages += 1
    page.with_name(page.name + ".gz").write_bytes(gzip_bytes(page.read_bytes()))

print(f"[wunder-onlyoffice] patched high-contrast cursors ({len(cursor_svgs)} assets, {patched_pages} new pages, {updated_pages} updated pages)")
PY
}

if [ "${PATCH_CURSORS}" = "true" ]; then
  patch_onlyoffice_cursors || true
fi

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
