#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import argparse
import json
from pathlib import Path
from typing import Any, Dict, List


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate JD and role persona markdown.")
    parser.add_argument("--input", required=True, help="Input JSON path.")
    parser.add_argument("--output", required=True, help="Output Markdown path.")
    return parser.parse_args()


def list_block(items: List[str]) -> List[str]:
    if not items:
        return ["（暂无）"]
    return [f"- {item}" for item in items]


def get_list(data: Dict[str, Any], key: str) -> List[str]:
    value = data.get(key, [])
    if isinstance(value, list):
        return [str(v) for v in value]
    if value:
        return [str(value)]
    return []


def build_markdown(data: Dict[str, Any]) -> str:
    title = data.get("role_title", "岗位 JD")
    lines: List[str] = [f"# {title}", ""]

    info_lines = []
    for label, key in [
        ("部门", "department"),
        ("地点", "location"),
        ("类型", "employment_type"),
        ("汇报对象", "reporting_to"),
    ]:
        value = data.get(key, "")
        if value:
            info_lines.append(f"- {label}：{value}")
    if info_lines:
        lines.extend(info_lines)
        lines.append("")

    lines.append("## 岗位职责")
    lines.extend(list_block(get_list(data, "responsibilities")))
    lines.append("")

    lines.append("## 任职要求")
    lines.extend(list_block(get_list(data, "requirements")))
    lines.append("")

    lines.append("## 加分项")
    lines.extend(list_block(get_list(data, "nice_to_have")))
    lines.append("")

    lines.append("## 关键技能")
    lines.extend(list_block(get_list(data, "skills")))
    lines.append("")

    lines.append("## 福利与发展")
    lines.extend(list_block(get_list(data, "benefits")))
    lines.append("")

    persona = data.get("persona", {}) if isinstance(data.get("persona"), dict) else {}
    lines.append("# 岗位画像模板")
    lines.append("")
    background = persona.get("background", "")
    lines.append("## 候选人背景")
    lines.append(background if background else "（暂无）")
    lines.append("")

    lines.append("## 能力标签")
    lines.extend(list_block(get_list(persona, "strengths")))
    lines.append("")

    lines.append("## 动机与偏好")
    lines.extend(list_block(get_list(persona, "motivators")))
    lines.append("")

    lines.append("## 筛选问题")
    lines.extend(list_block(get_list(persona, "screening_questions")))

    return "\n".join(lines).strip() + "\n"


def main() -> int:
    args = parse_args()
    data = json.loads(Path(args.input).read_text(encoding="utf-8-sig"))
    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(build_markdown(data), encoding="utf-8")
    print(f"Saved: {output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
