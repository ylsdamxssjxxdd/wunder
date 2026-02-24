#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import argparse
import json
import re
from collections import defaultdict
from pathlib import Path
from typing import Any, Dict, List

SECTION_ALIASES = {
    "议题": "topics",
    "主题": "topics",
    "讨论": "discussion",
    "讨论要点": "discussion",
    "决议": "decisions",
    "结论": "decisions",
    "行动项": "action_items",
    "待办": "action_items",
    "行动项/待办": "action_items",
    "时间节点": "milestones",
    "里程碑": "milestones",
    "风险": "risks",
    "依赖": "dependencies",
    "问题": "issues",
    "其他": "notes",
    "记录": "notes",
}

METADATA_LABELS = {
    "日期": "date",
    "主持": "host",
    "参会": "attendees",
    "地点": "location",
    "记录人": "recorder",
    "主题": "title",
    "会议目标": "goal",
}

DATE_PATTERN = re.compile(r"\b\d{4}[-/]\d{1,2}[-/]\d{1,2}\b")
SHORT_DATE_PATTERN = re.compile(r"\b\d{1,2}[-/]\d{1,2}\b")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate meeting minutes with action items and milestones.",
    )
    parser.add_argument("--input", required=True, help="Input notes text file.")
    parser.add_argument("--output", required=True, help="Output Markdown path.")
    parser.add_argument("--json", default="", help="Optional output JSON path.")
    return parser.parse_args()


def split_kv(text: str) -> tuple[str, str] | tuple[None, None]:
    for sep in ("：", ":"):
        if sep in text:
            key, value = text.split(sep, 1)
            return key.strip(), value.strip()
    return None, None


def normalize_section(label: str) -> str | None:
    clean = label.strip().strip("：:")
    return SECTION_ALIASES.get(clean)


def parse_metadata(line: str) -> tuple[str, str] | tuple[None, None]:
    key, value = split_kv(line)
    if not key:
        return None, None
    if key in METADATA_LABELS:
        return METADATA_LABELS[key], value
    return None, None


def normalize_key(key: str) -> str | None:
    key = key.strip()
    lower = key.lower()
    english_map = {
        "task": "description",
        "item": "description",
        "owner": "owner",
        "due": "due",
        "deadline": "due",
        "priority": "priority",
        "status": "status",
        "note": "note",
    }
    if lower in english_map:
        return english_map[lower]
    chinese_map = {
        "任务": "description",
        "事项": "description",
        "内容": "description",
        "行动项": "description",
        "待办": "description",
        "负责人": "owner",
        "责任人": "owner",
        "截止": "due",
        "截止日期": "due",
        "优先级": "priority",
        "状态": "status",
        "备注": "note",
    }
    return chinese_map.get(key)


def extract_date(text: str) -> str:
    match = DATE_PATTERN.search(text)
    if match:
        return match.group(0).replace("/", "-")
    match = SHORT_DATE_PATTERN.search(text)
    if match:
        return match.group(0).replace("/", "-")
    return ""


def parse_action_item(text: str) -> Dict[str, str]:
    item = {
        "description": "",
        "owner": "",
        "due": "",
        "priority": "",
        "status": "",
        "note": "",
    }
    parts = re.split(r"[；;|]", text)
    for part in parts:
        part = part.strip().lstrip("-•*")
        if not part:
            continue
        key, value = split_kv(part)
        if key:
            field = normalize_key(key)
            if field:
                item[field] = value
                continue
        if not item["description"]:
            item["description"] = part.strip()
        else:
            item["note"] = (item["note"] + " " + part.strip()).strip()
    if not item["due"]:
        item["due"] = extract_date(text)
    if not item["description"]:
        item["description"] = text.strip()
    return item


def parse_milestone(text: str) -> Dict[str, str]:
    text = text.strip().lstrip("-•*")
    date_text = extract_date(text)
    description = text
    if date_text:
        description = description.replace(date_text, "").strip(" ：:;-\t")
    if "：" in description or ":" in description:
        key, value = split_kv(description)
        if value:
            date_text = date_text or key
            description = value
    return {
        "date": date_text,
        "item": description,
    }


