#!/usr/bin/env python3
"""Refresh the design sheets everywhere they are used.

Run after editing any SVG under docs/wunder美术设计稿:

    python scripts/sync_art.py

It regenerates the 1200px previews embedded by the desktop help, rebuilds the
embedded manual JSON, and rebuilds the docs site (self-contained SVGs plus the
gallery index). The desktop app still needs its own rebuild: the previews are
compiled into the binary with include_bytes!.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STEPS = ("render_art_previews.py", "build_help_manual.py", "build_docs_site.py")


def main() -> None:
    for name in STEPS:
        print(f"== {name}")
        subprocess.run(
            [sys.executable, str(ROOT / "scripts" / name)],
            cwd=ROOT,
            check=True,
        )
    print("\n资产已同步；蛹需要重新编译才能把新预览图嵌进二进制。")


if __name__ == "__main__":
    main()
