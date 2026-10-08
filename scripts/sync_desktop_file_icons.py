#!/usr/bin/env python3
"""Sync the desktop workspace file icons with the web frontend.

The web workspace panel resolves icons through the vscode-icons theme
(`frontend/src/assets/vscode-icons-theme.json`) plus the curated fallback
tables in `frontend/src/components/chat/workspaceIcons.ts`. This script
mirrors that resolution, writes the resulting mapping as a compact JSON for
the desktop build, and copies the referenced icon assets from
`frontend/public/` into `frontend-slint/assets/`.

Re-run whenever the web theme JSON or workspaceIcons.ts tables change:
    python scripts/sync_desktop_file_icons.py
"""

from __future__ import annotations

import json
import re
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
THEME_JSON = ROOT / "frontend/src/assets/vscode-icons-theme.json"
WORKSPACE_ICONS_TS = ROOT / "frontend/src/components/chat/workspaceIcons.ts"
WEB_ICONS_DIR = ROOT / "frontend/public/vscode-icons/icons"
WEB_DOC_ICONS_DIR = ROOT / "frontend/public/doc-icons"
OUT_DIR = ROOT / "frontend-slint/assets/vscode-icons"
OUT_DOC_DIR = ROOT / "frontend-slint/assets/doc-icons"

DIAGRAM_EXTENSIONS = ["dio", "drawio", "drawio.xml"]


def parse_workspace_icons_ts() -> tuple[dict[str, str], set[str]]:
    source = WORKSPACE_ICONS_TS.read_text(encoding="utf-8")
    block = re.search(
        r"FALLBACK_EXTENSION_ICON_ENTRIES[^=]*=\s*\[(.*?)\];", source, re.S
    )
    if not block:
        raise SystemExit("FALLBACK_EXTENSION_ICON_ENTRIES not found in workspaceIcons.ts")
    fallback: dict[str, str] = {}
    for ext, icon_id in re.findall(r"\['([^']+)',\s*'([^']+)'\]", block.group(1)):
        fallback[ext.strip().lower()] = icon_id
    extra_block = re.search(r"EXTRA_ALLOWED_ICON_IDS\s*=\s*\[(.*?)\];", source, re.S)
    if not extra_block:
        raise SystemExit("EXTRA_ALLOWED_ICON_IDS not found in workspaceIcons.ts")
    extra = set(re.findall(r"'([^']+)'", extra_block.group(1)))
    return fallback, extra


def normalize_icon_path(icon_path: object) -> str:
    raw = str(icon_path or "").strip()
    while raw.startswith("../") or raw.startswith("./"):
        raw = raw[3:]
    return raw


def main() -> None:
    theme = json.loads(THEME_JSON.read_text(encoding="utf-8"))
    definitions: dict[str, dict] = theme.get("iconDefinitions") or {}
    default_id = str(theme.get("file") or "").strip()
    fallback, extra = parse_workspace_icons_ts()

    def icon_file(icon_id: str) -> str | None:
        definition = definitions.get(icon_id) or {}
        path = normalize_icon_path(definition.get("iconPath"))
        return path.split("/")[-1] if path else None

    def on_disk(icon_file_name: str | None) -> bool:
        return bool(icon_file_name) and (WEB_ICONS_DIR / str(icon_file_name)).is_file()

    default_file = icon_file(default_id)
    if not default_file:
        raise SystemExit("theme.file icon id has no iconPath")

    def resolve_id(icon_id: object) -> str:
        key = str(icon_id or "").strip()
        if key and key in definitions and (key == default_id or key in allowed_ids):
            resolved = icon_file(key)
            if resolved and on_disk(resolved):
                return resolved
        return default_file

    allowed_ids = {default_id, *fallback.values(), *extra} & set(definitions)

    # Web resolution order: fileExtensions first (last duplicate wins, mirroring
    # the JS Map construction), then the curated fallback list for extensions the
    # theme does not cover. Icon ids outside the allowed set degrade to the
    # default icon exactly like the web resolver.
    extensions: dict[str, str] = {}
    for key, value in (theme.get("fileExtensions") or {}).items():
        extensions[str(key).strip().lower()] = resolve_id(value)
    for ext, icon_id in fallback.items():
        if icon_id in definitions:
            extensions.setdefault(ext, resolve_id(icon_id))

    # File names only surface when the extension lookup misses, and the web
    # resolver treats a default result as a miss, so keep only real overrides.
    names: dict[str, str] = {}
    for key, value in (theme.get("fileNames") or {}).items():
        resolved = resolve_id(value)
        if resolved != default_file:
            names[str(key).strip().lower()] = resolved

    used = {default_file, *(extensions | names).values()}
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for stale in OUT_DIR.glob("*.svg"):
        stale.unlink()
    for icon in sorted(used):
        source = WEB_ICONS_DIR / icon
        if not source.is_file():
            raise SystemExit(f"icon missing on web disk: {icon}")
        shutil.copyfile(source, OUT_DIR / icon)

    OUT_DOC_DIR.mkdir(parents=True, exist_ok=True)
    for name in ("folder.png", "processon_flow.png"):
        shutil.copyfile(WEB_DOC_ICONS_DIR / name, OUT_DOC_DIR / name)

    mapping = {
        "default": default_file,
        "folder": "folder.png",
        "diagram": "processon_flow.png",
        "diagram-extensions": DIAGRAM_EXTENSIONS,
        "extensions": dict(sorted(extensions.items())),
        "names": dict(sorted(names.items())),
    }
    (OUT_DIR / "file-icon-map.json").write_text(
        json.dumps(mapping, ensure_ascii=False, indent=1, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        f"icons: {len(used)} files, extensions: {len(extensions)}, "
        f"names: {len(names)} -> {OUT_DIR}"
    )


if __name__ == "__main__":
    main()