def parse_notes(text: str) -> Dict[str, Any]:
    metadata: Dict[str, str] = {}
    sections: Dict[str, List[str]] = defaultdict(list)
    action_items: List[Dict[str, str]] = []
    milestones: List[Dict[str, str]] = []
    current_section = "notes"

    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue

        meta_key, meta_value = parse_metadata(line)
        if meta_key:
            metadata[meta_key] = meta_value
            continue

        if line.startswith("#"):
            label = line.lstrip("#").strip()
            section = normalize_section(label)
            current_section = section or "notes"
            continue

        if line.endswith(("：", ":")):
            section = normalize_section(line)
            if section:
                current_section = section
                continue

        if current_section == "action_items":
            action_items.append(parse_action_item(line))
        elif current_section == "milestones":
            milestones.append(parse_milestone(line))
        else:
            sections[current_section].append(line)

    return {
        "metadata": metadata,
        "sections": sections,
        "action_items": action_items,
        "milestones": milestones,
    }


def format_bullets(lines: List[str]) -> List[str]:
    if not lines:
        return ["（暂无）"]
    if any(line.lstrip().startswith(("-", "*")) for line in lines):
        return lines
    return [f"- {line}" for line in lines]


def render_table(headers: List[str], rows: List[List[str]]) -> List[str]:
    table = [
        "| " + " | ".join(headers) + " |",
        "| " + " | ".join(["---"] * len(headers)) + " |",
    ]
    for row in rows:
        table.append("| " + " | ".join(row) + " |")
    return table


def build_markdown(data: Dict[str, Any]) -> str:
    metadata = data["metadata"]
    sections = data["sections"]
    action_items = data["action_items"]
    milestones = data["milestones"]

    title = metadata.get("title") or "会议纪要"
    lines: List[str] = [f"# {title}", ""]

    meta_order = [
        ("date", "日期"),
        ("host", "主持"),
        ("attendees", "参会"),
        ("location", "地点"),
        ("recorder", "记录人"),
        ("goal", "会议目标"),
    ]
    meta_lines = []
    for key, label in meta_order:
        value = metadata.get(key, "")
        if value:
            meta_lines.append(f"- {label}：{value}")
    if meta_lines:
        lines.extend(meta_lines)
        lines.append("")

    topic_lines = sections.get("topics", []) + sections.get("discussion", [])
    lines.append("## 议题与讨论要点")
    lines.extend(format_bullets(topic_lines))
    lines.append("")

    lines.append("## 决议")
    lines.extend(format_bullets(sections.get("decisions", [])))
    lines.append("")

    lines.append("## 待办/行动项")
    if action_items:
        rows = []
        for idx, item in enumerate(action_items, start=1):
            rows.append(
                [
                    str(idx),
                    item.get("description", ""),
                    item.get("owner", ""),
                    item.get("due", ""),
                    item.get("priority", ""),
                    item.get("status", ""),
                    item.get("note", ""),
                ]
            )
        lines.extend(
            render_table(
                ["序号", "事项", "负责人", "截止", "优先级", "状态", "备注"],
                rows,
            )
        )
    else:
        lines.append("（暂无）")
    lines.append("")

    lines.append("## 时间节点清单")
    if milestones:
        rows = []
        for idx, item in enumerate(milestones, start=1):
            rows.append([str(idx), item.get("date", ""), item.get("item", "")])
        lines.extend(render_table(["序号", "日期", "事项"], rows))
    else:
        lines.append("（暂无）")
    lines.append("")

    risk_lines = sections.get("risks", []) + sections.get("dependencies", []) + sections.get("issues", [])
    lines.append("## 风险与依赖")
    lines.extend(format_bullets(risk_lines))
    lines.append("")

    lines.append("## 其他")
    lines.extend(format_bullets(sections.get("notes", [])))

    return "\n".join(lines).strip() + "\n"


def main() -> int:
    args = parse_args()
    input_path = Path(args.input)
    output_path = Path(args.output)
    json_path = Path(args.json) if args.json else None

    text = input_path.read_text(encoding="utf-8-sig")
    data = parse_notes(text)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(build_markdown(data), encoding="utf-8")

    if json_path:
        json_path.parent.mkdir(parents=True, exist_ok=True)
        json_path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")

    print(f"Saved Markdown: {output_path}")
    if json_path:
        print(f"Saved JSON: {json_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
