#!/usr/bin/env python3
"""Regenerate the greeting mark (``eva01-mark*``) from ``images/eva01-head.png``.

The greeting sits at the head of every chat transcript, right before the
14 px greeting line, in a 40 x 40 px slot. ``images/eva01-head.png`` is a
512 px render where every pixel is its own cell: at 40 px the picture is
compressed 12:1, so a plain downscale melts into a soft grey smear — that
is exactly what "the icon looks blurry" was on the cloud client.

The mark is therefore rendered on a pixel grid instead of being smooth
scaled:

- the artwork is trimmed to its bbox and resampled once onto an integer
  grid (one cell per device pixel at the target density);
- the delivered file is a nearest-neighbour blow-up of that grid, so the
  browser never invents half-transparent pixels between cells;
- the bottom 14 % fades out and the outer fins fade 6 %, so the flat neck
  and fin cuts dissolve instead of ending on a hard stroke;
- cells below alpha 24 are dropped and only the largest blob is kept, so
  the fade does not leave dust around the silhouette;
- ``image-rendering: pixelated`` in the front ends maps one grid cell to
  one device pixel.

Densities:

- web 1x (``eva01-mark.png``): 40 cells for a 40 px slot at DPR 1;
- web 2x (``eva01-mark@2x.png``): 80 cells for a 40 px slot at DPR 2;
- desktop (``assets/eva01-mark.png``): 40 cells blown up 4x, because the
  desktop targets 1x screens and Slint picks a single image.

Usage: python3 scripts/generate_eva01_mark.py
"""

from __future__ import annotations

from collections import deque
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "images" / "eva01-head.png"

WEB = ROOT / "frontend" / "public"
DESKTOP = ROOT / "crates" / "wunder-desktop" / "frontend" / "assets"

FILL = 0.85  # artwork width inside the square canvas
FADE = 0.14  # share of the artwork height that dissolves at the bottom
SIDE_FADE = 0.06  # share of the artwork width that dissolves at each fin
MIN_ALPHA = 24  # ringing / fade dust below this is dropped


def _largest_blob(alpha: np.ndarray) -> np.ndarray:
    """Keep the main silhouette, drop cells that the fade isolated."""
    height, width = alpha.shape
    seen = np.zeros_like(alpha, dtype=bool)
    best: list[tuple[int, int]] = []
    for start_y in range(height):
        for start_x in range(width):
            if alpha[start_y, start_x] == 0 or seen[start_y, start_x]:
                continue
            queue = deque([(start_y, start_x)])
            seen[start_y, start_x] = True
            blob: list[tuple[int, int]] = []
            while queue:
                y, x = queue.popleft()
                blob.append((y, x))
                for dy in (-1, 0, 1):
                    for dx in (-1, 0, 1):
                        ny, nx = y + dy, x + dx
                        if (
                            0 <= ny < height
                            and 0 <= nx < width
                            and not seen[ny, nx]
                            and alpha[ny, nx] > 0
                        ):
                            seen[ny, nx] = True
                            queue.append((ny, nx))
            if len(blob) > len(best):
                best = blob
    mask = np.zeros_like(alpha, dtype=bool)
    for y, x in best:
        mask[y, x] = True
    return mask


def build(cells: int) -> Image.Image:
    """One grid cell per pixel, ``cells`` wide, artwork filled to 85 %."""
    art = Image.open(SOURCE).convert("RGBA")
    art = art.crop(art.split()[3].getbbox())

    width = round(cells * FILL)
    height = round(art.height * width / art.width)
    pixels = np.array(art.resize((width, height), Image.LANCZOS), dtype=np.float32)

    ramp = np.linspace(1.0, 0.0, max(1, round(height * FADE)), dtype=np.float32) ** 1.15
    pixels[height - ramp.size :, :, 3] *= ramp[:, None]
    side = np.linspace(1.0, 0.0, max(1, round(width * SIDE_FADE)), dtype=np.float32) ** 1.15
    pixels[:, : side.size, 3] *= side[None, :]
    pixels[:, width - side.size :, 3] *= side[None, ::-1]

    canvas = Image.new("RGBA", (cells, cells), (0, 0, 0, 0))
    canvas.alpha_composite(
        Image.fromarray(np.clip(pixels, 0, 255).astype("uint8")),
        ((cells - width) // 2, (cells - height) // 2),
    )

    data = np.array(canvas)
    alpha = data[..., 3]
    alpha[alpha < MIN_ALPHA] = 0
    alpha[~_largest_blob(alpha)] = 0
    data[..., 3] = alpha
    return Image.fromarray(data, "RGBA")


def save(cells: int, scale: int, target: Path) -> None:
    build(cells).resize((cells * scale, cells * scale), Image.NEAREST).save(
        target, optimize=True
    )
    print(f"wrote {target.relative_to(ROOT)} ({cells} cells x {scale} = {cells * scale}px)")


def main() -> None:
    save(40, 1, WEB / "eva01-mark.png")  # 40 px slot at DPR 1
    save(80, 1, WEB / "eva01-mark@2x.png")  # 40 px slot at DPR 2
    save(40, 4, DESKTOP / "eva01-mark.png")  # single image for Slint


if __name__ == "__main__":
    main()
