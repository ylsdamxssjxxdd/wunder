#!/usr/bin/env python3
"""Regenerate the eva01 icon set with a small-size-safe green antenna.

At 16-32 px the antenna's green core averages below one pixel and the dark
outline takes over, so plain downscales render it black. The small frames
repaint the antenna as a solid green column before encoding.

The 256 px app-icon.png is the shared master: ICO frames keep its padding so
the Windows icon matches the desktop window icon and the existing tray asset.

Usage: python3 scripts/generate_eva01_icons.py
"""

from __future__ import annotations

import struct
from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "crates" / "wunder-desktop" / "frontend" / "assets"
SOURCE = ASSETS / "app-icon.png"
ICO = ROOT / "images" / "eva01-head.ico"

ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)
REPAIRED_SIZES = (16, 24, 32)
ANTENNA_X = range(118, 138)
ANTENNA_BOTTOM = 110

source = Image.open(SOURCE).convert("RGBA")


def antenna_color(top: int, bottom: int) -> tuple[int, int, int]:
    red = green = blue = count = 0
    for y in range(top, bottom):
        for x in ANTENNA_X:
            r, g, b, a = source.getpixel((x, y))
            if a > 200 and g > 120 and g > r + 40 and g > b + 40:
                red += r
                green += g
                blue += b
                count += 1
    if count == 0:
        return (120, 192, 72)
    return (red // count, green // count, blue // count)


def render(size: int, repair: bool) -> Image.Image:
    frame = source.resize((size, size), Image.Resampling.LANCZOS)
    pixels = frame.load()
    # LANCZOS ringing leaves a few near-transparent specks outside the shape;
    # drop them so the 16-32 px frames stay clean.
    for y in range(size):
        for x in range(size):
            if pixels[x, y][3] < 8:
                pixels[x, y] = (0, 0, 0, 0)
    if not repair:
        return frame
    scale = size / source.width
    end = max(1, round(ANTENNA_BOTTOM * scale))
    width = 1 if size < 32 else 2
    left = size // 2 - width // 2
    for y in range(end):
        top = int(y / scale)
        bottom = max(top + 1, int((y + 1) / scale))
        color = antenna_color(top, bottom)
        for x in range(left, left + width):
            pixels[x, y] = (*color, 255)
    return frame


def dib(image: Image.Image) -> bytes:
    image = image.convert("RGBA")
    width, height = image.size
    xor = bytearray()
    pixels = image.load()
    for y in range(height - 1, -1, -1):
        for x in range(width):
            red, green, blue, alpha = pixels[x, y]
            xor.extend((blue, green, red, alpha))
    stride = ((width + 31) // 32) * 4
    and_mask = bytes(stride * height)
    header = struct.pack(
        "<IiiHHIIiiII",
        40,
        width,
        height * 2,
        1,
        32,
        0,
        len(xor) + len(and_mask),
        0,
        0,
        0,
        0,
    )
    return header + xor + and_mask


def write_ico(frames: dict[int, Image.Image]) -> None:
    entries = []
    payloads = []
    offset = 6 + 16 * len(frames)
    for size, image in frames.items():
        payload = dib(image)
        dimension = 0 if size == 256 else size
        entries.append(
            struct.pack("<BBBBHHII", dimension, dimension, 0, 0, 1, 32, len(payload), offset)
        )
        payloads.append(payload)
        offset += len(payload)
    ICO.write_bytes(struct.pack("<HHH", 0, 1, len(entries)) + b"".join(entries) + b"".join(payloads))


def main() -> None:
    frames = {size: render(size, size in REPAIRED_SIZES) for size in ICO_SIZES}
    write_ico(frames)

    tray = frames[32]
    tray.save(ASSETS / "app-icon-32.png")
    (ASSETS / "app-icon.rgba").write_bytes(tray.tobytes())

    print(f"wrote {ICO.relative_to(ROOT)} ({ICO.stat().st_size} bytes, {len(frames)} frames)")
    print(f"wrote {(ASSETS / 'app-icon-32.png').relative_to(ROOT)}")
    print(f"wrote {(ASSETS / 'app-icon.rgba').relative_to(ROOT)} ({32 * 32 * 4} bytes)")


if __name__ == "__main__":
    main()
