#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import argparse
import csv
from collections import defaultdict
from pathlib import Path
from typing import Dict, List, Tuple


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Summarize CSV data and output SVG chart.")
    parser.add_argument("--input", required=True, help="Input CSV path.")
    parser.add_argument("--value-column", required=True, help="Numeric column name.")
    parser.add_argument("--group-by", default="", help="Group-by column for bar chart.")
    parser.add_argument("--date-column", default="", help="Date column for line chart.")
    parser.add_argument("--output-md", required=True, help="Output Markdown path.")
    parser.add_argument("--output-svg", required=True, help="Output SVG path.")
    parser.add_argument("--title", default="报表汇总", help="Report title.")
    return parser.parse_args()


def safe_float(value: str) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def read_rows(path: Path) -> List[Dict[str, str]]:
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle)
        return list(reader)


def summarize(values: List[float]) -> Dict[str, float]:
    if not values:
        return {"count": 0, "total": 0.0, "avg": 0.0, "min": 0.0, "max": 0.0}
    total = sum(values)
    return {
        "count": len(values),
        "total": total,
        "avg": total / len(values),
        "min": min(values),
        "max": max(values),
    }


def group_sum(rows: List[Dict[str, str]], group_by: str, value_column: str) -> Dict[str, float]:
    result: Dict[str, float] = defaultdict(float)
    for row in rows:
        value = safe_float(row.get(value_column, ""))
        if value is None:
            continue
        key = row.get(group_by, "未分类") or "未分类"
        result[key] += value
    return dict(result)


def date_sum(rows: List[Dict[str, str]], date_column: str, value_column: str) -> Dict[str, float]:
    result: Dict[str, float] = defaultdict(float)
    for row in rows:
        value = safe_float(row.get(value_column, ""))
        if value is None:
            continue
        key = row.get(date_column, "")
        if not key:
            continue
        result[key] += value
    return dict(sorted(result.items()))


def generate_bar_svg(labels: List[str], values: List[float], title: str) -> str:
    width, height = 1200, 600
    margin = 80
    chart_width = width - margin * 2
    chart_height = height - margin * 2
    count = max(1, len(labels))
    gap = 20
    bar_width = max(20, int((chart_width - gap * (count - 1)) / count))
    max_value = max(values) if values else 1

    svg = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        f'<rect width="{width}" height="{height}" fill="#f8fafc" />',
        f'<text x="{margin}" y="{margin - 30}" font-size="28" font-family="Microsoft YaHei, sans-serif" fill="#0f172a">{title}</text>',
    ]

    for idx, (label, value) in enumerate(zip(labels, values)):
        x = margin + idx * (bar_width + gap)
        bar_height = 0 if max_value == 0 else int((value / max_value) * chart_height)
        y = margin + (chart_height - bar_height)
        svg.append(f'<rect x="{x}" y="{y}" width="{bar_width}" height="{bar_height}" fill="#3b82f6" rx="6" />')
        svg.append(
            f'<text x="{x + bar_width / 2}" y="{height - margin + 24}" text-anchor="middle" font-size="16" font-family="Microsoft YaHei, sans-serif" fill="#334155">{label}</text>'
        )
        svg.append(
            f'<text x="{x + bar_width / 2}" y="{y - 8}" text-anchor="middle" font-size="14" font-family="Microsoft YaHei, sans-serif" fill="#1e293b">{value:.0f}</text>'
        )

    svg.append("</svg>")
    return "\n".join(svg)


