#!/usr/bin/env python3
"""One-off migration (already run): assets/help/manual.{zh,en}.json -> manual.{zh,en}.md.

The JSON sources were deleted after this migration. To re-run it, restore the
JSON files from git history first.

The help browser now reads Markdown directly and projects it through the same
`BlockView` the chat transcript uses. This script only existed to carry the old
structured JSON content over; from here on the .md files are the source of
truth and are edited by hand.

Usage (from crates/wunder-desktop/frontend/):
    python scripts/convert_help_manual.py
"""
from __future__ import annotations

import html
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HELP = ROOT / "assets" / "help"

# Characters that would otherwise open Markdown structure inside a literal
# span. Escaping with a backslash is enough for pulldown-cmark.
_SPECIAL = re.compile(r"([\\`*_\[\]<>])")


def escape(text: str) -> str:
    text = _SPECIAL.sub(r"\\\1", text)
    # A line-leading marker could still turn the paragraph into a heading/list.
    return text.replace("\n", " ")


def decorate(text: str, bold: bool, code: bool, kbd: bool, href: str) -> str:
    # Keep padding outside the delimiter: `** text **` is not strong emphasis.
    leading = text[: len(text) - len(text.lstrip())]
    trailing = text[len(text.rstrip()) :]
    core = text.strip()
    if not core:
        return text
    if code or kbd:
        # A code span uses a fence longer than any backtick run inside.
        longest = max((len(run) for run in re.findall(r"`+", core)), default=0)
        fence = "`" * (longest + 1)
        core = f"{fence} {core} {fence}"
    elif bold:
        core = f"**{core}**"
    if href:
        core = f"[{core}](topic:{href})"
    return leading + core + trailing


def spans_md(spans: list[dict]) -> str:
    out: list[str] = []
    for span in spans:
        text = escape(span.get("text", "") or "")
        out.append(
            decorate(
                text,
                bool(span.get("bold")),
                bool(span.get("code")),
                bool(span.get("kbd")),
                (span.get("href") or "").strip(),
            )
        )
    return "".join(out).strip()


def code_md(rows: list[list[dict]]) -> str:
    lines = []
    for row in rows:
        lines.append("".join(span.get("text", "") or "" for span in row))
    return "```\n" + "\n".join(lines) + "\n```"


def table_md(rows: list[dict]) -> str:
    if not rows:
        return ""
    lines: list[str] = []
    width = max(len(row["cells"]) for row in rows)
    header_at = next((i for i, row in enumerate(rows) if row.get("header")), None)

    def cells(row: dict) -> list[str]:
        values = [spans_md(cell) for cell in row["cells"]]
        return (values + [""] * width)[:width]

    for index, row in enumerate(rows):
        if index == header_at:
            continue
        lines.append("| " + " | ".join(c.replace("|", "\\|") for c in cells(row)) + " |")
        if index == 0 and header_at != 0:
            lines.append("| " + " | ".join("---" for _ in range(width)) + " |")
    if header_at is not None:
        head = ["| " + " | ".join(c.replace("|", "\\|") for c in cells(rows[header_at])) + " |",
                "| " + " | ".join("---" for _ in range(width)) + " |"]
        # The header belongs at the top of the table, matching CommonMark.
        lines = head + lines
    return "\n".join(lines)


def block_md(block: dict) -> str:
    kind = block["kind"]
    if kind == "h1":
        return "# " + spans_md(block["spans"])
    if kind == "h2":
        return "## " + spans_md(block["spans"])
    if kind in ("paragraph", "lead"):
        return spans_md(block["spans"])
    if kind == "list":
        marker = block.get("marker", "•")
        ordered = re.match(r"^(\d+)[.)]$", marker.strip())
        prefix = f"{ordered.group(1)}. " if ordered else "- "
        return prefix + spans_md(block["spans"])
    if kind in ("tip", "callout", "task"):
        # The three coloured callouts collapse into a block quote: BlockView
        # has one quote treatment, and these blocks all read as asides.
        return "> " + spans_md(block["spans"])
    if kind == "table":
        return table_md(block["rows"])
    if kind == "code":
        return code_md(block["rows"])
    if kind == "image":
        caption = (block.get("caption") or block.get("alt") or "").strip()
        return (
            f"![{escape(block.get('alt', '') or '')}](art/{block['src']}.png)\n\n"
            f"*{escape(caption)}*"
        )
    if kind == "cards":
        lines = []
        for card in block["cards"]:
            title = spans_md(card.get("title", []))
            summary = spans_md(card.get("summary", []))
            lines.append(f"- [**{title}**](topic:{card['target']})：{summary}")
        return "\n".join(lines)
    raise ValueError(f"unknown block kind: {kind}")


def convert(lang: str) -> None:
    data = json.loads((HELP / f"manual.{lang}.json").read_text(encoding="utf-8"))
    parts = [
        f"<!-- help-manual: {lang} -->",
        "<!-- 正文是标准 Markdown；topic: 链接指向站内主题，art/*.png 指向内嵌图册。 -->",
    ]
    for topic in data["topics"]:
        meta = (
            f'<!-- topic id="{html.escape(topic["id"], quote=True)}" '
            f'title="{html.escape(topic["title"], quote=True)}" '
            f'group="{html.escape(topic["group"], quote=True)}" '
            f'keywords="{html.escape(",".join(topic["keywords"]), quote=True)}" -->'
        )
        body = "\n\n".join(block_md(block) for block in topic["blocks"] if block_md(block))
        parts.append("\n".join([meta, "", body, ""]))
    output = HELP / f"manual.{lang}.md"
    output.write_text("\n".join(parts).rstrip() + "\n", encoding="utf-8")
    print(f"{output.name}: {len(data['topics'])} topics, {output.stat().st_size} bytes")


if __name__ == "__main__":
    for language in ("zh", "en"):
        convert(language)
