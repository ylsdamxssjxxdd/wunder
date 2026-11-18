#!/usr/bin/env python3
"""Regenerate the greeting mark (``eva01-mark*``) from ``images/eva01-head.png``.

The greeting sits at the head of every chat transcript, right before the
14 px greeting line, in a 40 x 40 px slot. ``images/eva01-head.png`` is a
512 px render where every pixel is its own cell, so a plain downscale to
40 px melts into a grey smear — that is what "the icon looks blurry" was.

The mark is therefore rebuilt on a pixel grid instead of being smooth scaled:

- the artwork is trimmed to its bbox and resampled onto an integer grid
  (one cell per device pixel at the target density). Each cell takes the
  **maximum** alpha of its source box and an alpha-weighted mean colour:
  averaging alpha is what erased the thin fins and the shoulder edges;
- cells below alpha 4 are dropped, and connected patches of one or two
  cells are dropped too, so the source edge does not leave dust;
- the silhouette is never faded: the source art is complete down to its
  V-shaped chest, so a fade only made the sides and the bottom look cut;
- the delivered file is a nearest-neighbour blow-up of that grid, so the
  browser never invents half-transparent pixels between cells;
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

FILL = 0.92  # artwork width inside the square canvas
MIN_ALPHA = 4  # cells below this only carry source antialiasing


def _downsample_max(art: Image.Image, width: int, height: int) -> np.ndarray:
    """One cell per target pixel: max alpha in the box, weighted-mean colour."""
    src = np.array(art, dtype=np.float32)
    src_h, src_w = src.shape[:2]
    xs = np.linspace(0, src_w, width + 1)
    ys = np.linspace(0, src_h, height + 1)
    out = np.zeros((height, width, 4), dtype=np.float32)
    for gy in range(height):
        y0 = int(round(ys[gy]))
        y1 = max(int(round(ys[gy + 1])), y0 + 1)
        for gx in range(width):
            x0 = int(round(xs[gx]))
            x1 = max(int(round(xs[gx + 1])), x0 + 1)
            block = src[y0:y1, x0:x1]
            alpha = block[..., 3]
            peak = alpha.max()
            if peak <= 0:
                continue
            weight = alpha / alpha.sum()
            out[gy, gx, :3] = (block[..., :3] * weight[..., None]).sum(axis=(0, 1))
            out[gy, gx, 3] = peak
    return out


def _drop_small_patches(alpha: np.ndarray, min_area: int) -> np.ndarray:
    """Keep only patches of at least ``min_area`` cells (8-connected)."""
    height, width = alpha.shape
    seen = np.zeros_like(alpha, dtype=bool)
    keep = np.zeros_like(alpha, dtype=bool)
    for start_y in range(height):
        for start_x in range(width):
            if alpha[start_y, start_x] == 0 or seen[start_y, start_x]:
                continue
            queue = deque([(start_y, start_x)])
            seen[start_y, start_x] = True
            patch: list[tuple[int, int]] = []
            while queue:
                y, x = queue.popleft()
                patch.append((y, x))
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
            if len(patch) >= min_area:
                for y, x in patch:
                    keep[y, x] = True
    return keep


def build(cells: int) -> Image.Image:
    """One grid cell per pixel, ``cells`` wide, artwork filled to ``FILL``."""
    art = Image.open(SOURCE).convert("RGBA")
    art = art.crop(art.split()[3].getbbox())

    width = round(cells * FILL)
    height = round(art.height * width / art.width)
    pixels = _downsample_max(art, width, height)

    canvas = np.zeros((cells, cells, 4), dtype=np.float32)
    ox, oy = (cells - width) // 2, (cells - height) // 2
    canvas[oy : oy + height, ox : ox + width] = pixels

    alpha = canvas[..., 3]
    alpha[alpha < MIN_ALPHA] = 0
    min_area = max(2, round(cells * 0.05))  # 1-2 cell specks only
    alpha[~_drop_small_patches(alpha, min_area)] = 0
    canvas[..., 3] = alpha
    return Image.fromarray(np.clip(canvas, 0, 255).astype("uint8"), "RGBA")


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
