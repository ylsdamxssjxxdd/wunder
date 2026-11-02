#!/usr/bin/env python3
"""Render the Inno Setup wizard banner (left column of the completed page).

The image area keeps a fixed 164:314 aspect ratio, so the canvas is exactly 2x
that (328x628): Inno Setup shrinks it to 202x386 at 96 DPI and stays close to
native on high-DPI displays.
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[1]
MARK = ROOT / "images" / "pupa.png"
OUT = ROOT / "images" / "wizard-banner.png"
FONT_DIR = Path("C:/Windows/Fonts")

W, H = 328, 628

CREAM_TOP = (255, 253, 249)
CREAM_MID = (253, 243, 226)
CREAM_BOTTOM = (247, 236, 217)
AMBER = (245, 138, 36)
RUST = (201, 67, 38)
INK = (43, 26, 18)
MUTED = (154, 132, 110)


def gradient() -> Image.Image:
    img = Image.new("RGB", (W, H))
    stops = ((0.0, CREAM_TOP), (0.46, CREAM_MID), (1.0, CREAM_BOTTOM))
    px = img.load()
    for y in range(H):
        t = y / (H - 1)
        for lo, hi in zip(stops, stops[1:]):
            if t <= hi[0]:
                span = hi[0] - lo[0] or 1.0
                k = (t - lo[0]) / span
                color = tuple(round(a + (b - a) * k) for a, b in zip(lo[1], hi[1]))
                break
        for x in range(W):
            px[x, y] = color
    return img.convert("RGBA")


def hexagon(cx: float, cy: float, r: float) -> list[tuple[float, float]]:
    return [
        (cx + r * math.cos(math.radians(60 * i - 90)), cy + r * math.sin(math.radians(60 * i - 90)))
        for i in range(6)
    ]


def honeycomb() -> Image.Image:
    """Pointy-top lattice that fades in toward the bottom of the banner."""
    layer = Image.new("L", (W, H), 0)
    draw = ImageDraw.Draw(layer)
    r = 27.0
    step_x = math.sqrt(3) * r
    step_y = 1.5 * r
    row = 0
    y = -r
    while y < H + r:
        offset = (step_x / 2) if row % 2 else 0.0
        x = -step_x + offset
        while x < W + step_x:
            draw.polygon(hexagon(x, y, r * 0.86), outline=255, width=2)
            x += step_x
        y += step_y
        row += 1

    mask = Image.new("L", (W, H))
    mpx = mask.load()
    for yy in range(H):
        t = yy / (H - 1)
        # Barely visible behind the mark, a light texture at the feet.
        value = round(9 + 24 * (t ** 2.2))
        for xx in range(W):
            mpx[xx, yy] = value
    band = Image.new("RGBA", (W, H), AMBER + (0,))
    band.putalpha(ImageChops.multiply(layer, mask))
    return band


def glow() -> Image.Image:
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    cx, cy, rad = W // 2, 236, 132
    draw.ellipse((cx - rad, cy - rad, cx + rad, cy + rad), fill=(255, 214, 140, 120))
    return layer.filter(ImageFilter.GaussianBlur(58))


def mark() -> Image.Image:
    src = Image.open(MARK).convert("RGBA")
    src = src.crop(src.getchannel("A").getbbox())
    target_w = 176
    target_h = round(src.height * target_w / src.width)
    # Two-step downscale keeps the painted edges from aliasing in one jump.
    art = src.resize((target_w * 2, target_h * 2), Image.LANCZOS).resize(
        (target_w, target_h), Image.LANCZOS
    )
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    shadow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    left = (W - target_w) // 2
    top = 236 - target_h // 2
    sd.ellipse(
        (left + 16, top + target_h - 26, left + target_w - 16, top + target_h + 16),
        fill=(122, 62, 20, 70),
    )
    layer = Image.alpha_composite(layer, shadow.filter(ImageFilter.GaussianBlur(12)))
    layer.paste(art, (left, top), art)
    return layer


def text() -> Image.Image:
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    bold = ImageFont.truetype(str(FONT_DIR / "msyhbd.ttc"), 46)
    regular = ImageFont.truetype(str(FONT_DIR / "msyh.ttc"), 21)

    draw.text((W // 2, 400), "心舰", font=bold, fill=INK, anchor="mm")
    word = "W U N D E R"
    draw.text((W // 2, 452), word, font=regular, fill=MUTED, anchor="mm")
    rule_w = 46
    draw.line(
        (W // 2 - rule_w, 480, W // 2 + rule_w, 480), fill=RUST + (150,), width=2
    )
    return layer


def render() -> Image.Image:
    img = gradient()
    img = Image.alpha_composite(img, glow())
    img = Image.alpha_composite(img, honeycomb())
    img = Image.alpha_composite(img, mark())
    img = Image.alpha_composite(img, text())
    return img


def main() -> None:
    assert W * 314 == H * 164, "banner must keep the 164:314 wizard image ratio"
    image = render()
    image.save(OUT)
    print(f"wrote {OUT} {image.size}")


if __name__ == "__main__":
    main()
