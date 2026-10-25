# AI生成
"""从文档站 Markdown 生成蛹内嵌帮助手册（中英两份）。

文档站页面是单一事实来源：本脚本把指定页面解析成手册的 Block 模型，
再与蛹专属主题（manual.local.{zh,en}.json）合并，按分组顺序输出
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
ART_ASSET_DIR = os.path.join(OUT_DIR, "art")

# 美术图册：每张设计稿一个主题，只带一张预览图。图太大不拼在同一页，
# 否则排版期要同时解码十张大图，内存和滚动都会失控。
ART_GROUP = {"zh": "美术", "en": "Art"}
ART_TOPICS = [
    {
        "id": "art-overview",
        "slug": "00-overview",
        "zh": {
            "title": "00 整体总览",
            "keywords": ["美术", "设计稿", "整体总览", "窗口布局"],
            "lead": "蛹桌面端的整窗基线：1462×866 逻辑像素，侧栏 240、阅读列 1000、输入区固定 154 高。"
                    "侧栏、时间线、输入区放在同一张图里，方便对照布局。",
            "caption": "00 整体总览 · 窗口 1462×866（参考截图 2558×1515，175% DPI）",
            "tip": "设计稿源文件在 docs/wunder美术设计稿/蛹。改完 SVG 后运行 python scripts/render_art_previews.py，"
                   "再运行 python scripts/build_help_manual.py 更新手册里的预览图。",
        },
        "en": {
            "title": "00 Overview",
            "keywords": ["art", "design sheet", "overview", "window layout"],
            "lead": "Whole-window baseline for the Pupa desktop app: 1462×866 logical pixels, a 240 sidebar, "
                    "a 1000 reading column and a 154-high composer band.",
            "caption": "00 Overview · window 1462×866 (reference screenshot 2558×1515 at 175% DPI)",
            "tip": "The sources live in docs/wunder美术设计稿/蛹. After editing an SVG run "
                   "python scripts/render_art_previews.py, then python scripts/build_help_manual.py to refresh the manual.",
        },
    },
    {
        "id": "art-foundations",
        "slug": "01-foundations",
        "zh": {
            "title": "01 基础规范",
            "keywords": ["美术", "基础规范", "颜色", "字阶", "间距", "圆角"],
            "lead": "中性色、强调色、语义色、字阶、间距尺度、圆角和窗口布局基线；"
                    "颜色与尺寸 token 与 design_tokens.slint 一一对应。",
            "caption": "01 基础规范 · 色彩、文字、间距、圆角",
        },
        "en": {
            "title": "01 Foundations",
            "keywords": ["art", "foundations", "color", "type scale", "spacing", "radii"],
            "lead": "Neutral, accent and semantic colors, the type scale, spacing, radii and the window layout "
                    "baseline; the tokens map to design_tokens.slint.",
            "caption": "01 Foundations · color, type, spacing, radii",
        },
    },
    {
        "id": "art-icons",
        "slug": "02-icons",
        "zh": {
            "title": "02 图标系统",
            "keywords": ["美术", "图标", "蜂标", "蛹图标", "线性图标"],
            "lead": "品牌图标、工具图标、状态图标和线性图标的重绘规则。应用图标是蛹；"
                    "蜂标只保留为渠道与协作的辅助标识。",
            "caption": "02 图标系统 · 品牌、工具、状态与线性图标",
        },
        "en": {
            "title": "02 Icons",
            "keywords": ["art", "icons", "bee mark", "pupa icon", "line icons"],
            "lead": "Rules for the brand mark, tool icons, state icons and line icons. The app icon is the pupa; "
                    "the bee mark stays only as an auxiliary channel and collaboration identifier.",
            "caption": "02 Icons · brand, tools, states and line icons",
        },
    },
    {
        "id": "art-shell",
        "slug": "03-shell-sidebar",
        "zh": {
            "title": "03 外壳与侧栏",
            "keywords": ["美术", "外壳", "侧栏", "标题栏", "分隔柄"],
            "lead": "标题栏、侧栏宽度、分组、工作区行、线程行、工作目录工具条，以及分隔柄在静止、悬停、"
                    "拖拽三态下的尺寸。侧栏分隔柄静止时不画条，悬停显示 2×32 胶囊。",
            "caption": "03 外壳与侧栏 · 标题栏、侧栏、分隔柄",
        },
        "en": {
            "title": "03 Shell and Sidebar",
            "keywords": ["art", "shell", "sidebar", "title bar", "splitter"],
            "lead": "Title bar, sidebar width, groups, workspace rows, thread rows, the working-directory toolbar, "
                    "and the splitter at rest, hover and drag. The sidebar splitter shows no rail at rest and a 2×32 pill on hover.",
            "caption": "03 Shell and sidebar · title bar, sidebar, splitter",
        },
    },
    {
        "id": "art-timeline",
        "slug": "04-timeline",
        "zh": {
            "title": "04 会话时间线",
            "keywords": ["美术", "时间线", "消息", "回合页脚", "状态胶囊"],
            "lead": "阅读列与滚动、用户消息、回合页脚、运行状态胶囊、Markdown 降级样式和空态。"
                    "流式输出只更新活动尾块，历史块保持稳定。",
            "caption": "04 会话时间线 · 消息、页脚、状态胶囊",
        },
        "en": {
            "title": "04 Timeline",
            "keywords": ["art", "timeline", "message", "turn footer", "state capsule"],
            "lead": "Reading column and scrolling, user messages, turn footers, the run-state capsule, Markdown "
                    "fallback styles and the empty state. Streaming updates only the active tail block.",
            "caption": "04 Timeline · messages, footers, state capsule",
        },
    },
    {
        "id": "art-process",
        "slug": "05-process-tools",
        "zh": {
            "title": "05 过程与工具",
            "keywords": ["美术", "过程", "工具", "思考", "子智能体"],
            "lead": "工作过程条、思考条目、工具行、展开后的输出面板、工具组和子智能体卡；"
                    "各状态的图标、折叠预算与缩进关系都在图里。",
            "caption": "05 过程与工具 · 过程条、工具行、子智能体",
        },
        "en": {
            "title": "05 Process and Tools",
            "keywords": ["art", "process", "tools", "thinking", "subagent"],
            "lead": "Process bar, thinking entries, tool rows, the expanded output panel, tool groups and subagent "
                    "cards, including icon sizes, fold budgets and indentation.",
            "caption": "05 Process and tools · process bar, tool rows, subagents",
        },
    },
    {
        "id": "art-plan",
        "slug": "06-plan-inquiry",
        "zh": {
            "title": "06 计划与问询",
            "keywords": ["美术", "计划", "问询", "目标", "无偏好"],
            "lead": "计划胶囊、展开卡片、目标横幅和问询面板。选项行包含「无偏好」与「其他」，"
                    "问询底部不设独立提交键。",
            "caption": "06 计划与问询 · 计划卡、目标横幅、问询面板",
        },
        "en": {
            "title": "06 Plan and Inquiry",
            "keywords": ["art", "plan", "inquiry", "goal", "no preference"],
            "lead": "Plan capsule, expanded card, goal banner and inquiry panel. Option rows include no preference "
                    "and other, and the inquiry panel has no separate submit button.",
            "caption": "06 Plan and inquiry · plan card, goal banner, inquiry panel",
        },
    },
    {
        "id": "art-composer",
        "slug": "07-composer",
        "zh": {
            "title": "07 输入区",
            "keywords": ["美术", "输入区", "发送", "停止", "队列"],
            "lead": "输入卡 972×100、底栏 30 高、附件行、录音中、生成中与发送停用等状态，"
                    "以及队列和模型菜单的规格。停止方块 10.5，发送箭头 16。",
            "caption": "07 输入区 · 输入卡、底栏、停止与队列",
        },
        "en": {
            "title": "07 Composer",
            "keywords": ["art", "composer", "send", "stop", "queue"],
            "lead": "The 972×100 input card, the 30-high footer, attachment rows, recording, generating and "
                    "disabled-send states, plus queue and model menu specs. Stop square 10.5, send arrow 16.",
            "caption": "07 Composer · input card, footer, stop and queue",
        },
    },
    {
        "id": "art-settings",
        "slug": "08-settings-dialogs",
        "zh": {
            "title": "08 设置与弹窗",
            "keywords": ["美术", "设置", "弹窗", "控件", "校验"],
            "lead": "设置行解剖、输入与选择控件、按钮、校验与提示，以及弹窗的密度和层级关系。",
            "caption": "08 设置与弹窗 · 设置行、控件、校验",
        },
        "en": {
            "title": "08 Settings and Dialogs",
            "keywords": ["art", "settings", "dialogs", "controls", "validation"],
            "lead": "Settings row anatomy, input and select controls, buttons, validation and hints, and the "
                    "density and layering of dialogs.",
            "caption": "08 Settings and dialogs · rows, controls, validation",
        },
    },
    {
        "id": "art-motion",
        "slug": "09-states-motion",
        "zh": {
            "title": "09 交互状态与动效",
            "keywords": ["美术", "交互", "状态", "动效", "键位", "断点"],
            "lead": "行与输入状态、动效时间表、性能红线、键位、无障碍反馈和断点；"
                    "长列表与流式输出期间哪些操作不能阻塞，也写在这一张。",
            "caption": "09 交互状态与动效 · 状态、时间表、键位",
        },
        "en": {
            "title": "09 States and Motion",
            "keywords": ["art", "interaction", "states", "motion", "keys", "breakpoints"],
            "lead": "Row and input states, the motion timeline, performance limits, key bindings, accessibility "
                    "feedback and breakpoints, including which actions must not block during streaming.",
            "caption": "09 States and motion · states, timing, keys",
        },
    },
]

# 文档站来源主题：(id, 分组中, 分组英, 相对 zh-CN/ 与 en/ 的路径, 关键词中, 关键词英)
DOCS_TOPICS = [
    ("forms", "开始", "Getting Started", "start/forms.md",
     ["形态", "蛹", "蜂巢", "舵机", "舰体"],
     ["form", "Pupa", "Beehive", "Helm", "Hull"]),
    ("install", "开始", "Getting Started", "start/desktop.md",
     ["安装", "启动", "下载", "桌面端"],
     ["install", "launch", "download", "desktop"]),
    ("core-overview", "核心", "Core", "concepts/index.md",
     ["多快好省", "多平台", "多形态", "启动速度", "响应速度", "稳定性", "智能程度",
      "CPU 占用", "内存占用", "上下文消耗"],
     ["Diverse", "Fast", "Superior", "Efficient", "multi-platform", "multi-form",
      "startup speed", "response speed", "stability", "intelligence",
      "CPU usage", "memory usage", "context consumption"]),
    ("core-goal", "核心能力", "Core Capabilities", "concepts/core-goal.md",
     ["目标", "目标态", "相位", "轮上限"],
     ["goal", "goal state", "phase", "round cap"]),
    ("core-compatibility", "核心能力", "Core Capabilities", "concepts/core-compatibility.md",
     ["兼容性", "Win7", "Ubuntu", "离线"],
     ["compatibility", "Windows 7", "Ubuntu", "offline"]),
    ("core-context-compression", "核心能力", "Core Capabilities", "concepts/core-context-compression.md",
     ["上下文压缩", "压缩", "上下文"],
     ["context compaction", "compaction", "context"]),
    ("core-subagent", "核心能力", "Core Capabilities", "concepts/core-subagent.md",
     ["子智能体", "派生", "协作"],
     ["subagent", "spawn", "collaboration"]),
    ("core-scheduled-tasks", "核心能力", "Core Capabilities", "concepts/core-scheduled-tasks.md",
     ["定时任务", "计划", "cron"],
     ["scheduled task", "schedule", "cron"]),
    ("core-channels", "核心能力", "Core Capabilities", "concepts/core-channels.md",
     ["渠道", "飞书", "QQ", "微信", "IM"],
     ["channel", "Feishu", "QQ", "WeChat", "IM"]),
    ("core-memory", "核心能力", "Core Capabilities", "concepts/core-memory.md",
     ["记忆", "长期记忆", "知识库"],
     ["memory", "long-term memory", "knowledge base"]),
    ("core-multithreading", "核心能力", "Core Capabilities", "concepts/core-multithreading.md",
     ["多线程", "并发", "线程", "队列"],
     ["multi-threading", "concurrency", "thread", "queue"]),
    ("core-multi-user-management", "核心能力", "Core Capabilities", "concepts/core-multi-user-management.md",
     ["多租户", "账号", "配额", "权限"],
     ["multi-tenancy", "account", "quota", "permission"]),
    ("tools-overview", "工具", "Tools", "tools/index.md",
     ["工具", "工具矩阵", "MCP", "别名"],
     ["tools", "tool matrix", "MCP", "alias"]),
    ("desktop-ui", "界面", "Surfaces", "surfaces/desktop-ui.md",
     ["界面", "桌面端", "侧栏", "布局"],
     ["interface", "desktop", "sidebar", "layout"]),
    ("interlink", "能力", "Capabilities", "capabilities/interlink.md",
     ["三端互通", "设备互通", "隧道", "远程"],
     ["interlink", "devices", "tunnel", "remote"]),
]

# 最终顺序：先按分组，组内按下表。local 主题来自 manual.local.{lang}.json。
ORDER = [
    "welcome", "quickstart", "forms", "install",
    "core-overview",
    "core-goal", "core-compatibility", "core-context-compression", "core-subagent",
    "core-scheduled-tasks", "core-channels", "core-memory", "core-multithreading",
    "core-multi-user-management",
    "tools-overview", "tools",
    "interface", "desktop-ui",
    "art-overview", "art-foundations", "art-icons", "art-shell", "art-timeline",
    "art-process", "art-plan", "art-composer", "art-settings", "art-motion",
    "interlink", "settings", "models", "threads", "workspace", "multimodal",
    "privacy", "troubleshooting", "glossary",
]

IMG_RE = re.compile(r"!\[[^\]]*\]\([^)]*\)")
TAG_RE = re.compile(r"<[^>]+>")
INLINE_RE = re.compile(r"(\*\*.+?\*\*|`.+?`|\[[^\]]+\]\([^)]+\))")
LINK_RE = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")
CARD_RE = re.compile(
    r'<a class="docs-card"\s+href="([^"]+)"[^>]*>\s*<strong>(.*?)</strong>\s*<span>(.*?)</span>',
    re.S,
)

# 部分手册主题只抽取总览章节，避免把仅属于文档站的深入阅读表带进蛹内嵌手册。
TOPIC_SECTIONS = {
    "core-overview": {"多快好省", "Diverse, Fast, Superior, Efficient"},
}


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


def filter_sections(blocks, headings):
    """只保留指定 H2 章节（含章节前的导语），找不到章节直接失败。"""
    out = []
    keep = True
    matched = False
    for block in blocks:
        if block["kind"] == "h2":
            heading = "".join(s["text"] for s in block["spans"]).strip()
            keep = heading in headings
            matched = matched or keep
        if keep:
            out.append(block)
    if not matched:
        print("[缺失章节] %s" % " / ".join(sorted(headings)))
        sys.exit(1)
    return out


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


def art_topic(spec, lang):
    data = spec[lang]
    blocks = [
        {"kind": "h1", "spans": [span(data["title"])]},
        {"kind": "lead", "spans": [span(data["lead"])]},
        {"kind": "image", "src": spec["slug"], "alt": data["title"], "caption": data["caption"]},
    ]
    if data.get("tip"):
        blocks.append({"kind": "tip", "spans": [span(data["tip"])]})
    return {
        "id": spec["id"],
        "title": data["title"],
        "group": ART_GROUP[lang],
        "keywords": data["keywords"],
        "blocks": blocks,
    }


def verify_art_previews():
    """The PNGs are generated straight into the embed directory."""
    missing = [spec["slug"] + ".png" for spec in ART_TOPICS
               if not os.path.exists(os.path.join(ART_ASSET_DIR, spec["slug"] + ".png"))]
    if missing:
        print("[缺失] %s；先运行 python scripts/render_art_previews.py" % ", ".join(missing))
        sys.exit(1)
    print("art: %d 张预览图 -> %s" % (len(ART_TOPICS), os.path.relpath(ART_ASSET_DIR, ROOT)))


def main():
    verify_art_previews()
    links = build_links()
    topics = {"zh": {}, "en": {}}

    for tid, gz, ge, src, kz, ke in DOCS_TOPICS:
        for lang, group, kws, base in (("zh", gz, kz, "zh-CN"), ("en", ge, ke, "en")):
            path = os.path.join(DOCS, base, src)
            if not os.path.exists(path):
                print("[缺失] %s" % path)
                sys.exit(1)
            title, blocks = parse_markdown(read(path), links)
            allowed = TOPIC_SECTIONS.get(tid)
            if allowed:
                blocks = filter_sections(blocks, allowed)
            head = read(path).split("\n")
            fm_title = None
            if head and head[0].strip() == "---":
                for line in head[1:]:
                    if line.strip() == "---":
                        break
                    if line.startswith("title:"):
                        fm_title = line.split(":", 1)[1].strip().strip('"')
            name = fm_title or title or tid
            # 与蛹专属主题保持一致：正文开头带一个 H1 标题块。
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
            if topic.get("id") == "shortcuts":
                continue
            topics[lang][topic["id"]] = topic
        for spec in ART_TOPICS:
            topic = art_topic(spec, lang)
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
