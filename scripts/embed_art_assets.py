#!/usr/bin/env python3
"""Inline PNG assets into the art sheets that reference them.

Sheets are opened both from the source directory and as `<img>` in the docs
gallery. Browsers do not load nested external resources from an SVG used as an
image, so any screenshot or mark referenced by a sheet has to be embedded as a
data URI. The SVG keeps `data-asset="<file>"` as the marker, so re-running this
script after replacing the PNG refreshes the payload instead of stacking copies.

Run from the repository root (sync_art.py does this first):

    python scripts/embed_art_assets.py
"""

from __future__ import annotations

import base64
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SHEET_ROOT = ROOT / "docs" / "wunder美术设计稿"
SHEETS = ("desktop", "cli")
IMAGE_RE = re.compile(r'(<image\b[^>]*\bdata-asset="([^"]+)"[^>]*\bhref=")([^"]*)(")')


def main() -> int:
    changed = 0
    for form in SHEETS:
        for sheet in sorted((SHEET_ROOT / form).glob("*.svg")):
            text = sheet.read_text(encoding="utf-8")
            assets = sheet.parent / "assets"

            def replace(match: re.Match[str]) -> str:
                name = match.group(2)
                asset = assets / name
                if not asset.is_file():
                    raise FileNotFoundError(f"{sheet.name}: 缺少资产 {asset}")
                payload = base64.b64encode(asset.read_bytes()).decode("ascii")
                return f"{match.group(1)}data:image/png;base64,{payload}{match.group(4)}"

            updated = IMAGE_RE.sub(replace, text)
            if updated != text:
                sheet.write_text(updated, encoding="utf-8", newline="\n")
                changed += 1
    print(f"内嵌资产：更新 {changed} 张分页")
    return 0


if __name__ == "__main__":
    sys.exit(main())
