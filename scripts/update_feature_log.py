#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
用于向 docs/功能迭代.md 写入分类化变更记录的脚本。
默认使用 UTF-8 BOM（utf-8-sig）写入，避免 Windows 环境乱码。
"""

from __future__ import annotations

import argparse
import codecs
from datetime import datetime
from pathlib import Path
import sys


MARKER_START = "<!-- changelog:start -->"
MARKER_END = "<!-- changelog:end -->"

CATEGORY_ORDER = [
    "新增",
    "变更",
    "修复",
    "性能",
    "文档",
    "重构",
    "安全",
    "工程",
    "测试",
    "移除",
    "弃用",
]

CATEGORY_ALIASES = {
    "新增": "新增",
    "added": "新增",
    "add": "新增",
    "feature": "新增",
    "new": "新增",
    "变更": "变更",
    "changed": "变更",
    "change": "变更",
    "update": "变更",
    "调整": "变更",
    "修复": "修复",
    "fixed": "修复",
    "fix": "修复",
    "bugfix": "修复",
    "性能": "性能",
    "perf": "性能",
    "performance": "性能",
    "文档": "文档",
    "docs": "文档",
    "doc": "文档",
    "重构": "重构",
    "refactor": "重构",
    "安全": "安全",
    "security": "安全",
    "工程": "工程",
    "infra": "工程",
    "build": "工程",
    "tooling": "工程",
    "测试": "测试",
    "test": "测试",
    "tests": "测试",
    "移除": "移除",
    "remove": "移除",
    "removed": "移除",
    "弃用": "弃用",
    "deprecate": "弃用",
    "deprecated": "弃用",
}

CATEGORY_RANK = {name: idx for idx, name in enumerate(CATEGORY_ORDER)}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="将分类化迭代记录写入 docs/功能迭代.md。",
    )
    parser.add_argument(
        "content",
        nargs="+",
        help="迭代内容（不含日期与分类前缀）。",
    )
    parser.add_argument(
        "--type",
        default="变更",
        help="变更类型：新增/变更/修复/性能/文档/重构/安全/工程/测试/移除/弃用。",
    )
    parser.add_argument(
        "--scope",
        action="append",
        default=[],
        help="范围标签，可重复传入或用逗号分隔，如 backend,docs。",
    )
    parser.add_argument(
        "--date",
        default="",
        help="可选，指定日期，格式：YYYY-MM-DD（默认今天）。",
    )
    parser.add_argument(
        "--path",
        default="docs/功能迭代.md",
        help="功能迭代文档路径（相对仓库根目录或绝对路径）。",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="若已存在相同记录仍强制插入。",
    )
    parser.add_argument(
        "--no-bom",
        action="store_true",
        help="禁用 UTF-8 BOM 写入。",
    )
    return parser.parse_args()


def normalize_category(raw: str) -> str:
    key = raw.strip()
    if not key:
        return "变更"
    normalized = CATEGORY_ALIASES.get(key) or CATEGORY_ALIASES.get(key.lower())
    if normalized:
        return normalized
    raise ValueError(
        "未知分类：{value}。可选：{options}".format(
            value=raw,
            options="、".join(CATEGORY_ORDER),
        )
    )


def normalize_scopes(raw_scopes: list[str]) -> list[str]:
    scopes: list[str] = []
    for raw in raw_scopes:
        for part in raw.split(","):
            item = part.strip()
            if not item:
                continue
            if item.isascii():
                item = item.lower()
            scopes.append(item)

    seen: set[str] = set()
    deduped: list[str] = []
    for scope in scopes:
        if scope in seen:
            continue
        seen.add(scope)
        deduped.append(scope)
    return deduped


def format_date(date_text: str) -> str:
    if date_text:
        try:
            target = datetime.strptime(date_text, "%Y-%m-%d")
        except ValueError as exc:
            raise ValueError("日期格式错误，应为 YYYY-MM-DD。") from exc
    else:
        target = datetime.now()
    return f"{target.year}-{target.month:02d}-{target.day:02d}"


def format_date_cn(date_text: str) -> str:
    if date_text:
        try:
            target = datetime.strptime(date_text, "%Y-%m-%d")
        except ValueError as exc:
            raise ValueError("日期格式错误，应为 YYYY-MM-DD。") from exc
    else:
        target = datetime.now()
    return f"{target.year}年-{target.month:02d}月-{target.day:02d}日"


def resolve_target_path(path_text: str) -> Path:
    path = Path(path_text)
    if path.is_absolute():
        return path
    repo_root = Path(__file__).resolve().parents[1]
    return (repo_root / path).resolve()


def read_existing_content(path: Path) -> tuple[str, str]:
    if not path.exists():
        return "", "\r\n"

    data = path.read_bytes()
    text = data.decode("utf-8-sig")
    newline = "\r\n" if "\r\n" in text else "\n"
    return text, newline


def build_entry_line(content: str, scopes: list[str]) -> str:
    tags = "".join(f"[{scope}]" for scope in scopes)
    if tags:
        tags += " "
    return f"- {tags}{content}"


def build_minimal_changelog(
    date: str,
    category: str,
    entry_line: str,
    newline: str,
) -> str:
    lines = [
        "# 功能迭代",
        "",
        MARKER_START,
        f"## {date}",
        f"### {category}",
        entry_line,
        MARKER_END,
        "",
    ]
    return newline.join(lines)


def insert_legacy_entry(
    original_text: str,
    newline: str,
    date_text: str,
    content: str,
    force: bool,
) -> str:
    entry_line = f"{format_date_cn(date_text)}：{content}"
    if not original_text:
        return entry_line + newline

    first_line = original_text.splitlines()[0] if original_text else ""
    if first_line == entry_line and not force:
        return original_text

    return entry_line + newline + original_text


def insert_into_section(
    section_lines: list[str],
    date: str,
    category: str,
    entry_line: str,
    force: bool,
) -> list[str]:
    date_heading = f"## {date}"
    category_heading = f"### {category}"

    date_idx = None
    for idx, line in enumerate(section_lines):
        if line.strip() == date_heading:
            date_idx = idx
            break

    if date_idx is None:
        new_block = [date_heading, category_heading, entry_line]
        if section_lines and section_lines[0].strip():
            new_block.append("")
        return new_block + section_lines

    date_end = len(section_lines)
    for idx in range(date_idx + 1, len(section_lines)):
        if section_lines[idx].strip().startswith("## "):
            date_end = idx
            break

    category_idx = None
    for idx in range(date_idx + 1, date_end):
        if section_lines[idx].strip() == category_heading:
            category_idx = idx
            break

    if category_idx is not None:
        category_end = date_end
        for idx in range(category_idx + 1, date_end):
            line = section_lines[idx].strip()
            if line.startswith("### ") or line.startswith("## "):
                category_end = idx
                break
        if entry_line in section_lines[category_idx + 1 : category_end] and not force:
            return section_lines
        while category_idx + 1 < len(section_lines) and section_lines[
            category_idx + 1
        ].strip() == "":
            del section_lines[category_idx + 1]
        section_lines.insert(category_idx + 1, entry_line)
        return section_lines

    target_rank = CATEGORY_RANK.get(category, len(CATEGORY_ORDER))
    insert_idx = date_end
    for idx in range(date_idx + 1, date_end):
        line = section_lines[idx].strip()
        if not line.startswith("### "):
            continue
        existing = line[4:].strip()
        rank = CATEGORY_RANK.get(existing, len(CATEGORY_ORDER))
        if rank > target_rank:
            insert_idx = idx
            break

    section_lines[insert_idx:insert_idx] = [category_heading, entry_line]
    return section_lines


def insert_entry(
    original_text: str,
    newline: str,
    date_text: str,
    category: str,
    entry_line: str,
    force: bool,
) -> str:
    if not original_text:
        return build_minimal_changelog(date_text, category, entry_line, newline)

    lines = original_text.splitlines()
    if MARKER_START not in lines or MARKER_END not in lines:
        return insert_legacy_entry(
            original_text=original_text,
            newline=newline,
            date_text=date_text,
            content=entry_line.lstrip("- ").strip(),
            force=force,
        )

    start_idx = lines.index(MARKER_START)
    end_idx = lines.index(MARKER_END)
    if start_idx >= end_idx:
        raise ValueError("changelog 标记顺序错误。")

    section_lines = lines[start_idx + 1 : end_idx]
    updated_section = insert_into_section(
        section_lines=section_lines,
        date=date_text,
        category=category,
        entry_line=entry_line,
        force=force,
    )
    updated_lines = lines[: start_idx + 1] + updated_section + lines[end_idx:]

    new_text = newline.join(updated_lines)
    if original_text.endswith("\r\n") or original_text.endswith("\n"):
        new_text += newline
    return new_text


def main() -> int:
    args = parse_args()
    content = " ".join(args.content).strip()
    if not content:
        print("迭代内容不能为空。", file=sys.stderr)
        return 1

    try:
        date_text = format_date(args.date)
    except ValueError as exc:
        print(str(exc), file=sys.stderr)
        return 2

    try:
        category = normalize_category(args.type)
    except ValueError as exc:
        print(str(exc), file=sys.stderr)
        return 3

    scopes = normalize_scopes(args.scope)
    entry_line = build_entry_line(content, scopes)

    target_path = resolve_target_path(args.path)
    target_path.parent.mkdir(parents=True, exist_ok=True)

    original_text, newline = read_existing_content(target_path)
    new_text = insert_entry(
        original_text=original_text,
        newline=newline,
        date_text=date_text,
        category=category,
        entry_line=entry_line,
        force=args.force,
    )

    if new_text == original_text:
        print("记录已存在，未重复写入。")
        return 0

    use_bom = not args.no_bom
    encoded = (codecs.BOM_UTF8 if use_bom else b"") + new_text.encode("utf-8")
    target_path.write_bytes(encoded)
    print(f"已写入：{target_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
