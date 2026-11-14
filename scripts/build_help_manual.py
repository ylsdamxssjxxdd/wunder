#!/usr/bin/env python3
"""从使用说明书生成桌面版内嵌帮助手册。

说明书（docs/使用说明书）是唯一内容源：本脚本按 site.json 的分区、分组与页面顺序
生成 crates/wunder-desktop/frontend/assets/help/manual.zh.md 与 manual.en.md，并把页面里
引用的截图复制到 assets/help/art/。

用法：
    python3 scripts/build_help_manual.py           # 生成
    python3 scripts/build_help_manual.py --check   # 只校验是否与说明书一致
"""

from __future__ import annotations

import json
import re
import shutil
import sys
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
MANUAL_DIR = REPO_ROOT / "docs" / "使用说明书"
SITE_CONFIG = MANUAL_DIR / "site.json"
IMAGES_DIR = REPO_ROOT / "docs" / "images"
HELP_DIR = REPO_ROOT / "crates" / "wunder-desktop" / "frontend" / "assets" / "help"
ART_DIR = HELP_DIR / "art"
ANDROID_HELP_DIR = REPO_ROOT / "crates" / "wunder-android" / "assets" / "help"

# 说明书页签 -> 帮助手册分区键。
BLOCK_KEYS = {
    "形态": "forms",
    "Forms": "forms",
    "工具": "tools",
    "Tools": "tools",
    "能力": "capabilities",
    "Capabilities": "capabilities",
}
LANGUAGES = (("zh-CN", "manual.zh.md"), ("en", "manual.en.md"))

FRONTMATTER = re.compile(r"\A---\n(.*?)\n---\n", re.S)
H1 = re.compile(r"\A\s*#\s+[^\n]*\n+")
IMAGE_TAG = re.compile(r'<img\s+[^>]*src="([^"]+)"[^>]*>')
ALT_ATTR = re.compile(r'alt="([^"]*)"')
FIGCAPTION = re.compile(r"<figcaption>(.*?)</figcaption>", re.S)
STRUCTURE_TAG = re.compile(r"^\s*</?(?:div|figure|span|p)(?:\s[^>]*)?>\s*$")
LINK = re.compile(r"\]\((/docs/[^)\s]+)\)")
TAG = re.compile(r"<[^>]+>")
# 手机端不嵌图册：生成时把图片行去掉，图注保留。
IMAGE_LINE = re.compile(r"^!\[[^\]]*\]\(art/[^)]*\)[ \t]*\n?", re.M)
BLANK_RUN = re.compile("\n" + chr(123) + "3," + chr(125))


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def parse_frontmatter(text: str) -> tuple[dict[str, str], str]:
    match = FRONTMATTER.match(text)
    if not match:
        return {}, text
    meta: dict[str, str] = {}
    for line in match.group(1).splitlines():
        if ":" not in line:
            continue
        key, _, value = line.partition(":")
        meta[key.strip()] = value.strip()
    return meta, text[match.end():]


def topic_id(slug: str) -> str:
    return slug.replace("/", "-")


