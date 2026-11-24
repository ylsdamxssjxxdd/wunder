#!/usr/bin/env python3
"""Regenerate the smooth, density-specific greeting marks from the source art.

The 40 CSS px greeting slot needs real resampling rather than a nearest-neighbour
pixel grid: Lanczos preserves the source shading and thin silhouette, while a
small unsharp pass keeps the eyes, fins and V-shaped chest legible after the
reduction. Each output is rendered directly from the 512 px source to avoid
accumulating multiple resize passes.

Outputs:
- web: 40 / 80 / 120 px for DPR 1 / 2 / 3;
- desktop and Android: one shared 160 px asset for the 40 px Slint slot.

Usage: python3 scripts/generate_eva01_mark.py
"""

from pathlib import Path

from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "images" / "eva01-head.png"
WEB = ROOT / "frontend" / "public"
DESKTOP = ROOT / "crates" / "wunder-desktop" / "frontend" / "assets"

FILL = 0.92


def build(size: int) -> Image.Image:
    with Image.open(SOURCE) as source:
        art = source.convert("RGBA")
    art = art.crop(art.getchannel("A").getbbox())

    width = round(size * FILL)
    height = round(art.height * width / art.width)
    art = art.resize((width, height), Image.Resampling.LANCZOS)
    art = art.filter(ImageFilter.UnsharpMask(radius=0.65, percent=45, threshold=2))

    canvas = Image.new("RGBA", (size, size))
    canvas.alpha_composite(art, ((size - width) // 2, (size - height) // 2))
    return canvas


def save(size: int, target: Path) -> None:
    build(size).save(target, optimize=True)
    print(f"wrote {target.relative_to(ROOT)} ({size}px)")


def main() -> None:
    save(40, WEB / "eva01-mark.png")
    save(80, WEB / "eva01-mark@2x.png")
    save(120, WEB / "eva01-mark@3x.png")
    save(160, DESKTOP / "eva01-mark.png")


if __name__ == "__main__":
    main()
