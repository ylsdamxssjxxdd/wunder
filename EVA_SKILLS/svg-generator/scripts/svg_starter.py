#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import argparse
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate a clean SVG diagram for PPT/academic use.",
    )
    parser.add_argument("--output", default="out.svg", help="Output SVG path.")
    parser.add_argument("--width", type=int, default=1600, help="Canvas width in px.")
    parser.add_argument("--height", type=int, default=900, help="Canvas height in px.")
    parser.add_argument("--title", default="Diagram Title", help="Title text.")
    parser.add_argument("--subtitle", default="", help="Optional subtitle text.")
    parser.add_argument(
        "--steps",
        nargs="*",
        default=["Collect", "Process", "Publish"],
        help="Step labels for the flow.",
    )
    parser.add_argument("--accent", default="#3b82f6", help="Accent color.")
    return parser.parse_args()


def escape_xml(text: str) -> str:
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
        .replace("'", "&apos;")
    )


def normalize_color(value: str, fallback: str) -> str:
    if not value:
        return fallback
    value = value.strip()
    if value.startswith("#"):
        return value
    return f"#{value}"


def build_svg(width: int, height: int, title: str, subtitle: str, steps: list[str], accent: str) -> str:
    colors = {
        "bg": "#f8fafc",
        "card": "#ffffff",
        "stroke": "#d8e2f2",
        "text": "#0f172a",
        "muted": "#64748b",
    }
    accent = normalize_color(accent, "#3b82f6")

    margin_x = int(width * 0.08)
    margin_y = int(height * 0.12)
    step_count = max(1, len(steps))
    gap = int(width * 0.04)
    box_width = int((width - margin_x * 2 - gap * (step_count - 1)) / step_count)
    box_width = max(200, min(340, box_width))
    total_width = box_width * step_count + gap * (step_count - 1)
    start_x = int((width - total_width) / 2)
    box_height = int(height * 0.16)
    box_y = int(height * 0.45)
    radius = 18

    title_y = margin_y
    subtitle_y = title_y + 52

    svg_parts = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        "  <defs>",
        "    <marker id=\"arrow\" markerWidth=\"12\" markerHeight=\"12\" refX=\"10\" refY=\"6\" orient=\"auto\">",
        f"      <path d=\"M0,0 L12,6 L0,12 Z\" fill=\"{accent}\" />",
        "    </marker>",
        "    <filter id=\"shadow\" x=\"-20%\" y=\"-20%\" width=\"140%\" height=\"140%\">",
        "      <feDropShadow dx=\"0\" dy=\"8\" stdDeviation=\"12\" flood-color=\"#1e293b\" flood-opacity=\"0.12\" />",
        "    </filter>",
        "  </defs>",
        f"  <rect width=\"{width}\" height=\"{height}\" fill=\"{colors['bg']}\" />",
        f"  <text x=\"{margin_x}\" y=\"{title_y}\" font-size=\"46\" font-family=\"Microsoft YaHei, PingFang SC, Noto Sans CJK SC, Arial, sans-serif\" fill=\"{colors['text']}\">",
        f"    {escape_xml(title)}",
        "  </text>",
    ]

    if subtitle:
        svg_parts.extend(
            [
                f"  <text x=\"{margin_x}\" y=\"{subtitle_y}\" font-size=\"24\" font-family=\"Microsoft YaHei, PingFang SC, Noto Sans CJK SC, Arial, sans-serif\" fill=\"{colors['muted']}\">",
                f"    {escape_xml(subtitle)}",
                "  </text>",
            ]
        )

    for idx, label in enumerate(steps):
        x = start_x + idx * (box_width + gap)
        svg_parts.extend(
            [
                f"  <rect x=\"{x}\" y=\"{box_y}\" width=\"{box_width}\" height=\"{box_height}\" rx=\"{radius}\" fill=\"{colors['card']}\" stroke=\"{colors['stroke']}\" filter=\"url(#shadow)\" />",
                f"  <text x=\"{x + box_width / 2}\" y=\"{box_y + box_height / 2}\" font-size=\"28\" font-family=\"Microsoft YaHei, PingFang SC, Noto Sans CJK SC, Arial, sans-serif\" text-anchor=\"middle\" dominant-baseline=\"middle\" fill=\"{colors['text']}\">",
                f"    {escape_xml(label)}",
                "  </text>",
            ]
        )
        if idx < step_count - 1:
            line_x1 = x + box_width
            line_x2 = x + box_width + gap
            line_y = box_y + box_height / 2
            svg_parts.append(
                f"  <line x1=\"{line_x1}\" y1=\"{line_y}\" x2=\"{line_x2}\" y2=\"{line_y}\" stroke=\"{accent}\" stroke-width=\"3\" marker-end=\"url(#arrow)\" />"
            )

    svg_parts.append("</svg>")
    return "\n".join(svg_parts)


def main() -> int:
    args = parse_args()
    output_path = Path(args.output).resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    svg_text = build_svg(
        args.width,
        args.height,
        args.title,
        args.subtitle,
        args.steps,
        args.accent,
    )
    output_path.write_text(svg_text, encoding="utf-8")
    print(f"Saved: {output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
