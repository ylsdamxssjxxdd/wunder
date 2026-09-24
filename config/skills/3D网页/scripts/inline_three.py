#!/usr/bin/env python3
"""Inline the bundled Three.js runtime into a self-contained HTML file."""

from __future__ import annotations

import argparse
from pathlib import Path


MARKER = "<!-- INLINE:three.min.js -->"


def inline_three(source: Path, destination: Path, skill_root: Path) -> None:
    html = source.read_text(encoding="utf-8")
    count = html.count(MARKER)
    if count != 1:
        raise ValueError(f"expected exactly one {MARKER!r}, found {count}")

    runtime = (skill_root / "assets" / "three.min.js").read_text(encoding="utf-8")
    license_text = (skill_root / "assets" / "LICENSE-THREE.txt").read_text(encoding="utf-8").strip()
    license_comment = "/* Bundled Three.js license:\n" + license_text + "\n*/\n"
    output = html.replace(MARKER, license_comment + runtime)
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
