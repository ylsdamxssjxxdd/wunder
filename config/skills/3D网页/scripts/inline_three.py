#!/usr/bin/env python3
"""Inline the bundled Three.js runtime into a self-contained HTML file.

Accepts two source forms:
- a template containing the marker ``<!-- INLINE:three.min.js -->`` inside one
  <script> block, or
- a source file whose <script src="..."> tag references assets/three.min.js
  (the readable examples form).
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path


MARKER = "<!-- INLINE:three.min.js -->"
SRC_PATTERN = re.compile(r'<script src="(?:\.\./)*assets/three\.min\.js"></script>')


def inline_three(source: Path, destination: Path, skill_root: Path) -> None:
    html = source.read_text(encoding="utf-8")
    runtime = (skill_root / "assets" / "three.min.js").read_text(encoding="utf-8")
    license_text = (skill_root / "assets" / "LICENSE-THREE.txt").read_text(encoding="utf-8").strip()
    license_comment = "/* Bundled Three.js license:\n" + license_text + "\n*/\n"

    count = html.count(MARKER)
    if count == 1:
        output = html.replace(MARKER, license_comment + runtime)
    elif count == 0:
        # Source form: replace the runtime <script src> tag with the embedded
        # runtime. Use a replacement function so backslashes in the runtime are
        # kept literally instead of being interpreted as regex escapes.
        matches = SRC_PATTERN.findall(html)
        if len(matches) != 1:
            raise ValueError(
                f"expected exactly one {MARKER!r} or one three.min.js <script src>, "
                f"found marker={count}, src tags={len(matches)}"
            )
        output = SRC_PATTERN.sub(
            lambda _m: "<script>\n" + license_comment + runtime + "\n</script>", html
        )
    else:
        raise ValueError(f"expected exactly one {MARKER!r}, found {count}")

    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(output, encoding="utf-8", newline="\n")


def main() -> None:
    parser = argparse.ArgumentParser(description="Inline the bundled Three.js runtime into HTML")
    parser.add_argument("source", type=Path, help="HTML containing the inline marker")
    parser.add_argument("destination", type=Path, help="output HTML path")
    args = parser.parse_args()
    inline_three(args.source, args.destination, Path(__file__).resolve().parents[1])


if __name__ == "__main__":
    main()
