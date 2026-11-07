"""Gate: the shell fills its window and the chat keeps one centred reading column.

A layout row whose children are all rigid hands a fixed implicit width back to the
Window, which then lays out at that width while the platform window stays larger --
the chat column stops short and the right side of the screen goes empty. That is
invisible to compilation and to pixel-free assertions, so it is caught here by
measuring where the composer card actually sits. It must be the reading column wide
(`COLUMN`, or the chat pane when the pane is narrower) and centred inside the pane:
a collapsed shell misses the width, a column that forgot its margins misses the centre.
"""
import os
from pathlib import Path
import subprocess
import sys

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "temp_dir" / "shell-width"
SIDEBAR_BG = (246, 245, 243)
CARD_BG = (255, 255, 255)
# `Theme.timeline-column-width`; the composer card and the transcript share it.
COLUMN = 1000
# Rounded corners and the 1px hairline make the painted card a pixel or two short.
TOLERANCE = 8
SIZES = [(1280, 800), (1920, 1080)]


def render(width, height):
    png = OUT / f"shell-{width}x{height}.png"
    env = {**os.environ, "SLINT_BACKEND": "software"}
    subprocess.run(
        ["slint-viewer", "--screenshot", str(png), "--size", f"{width}x{height}",
         "frontend-slint/ui/main.slint"],
        cwd=ROOT, env=env, check=True, capture_output=True, text=True,
    )
    return png


def pane_left(image):
    """First column past the sidebar: the sidebar is the only surface that tone."""
    for x in range(image.width):
        if image.getpixel((x, 5)) != SIDEBAR_BG:
            return x
    return image.width


def card_edges(image):
    """Left and right edge of the composer card on a row that is all card."""
    row = image.height - 60
    xs = [x for x in range(image.width) if image.getpixel((x, row)) == CARD_BG]
    return (min(xs), max(xs)) if xs else None


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    failures = []
    for width, height in SIZES:
        image = Image.open(render(width, height)).convert("RGB")
        if image.size != (width, height):
            failures.append(f"{width}x{height}: viewer returned {image.size}")
            continue
        edges = card_edges(image)
        if edges is None:
            failures.append(f"{width}x{height}: no composer card found to measure")
            continue
        left, right = edges
        pane = pane_left(image)
        want = min(width - pane, COLUMN)
        if abs((right - left) - want) > TOLERANCE:
            failures.append(
                f"{width}x{height}: chat column is {right - left}px, expected {want}px "
                "(a layout row is missing a stretchable item, or the column lost its cap)"
            )
            continue
        gutter_left, gutter_right = left - pane, width - right
        if abs(gutter_left - gutter_right) > TOLERANCE:
            failures.append(
                f"{width}x{height}: chat column is not centred "
                f"(left gutter {gutter_left}px, right gutter {gutter_right}px)"
            )
            continue
        print(f"{width}x{height}: chat column {right - left}px centred at x={(left + right) // 2}")
    if failures:
        print("\n".join(f"FAIL {line}" for line in failures))
        return 1
    print("PASS: shell fills its window and keeps one centred reading column")
    return 0


if __name__ == "__main__":
    sys.exit(main())
