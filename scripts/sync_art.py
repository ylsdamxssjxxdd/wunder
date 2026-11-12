#!/usr/bin/env python3
"""Refresh the design sheets everywhere they are used outside the desktop help.

Run after editing any SVG under docs/wunder美术设计稿 (蛹 or 舵机):

    python scripts/sync_art.py

The desktop help no longer embeds design-sheet previews: it ships a real
interface screenshot instead, so no raster step is needed here. This script
rebuilds the docs site (self-contained SVGs plus the gallery index).

The desktop app still needs its own rebuild when the embedded screenshot
changes, because the image is compiled into the binary with include_bytes!.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STEPS = ("build_docs_site.py",)


def main() -> None:
    for name in STEPS:
        print(f"== {name}")
        subprocess.run(
            [sys.executable, str(ROOT / "scripts" / name)],
            cwd=ROOT,
            check=True,
        )
    print("\n资产已同步；文档站已重建。")


if __name__ == "__main__":
    main()
