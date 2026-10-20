#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""egui 视觉回归门槛（V-07）。

用固定 fixture、固定视口和固定缩放启动 `wunder-egui` 演示窗口，截图后与基线
做逐像素比较，输出差异图和阈值报告。脚本只依赖标准库，PNG 编解码是内置的最小
实现，避免为截图工具引入 Pillow 之类的运行时依赖。

用法（仓库根目录）：

    # 生成/更新基线
    python scripts/check_egui_visual.py --update

    # 与基线比较，超阈值时返回 1
    python scripts/check_egui_visual.py

    # 指定二进制和基线目录
    python scripts/check_egui_visual.py --bin target/release/wunder-egui.exe \
        --baseline-dir temp_dir/egui-visual/baseline

比较口径见 `docs/egui实现清单表格.md` §2：先比几何和状态层次，再接受文字栅格
差异，因此阈值按"明显变化的像素占比"和"整体平均通道差"双重判定，而不是要求
逐像素完全一致。

已知噪声：设置页搜索框有光标闪烁，两次截图的该处像素不同（实测约 0.08% 的
像素、峰值 < 100），低于默认 0.4% 门槛；其余 fixture 在固定环境下逐像素相同。
基线依赖视口、缩放和系统字体，跨机器比较时把 `--baseline-dir` 指向同环境基线。
"""

from __future__ import annotations

import argparse
import json
import os
import struct
import subprocess
import sys
import zlib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
ARTIFACT_ROOT = REPO_ROOT / "temp_dir" / "egui-visual"

# 固定 fixture：名称、启动参数、逻辑视口。视口必须与基线一致，否则先比尺寸。
FIXTURES: list[tuple[str, list[str], str]] = [
    ("chat", ["--demo"], "1440x900"),
    ("workflow", ["--demo-workflow"], "1440x900"),
    ("tool-detail", ["--demo-tool"], "1440x900"),
]
SETTINGS_CATEGORIES = [
    "general",
    "account",
    "tools",
    "agent",
    "companion",
    "cron",
    "memory",
    "channels",
    "devices",
    "prompts",
    "archived",
]
FIXTURES += [
    (f"settings-{category}", [f"--demo-settings={category}"], "1440x900")
    for category in SETTINGS_CATEGORIES
]
# 1024x700 用于核对刚好落在断点上的二栏几何；980x700 用于核对窄窗收起态。
FIXTURES += [
    ("chat-compact", ["--demo"], "1024x700"),
    ("chat-narrow", ["--demo"], "980x700"),
]

# 判定阈值。单像素容差吸收文字抗锯齿，像素占比和平均差约束真正的布局/颜色变化。
DEFAULT_PIXEL_TOLERANCE = 12
DEFAULT_MAX_CHANGED_RATIO = 0.004
DEFAULT_MAX_MEAN_DELTA = 1.0


def read_png(path: Path) -> tuple[int, int, bytes]:
    """读取 8 位 RGB/RGBA 非交错 PNG，返回 (宽, 高, RGBA 字节)。"""
    raw = path.read_bytes()
    if raw[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"不是 PNG 文件：{path}")
    offset = 8
    width = height = 0
    color_type = 6
    bit_depth = 8
    idat = bytearray()
    while offset < len(raw):
        (length,) = struct.unpack(">I", raw[offset : offset + 4])
        kind = raw[offset + 4 : offset + 8]
        body = raw[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, _, _, interlace = struct.unpack(
                ">IIBBBBB", body
            )
            if bit_depth != 8 or interlace != 0 or color_type not in (2, 6):
                raise SystemExit(
                    f"只支持 8 位 RGB/RGBA 非交错 PNG：{path}"
                    f"（depth={bit_depth}, type={color_type}, interlace={interlace}）"
                )
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    if not width or not height:
        raise SystemExit(f"PNG 缺少 IHDR：{path}")

    channels = 4 if color_type == 6 else 3
    stride = width * channels
    data = zlib.decompress(bytes(idat))
    previous = bytearray(stride)
    rgba = bytearray(width * height * 4)
    position = 0
    for row in range(height):
        filter_type = data[position]
        position += 1
        line = bytearray(data[position : position + stride])
        position += stride
        if filter_type == 1:
            for index in range(channels, stride):
                line[index] = (line[index] + line[index - channels]) & 0xFF
        elif filter_type == 2:
            for index in range(stride):
                line[index] = (line[index] + previous[index]) & 0xFF
        elif filter_type == 3:
            for index in range(stride):
                left = line[index - channels] if index >= channels else 0
                line[index] = (line[index] + ((left + previous[index]) >> 1)) & 0xFF
        elif filter_type == 4:
            for index in range(stride):
                left = line[index - channels] if index >= channels else 0
                up = previous[index]
                up_left = previous[index - channels] if index >= channels else 0
                estimate = left + up - up_left
                pa, pb, pc = (
                    abs(estimate - left),
                    abs(estimate - up),
                    abs(estimate - up_left),
                )
                if pa <= pb and pa <= pc:
                    predictor = left
                elif pb <= pc:
                    predictor = up
                else:
                    predictor = up_left
                line[index] = (line[index] + predictor) & 0xFF
        elif filter_type != 0:
            raise SystemExit(f"未知 PNG 行过滤器 {filter_type}：{path}")
        previous = line
        row_offset = row * width * 4
        if channels == 4:
            rgba[row_offset : row_offset + width * 4] = line
        else:
            for column in range(width):
                source = column * 3
                target = row_offset + column * 4
                rgba[target : target + 3] = line[source : source + 3]
                rgba[target + 3] = 0xFF
    return width, height, bytes(rgba)


def write_png(path: Path, width: int, height: int, rgba: bytes) -> None:
    """写出 8 位 RGBA PNG（行过滤器固定为 0，便于脚本内自检）。"""
    stride = width * 4
    raw = bytearray()
    for row in range(height):
        raw.append(0)
        raw += rgba[row * stride : (row + 1) * stride]
    def chunk(kind: bytes, body: bytes) -> bytes:
        return (
            struct.pack(">I", len(body))
            + kind
            + body
            + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 6))
        + chunk(b"IEND", b"")
    )


def compare(
    baseline: tuple[int, int, bytes],
    candidate: tuple[int, int, bytes],
    tolerance: int,
) -> dict[str, object]:
    base_w, base_h, base_pixels = baseline
    new_w, new_h, new_pixels = candidate
    if (base_w, base_h) != (new_w, new_h):
        return {
            "size_mismatch": True,
            "baseline_size": [base_w, base_h],
            "candidate_size": [new_w, new_h],
        }

    total_pixels = base_w * base_h
    changed = 0
    channel_sum = 0
    worst = 0
    diff = bytearray(total_pixels * 4)
    for index in range(total_pixels):
        offset = index * 4
        pixel_delta = 0
        for channel in range(4):
            delta = abs(base_pixels[offset + channel] - new_pixels[offset + channel])
            channel_sum += delta
            if delta > pixel_delta:
                pixel_delta = delta
        worst = max(worst, pixel_delta)
        if pixel_delta > tolerance:
            changed += 1
            diff[offset] = min(255, pixel_delta * 8)
            diff[offset + 1] = 0
            diff[offset + 2] = 0
            diff[offset + 3] = 255
        else:
            # 保留 18% 的基线灰度，便于定位差异所在的界面区域。
            base = (
                base_pixels[offset] * 3
                + base_pixels[offset + 1] * 6
                + base_pixels[offset + 2]
            ) // 10
            grey = (base * 18) // 100
            diff[offset] = grey
            diff[offset + 1] = grey
            diff[offset + 2] = grey
            diff[offset + 3] = 255
    return {
        "size_mismatch": False,
        "changed_pixels": changed,
        "changed_ratio": changed / total_pixels if total_pixels else 0.0,
        "mean_channel_delta": channel_sum / (total_pixels * 4) if total_pixels else 0.0,
        "worst_channel_delta": worst,
        "diff_pixels": bytes(diff),
    }


def capture(binary: Path, args: list[str], viewport: str, target: Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        target.unlink()
    env = dict(os.environ)
    env["WUNDER_EGUI_CAPTURE_TO"] = str(target)
    env["WUNDER_EGUI_VIEWPORT"] = viewport
    completed = subprocess.run(
        [str(binary), *args],
        cwd=str(REPO_ROOT),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=120,
        check=False,
    )
    if not target.exists():
        message = completed.stderr.decode("utf-8", "replace").strip()
        raise SystemExit(
            f"截图失败：{binary.name} {' '.join(args)} (exit={completed.returncode})\n{message}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description="egui 固定 fixture 截图差异门槛")
    parser.add_argument(
        "--bin",
        default=str(
            REPO_ROOT
            / "target"
            / "debug"
            / ("wunder-egui.exe" if os.name == "nt" else "wunder-egui")
        ),
        help="带 fixtures feature 构建的 wunder-egui 可执行文件",
    )
    parser.add_argument(
        "--baseline-dir",
        default=str(ARTIFACT_ROOT / "baseline"),
        help="基线目录（默认 temp_dir/egui-visual/baseline）",
    )
    parser.add_argument(
        "--output-dir",
        default=str(ARTIFACT_ROOT / "current"),
        help="本次截图与差异图输出目录",
    )
    parser.add_argument("--update", action="store_true", help="用本次截图覆盖基线")
    parser.add_argument(
        "--pixel-tolerance",
        type=int,
        default=DEFAULT_PIXEL_TOLERANCE,
        help="单像素任一通道差值不超过该值时视为抗锯齿噪声",
    )
    parser.add_argument(
        "--max-changed-ratio",
        type=float,
        default=DEFAULT_MAX_CHANGED_RATIO,
        help="允许明显变化的像素占比上限",
    )
    parser.add_argument(
        "--max-mean-delta",
        type=float,
        default=DEFAULT_MAX_MEAN_DELTA,
        help="允许的全图平均通道差上限",
    )
    parser.add_argument("--only", default="", help="只跑名称包含该子串的 fixture")
    args = parser.parse_args()

    binary = Path(args.bin)
    if not binary.exists():
        raise SystemExit(
            f"找不到 {binary}。先执行：\n"
            "  cargo build -p wunder-egui --bin wunder-egui --features fixtures"
        )
    baseline_dir = Path(args.baseline_dir)
    output_dir = Path(args.output_dir)
    diff_dir = output_dir / "diff"

    fixtures = [f for f in FIXTURES if not args.only or args.only in f[0]]
    if not fixtures:
        raise SystemExit(f"没有匹配 --only {args.only} 的 fixture")

    results: list[dict[str, object]] = []
    failures = 0
    for name, fixture_args, viewport in fixtures:
        current = output_dir / f"{name}.png"
        capture(binary, fixture_args, viewport, current)
        baseline = baseline_dir / f"{name}.png"
        current_image = read_png(current)
        width, height = current_image[0], current_image[1]
        requested = tuple(int(part) for part in viewport.split("x"))
        if (width, height) != requested:
            print(
                f"[dpi     ] {name:22} 请求 {viewport} 逻辑像素，"
                f"实得 {width}x{height}（系统缩放）"
            )

        if args.update:
            baseline.parent.mkdir(parents=True, exist_ok=True)
            baseline.write_bytes(current.read_bytes())
            results.append(
                {
                    "fixture": name,
                    "viewport": viewport,
                    "size": [width, height],
                    "action": "baseline",
                }
            )
            print(f"[baseline] {name:22} {viewport}  -> {width}x{height}")
            continue

        if not baseline.exists():
            failures += 1
            results.append(
                {"fixture": name, "viewport": viewport, "error": "missing baseline"}
            )
            print(f"[missing ] {name:22} 没有基线，先运行 --update")
            continue

        baseline_image = read_png(baseline)
        report = compare(baseline_image, current_image, args.pixel_tolerance)
        entry: dict[str, object] = {
            "fixture": name,
            "viewport": viewport,
            "size": [width, height],
            "baseline": str(baseline),
            "current": str(current),
        }
        if report.get("size_mismatch"):
            failures += 1
            entry["error"] = "viewport mismatch"
            entry["baseline_size"] = report["baseline_size"]
            entry["candidate_size"] = report["candidate_size"]
            print(
                f"[size    ] {name:22} 基线 {report['baseline_size']} "
                f"与本次 {report['candidate_size']} 不一致"
            )
        else:
            ratio = float(report["changed_ratio"])
            mean = float(report["mean_channel_delta"])
            ok = ratio <= args.max_changed_ratio and mean <= args.max_mean_delta
            entry.update(
                {
                    "changed_ratio": round(ratio, 6),
                    "mean_channel_delta": round(mean, 4),
                    "worst_channel_delta": report["worst_channel_delta"],
                    "passed": ok,
                }
            )
            if not ok:
                failures += 1
                write_png(
                    diff_dir / f"{name}.png",
                    baseline_image[0],
                    baseline_image[1],
                    report["diff_pixels"],  # type: ignore[arg-type]
                )
                entry["diff"] = str(diff_dir / f"{name}.png")
            print(
                f"[{'ok' if ok else 'diff':8}] {name:22} "
                f"变化 {ratio * 100:6.3f}%  平均差 {mean:5.3f}  "
                f"峰值 {report['worst_channel_delta']:3}"
            )
        results.append(entry)

    report_path = output_dir / "report.json"
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(
        json.dumps(
            {
                "binary": str(binary),
                "pixel_tolerance": args.pixel_tolerance,
                "max_changed_ratio": args.max_changed_ratio,
                "max_mean_delta": args.max_mean_delta,
                "updated": bool(args.update),
                "results": results,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    print(f"\n报告：{report_path}")
    if args.update:
        print(f"基线已更新：{baseline_dir}")
        return 0
    if failures:
        print(f"{failures} 项未通过，差异图见 {diff_dir}")
        return 1
    print(f"{len(results)} 项全部在阈值内")
    return 0


if __name__ == "__main__":
    sys.exit(main())
