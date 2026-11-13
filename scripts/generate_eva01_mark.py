#!/usr/bin/env python3
"""Regenerate the greeting mark (``assets/eva01-mark.png``).

The greeting sits at the head of every chat transcript, right before the
14 px greeting line. The window icon does not fit there: its artwork is cut
flat at the neck and at both fins, and at 30 px those straight edges read as
a cropped picture. This mark is prepared for the small slot instead:

- trimmed to the artwork, so no transparent margin from the 512 canvas is
  carried into the layout;
- the bottom 12 % fades out, so the cut neckline dissolves instead of ending
  on a hard horizontal stroke;
- the art occupies 85 % of a square canvas, which gives the rounded tile in
  ``timeline.slint`` room around every edge.

Rendered at 2x of the 40 px display size, so a HiDPI screen still gets one
source pixel per device pixel at scale 2 and a clean 2:1 downscale at 1x.

Usage: python3 scripts/generate_eva01_mark.py
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "images" / "eva01-head.png"
TARGET = ROOT / "crates" / "wunder-desktop" / "frontend" / "assets" / "eva01-mark.png"

BOX = 80  # 2x of the 40 px tile
FILL = 0.85  # artwork width inside the square canvas
FADE = 0.12  # share of the artwork height that dissolves at the bottom
SIDE_FADE = 0.05  # share of the artwork width that dissolves at each fin


def build() -> Image.Image:
    source = Image.open(SOURCE).convert("RGBA")
    art = source.crop(source.split()[3].getbbox())

    width = round(BOX * FILL)
    height = round(art.height * width / art.width)
    pixels = np.array(art.resize((width, height), Image.LANCZOS), dtype=np.float32)

    ramp = np.linspace(1.0, 0.0, max(1, round(height * FADE)), dtype=np.float32) ** 1.5
    pixels[height - ramp.size :, :, 3] *= ramp[:, None]
    # The outer fins were cropped flat in the source render too; a short side
    # ramp turns those vertical edges into a soft outline.
    side = np.linspace(1.0, 0.0, max(1, round(width * SIDE_FADE)), dtype=np.float32) ** 1.5
    pixels[:, : side.size, 3] *= side[None, :]
    pixels[:, width - side.size :, 3] *= side[None, ::-1]

    canvas = Image.new("RGBA", (BOX, BOX), (0, 0, 0, 0))
    canvas.alpha_composite(
        Image.fromarray(np.clip(pixels, 0, 255).astype("uint8")),
        ((BOX - width) // 2, (BOX - height) // 2),
    )
    return canvas


def main() -> None:
    build().save(TARGET)
    print(f"wrote {TARGET.relative_to(ROOT)} ({TARGET.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
