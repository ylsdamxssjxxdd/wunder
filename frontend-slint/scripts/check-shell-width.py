"""Gate: the workbench shell must fill its window, never collapse to content width.

A layout row whose children are all rigid hands a fixed implicit width back to the
Window, which then lays out at that width while the platform window stays larger --
the chat column stops short and the right side of the screen goes empty. That is
invisible to compilation and to pixel-free assertions, so it is caught here by
measuring where content actually ends.
"""
import os
from pathlib import Path
import subprocess
import sys

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "temp_dir" / "shell-width"
SIDEBAR_BG = (246, 245, 243)
CANVAS_BG = (251, 250, 248)
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


def rightmost_content(image):
    width, height = image.size
    best = 0
    for row in (int(height * 0.35), int(height * 0.6), height - 24):
        for x in range(width - 1, best, -1):
            column = {image.getpixel((x, y)) for y in range(0, height, 7)}
            if any(pixel not in (SIDEBAR_BG, CANVAS_BG) for pixel in column):
                best = max(best, x)
                break
    return best


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    failures = []
    for width, height in SIZES:
        image = Image.open(render(width, height)).convert("RGB")
        if image.size != (width, height):
            failures.append(f"{width}x{height}: viewer returned {image.size}")
            continue
        right = rightmost_content(image)
        if right < width - 12:
            failures.append(
                f"{width}x{height}: content ends at x={right}, {width - right - 1}px "
                "of the window is empty (a layout row is missing a stretchable item)"
            )
        else:
            print(f"{width}x{height}: content reaches x={right}")
    if failures:
        print("\n".join(f"FAIL {line}" for line in failures))
        return 1
    print("PASS: shell fills its window at every checked size")
    return 0


if __name__ == "__main__":
    sys.exit(main())
