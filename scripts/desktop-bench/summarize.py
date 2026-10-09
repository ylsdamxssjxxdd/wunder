#!/usr/bin/env python3
"""Merge desktop-bench JSON reports into a side-by-side markdown table.

Usage:
  python summarize.py --out report.md file1.json file2.json ...

Each input is one of: startup / memory / package report produced by the
desktop-bench PowerShell scripts. Output is a markdown fragment ready to
paste into a docs/ baseline report.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def fmt(value: object, unit: str = "") -> str:
    if value is None or value == -1:
        return "n/a"
    if isinstance(value, float):
        text = f"{value:.1f}"
    else:
        text = str(value)
    return f"{text}{unit}"


def startup_rows(data: dict) -> list[tuple[str, str]]:
    return [
        ("T1 window visible median", fmt(data.get("median_ms"), " ms")),
        ("T1 min / max", f"{fmt(data.get('min_ms'), ' ms')} / {fmt(data.get('max_ms'), ' ms')}"),
        ("T1 valid samples", f"{data.get('valid_count')}/{data.get('runs')}"),
    ]


def memory_rows(data: dict) -> list[tuple[str, str]]:
    return [
        ("processes", fmt(data.get("end_process_count"))),
        ("steady working set", fmt(data.get("steady_working_set_mb"), " MB")),
        ("steady private", fmt(data.get("steady_private_mb"), " MB")),
        ("peak working set", fmt(data.get("peak_working_set_mb"), " MB")),
        ("peak private", fmt(data.get("peak_private_mb"), " MB")),
    ]


def package_rows(data: dict) -> list[tuple[str, str]]:
    rows: list[tuple[str, str]] = []
    if data.get("target_type") == "directory":
        rows.append(("installed size", fmt(data.get("total_size_mb"), " MB")))
        rows.append(("file count", fmt(data.get("file_count"))))
        rows.append(("exe / dll / node / asar", (
            f"{data.get('exe_count')} / {data.get('dll_count')} / "
            f"{data.get('node_count')} / {data.get('asar_count')}"
        )))
        top = data.get("top10_files") or []
        if top:
            rows.append(("largest file", f"{top[0]['path']} ({fmt(top[0]['size_mb'], ' MB')})"))
    else:
        rows.append(("installer size", fmt(data.get("installer_size_mb"), " MB")))
    return rows


ROW_BUILDERS = {
    "startup": startup_rows,
    "memory": memory_rows,
    "package": package_rows,
}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", nargs="+", help="JSON report files")
    parser.add_argument("--out", default="", help="output markdown file (default: stdout)")
    args = parser.parse_args()

    reports: list[tuple[str, str, dict]] = []
    for raw in args.inputs:
        path = Path(raw)
        data = json.loads(path.read_text(encoding="utf-8-sig"))
        kind = data.get("kind", "unknown")
        name = data.get("label") or data.get("process_name") or Path(data.get("exe", path.name)).name
        reports.append((kind, str(name), data))

    lines: list[str] = ["# Desktop bench summary", ""]

    # One block per kind so metrics of the same family sit together.
    for kind in ("startup", "memory", "package"):
        group = [(name, data) for k, name, data in reports if k == kind]
        if not group:
            continue
        # Union of metric names, in first-seen order.
        metric_names: list[str] = []
        per_report: dict[str, dict[str, str]] = {}
        for name, data in group:
            values: dict[str, str] = {}
            for metric, value in ROW_BUILDERS[kind](data):
                values[metric] = value
                if metric not in metric_names:
                    metric_names.append(metric)
            per_report[name] = values

        lines.append(f"## {kind}")
        lines.append("")
        header = "| metric | " + " | ".join(per_report) + " |"
        sep = "| --- | " + " | ".join(["---:"] * len(per_report)) + " |"
        lines.append(header)
        lines.append(sep)
        for metric in metric_names:
            cells = " | ".join(per_report[name].get(metric, "-") for name in per_report)
            lines.append(f"| {metric} | {cells} |")
        lines.append("")

    text = "\n".join(lines)
    if args.out:
        Path(args.out).write_text(text, encoding="utf-8")
        print(f"written: {args.out}")
    else:
        print(text)


if __name__ == "__main__":
    main()
