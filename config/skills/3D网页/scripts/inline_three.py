#!/usr/bin/env python3
"""Inline the bundled Three.js runtimes into a self-contained HTML file.

Two source forms are accepted for every bundled asset (Three.js core and the
GLB loader):

- a marker comment ``<!-- INLINE:<asset> -->`` inside one <script> block, e.g.
  ``<!-- INLINE:three.min.js -->`` or ``<!-- INLINE:GLTFLoader.js -->``;
- a ``<script src="...assets/<asset>"></script>`` tag (the readable examples
  form), e.g. ``<script src="../assets/GLTFLoader.js"></script>``.

Every marker/tag found is replaced in place, so a page that needs both the
runtime and the loader ends up with both embedded and no external reference.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

ASSETS = ("three.min.js", "GLTFLoader.js")
LICENSE = "LICENSE-THREE.txt"


def _asset_code(skill_root: Path, asset: str, license_comment: str) -> str:
    source = (skill_root / "assets" / asset).read_text(encoding="utf-8")
    if not source.endswith("\n"):
        source += "\n"
    return license_comment + source


def _asset_script(skill_root: Path, asset: str, license_comment: str) -> str:
    return "<script>\n" + _asset_code(skill_root, asset, license_comment) + "</script>"


def inline_assets(source: Path, destination: Path, skill_root: Path) -> list[str]:
    html = source.read_text(encoding="utf-8")
    license_text = (skill_root / "assets" / LICENSE).read_text(encoding="utf-8").strip()
    license_comment = "/* Bundled Three.js license:\n" + license_text + "\n*/\n"
    bundled: list[str] = []

    for asset in ASSETS:
        marker = "<!-- INLINE:" + asset + " -->"
        tag = re.compile(
            r'<script src="(?:\.\./)*assets/' + re.escape(asset) + r'"></script>'
        )
        count = html.count(marker)
        has_tag = bool(tag.search(html))
        if count == 0 and not has_tag:
            continue
        if count > 1:
            raise ValueError(f"expected at most one {marker!r}, found {count}")
        if count == 1:
            # The marker already sits inside a <script> element, so only the code
            # goes in: wrapping it again would nest script tags and break the page.
            html = html.replace(marker, _asset_code(skill_root, asset, license_comment))
        else:
            # Use a replacement function so backslashes in the runtime are kept
            # literally instead of being read as regex escapes.
            body = _asset_script(skill_root, asset, license_comment)
            html = tag.sub(lambda _m: body, html, count=1)
        bundled.append(asset)

    if not bundled:
        raise ValueError(
            "no bundled asset marker found; expected one of "
            + ", ".join("<!-- INLINE:" + asset + " -->" for asset in ASSETS)
        )

    leftover = re.findall(r'<script src="[^"]*assets/[^"]*"></script>', html)
    if leftover:
        raise ValueError(
            "HTML still references bundled assets after inlining: " + ", ".join(leftover)
        )

    destination.parent.mkdir(parents=True, exist_ok=True)
    # Python 3.8 没有 Path.write_text(newline=...)，显式开文件才能固定行尾。
    with open(destination, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(html)
    return bundled


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Inline the bundled Three.js runtime and GLB loader into HTML"
    )
    parser.add_argument("source", type=Path, help="HTML containing the inline markers")
    parser.add_argument("destination", type=Path, help="output HTML path")
    args = parser.parse_args()
    bundled = inline_assets(args.source, args.destination, Path(__file__).resolve().parents[1])
    print("inlined: " + ", ".join(bundled))


if __name__ == "__main__":
    main()