def generate_line_svg(labels: List[str], values: List[float], title: str) -> str:
    width, height = 1200, 600
    margin = 80
    chart_width = width - margin * 2
    chart_height = height - margin * 2
    count = max(1, len(labels))
    max_value = max(values) if values else 1

    points: List[Tuple[float, float]] = []
    for idx, value in enumerate(values):
        x = margin + (chart_width / max(1, count - 1)) * idx if count > 1 else margin + chart_width / 2
        y = margin + (chart_height - (0 if max_value == 0 else (value / max_value) * chart_height))
        points.append((x, y))

    path = " ".join([f"L {x:.2f} {y:.2f}" for x, y in points])
    if path:
        path = "M " + path[2:]

    svg = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        f'<rect width="{width}" height="{height}" fill="#f8fafc" />',
        f'<text x="{margin}" y="{margin - 30}" font-size="28" font-family="Microsoft YaHei, sans-serif" fill="#0f172a">{title}</text>',
        f'<path d="{path}" fill="none" stroke="#0ea5e9" stroke-width="3" />',
    ]

    for idx, (label, value) in enumerate(zip(labels, values)):
        x, y = points[idx]
        svg.append(f'<circle cx="{x:.2f}" cy="{y:.2f}" r="5" fill="#0ea5e9" />')
        svg.append(
            f'<text x="{x:.2f}" y="{height - margin + 24}" text-anchor="middle" font-size="16" font-family="Microsoft YaHei, sans-serif" fill="#334155">{label}</text>'
        )
        svg.append(
            f'<text x="{x:.2f}" y="{y - 8:.2f}" text-anchor="middle" font-size="14" font-family="Microsoft YaHei, sans-serif" fill="#1e293b">{value:.0f}</text>'
        )

    svg.append("</svg>")
    return "\n".join(svg)


def build_report(
    title: str,
    summary: Dict[str, float],
    group_stats: Dict[str, float],
    date_stats: Dict[str, float],
) -> str:
    lines = [f"# {title}", "", "## 总览", ""]
    lines.append(f"- 数据量：{summary['count']}")
    lines.append(f"- 总计：{summary['total']:.2f}")
    lines.append(f"- 平均：{summary['avg']:.2f}")
    lines.append(f"- 最小：{summary['min']:.2f}")
    lines.append(f"- 最大：{summary['max']:.2f}")
    lines.append("")

    if group_stats:
        lines.append("## 分组汇总")
        lines.append("")
        lines.append("| 分组 | 合计 |")
        lines.append("| --- | --- |")
        for key, value in sorted(group_stats.items(), key=lambda item: item[1], reverse=True):
            lines.append(f"| {key} | {value:.2f} |")
        lines.append("")

    if date_stats:
        lines.append("## 按日期汇总")
        lines.append("")
        lines.append("| 日期 | 合计 |")
        lines.append("| --- | --- |")
        for key, value in date_stats.items():
            lines.append(f"| {key} | {value:.2f} |")
        lines.append("")

    return "\n".join(lines).strip() + "\n"


def main() -> int:
    args = parse_args()
    input_path = Path(args.input)
    output_md = Path(args.output_md)
    output_svg = Path(args.output_svg)

    rows = read_rows(input_path)
    values = [safe_float(row.get(args.value_column, "")) for row in rows]
    values = [v for v in values if v is not None]

    summary = summarize(values)
    group_stats = group_sum(rows, args.group_by, args.value_column) if args.group_by else {}
    date_stats = date_sum(rows, args.date_column, args.value_column) if args.date_column else {}

    report = build_report(args.title, summary, group_stats, date_stats)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    output_md.write_text(report, encoding="utf-8")

    if group_stats:
        labels = list(group_stats.keys())
        data = list(group_stats.values())
        svg_text = generate_bar_svg(labels, data, f"{args.title}（分组汇总）")
    elif date_stats:
        labels = list(date_stats.keys())
        data = list(date_stats.values())
        svg_text = generate_line_svg(labels, data, f"{args.title}（时间序列）")
    else:
        svg_text = generate_bar_svg(["总计"], [summary["total"]], args.title)

    output_svg.parent.mkdir(parents=True, exist_ok=True)
    output_svg.write_text(svg_text, encoding="utf-8")

    print(f"Saved: {output_md}")
    print(f"Saved: {output_svg}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
