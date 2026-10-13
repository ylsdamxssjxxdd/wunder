"""从文档站 Markdown 生成蜂窝内嵌帮助手册（中英两份）。

文档站页面是单一事实来源：本脚本把指定页面解析成手册的 Block 模型，
再与蜂窝专属主题（manual.local.{zh,en}.json）合并，按分组顺序输出
`assets/help/manual.zh.json` 与 `assets/help/manual.en.json`。

用法：python scripts/build_help_manual.py
"""

import io
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOCS = os.path.join(ROOT, "docs", "使用说明书")
OUT_DIR = os.path.join(ROOT, "frontend-slint", "assets", "help")

# 文档站来源主题：(id, 分组中, 分组英, 相对 zh-CN/ 与 en/ 的路径, 关键词中, 关键词英)
DOCS_TOPICS = [
    ("forms", "开始", "Getting Started", "start/forms.md",
     ["形态", "蜂窝", "蜂巢", "舵机", "舰体"],
     ["form", "Honeycomb", "Beehive", "Helm", "Hull"]),
    ("install", "开始", "Getting Started", "start/desktop.md",
     ["安装", "启动", "下载", "桌面端"],
     ["install", "launch", "download", "desktop"]),
    ("core-goal", "核心", "Core", "concepts/core-goal.md",
     ["目标", "目标态", "相位", "轮上限"],
     ["goal", "goal state", "phase", "round cap"]),
    ("core-compatibility", "核心", "Core", "concepts/core-compatibility.md",
     ["兼容性", "Win7", "Ubuntu", "离线"],
     ["compatibility", "Windows 7", "Ubuntu", "offline"]),
    ("core-context-compression", "核心", "Core", "concepts/core-context-compression.md",
     ["上下文压缩", "压缩", "上下文"],
     ["context compaction", "compaction", "context"]),
    ("core-subagent", "核心", "Core", "concepts/core-subagent.md",
     ["子智能体", "派生", "协作"],
     ["subagent", "spawn", "collaboration"]),
    ("core-scheduled-tasks", "核心", "Core", "concepts/core-scheduled-tasks.md",
     ["定时任务", "计划", "cron"],
     ["scheduled task", "schedule", "cron"]),
    ("core-channels", "核心", "Core", "concepts/core-channels.md",
     ["渠道", "飞书", "QQ", "微信", "IM"],
     ["channel", "Feishu", "QQ", "WeChat", "IM"]),
    ("core-memory", "核心", "Core", "concepts/core-memory.md",
     ["记忆", "长期记忆", "知识库"],
     ["memory", "long-term memory", "knowledge base"]),
    ("core-multithreading", "核心", "Core", "concepts/core-multithreading.md",
     ["多线程", "并发", "线程", "队列"],
     ["multi-threading", "concurrency", "thread", "queue"]),
    ("core-multi-user-management", "核心", "Core", "concepts/core-multi-user-management.md",
     ["多租户", "账号", "配额", "权限"],
     ["multi-tenancy", "account", "quota", "permission"]),
    ("tools-overview", "工具", "Tools", "tools/index.md",
     ["工具", "工具矩阵", "MCP", "别名"],
     ["tools", "tool matrix", "MCP", "alias"]),
    ("desktop-ui", "界面", "Surfaces", "surfaces/desktop-ui.md",
     ["界面", "桌面端", "侧栏", "布局"],
     ["interface", "desktop", "sidebar", "layout"]),
    ("interlink", "参考", "Reference", "reference/interlink.md",
     ["三端互通", "设备互通", "隧道", "远程"],
     ["interlink", "devices", "tunnel", "remote"]),
]

# 最终顺序：先按分组，组内按下表。local 主题来自 manual.local.{lang}.json。
ORDER = [
    "welcome", "quickstart", "forms", "install",
    "core-goal", "core-compatibility", "core-context-compression", "core-subagent",
    "core-scheduled-tasks", "core-channels", "core-memory", "core-multithreading",
    "core-multi-user-management",
    "tools-overview", "tools",
    "interface", "desktop-ui",
    "interlink", "settings", "models", "threads", "workspace", "multimodal",
    "privacy", "troubleshooting", "shortcuts", "glossary",
]

