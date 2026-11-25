#!/usr/bin/env python3
"""Regenerate the greeting marks at the exact pixel grids they are drawn on.

The desktop UI is painted by Slint's software rasteriser: it samples bitmaps
bilinearly (and ignores `image-rendering`), so a texture larger than its slot
is point-sampled — one texel out of N survives and the thin outline, the eyes
and the green antenna break into uneven stair steps. The greeting slot is a
fixed 40 logical px, so each asset below is rendered straight from the 512 px
source at the physical size of one device-pixel ratio; the UI picks the one
that lands 1:1 and the mark is then copied texel for texel.

Lanczos + a small unsharp pass keeps the source shading and the thin
silhouette; every size is rendered directly from the source so no resize
passes accumulate.

Outputs:
- web: 40 / 80 / 120 px for DPR 1 / 2 / 3 (the browser picks via srcset);
- desktop and Android: `eva01-mark.png` for DPR 1 plus `eva01-mark-<px>.png`
  for the other ratios (see DEVICE_RATIOS).

Usage: python3 scripts/generate_eva01_mark.py
"""

from pathlib import Path

from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "images" / "eva01-head.png"
WEB = ROOT / "frontend" / "public"
DESKTOP = ROOT / "crates" / "wunder-desktop" / "frontend" / "assets"

# The greeting slot is 40 logical px. These ratios cover the scaling steps the
# platforms actually offer (Windows 100-400 %, GTK/Wayland 100-300 %, Android
# density buckets); anything in between picks the nearest step, which costs at
# most a few percent of resampling instead of the old 4x point sampling.
DEVICE_RATIOS = (1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0, 3.5, 4.0)
SLOT = 40
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
    for ratio in DEVICE_RATIOS:
        pixels = round(SLOT * ratio)
        name = "eva01-mark.png" if ratio == 1.0 else f"eva01-mark-{pixels}.png"
        save(pixels, DESKTOP / name)


if __name__ == "__main__":
    main()
