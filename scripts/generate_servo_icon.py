#!/usr/bin/env python3
"""Build the transparent 256 px CLI icon and its Win7-compatible DIB ICO."""

from __future__ import annotations

import struct
from pathlib import Path

import cv2
import numpy as np
from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "images" / "servo.png"
PNG = ROOT / "images" / "servo.png"
ICO = ROOT / "images" / "servo.ico"
SIZES = (16, 24, 32, 48, 64, 128, 256)

# A loose object-shaped seed avoids learning the dark servo body as backdrop while
# keeping the watermark outside the foreground region. GrabCut refines its edges.
SEED = np.array(
    [
        (800, 380), (1080, 390), (1270, 445), (1410, 555), (1510, 710),
        (1550, 850), (1560, 960), (1650, 990), (1770, 1050), (1810, 1140),
        (1790, 1240), (1710, 1320), (1625, 1320), (1570, 1280), (1560, 1410),
        (1480, 1540), (1410, 1665), (1330, 1690), (1260, 1640), (1150, 1660),
        (980, 1640), (820, 1570), (650, 1500), (490, 1400), (380, 1300),
        (210, 1080), (250, 1030), (390, 990), (375, 820), (410, 650),
        (500, 500), (650, 415),
    ],
    dtype=np.int32,
)


def cutout() -> Image.Image:
    source_image = Image.open(SOURCE).convert("RGBA")
    if source_image.getchannel("A").getextrema() != (255, 255):
        alpha = source_image.getchannel("A")
        source_image = source_image.crop(alpha.getbbox())
        side = max(source_image.size)
        padded = Image.new("RGBA", (side, side), (0, 0, 0, 0))
        padded.alpha_composite(
            source_image, ((side - source_image.width) // 2, (side - source_image.height) // 2)
        )
        return padded.resize((256, 256), Image.Resampling.LANCZOS)

    bgr = cv2.imread(str(SOURCE), cv2.IMREAD_COLOR)
    if bgr is None:
        raise FileNotFoundError(SOURCE)
    height, width = bgr.shape[:2]
    rgb = cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)
    border = np.concatenate((rgb[:120].reshape(-1, 3), rgb[-100:].reshape(-1, 3),
                             rgb[:, :100].reshape(-1, 3), rgb[:, -100:].reshape(-1, 3)))
    background = np.median(border, axis=0)
    distance = np.linalg.norm(rgb.astype(np.float32) - background, axis=2)

    interior = np.zeros((height, width), dtype=np.uint8)
    cv2.fillPoly(interior, [SEED], 1)
    mask = np.where(interior, cv2.GC_PR_FGD, cv2.GC_BGD).astype(np.uint8)
    mask[(interior > 0) & (distance > 72)] = cv2.GC_FGD
    mask[(interior > 0) & (distance < 5)] = cv2.GC_PR_BGD

    bg_model = np.zeros((1, 65), dtype=np.float64)
    fg_model = np.zeros((1, 65), dtype=np.float64)
    cv2.grabCut(bgr, mask, None, bg_model, fg_model, 6, cv2.GC_INIT_WITH_MASK)
    foreground = np.isin(mask, (cv2.GC_FGD, cv2.GC_PR_FGD)).astype(np.uint8)
    _, labels, stats, _ = cv2.connectedComponentsWithStats(foreground, 8)
    foreground = (labels == 1 + np.argmax(stats[1:, cv2.CC_STAT_AREA])).astype(np.uint8)
    foreground = cv2.morphologyEx(
        foreground, cv2.MORPH_CLOSE, cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (3, 3))
    )
    _, labels, stats, _ = cv2.connectedComponentsWithStats(foreground, 8)
    foreground = (labels == 1 + np.argmax(stats[1:, cv2.CC_STAT_AREA])).astype(np.uint8)
    alpha = cv2.GaussianBlur(foreground.astype(np.float32), (0, 0), 0.55)
    rgba = np.dstack((rgb, np.clip(alpha * 255, 0, 255).astype(np.uint8)))

    ys, xs = np.nonzero(foreground)
    left, top, right, bottom = xs.min(), ys.min(), xs.max() + 1, ys.max() + 1
    crop = Image.fromarray(rgba, "RGBA").crop((left, top, right, bottom))
    # Give the cutout a little transparent breathing room, then centre on the canvas.
    side = max(crop.width, crop.height)
    padded = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    padded.alpha_composite(crop, ((side - crop.width) // 2, (side - crop.height) // 2))
    return padded.resize((256, 256), Image.Resampling.LANCZOS)


def dib_icon(image: Image.Image) -> bytes:
    """Encode a 32-bit uncompressed ICO image; old MinGW windres accepts this DIB form."""
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
    header = struct.pack("<IiiHHIIiiII", 40, width, height * 2, 1, 32, 0,
                         len(xor) + len(and_mask), 0, 0, 0, 0)
    return header + xor + and_mask


def write_ico(image: Image.Image) -> None:
    entries = []
    payloads = []
    offset = 6 + 16 * len(SIZES)
    for size in SIZES:
        frame = image.resize((size, size), Image.Resampling.LANCZOS)
        payload = dib_icon(frame)
        dimension = 0 if size == 256 else size
        entries.append(struct.pack("<BBBBHHII", dimension, dimension, 0, 0, 1, 32,
                                   len(payload), offset))
        payloads.append(payload)
        offset += len(payload)
    ICO.write_bytes(struct.pack("<HHH", 0, 1, len(entries)) + b"".join(entries) + b"".join(payloads))


def main() -> None:
    image = cutout()
    image.save(PNG, optimize=True)
    write_ico(image)
    print(f"wrote transparent 256x256 PNG and DIB ICO: {PNG}, {ICO}")


if __name__ == "__main__":
    main()