IMG_RE = re.compile(r"!\[[^\]]*\]\([^)]*\)")
TAG_RE = re.compile(r"<[^>]+>")
INLINE_RE = re.compile(r"(\*\*.+?\*\*|`.+?`|\[[^\]]+\]\([^)]+\))")
LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
CARD_RE = re.compile(
    r'<a class="docs-card"\s+href="([^"]+)"[^>]*>\s*<strong>(.*?)</strong>\s*<span>(.*?)</span>',
    re.S,
)


def span(text, bold=False, code=False, kbd=False, href=""):
    return {"text": text, "bold": bold, "code": code, "kbd": kbd, "href": href}


def parse_spans(text, links):
    """把行内标记拆成 span 列表。链接目标能映射到手册主题才保留 href。"""
    text = IMG_RE.sub("", text)
    text = TAG_RE.sub("", text)
    spans = []
    pos = 0
    for m in INLINE_RE.finditer(text):
        if m.start() > pos:
            spans.append(span(text[pos:m.start()]))
        tok = m.group(0)
        if tok.startswith("**"):
            spans.append(span(tok[2:-2], bold=True))
        elif tok.startswith("`"):
            spans.append(span(tok[1:-1], code=True))
        else:
            lm = LINK_RE.match(tok)
            if lm:
                spans.append(span(lm.group(1), href=links.get(lm.group(2), "")))
        pos = m.end()
    if pos < len(text):
        spans.append(span(text[pos:]))
    return [s for s in spans if s["text"] or s["href"]]


def split_row(line):
    return [c.strip() for c in line.strip().strip("|").split("|")]


def parse_table(raw_rows, links):
    if len(raw_rows) < 2:
        return None
    out = [{"header": True, "cells": [parse_spans(c, links) for c in split_row(raw_rows[0])]}]
    for r in raw_rows[2:]:
        cells = [parse_spans(c, links) for c in split_row(r)]
        out.append({"header": False, "cells": cells})
    return {"kind": "table", "rows": out}


def parse_cards(chunk, links):
    cards = []
    for m in CARD_RE.finditer(chunk):
        target = links.get(m.group(1), "")
        if not target:
            continue
        cards.append({
            "target": target,
            "title": parse_spans(m.group(2), links),
            "summary": parse_spans(m.group(3), links),
        })
    if not cards:
        return None
    return {"kind": "cards", "cards": cards}


BULLET_RE = re.compile(r"^(\d+\.|[-*])\s+")


def parse_markdown(text, links):
    lines = text.split("\n")
    n = len(lines)
    i = 0
    # 跳过 YAML front matter
    if lines and lines[0].strip() == "---":
        for j in range(1, n):
            if lines[j].strip() == "---":
                i = j + 1
                break
    title = None
    blocks = []
    lead_done = False
    while i < n:
        raw = lines[i]
        s = raw.strip()
        if not s or s == "---":
            i += 1
            continue
        if s.startswith("```"):
            i += 1
            code = []
            while i < n and not lines[i].strip().startswith("```"):
                code.append(lines[i].rstrip())
                i += 1
            i += 1
            rows = [[span(line, code=True)] for line in code if line.strip()]
            if rows:
                blocks.append({"kind": "code", "rows": rows})
            continue
        if s.startswith("<div") or s.startswith("<a "):
            chunk = []
            while i < n and not lines[i].strip().startswith("</div>"):
                chunk.append(lines[i])
                i += 1
            i += 1
            cards = parse_cards("\n".join(chunk), links)
            if cards:
                blocks.append(cards)
            continue
        if s.startswith("# "):
            if title is None:
                title = s[2:].strip()
            i += 1
            continue
        if s.startswith("##"):
            blocks.append({"kind": "h2", "spans": parse_spans(s.lstrip("#").strip(), links)})
            i += 1
            continue
        if s.startswith(">"):
            blocks.append({"kind": "callout", "spans": parse_spans(s.lstrip(">").strip(), links)})
            i += 1
            continue
        if s.startswith("|"):
            raw_rows = []
            while i < n and lines[i].strip().startswith("|"):
                raw_rows.append(lines[i])
                i += 1
            table = parse_table(raw_rows, links)
            if table:
                blocks.append(table)
            continue
        if BULLET_RE.match(s):
            items = []
            while i < n and BULLET_RE.match(lines[i].strip()):
                items.append(BULLET_RE.sub("", lines[i].strip()))
                i += 1
            for item in items:
                blocks.append({"kind": "list", "marker": "•", "spans": parse_spans(item, links)})
            continue
        para = []
        while i < n and lines[i].strip() and not lines[i].strip().startswith(
            ("#", "```", "|", ">", "- ", "* ", "<div", "<a ")
        ) and not BULLET_RE.match(lines[i].strip()):
            para.append(lines[i].strip())
            i += 1
        if para:
            kind = "lead" if not lead_done else "paragraph"
            lead_done = True
            blocks.append({"kind": kind, "spans": parse_spans(" ".join(para), links)})
    return title, blocks