def escape_attr(value: str) -> str:
    return (
        value.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


class Converter:
    """说明书 Markdown -> 帮助手册 Markdown。"""

    def __init__(self, language: str, slugs: dict[str, str], art: set[str]) -> None:
        self.language = language
        self.slugs = slugs
        self.art = art

    def target(self, href: str) -> str | None:
        """站内链接换算成帮助主题 id；站外与跨语言链接返回 None。"""
        path = href.split("#", 1)[0]
        rel = path[len("/docs/"):].strip("/")
        if not rel:
            return None
        parts = rel.split("/")
        if parts[0] != self.language:
            return None
        if rel in self.slugs:
            return self.slugs[rel]
        indexed = rel + "/index"
        if indexed in self.slugs:
            return self.slugs[indexed]
        return None

    def body(self, text: str) -> str:
        _, body = parse_frontmatter(text)
        # 主题名由工具条显示，正文不再重复一遍一级标题。
        body = H1.sub("", body).lstrip("\n")
        lines: list[str] = []
        for raw in body.splitlines():
            line = raw.rstrip()
            if STRUCTURE_TAG.match(line):
                continue
            line = self.figure(line)
            line = self.links(line)
            if "<" in line:
                line = TAG.sub("", line).strip()
            lines.append(line)
        joined = BLANK_RUN.sub("\n\n", "\n".join(lines))
        return joined.strip()

    def figure(self, line: str) -> str:
        """截图与图注换成帮助手册能渲染的 Markdown。"""
        out: list[str] = []
        for src in IMAGE_TAG.findall(line):
            name = Path(src).name
            if not src.startswith("/docs/images/"):
                continue
            self.art.add(name)
            alt = "截图"
            match = ALT_ATTR.search(line)
            if match:
                alt = match.group(1)
            out.append("![" + alt + "](art/" + name + ")")
        caption = FIGCAPTION.search(line)
        if caption:
            text = re.sub(r"\s+", " ", TAG.sub("", caption.group(1))).strip()
            if text:
                out.append("*" + text + "*")
        if out:
            return "\n".join(out)
        return IMAGE_TAG.sub("", line)

    def links(self, line: str) -> str:
        def repl(match: re.Match[str]) -> str:
            ident = self.target(match.group(1))
            if ident:
                return "](topic:" + ident + ")"
            # 没有对应主题时只留链接文字。
            return "]"

        return LINK.sub(repl, line)


def headings(body: str, limit: int = 2) -> list[str]:
    found = [m.group(2).strip() for m in re.finditer(r"^(#{2,3})\s+(.+)$", body, re.M)]
    return found[:limit]


def build_manual(language: str, pages: list[dict[str, str]], art: set[str]) -> str:
    slugs = {page["slug"]: topic_id(page["slug"][len(language) + 1:]) for page in pages}
    converter = Converter(language, slugs, art)
    chunks: list[str] = [
        "<!-- help-manual: " + language + " -->",
        "<!-- 由 scripts/build_help_manual.py 从 docs/使用说明书 生成，请勿手工修改。 -->",
    ]
    for page in pages:
        source = MANUAL_DIR / (page["slug"] + ".md")
        text = source.read_text(encoding="utf-8")
        meta, _ = parse_frontmatter(text)
        full_title = meta.get("title") or page["slug"]
        # 目录里显示短名：优先用页面的 nav_title；缺省时去掉与分组重名的前缀。
        title = meta.get("nav_title") or ""
        if not title:
            title = full_title
            group = page["group"]
            for prefix in (group, group + " "):
                if title != prefix and title.startswith(prefix):
                    title = title[len(prefix):].strip()
                    break
        body = converter.body(text)
        keywords = [full_title] + [h for h in headings(body) if h != title]
        header = (
            '<!-- topic id="' + slugs[page["slug"]] + '"'
            ' block="' + page["block"] + '"'
            ' title="' + escape_attr(title) + '"'
            ' group="' + escape_attr(page["group"]) + '"'
            ' keywords="' + escape_attr(",".join(keywords)) + '" -->'
        )
        chunks.append(header)
        chunks.append(body)
    return "\n\n".join(chunks) + "\n"


def collect_pages(config: dict[str, Any], language: str) -> list[dict[str, str]]:
    pages: list[dict[str, str]] = []
    for entry in config["navigation"]["languages"]:
        if entry["language"] != language:
            continue
        for tab in entry["tabs"]:
            block = BLOCK_KEYS.get(tab["tab"])
            if block is None:
                raise SystemExit("未知页签: " + str(tab["tab"]))
            for group in tab["groups"]:
                for slug in group["pages"]:
                    pages.append({"slug": slug, "block": block, "group": group["group"]})
    return pages


def main() -> int:
    check = "--check" in sys.argv
    config = read_json(SITE_CONFIG)
    art_needed: set[str] = set()
    outputs: dict[Path, str] = {}
    for language, filename in LANGUAGES:
        pages = collect_pages(config, language)
        text = build_manual(language, pages, art_needed)
        outputs[HELP_DIR / filename] = text
        outputs[ANDROID_HELP_DIR / filename] = IMAGE_LINE.sub("", text)

    stale = [
        str(path.relative_to(REPO_ROOT))
        for path, content in outputs.items()
        if (path.read_text(encoding="utf-8") if path.exists() else "") != content
    ]
    missing_art = sorted(
        name
        for name in art_needed
        if not (ART_DIR / name).exists()
        or (ART_DIR / name).stat().st_mtime < (IMAGES_DIR / name).stat().st_mtime
    )

    if check:
        if stale or missing_art:
            print("帮助手册与说明书不一致: " + ", ".join(stale + missing_art))
            return 1
        print("帮助手册已与说明书同步")
        return 0

    ART_DIR.mkdir(parents=True, exist_ok=True)
    for name in sorted(art_needed):
        source = IMAGES_DIR / name
        if source.is_file():
            shutil.copy2(source, ART_DIR / name)
    kept = {p.name for p in ART_DIR.glob("*.png")} - art_needed
    for name in kept:
        (ART_DIR / name).unlink()
    for path, content in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    total = sum(content.count("<!-- topic ") for content in outputs.values())
    print(
        "帮助手册已生成: "
        + str(len(outputs))
        + " 个文件, "
        + str(total)
        + " 个主题, "
        + str(len(art_needed))
        + " 张配图"
    )
    if kept:
        print("已移除不再引用的配图: " + ", ".join(sorted(kept)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
