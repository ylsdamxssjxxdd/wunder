# AI生成
#!/usr/bin/env python3
"""Render form-bench results into a markdown matrix + csv.

Input is the aggregated form-bench.json produced by form-bench.ps1
(records: [{ form, collector, metric, value, unit, source, details }]).

Usage:
  python scripts/form-bench/summarize.py --in target/form-bench/form-bench.json ^
      --out-md target/form-bench/form-bench.md --out-csv target/form-bench/form-bench.csv
"""

from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path
from typing import Any, Dict, List

# metric key -> (label, unit) ; column order mirrors the README performance matrix
METRIC_ORDER = [
    ("startup", "启动速度", "ms"),
    ("memory", "内存占用", "MB"),
    ("cpu", "CPU 占用", "%"),
    ("package_size", "包体积", "MB"),
    ("concurrent_agent_threads", "并发智能体线程", "sessions"),
    ("chat_page_perf", "聊天页面性能", "ms"),
]

FORM_LABELS = {
    "hull": "舰体 Hull",
    "bridge": "舰桥 Bridge",
    "beehive": "蜂巢 Beehive",
    "honeycomb": "蜂窝 Honeycomb",
    "helm": "舵机 Helm",
}


def fmt(value: Any, unit: str) -> str:
    if value is None:
        return "—"
    if isinstance(value, bool):
        return "是" if value else "否"
    if isinstance(value, (int, float)):
        if isinstance(value, float) and value.is_integer():
            value = int(value)
        return f"{value} {unit}".strip()
    return str(value)


def load(path: Path) -> Dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def build_lookup(records: List[Dict[str, Any]]) -> Dict[str, Dict[str, Dict[str, Any]]]:
    table: Dict[str, Dict[str, Dict[str, Any]]] = {}
    for rec in records:
        form = rec.get("form")
        metric = rec.get("metric")
        if not form or not metric:
            continue
        table.setdefault(form, {})[metric] = rec
    return table


def render_markdown(report: Dict[str, Any], table: Dict[str, Dict[str, Dict[str, Any]]]) -> str:
    forms = report.get("forms") or list(table.keys())
    lines: List[str] = []
    lines.append("# form-bench 结果")
    lines.append("")
    lines.append(f"- 生成时间：{report.get('generated', '')}")
    lines.append(f"- base URL：{report.get('baseUrl', '')}")
    lines.append(f"- 采集指标：{', '.join(report.get('metrics', []))}")
    lines.append("")
    lines.append("## 性能矩阵（对齐 README）")
    lines.append("")
    header = "| 形态 | " + " | ".join(label for _, label, _ in METRIC_ORDER) + " |"
    sep = "| --- | " + " | ".join("---" for _ in METRIC_ORDER) + " |"
    lines.append(header)
    lines.append(sep)
    for form in forms:
        label = FORM_LABELS.get(form, form)
        cells = []
        for metric, _, unit in METRIC_ORDER:
            rec = table.get(form, {}).get(metric)
            cells.append(fmt(rec.get("value"), unit) if rec else "—")
        lines.append(f"| {label} | " + " | ".join(cells) + " |")
    lines.append("")

    lines.append("## 明细与备注")
    lines.append("")
    for form in forms:
        lines.append(f"### {FORM_LABELS.get(form, form)} (`{form}`)")
        lines.append("")
        for metric, label, unit in METRIC_ORDER:
            rec = table.get(form, {}).get(metric)
            if not rec:
                lines.append(f"- **{label}**：—（未采集）")
                continue
            value = fmt(rec.get("value"), unit)
            src = rec.get("source") or ""
            details = rec.get("details") or {}
            note = details.get("error") or details.get("note") or ""
            suffix = f" — {note}" if note else ""
            lines.append(f"- **{label}**：{value}（来源 `{src}`）{suffix}")
        lines.append("")
    return "\n".join(lines) + "\n"


def render_csv(report: Dict[str, Any], records: List[Dict[str, Any]], out: Path) -> None:
    fields = ["form", "metric", "value", "unit", "collector", "source"]
    with out.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fields)
        writer.writeheader()
        for rec in records:
            writer.writerow({
                "form": rec.get("form", ""),
                "metric": rec.get("metric", ""),
                "value": "" if rec.get("value") is None else rec.get("value"),
                "unit": rec.get("unit", ""),
                "collector": rec.get("collector", ""),
                "source": rec.get("source", ""),
            })


def main() -> int:
    parser = argparse.ArgumentParser(description="form-bench summarizer")
    parser.add_argument("--in", dest="in_path", required=True)
    parser.add_argument("--out-md", dest="out_md", required=True)
    parser.add_argument("--out-csv", dest="out_csv", default="")
    args = parser.parse_args()

    report = load(Path(args.in_path))
    records = report.get("records", [])
    table = build_lookup(records)

    md = render_markdown(report, table)
    Path(args.out_md).write_text(md, encoding="utf-8")
    print(f"[summarize] wrote {args.out_md} ({len(records)} records)")

    if args.out_csv:
        render_csv(report, records, Path(args.out_csv))
        print(f"[summarize] wrote {args.out_csv}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())