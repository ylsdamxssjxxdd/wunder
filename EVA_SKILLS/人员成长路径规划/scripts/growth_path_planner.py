#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import argparse
import json
from pathlib import Path
from typing import Any, Dict, List


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate growth path report and skill gap chart.")
    parser.add_argument("--input", required=True, help="Input JSON path.")
    parser.add_argument("--output-md", required=True, help="Output Markdown path.")
    parser.add_argument("--output-svg", required=True, help="Output SVG chart path.")
    parser.add_argument("--title", default="", help="Report title.")
    return parser.parse_args()


def read_json(path: Path) -> Dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def safe_list(value: Any) -> List[Any]:
    return value if isinstance(value, list) else []


def safe_str(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value.strip()
    if isinstance(value, (int, float)):
        return str(value)
    return ""


def safe_number(value: Any, default: float = 0.0) -> float:
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def normalize_skills(raw_skills: Any) -> List[Dict[str, Any]]:
    items: List[Dict[str, Any]] = []
    for item in safe_list(raw_skills):
        if not isinstance(item, dict):
            continue
        name = safe_str(item.get("name"))
        if not name:
            continue
        current = safe_number(item.get("current"), 0.0)
        target = safe_number(item.get("target"), current)
        gap = max(0.0, target - current)
        items.append({"name": name, "current": current, "target": target, "gap": gap})
    return items


def format_bullets(lines: List[str], title: str, items: List[str]) -> None:
    if not items:
        return
    lines.append(f"## {title}")
    lines.append("")
    for item in items:
        lines.append(f"- {item}")
    lines.append("")


def build_report(data: Dict[str, Any], title: str) -> str:
    person = data.get("person", {}) if isinstance(data.get("person"), dict) else {}
    goal = data.get("goal", {}) if isinstance(data.get("goal"), dict) else {}
    experience = data.get("experience", {}) if isinstance(data.get("experience"), dict) else {}
    skills = normalize_skills(data.get("skills"))
    milestones = data.get("milestones", {}) if isinstance(data.get("milestones"), dict) else {}
    actions = data.get("actions", {}) if isinstance(data.get("actions"), dict) else {}
    sources = safe_list(data.get("data_sources"))
    notes = safe_str(data.get("notes"))

    report_title = title or "人员成长路径规划报告"
    name = safe_str(person.get("name"))
    if name:
        report_title = f"{report_title} - {name}"

    lines: List[str] = [f"# {report_title}", "", "## 基本信息", ""]

    def add_kv(key: str, value: Any) -> None:
        value_text = safe_str(value)
        if value_text:
            lines.append(f"- {key}：{value_text}")

    add_kv("姓名", person.get("name"))
    add_kv("当前岗位", person.get("current_role"))
    add_kv("目标岗位", person.get("target_role"))
    add_kv("当前职级", person.get("level"))
    add_kv("目标职级", goal.get("target_level"))
    add_kv("从业年限", person.get("experience_years"))
    add_kv("部门/区域", person.get("department"))
    add_kv("发展方向", goal.get("direction"))
    add_kv("时间跨度（月）", goal.get("timeframe_months"))
    lines.append("")

    projects = safe_list(experience.get("projects"))
    achievements = safe_list(experience.get("achievements"))
    if projects or achievements:
        lines.append("## 关键经历与成果")
        lines.append("")
        for project in projects:
            if not isinstance(project, dict):
                continue
            project_name = safe_str(project.get("name"))
            role = safe_str(project.get("role"))
            impact = safe_str(project.get("impact"))
            skill_tags = ", ".join([safe_str(s) for s in safe_list(project.get("skills")) if safe_str(s)])
            detail = "、".join(filter(None, [role, impact, skill_tags]))
            if project_name:
                lines.append(f"- {project_name}{f'（{detail}）' if detail else ''}")
        for achievement in achievements:
            if safe_str(achievement):
                lines.append(f"- {safe_str(achievement)}")
        lines.append("")

    if skills:
        strengths = sorted(skills, key=lambda item: item["current"], reverse=True)[:3]
        gaps = [item for item in sorted(skills, key=lambda item: item["gap"], reverse=True) if item["gap"] > 0]
        lines.append("## 能力现状与差距")
        lines.append("")
        if strengths:
            lines.append("- 优势能力：" + "、".join([item["name"] for item in strengths]))
        if gaps:
            lines.append("- 主要差距：" + "、".join([item["name"] for item in gaps[:3]]))
        lines.append("")

        lines.append("| 能力 | 当前 | 目标 | 差距 |")
        lines.append("| --- | --- | --- | --- |")
        for item in skills:
            lines.append(f"| {item['name']} | {item['current']:.1f} | {item['target']:.1f} | {item['gap']:.1f} |")
        lines.append("")

    lines.append("## 成长路径规划")
    lines.append("")

    def add_milestone(title_text: str, items: Any) -> None:
        milestones_list = [safe_str(item) for item in safe_list(items) if safe_str(item)]
        if not milestones_list:
            return
        lines.append(f"### {title_text}")
        lines.append("")
        for item in milestones_list:
            lines.append(f"- {item}")
        lines.append("")

    add_milestone("短期（0-3个月）", milestones.get("short_term"))
    add_milestone("中期（4-9个月）", milestones.get("mid_term"))
    add_milestone("长期（10-12个月）", milestones.get("long_term"))

    if not any([milestones.get("short_term"), milestones.get("mid_term"), milestones.get("long_term")]):
        lines.append("- 请补充里程碑信息（短/中/长期目标）。")
        lines.append("")

    lines.append("## 行动建议")
    lines.append("")

    def add_action(title_text: str, items: Any) -> None:
        action_items = [safe_str(item) for item in safe_list(items) if safe_str(item)]
        if not action_items:
            return
        lines.append(f"### {title_text}")
        lines.append("")
        for item in action_items:
            lines.append(f"- {item}")
        lines.append("")

    add_action("学习与认证", actions.get("learning"))
    add_action("项目与实践", actions.get("projects"))
    add_action("导师与支持", actions.get("mentors"))
    add_action("习惯与输出", actions.get("habits"))

    if not any(actions.get(key) for key in ["learning", "projects", "mentors", "habits"]):
        gaps = [item for item in sorted(skills, key=lambda item: item["gap"], reverse=True) if item["gap"] > 0]
        if gaps:
            lines.append("### 建议方向")
            lines.append("")
            for item in gaps[:3]:
                lines.append(f"- 针对 {item['name']} 制定专项学习与实践计划，设置阶段性验收标准。")
            lines.append("")

    lines.append("## 指标与评估节奏")
    lines.append("")
    lines.append("- 每季度复盘一次能力差距与里程碑达成情况。")
    lines.append("- 建立量化指标（如系统可用性、性能指标、交付质量、影响力范围）。")
    lines.append("")

    if sources:
        lines.append("## 数据来源")
        lines.append("")
        for source in sources:
            if not isinstance(source, dict):
                continue
            source_type = safe_str(source.get("type"))
            name = safe_str(source.get("name"))
            query = safe_str(source.get("query"))
            text = " / ".join(filter(None, [source_type, name, query]))
            if text:
                lines.append(f"- {text}")
        lines.append("")

    if notes:
        format_bullets(lines, "备注", [notes])

    return "\n".join(lines).strip() + "\n"


def generate_skill_gap_svg(skills: List[Dict[str, Any]], title: str) -> str:
    width, height = 1200, 650
    margin = 80
    chart_width = width - margin * 2
    chart_height = height - margin * 2

    if not skills:
        return "\n".join(
            [
                '<?xml version="1.0" encoding="UTF-8"?>',
                f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
                f'<rect width="{width}" height="{height}" fill="#f8fafc" />',
                f'<text x="{margin}" y="{margin}" font-size="28" font-family="Microsoft YaHei, sans-serif" fill="#0f172a">{title}</text>',
                f'<text x="{width / 2}" y="{height / 2}" text-anchor="middle" font-size="20" font-family="Microsoft YaHei, sans-serif" fill="#64748b">暂无能力数据</text>',
                "</svg>",
            ]
        )

    max_value = max([max(item["current"], item["target"]) for item in skills] + [1])
    group_count = len(skills)
    group_gap = 30
    bar_gap = 10
    group_width = chart_width / group_count if group_count else chart_width
    bar_width = max(12, int((group_width - bar_gap) / 2))
    group_width = bar_width * 2 + bar_gap

    svg = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        f'<rect width="{width}" height="{height}" fill="#f8fafc" />',
        f'<text x="{margin}" y="{margin - 30}" font-size="28" font-family="Microsoft YaHei, sans-serif" fill="#0f172a">{title}</text>',
        f'<line x1="{margin}" y1="{height - margin}" x2="{width - margin}" y2="{height - margin}" stroke="#94a3b8" stroke-width="2" />',
        f'<rect x="{width - margin - 220}" y="{margin - 50}" width="12" height="12" fill="#2563eb" />',
        f'<text x="{width - margin - 200}" y="{margin - 40}" font-size="14" font-family="Microsoft YaHei, sans-serif" fill="#1f2937">当前</text>',
        f'<rect x="{width - margin - 140}" y="{margin - 50}" width="12" height="12" fill="#f97316" />',
        f'<text x="{width - margin - 120}" y="{margin - 40}" font-size="14" font-family="Microsoft YaHei, sans-serif" fill="#1f2937">目标</text>',
    ]

    for idx, item in enumerate(skills):
        group_x = margin + idx * (group_width + group_gap)
        current_height = 0 if max_value == 0 else int((item["current"] / max_value) * chart_height)
        target_height = 0 if max_value == 0 else int((item["target"] / max_value) * chart_height)

        current_x = group_x
        current_y = margin + (chart_height - current_height)
        target_x = group_x + bar_width + bar_gap
        target_y = margin + (chart_height - target_height)

        svg.append(
            f'<rect x="{current_x}" y="{current_y}" width="{bar_width}" height="{current_height}" fill="#2563eb" rx="4" />'
        )
        svg.append(
            f'<rect x="{target_x}" y="{target_y}" width="{bar_width}" height="{target_height}" fill="#f97316" rx="4" />'
        )
        svg.append(
            f'<text x="{current_x + bar_width / 2}" y="{current_y - 6}" text-anchor="middle" font-size="12" font-family="Microsoft YaHei, sans-serif" fill="#1e293b">{item["current"]:.1f}</text>'
        )
        svg.append(
            f'<text x="{target_x + bar_width / 2}" y="{target_y - 6}" text-anchor="middle" font-size="12" font-family="Microsoft YaHei, sans-serif" fill="#1e293b">{item["target"]:.1f}</text>'
        )
        svg.append(
            f'<text x="{group_x + group_width / 2}" y="{height - margin + 24}" text-anchor="middle" font-size="14" font-family="Microsoft YaHei, sans-serif" fill="#334155">{item["name"]}</text>'
        )

    svg.append("</svg>")
    return "\n".join(svg)


def main() -> int:
    args = parse_args()
    input_path = Path(args.input)
    output_md = Path(args.output_md)
    output_svg = Path(args.output_svg)

    data = read_json(input_path)
    skills = normalize_skills(data.get("skills"))
    report = build_report(data, args.title)

    output_md.parent.mkdir(parents=True, exist_ok=True)
    output_md.write_text(report, encoding="utf-8")

    chart_title = args.title or "能力差距图"
    svg_text = generate_skill_gap_svg(skills, chart_title)
    output_svg.parent.mkdir(parents=True, exist_ok=True)
    output_svg.write_text(svg_text, encoding="utf-8")

    print(f"Saved: {output_md}")
    print(f"Saved: {output_svg}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
