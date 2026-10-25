#!/usr/bin/env python3
"""Render the desktop UI design sheets to PNG previews for the native help.

The docs site embeds the SVG sheets directly; only the Slint help needs
rasters because it decodes them from memory. Output therefore goes straight
to the one place that embeds them, with no second PNG copy in the repo.
Rerun after editing a sheet:

    python scripts/render_art_previews.py
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import tempfile
import urllib.parse
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SOURCE_DIR = ROOT / "docs" / "wunder美术设计稿"
OUTPUT_DIR = ROOT / "frontend-slint" / "assets" / "help" / "art"
PREVIEW_WIDTH = 1200

SHEETS = {
    "00-整体总览": "00-overview",
    "01-基础规范": "01-foundations",
    "02-图标系统": "02-icons",
    "03-外壳与侧栏": "03-shell-sidebar",
    "04-会话时间线": "04-timeline",
    "05-过程与工具": "05-process-tools",
    "06-计划与问询": "06-plan-inquiry",
    "07-输入区": "07-composer",
    "08-设置与弹窗": "08-settings-dialogs",
    "09-交互状态与动效": "09-states-motion",
}

EDGE_CANDIDATES = [
    os.environ.get("MSEDGE_BIN", ""),
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
]


def find_edge() -> str:
    for candidate in EDGE_CANDIDATES:
        if candidate and Path(candidate).is_file():
            return candidate
    found = shutil.which("msedge")
    if found:
        return found
    raise SystemExit("Microsoft Edge not found; set MSEDGE_BIN to msedge.exe")


def sheet_size(svg: Path) -> tuple[int, int]:
    head = svg.read_text(encoding="utf-8")[:400]
    width = re.search(r'\bwidth="([\d.]+)"', head)
    height = re.search(r'\bheight="([\d.]+)"', head)
    if width and height:
        return int(float(width.group(1))), int(float(height.group(1)))
    viewbox = re.search(r'viewBox="([\d.\s]+)"', head)
    if viewbox:
        _, _, w, h = viewbox.group(1).split()
        return int(float(w)), int(float(h))
    raise SystemExit(f"cannot determine size: {svg.name}")


def render(edge: str, svg: Path, target: Path, size: tuple[int, int], profile: Path) -> None:
    uri = "file:///" + urllib.parse.quote(svg.as_posix(), safe="/:")
    command = [
        edge,
        "--headless=new",
        "--disable-gpu",
        "--no-sandbox",
        "--hide-scrollbars",
        f"--user-data-dir={profile}",
        f"--window-size={size[0]},{size[1]}",
        f"--screenshot={target}",
        uri,
    ]
    result = subprocess.run(command, capture_output=True, text=True, timeout=120)
    if not target.is_file():
        raise SystemExit(
            f"render failed for {svg.name} (exit {result.returncode})\n"
            f"{result.stderr[-2000:]}"
        )


def main() -> None:
    edge = find_edge()
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="art-render-") as scratch:
        profile = Path(scratch) / "profile"
        for stem, slug in SHEETS.items():
            svg = SOURCE_DIR / f"{stem}.svg"
            if not svg.is_file():
                raise SystemExit(f"missing sheet: {svg}")
            raw = Path(scratch) / f"{slug}.png"
            render(edge, svg, raw, sheet_size(svg), profile)
            with Image.open(raw) as image:
                image = image.convert("RGB")
                height = round(image.height * PREVIEW_WIDTH / image.width)
                image = image.resize((PREVIEW_WIDTH, height), Image.LANCZOS)
                output = OUTPUT_DIR / f"{slug}.png"
                image.save(output, "PNG", optimize=True)
                print(f"{output.relative_to(ROOT)}  {output.stat().st_size // 1024} KiB")


if __name__ == "__main__":
    main()
