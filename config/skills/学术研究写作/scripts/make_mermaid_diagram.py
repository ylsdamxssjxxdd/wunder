#!/usr/bin/env python3
# -*- coding: utf-8 -*-
from __future__ import annotations

import argparse
from pathlib import Path


def parse_items(raw_items: list[str]) -> list[str]:
    items: list[str] = []
    for raw in raw_items:
        for part in raw.split(","):
            item = part.strip()
            if item:
                items.append(item)
    return items


def build_flowchart(items: list[str], direction: str) -> str:
    lines = [f"flowchart {direction}"]
    for index, item in enumerate(items, start=1):
        lines.append(f"    N{index}[{item}]")
    for index in range(1, len(items)):
        lines.append(f"    N{index} --> N{index + 1}")
    return "\n".join(lines)


def build_concept(items: list[str], center: str, direction: str) -> str:
    lines = [f"flowchart {direction}", f"    C(({center}))"]
    for index, item in enumerate(items, start=1):
        lines.append(f"    C --> N{index}[{item}]")
    return "\n".join(lines)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="生成 Mermaid 图表草案。")
    parser.add_argument(
        "--type",
        choices=("flow", "concept"),
        default="flow",
        help="图表类型：flow 为流程图，concept 为概念图。",
    )
    parser.add_argument(
        "--direction",
        choices=("TD", "LR"),
        default="TD",
        help="图表方向：TD 自上而下，LR 从左到右。",
    )
    parser.add_argument(
        "--center",
        default="核心概念",
        help="概念图中心节点，仅 type=concept 时使用。",
    )
    parser.add_argument(
        "--item",
        action="append",
        required=True,
        help="节点，可重复传入，也可用英文逗号分隔。",
    )
    parser.add_argument("--output", help="输出 Markdown 文件。")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    items = parse_items(args.item)
    if len(items) < 2:
        raise SystemExit("至少需要 2 个节点。")

    if args.type == "concept":
        diagram = build_concept(items, args.center, args.direction)
    else:
        diagram = build_flowchart(items, args.direction)

    content = f"```mermaid\n{diagram}\n```\n"
    if args.output:
        output_path = Path(args.output).expanduser().resolve()
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(content, encoding="utf-8")
        print(str(output_path))
    else:
        print(content, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
