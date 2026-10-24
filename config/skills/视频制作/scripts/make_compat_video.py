"""Generate a small Wunder intro MP4 with the Win7 supplement toolchain.

This is the ia32 fallback path: Pillow creates deterministic PNG frames and
the bundled FFmpeg encodes them. It is intentionally separate from Remotion's
renderer, whose Windows compositor has no ia32 build.
"""

from __future__ import print_function

import argparse
import os
import shutil
import subprocess
import tempfile

from PIL import Image, ImageDraw, ImageFont


def font_path(name):
    candidates = [
        os.environ.get("WUNDER_VIDEO_FONT", ""),
        os.path.join(os.path.dirname(__file__), "..", "..", "..", "..", "config", "fonts", name),
        os.path.join(os.path.dirname(__file__), "..", "..", "..", "..", "temp_dir", "win7-gnu-lab", "win7-supplement", "stage", "package-root", "opt", "python", "Lib", "site-packages", "matplotlib", "mpl-data", "fonts", "ttf", "DejaVuSans-Bold.ttf"),
    ]
    for path in candidates:
        if path and os.path.isfile(path):
            return os.path.abspath(path)
    return None


def load_font(size, bold=False):
    # SimHei is part of the product font bundle and carries the Chinese text
    # used by the default card. Prefer it even for bold labels: a missing glyph
    # is worse than a slightly lighter weight on legacy Windows.
    path = font_path("simhei.ttf")
    if path is None:
        path = font_path("DejaVuSans-Bold.ttf")
    return ImageFont.truetype(path, size) if path else ImageFont.load_default()


def draw_frame(index, total, width, height):
    image = Image.new("RGB", (width, height), "#eef5fb")
    draw = ImageDraw.Draw(image)
    progress = float(index) / max(1, total - 1)
    blue = "#2f80ed"
    navy = "#102a43"
    muted = "#486581"
    draw.rectangle((0, 0, width, 12), fill=blue)
    draw.text((82, 72), "WUNDER", font=load_font(30, True), fill=blue)
    draw.text((82, 145), "让智能体把目标变成结果", font=load_font(56, True), fill=navy)
    draw.text((84, 235), "本地 AI 工作台 · 舵机 · 舰体", font=load_font(29), fill=muted)

    radius = 92 + int(18 * progress)
    cx, cy = width - 180, 205
    draw.ellipse((cx - radius, cy - radius, cx + radius, cy + radius), fill=blue)
    draw.ellipse((cx - 35, cy - 35, cx + 35, cy + 35), fill="#ffffff")
    draw.line((cx - 20, cy, cx + 20, cy), fill=blue, width=8)
    draw.line((cx, cy - 20, cx, cy + 20), fill=blue, width=8)

    cards = [("拆解目标", 82), ("调用工具", 330), ("交付结果", 578)]
    for label, x in cards:
        draw.rounded_rectangle((x, 340, x + 210, 435), radius=18, fill="#ffffff", outline="#56ccf2", width=3)
        bbox = draw.textbbox((0, 0), label, font=load_font(27, True))
        tw = bbox[2] - bbox[0]
        draw.text((x + (210 - tw) / 2, 372), label, font=load_font(27, True), fill=navy)

    draw.text((82, height - 92), "wunder · 智能体调度平台", font=load_font(28, True), fill=navy)
    draw.rectangle((82, height - 42, 82 + int((width - 164) * progress), height - 34), fill=blue)
    return image


def main():
    parser = argparse.ArgumentParser(description="Create an ia32-compatible Wunder intro MP4")
    parser.add_argument("output", help="output .mp4 path")
    parser.add_argument("--ffmpeg", default=os.environ.get("WUNDER_FFMPEG_BIN", "ffmpeg.exe"))
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--fps", type=int, default=30)
    parser.add_argument("--seconds", type=float, default=6.0)
    args = parser.parse_args()
    total = max(1, int(round(args.fps * args.seconds)))
    output = os.path.abspath(args.output)
    parent = os.path.dirname(output)
    if parent and not os.path.isdir(parent):
        os.makedirs(parent)
    temp_root = tempfile.mkdtemp(prefix="wunder-video-")
    try:
        for index in range(total):
            draw_frame(index, total, args.width, args.height).save(os.path.join(temp_root, "frame-%06d.png" % index))
        command = [
            args.ffmpeg, "-y", "-framerate", str(args.fps), "-i", os.path.join(temp_root, "frame-%06d.png"),
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart", output,
        ]
        subprocess.check_call(command)
        if not os.path.isfile(output) or os.path.getsize(output) == 0:
            raise RuntimeError("FFmpeg returned success but output is empty")
        print("created %s (%d bytes, %dx%d, %.2fs, %dfps)" % (output, os.path.getsize(output), args.width, args.height, total / float(args.fps), args.fps))
    finally:
        shutil.rmtree(temp_root, ignore_errors=True)


if __name__ == "__main__":
    main()