def build_links():
    """文档站内链 URL -> 手册主题 id（中英两种前缀都映射）。"""
    links = {}
    for tid, _gz, _ge, src, _kz, _ke in DOCS_TOPICS:
        stem = src[:-3] if src.endswith(".md") else src
        links["/docs/zh-CN/%s/" % stem] = tid
        links["/docs/en/%s/" % stem] = tid
        links["/docs/zh-CN/%s" % stem] = tid
        links["/docs/en/%s" % stem] = tid
    return links


def read(path):
    with io.open(path, encoding="utf-8") as f:
        return f.read()


def main():
    links = build_links()
    topics = {"zh": {}, "en": {}}

    for tid, gz, ge, src, kz, ke in DOCS_TOPICS:
        for lang, group, kws, base in (("zh", gz, kz, "zh-CN"), ("en", ge, ke, "en")):
            path = os.path.join(DOCS, base, src)
            if not os.path.exists(path):
                print("[缺失] %s" % path)
                sys.exit(1)
            title, blocks = parse_markdown(read(path), links)
            head = read(path).split("\n")
            fm_title = None
            if head and head[0].strip() == "---":
                for line in head[1:]:
                    if line.strip() == "---":
                        break
                    if line.startswith("title:"):
                        fm_title = line.split(":", 1)[1].strip().strip('"')
            name = fm_title or title or tid
            # 与蜂窝专属主题保持一致：正文开头带一个 H1 标题块。
            blocks.insert(0, {"kind": "h1", "spans": [span(name)]})
            topics[lang][tid] = {
                "id": tid,
                "title": name,
                "group": group,
                "keywords": list(kws),
                "blocks": blocks,
            }

    for lang in ("zh", "en"):
        local_path = os.path.join(OUT_DIR, "manual.local.%s.json" % lang)
        if not os.path.exists(local_path):
            print("[缺失] %s" % local_path)
            sys.exit(1)
        for topic in json.loads(read(local_path))["topics"]:
            topics[lang][topic["id"]] = topic

    for lang in ("zh", "en"):
        ordered = []
        for tid in ORDER:
            if tid not in topics[lang]:
                print("[缺失主题] %s (%s)" % (tid, lang))
                sys.exit(1)
            ordered.append(topics[lang][tid])
        # 索引页：关键词 -> 主题下标，同名词保留首次出现
        index = []
        seen = set()
        for i, topic in enumerate(ordered):
            for kw in topic.get("keywords", []):
                if kw in seen:
                    continue
                seen.add(kw)
                index.append({"keyword": kw, "topic": i})
        out = {"topics": ordered, "index": index}
        path = os.path.join(OUT_DIR, "manual.%s.json" % lang)
        with io.open(path, "w", encoding="utf-8") as f:
            json.dump(out, f, ensure_ascii=False, indent=2)
            f.write("\n")
        print("%s: %d 个主题 / %d 条索引 -> %s"
              % (lang, len(ordered), len(index), os.path.relpath(path, ROOT)))


if __name__ == "__main__":
    main()
