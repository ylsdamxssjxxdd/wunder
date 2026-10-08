from pathlib import Path


OUT = Path("docs/使用说明书/assets")
OUT.mkdir(parents=True, exist_ok=True)

W = 1200
H = 720

BG_TOP = "#FBF5EE"
BG_BOTTOM = "#F1E7DA"
INK = "#21313A"
MUTED = "#63707A"
LINE = "#D7C5AF"
SOFT_LINE = "#E8DCCD"
WHITE = "#FFFDF9"

TONES = {
    "amber": {
        "panel": "#FFF5E7",
        "panel_alt": "#FFF0D9",
        "accent": "#D68A2B",
        "stroke": "#E4C391",
        "soft": "#FDF1DD",
    },
    "teal": {
        "panel": "#EDF8F4",
        "panel_alt": "#DFF1EA",
        "accent": "#2F8B73",
        "stroke": "#9BCDBD",
        "soft": "#E7F6F1",
    },
    "coral": {
        "panel": "#FDF0EA",
        "panel_alt": "#F9E0D5",
        "accent": "#C96D4C",
        "stroke": "#E7B4A0",
        "soft": "#FBEAE2",
    },
    "slate": {
        "panel": "#EEF1F4",
        "panel_alt": "#E2E7EC",
        "accent": "#4C6477",
        "stroke": "#B8C4CE",
        "soft": "#F2F5F7",
    },
    "sky": {
        "panel": "#EEF6FD",
        "panel_alt": "#DDEAF7",
        "accent": "#3D78B0",
        "stroke": "#A9C5E0",
        "soft": "#EAF3FB",
    },
}

HEAD = f"""<?xml version='1.0' encoding='UTF-8'?>
<svg xmlns='http://www.w3.org/2000/svg' width='{W}' height='{H}' viewBox='0 0 {W} {H}'>
<style>
  .title {{ font: 700 38px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {INK}; }}
  .subtitle {{ font: 400 18px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {MUTED}; }}
  .eyebrow {{ font: 700 14px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {INK}; letter-spacing: 1px; }}
  .badge {{ font: 700 13px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {INK}; }}
  .card-title {{ font: 700 24px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {INK}; }}
  .card-body {{ font: 400 16px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {INK}; }}
  .small {{ font: 400 14px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {MUTED}; }}
  .center-title {{ font: 700 26px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {INK}; }}
  .center-body {{ font: 400 16px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {INK}; }}
  .ribbon {{ font: 700 17px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {INK}; }}
  .tag {{ font: 700 12px "Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif; fill: {WHITE}; letter-spacing: 0.5px; }}
  .flow {{ stroke: {INK}; stroke-width: 3.2; fill: none; stroke-linecap: round; stroke-linejoin: round; marker-end: url(#arrow); }}
  .flow-soft {{ stroke: {MUTED}; stroke-width: 2.6; fill: none; stroke-linecap: round; stroke-linejoin: round; marker-end: url(#arrow-soft); }}
  .dash {{ stroke-dasharray: 8 8; }}
</style>
<defs>
  <linearGradient id='bg-grad' x1='0' y1='0' x2='0' y2='1'>
    <stop offset='0%' stop-color='{BG_TOP}'/>
    <stop offset='100%' stop-color='{BG_BOTTOM}'/>
  </linearGradient>
  <filter id='shadow' x='-10%' y='-10%' width='120%' height='120%'>
    <feDropShadow dx='0' dy='10' stdDeviation='12' flood-color='#B39C82' flood-opacity='0.16'/>
  </filter>
  <pattern id='grid' width='40' height='40' patternUnits='userSpaceOnUse'>
    <path d='M 40 0 L 0 0 0 40' fill='none' stroke='{SOFT_LINE}' stroke-width='1'/>
  </pattern>
  <marker id='arrow' markerWidth='11' markerHeight='11' refX='8.5' refY='5.5' orient='auto'>
    <path d='M0,0 L11,5.5 L0,11 z' fill='{INK}'/>
  </marker>
  <marker id='arrow-soft' markerWidth='10' markerHeight='10' refX='8' refY='5' orient='auto'>
    <path d='M0,0 L10,5 L0,10 z' fill='{MUTED}'/>
  </marker>
</defs>
"""

TAIL = "</svg>\n"


def esc(value: str) -> str:
    return value.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def char_units(ch: str) -> float:
    if ch == " ":
        return 0.34
    if ord(ch) > 127:
        return 1.0
    if ch in "MW@#%&":
        return 0.9
    if ch in "il.:,;|'![]()":
        return 0.4
    return 0.62


def text_units(text: str) -> float:
    return sum(char_units(ch) for ch in text)


def wrap_text(text: str, max_units: float) -> list[str]:
    chunks: list[str] = []
    for raw in text.split("\n"):
        raw = raw.strip()
        if not raw:
            chunks.append("")
            continue
        buf = ""
        units = 0.0
        for ch in raw:
            next_units = units + char_units(ch)
            if buf and next_units > max_units:
                chunks.append(buf.rstrip())
                buf = ch
                units = char_units(ch)
            else:
                buf += ch
                units = next_units
        if buf:
            chunks.append(buf.rstrip())
    return chunks or [""]


def line_count(text: str, width: int, font_size: int) -> int:
    max_units = max(4.0, (width - 8) / (font_size * 0.95))
    return len(wrap_text(text, max_units))


def text_block(
    x: int,
    y: int,
    content: str | list[str],
    width: int,
    cls: str,
    font_size: int,
    anchor: str = "start",
    line_height: int | None = None,
    style: str | None = None,
) -> str:
    lines = content if isinstance(content, list) else wrap_text(content, max(4.0, width / (font_size * 0.95)))
    if line_height is None:
        line_height = int(font_size * 1.45)
    spans = []
    for index, line in enumerate(lines):
        dy = "0" if index == 0 else str(line_height)
        spans.append(f"<tspan x='{x}' dy='{dy}'>{esc(line)}</tspan>")
    style_attr = f" style='{style}'" if style else ""
    return f"<text x='{x}' y='{y}' class='{cls}' text-anchor='{anchor}'{style_attr}>{''.join(spans)}</text>"


def auto_pill_width(label: str, font_size: int = 13, min_w: int = 78, pad: int = 28) -> int:
    return max(min_w, int(text_units(label) * font_size * 0.98 + pad))


def pill(x: int, y: int, w: int, h: int, label: str, fill: str, cls: str = "badge", text_fill: str | None = None) -> str:
    fill_attr = text_fill or INK
    return (
        f"<rect x='{x}' y='{y}' width='{w}' height='{h}' rx='{h // 2}' ry='{h // 2}' fill='{fill}'/>"
        f"<text x='{x + w / 2:.1f}' y='{y + h / 2 + 4:.1f}' class='{cls}' text-anchor='middle' fill='{fill_attr}'>{esc(label)}</text>"
    )


def pill_auto(
    x: int,
    y: int,
    label: str,
    fill: str,
    cls: str = "badge",
    text_fill: str | None = None,
    h: int = 30,
    font_size: int = 13,
    min_w: int = 78,
    pad: int = 28,
) -> str:
    return pill(x, y, auto_pill_width(label, font_size, min_w, pad), h, label, fill, cls, text_fill)


def glow(cx: int, cy: int, rx: int, ry: int, fill: str, opacity: float) -> str:
    return f"<ellipse cx='{cx}' cy='{cy}' rx='{rx}' ry='{ry}' fill='{fill}' opacity='{opacity}'/>"


def page_background() -> str:
    return (
        f"<rect x='0' y='0' width='{W}' height='{H}' fill='url(#bg-grad)'/>"
        f"<rect x='0' y='0' width='{W}' height='{H}' fill='url(#grid)' opacity='0.45'/>"
        f"{glow(150, 90, 200, 110, '#FFF7E8', 0.8)}"
        f"{glow(1050, 620, 240, 140, '#F7EEE3', 0.7)}"
        f"{glow(980, 120, 190, 90, '#E8F4F0', 0.45)}"
    )


def page_header(index: str, group: str, title: str, subtitle: str) -> str:
    title_lines = wrap_text(title, max(8.0, 910 / (38 * 0.95)))
    subtitle_y = 122 + (len(title_lines) - 1) * 44 + 44
    return (
        pill_auto(60, 42, f"核心 {index} · {group}", "#F2E6D7", "eyebrow", h=34, font_size=14, min_w=172, pad=34)
        + text_block(60, 122, title_lines, 910, "title", 38, line_height=44)
        + text_block(60, subtitle_y, subtitle, 940, "subtitle", 18, line_height=28)
        + pill(970, 48, 170, 30, "WUNDER CORE MAP", "#23313B", "tag", WHITE)
    )


def conclusion(text: str) -> str:
    return (
        f"<rect x='60' y='634' width='1080' height='56' rx='22' ry='22' fill='{WHITE}' stroke='{LINE}' stroke-width='2' filter='url(#shadow)'/>"
        + pill_auto(78, 646, "核心结论", "#21313B", "tag", WHITE, h=30, font_size=12, min_w=96, pad=30)
        + text_block(194, 670, text, 920, "ribbon", 17)
    )


def card(
    x: int,
    y: int,
    w: int,
    h: int,
    title: str,
    body: str | list[str],
    tone: str,
    tag: str | None = None,
    title_size: int = 24,
) -> str:
    palette = TONES[tone]
    compact = h <= 150
    tag_w = auto_pill_width(tag, 12, 78, 28) if tag else 0
    title_top = y + (50 if compact else 58)
    content_bottom = y + h - 26
    title_gap = 10 if compact else 14
    item_gap = 6 if compact else 10
    title_sizes = []
    for size in [title_size, title_size - 2, title_size - 4, 20, 18]:
        if size >= 18 and size not in title_sizes:
            title_sizes.append(size)
    body_sizes = [16, 15, 14, 13]
    layout = None
    for title_px in title_sizes:
        title_line_height = max(24, int(title_px * 1.18))
        title_width = w - 48 - (tag_w + 18 if tag else 0)
        title_lines = wrap_text(title, max(4.0, title_width / (title_px * 0.98)))
        for body_px in body_sizes:
            body_line_height = max(20, int(body_px * 1.45))
            cursor = title_top + len(title_lines) * title_line_height + title_gap
            if isinstance(body, list):
                ok = True
                wrapped_items: list[list[str]] = []
                for item in body:
                    wrapped = wrap_text(item, max(4.0, (w - 72) / (body_px * 0.98)))
                    wrapped_items.append(wrapped)
                    cursor += len(wrapped) * body_line_height + item_gap
                    if cursor > content_bottom:
                        ok = False
                        break
                if ok:
                    layout = (title_px, body_px, title_line_height, body_line_height, title_lines, wrapped_items)
                    break
            else:
                wrapped_body = wrap_text(body, max(4.0, (w - 48) / (body_px * 0.98)))
                cursor += len(wrapped_body) * body_line_height
                if cursor <= content_bottom:
                    layout = (title_px, body_px, title_line_height, body_line_height, title_lines, wrapped_body)
                    break
        if layout:
            break
    if layout is None:
        title_px = 18
        body_px = 13
        title_line_height = 24
        body_line_height = 20
        title_width = w - 48 - (tag_w + 18 if tag else 0)
        title_lines = wrap_text(title, max(4.0, title_width / (title_px * 0.98)))
        if isinstance(body, list):
            wrapped_items = [wrap_text(item, max(4.0, (w - 72) / (body_px * 0.98))) for item in body]
            layout = (title_px, body_px, title_line_height, body_line_height, title_lines, wrapped_items)
        else:
            wrapped_body = wrap_text(body, max(4.0, (w - 48) / (body_px * 0.98)))
            layout = (title_px, body_px, title_line_height, body_line_height, title_lines, wrapped_body)
    title_px, body_px, title_line_height, body_line_height, title_lines, wrapped_body = layout
    parts = [
        f"<g filter='url(#shadow)'>"
        f"<rect x='{x}' y='{y}' width='{w}' height='{h}' rx='28' ry='28' fill='{palette['panel']}' stroke='{palette['stroke']}' stroke-width='2.2'/>"
        f"<rect x='{x}' y='{y}' width='{w}' height='16' rx='28' ry='28' fill='{palette['accent']}' opacity='0.92'/>"
        f"<rect x='{x + 18}' y='{y + 24}' width='{w - 36}' height='{h - 42}' rx='22' ry='22' fill='{palette['soft']}' opacity='0.58'/>"
        f"</g>"
    ]
    if tag:
        parts.append(pill(x + w - tag_w - 22, y + 24, tag_w, 28, tag, palette["accent"], "tag", WHITE))
    parts.append(
        text_block(
            x + 24,
            title_top,
            title_lines,
            w - 48,
            "card-title",
            title_px,
            line_height=title_line_height,
            style=f"font-size:{title_px}px",
        )
    )
    title_h = len(title_lines) * title_line_height
    body_y = title_top + title_h + title_gap
    if isinstance(body, list):
        cursor = body_y
        for item in wrapped_body:
            parts.append(f"<circle cx='{x + 34}' cy='{cursor - 6}' r='4.5' fill='{palette['accent']}'/>")
            parts.append(
                text_block(
                    x + 48,
                    cursor,
                    item,
                    w - 72,
                    "card-body",
                    body_px,
                    line_height=body_line_height,
                    style=f"font-size:{body_px}px",
                )
            )
            cursor += len(item) * body_line_height + item_gap
    else:
        parts.append(
            text_block(
                x + 24,
                body_y,
                wrapped_body,
                w - 48,
                "card-body",
                body_px,
                line_height=body_line_height,
                style=f"font-size:{body_px}px",
            )
        )
    return "".join(parts)


def center_disc(cx: int, cy: int, r: int, title: str, body: str, tone: str) -> str:
    palette = TONES[tone]
    title_lines = wrap_text(title, 10)
    body_lines = wrap_text(body, 16)
    parts = [
        f"<circle cx='{cx}' cy='{cy}' r='{r + 20}' fill='{palette['soft']}' opacity='0.72'/>",
        f"<circle cx='{cx}' cy='{cy}' r='{r + 6}' fill='{WHITE}' stroke='{palette['stroke']}' stroke-width='3'/>",
        f"<circle cx='{cx}' cy='{cy}' r='{r - 18}' fill='{palette['panel_alt']}' stroke='{palette['accent']}' stroke-width='2.5'/>",
        text_block(cx, cy - 8, title_lines, 220, "center-title", 26, "middle", 30),
        text_block(cx, cy + 40, body_lines, 220, "center-body", 16, "middle", 24),
    ]
    return "".join(parts)


def line_arrow(x1: int, y1: int, x2: int, y2: int, soft: bool = False, dashed: bool = False) -> str:
    cls = "flow-soft" if soft else "flow"
    extra = " dash" if dashed else ""
    return f"<path class='{cls}{extra}' d='M{x1},{y1} L{x2},{y2}'/>"


def curve_arrow(d: str, soft: bool = False, dashed: bool = False) -> str:
    cls = "flow-soft" if soft else "flow"
    extra = " dash" if dashed else ""
    return f"<path class='{cls}{extra}' d='{d}'/>"


def tag_stack(x: int, y: int, labels: list[str], tone: str, per_row: int = 2) -> str:
    palette = TONES[tone]
    parts = []
    px = x
    py = y
    count = 0
    for label in labels:
        width = auto_pill_width(label, 13, 90, 34)
        if count == per_row:
            count = 0
            px = x
            py += 42
        parts.append(pill(px, py, width, 30, label, palette["panel_alt"]))
        px += width + 12
        count += 1
    return "".join(parts)


def save(name: str, parts: list[str]) -> None:
    (OUT / name).write_text(HEAD + page_background() + "".join(parts) + TAIL, encoding="utf-8")


save(
    "core-overview-map.svg",
    [
        page_header("00", "总览", "Wunder 的 11 个核心不是功能清单，而是一套统一运行结构", "先看三层骨架，再进入细页。执行内核决定系统能不能跑，接入治理决定能不能扩，交付保障决定能不能长期用。"),
        f"<rect x='60' y='214' width='1080' height='72' rx='26' ry='26' fill='{WHITE}' stroke='{LINE}' stroke-width='2' filter='url(#shadow)'/>",
        pill_auto(82, 234, "阅读顺序", "#23313B", "tag", WHITE, h=30, font_size=12, min_w=110, pad=28),
        text_block(232, 259, "先建立执行内核，再处理入口与治理，最后补齐实时、稳定与可观测能力。", 860, "ribbon", 17),
        card(60, 314, 320, 286, "执行内核", ["智能体循环是主链路。", "工具把能力变成可调度对象。", "蜂群、压缩、记忆都围绕线程一致性服务。"], "amber", "01-05"),
        tag_stack(86, 500, ["智能体循环", "工具", "蜂群", "上下文压缩", "记忆"], "amber", 3),
        card(440, 314, 320, 286, "接入与治理", ["渠道只做协议适配。", "定时任务把时间纳入能力边界。", "多用户管理把身份、权限与隔离前置。"], "teal", "06-08"),
        tag_stack(466, 502, ["渠道", "定时任务", "多用户管理"], "teal", 2),
        card(820, 314, 320, 286, "交付保障", ["实时性负责持续可见。", "稳定性负责风险不扩散。", "可观测性负责事实、回放与画像统一。"], "coral", "09-11"),
        tag_stack(846, 502, ["实时性", "稳定性", "可观测性"], "coral", 2),
        line_arrow(380, 456, 440, 456),
        line_arrow(760, 456, 820, 456),
        conclusion("核心不是堆出 11 个名词，而是让所有能力都服从同一运行真相、同一治理边界和同一交付标准。"),
    ],
)

save(
    "core-agent-loop.svg",
    [
        page_header("01", "执行内核", "智能体循环不是 UI 流程，而是系统主链路", "真正要保护的是线程事实。用户输入、模型判断、工具执行、状态收敛、事件投影都必须围绕主线程形成闭环。"),
        center_disc(600, 370, 104, "主线程", "唯一真相\n所有动作回流", "amber"),
        card(90, 282, 220, 178, "输入进入线程", "用户轮次先入主线程，初始化时 prompt 冻结。", "amber", "入口"),
        card(470, 144, 260, 178, "模型判断", "读取上下文后，决定直接回答还是继续调工具。", "teal", "决策"),
        card(440, 448, 280, 184, "工具动作", "命令、文件、外部能力都只是 observation 来源，结果必须再回线程。", "coral", "执行"),
        card(860, 282, 240, 182, "状态收敛与投影", "重试、等待、恢复与终态统一记录，前端看到的只是投影。", "slate", "收束"),
        curve_arrow("M312,374 C380,314 448,276 494,286"),
        curve_arrow("M650,266 C670,240 724,232 756,252"),
        curve_arrow("M880,374 C830,446 764,504 704,520"),
        curve_arrow("M490,520 C404,508 336,466 300,414"),
        line_arrow(730, 230, 848, 316, True),
        conclusion("线程不是展示概念，而是执行一致性的硬边界。只要有一步绕开线程，系统就会出现两套事实。"),
    ],
)

save(
    "core-tools.svg",
    [
        page_header("02", "执行内核", "工具的价值不在“能做事”，而在“可治理地做事”", "对模型来说一切皆工具，但对系统来说，工具必须有统一描述、统一预算、统一权限和统一结果裁剪。"),
        card(80, 248, 250, 262, "工具规格", ["描述足够清晰，模型才会稳定调用。", "参数必须结构化，失败信号必须明确。"], "amber", "定义"),
        card(390, 206, 360, 304, "执行治理层", ["预算控制：限制时间、token、资源开销。", "权限与审批：避免危险动作直接穿透。", "结果裁剪：把必要事实返回给模型，而不是把噪声原样塞回上下文。"], "teal", "内核"),
        card(820, 212, 300, 134, "入模结果", ["只保留能支持下一轮判断的事实。"], "coral", "模型侧"),
        card(820, 376, 300, 134, "展示投影", ["审批、工作流卡片、增量输出都来自同一执行事实。"], "slate", "界面侧"),
        line_arrow(330, 378, 390, 378),
        line_arrow(750, 300, 820, 278),
        line_arrow(750, 416, 820, 444),
        f"<rect x='390' y='540' width='730' height='56' rx='22' ry='22' fill='{WHITE}' stroke='{LINE}' stroke-width='2'/>",
        pill_auto(412, 553, "统一协议", "#2F8B73", "tag", WHITE, h=28, font_size=12, min_w=104, pad=28),
        text_block(552, 576, "模型调用、执行层治理、前端投影必须基于同一份工具事实，而不是各写各的解释。", 540, "small", 14),
        conclusion("工具不是插件堆砌，而是把能力压成一种系统能调度、能约束、能解释的公共对象。"),
    ],
)

save(
    "core-swarm.svg",
    [
        page_header("03", "执行内核", "蜂群不是“多开几个模型”，而是正式的协作语义", "母蜂负责拆解与归并，工蜂必须以新线程执行。关键不在并行本身，而在边界是否清楚、结果是否可回收。"),
        card(430, 154, 340, 150, "母蜂主线程", "拆任务、派工、汇总结论，所有协作最终重新回到母蜂主链路。", "amber", "主控"),
        f"<rect x='90' y='310' width='1020' height='222' rx='34' ry='34' fill='{WHITE}' stroke='{SOFT_LINE}' stroke-width='2'/>",
        text_block(118, 350, "蜂群运行面", 200, "subtitle", 18),
        card(126, 372, 250, 132, "工蜂 A", ["新建线程执行。", "持续回报进度与结论。"], "teal", "新线程"),
        card(474, 372, 250, 132, "工蜂 B", ["必要时再拆子任务。", "但它的上下文必须独立。"], "sky", "新线程"),
        card(822, 372, 250, 132, "工蜂 C", ["返回结果，也要返回失败原因。"], "coral", "新线程"),
        f"<rect x='266' y='550' width='668' height='50' rx='20' ry='20' fill='#23313B' opacity='0.96'/>",
        text_block(600, 582, "结果归并层：协作可以并行发生，但结论必须重新收束到母蜂线程。", 620, "tag", 12, "middle", 18),
        curve_arrow("M526,298 C470,330 360,356 250,372"),
        curve_arrow("M600,298 C600,328 600,348 600,372"),
        curve_arrow("M674,298 C734,330 846,356 948,372"),
        curve_arrow("M250,504 C314,528 402,544 466,552", True),
        curve_arrow("M600,504 C600,526 600,538 600,552", True),
        curve_arrow("M948,504 C880,528 798,544 734,552", True),
        conclusion("蜂群的核心不是更多智能体，而是把拆解、隔离、回报、归并这四件事做成正式结构。"),
    ],
)

save(
    "core-context-compression.svg",
    [
        page_header("04", "执行内核", "上下文压缩不是简单截断，而是有约束地收缩", "压缩必须同时满足三件事：保留有效信息、留下可追溯痕迹、绝不伪造线程事实。"),
        card(72, 246, 252, 284, "完整上下文", ["历史消息。", "工具 observation。", "当前请求与运行状态。"], "amber", "原料"),
        card(388, 204, 286, 110, "Level 1", "去掉中间噪声与可再生细节。", "teal", "轻压缩"),
        card(424, 330, 250, 110, "Level 2", "把可合并历史整理成摘要。", "sky", "摘要"),
        card(458, 456, 216, 110, "Level 3 / 4", "丢弃早期轮次或进入紧急降级。", "coral", "强压缩"),
        card(756, 250, 280, 160, "可继续执行的窗口", ["目标不是“更短”，而是“还能正确判断与继续执行”。"], "slate", "目标"),
        card(984, 228, 156, 242, "观测轨", "记录事件\n标注损失\n支持复盘", "teal", "审计", 20),
        line_arrow(324, 388, 388, 260),
        line_arrow(324, 388, 424, 386),
        line_arrow(324, 388, 458, 512),
        line_arrow(674, 260, 756, 302),
        line_arrow(674, 386, 756, 332),
        line_arrow(674, 512, 756, 362),
        line_arrow(1036, 330, 984, 330, True),
        conclusion("压缩的成功标准不是字数下降，而是线程还能继续跑、损失还能被解释、事实没有被篡改。"),
    ],
)

save(
    "core-memory.svg",
    [
        page_header("05", "执行内核", "记忆的关键不是“放更多内容”，而是保护线程认知底座", "长期记忆只允许在线程初始化时注入一次。运行中可以 recall，但不能反复改写 system prompt。"),
        f"<rect x='70' y='520' width='1060' height='94' rx='32' ry='32' fill='{TONES['slate']['panel']}' stroke='{TONES['slate']['stroke']}' stroke-width='2.2' filter='url(#shadow)'/>",
        pill_auto(96, 548, "冻结底座", TONES["slate"]["accent"], "tag", WHITE, h=30, font_size=12, min_w=112, pad=28),
        text_block(268, 574, "线程初始化后，prompt 语义不再漂移。这样模型缓存稳定、行为稳定、历史线程也不会被新模板反写。", 822, "ribbon", 17),
        card(88, 218, 250, 190, "初始化注入", ["构造 system prompt。", "只在这一刻注入长期记忆。"], "amber", "一次性"),
        center_disc(600, 322, 96, "冻结边界", "后续轮次\n不重写底座", "teal"),
        card(840, 214, 272, 204, "运行期补充", "需要资料时，通过 recall 或工具再取回；补充的是工作信息，不是再改线程底座。", "coral", "运行中"),
        curve_arrow("M338,312 C414,286 472,278 504,290"),
        curve_arrow("M696,290 C754,278 804,284 840,304"),
        curve_arrow("M980,410 C980,470 892,506 760,520", True, True),
        text_block(980, 446, "只补工作面", 120, "small", 14, "middle"),
        conclusion("长期记忆要提升可用性，但不能以破坏线程稳定为代价。底座一旦漂移，整个运行链路都会失真。"),
    ],
)

save(
    "core-channels.svg",
    [
        page_header("06", "接入与治理", "渠道可以很多，但运行内核只能有一套", "HTTP、WebSocket、Desktop、CLI、第三方渠道只是入口形态不同。它们可以适配协议，但不能改写线程、工具和治理语义。"),
        card(60, 214, 210, 122, "HTTP", ["通用执行入口。"], "amber", "入口"),
        card(60, 356, 210, 122, "WebSocket", ["实时会话入口。"], "teal", "入口"),
        card(60, 498, 210, 122, "桌面与命令行", "本地工作台与终端。", "sky", "入口"),
        card(298, 278, 236, 240, "协议适配层", ["把外部协议翻成统一请求。", "处理身份、连接、附件与会话包装。"], "slate", "适配"),
        center_disc(764, 372, 112, "统一运行内核", "线程\n工具\n事件\n治理", "amber"),
        card(948, 250, 188, 244, "第三方渠道", ["Webhook。", "长连接。", "企业协同平台。"], "coral", "扩展"),
        line_arrow(270, 276, 298, 332),
        line_arrow(270, 418, 298, 398),
        line_arrow(270, 560, 298, 462),
        line_arrow(534, 398, 646, 382),
        line_arrow(876, 372, 948, 372),
        conclusion("渠道层只负责接入差异，不能偷偷长出另一套运行逻辑。入口越多，越要守住同一内核。"),
    ],
)

save(
    "core-scheduled-tasks.svg",
    [
        page_header("07", "接入与治理", "定时任务把系统时间轴正式纳入能力边界", "它不是 sleep 的放大版，而是拥有规则、唤醒、执行、记录和治理语义的后台运行链。"),
        f"<path class='flow-soft' d='M110,222 L1090,222'/>",
        pill_auto(118, 202, "at / every", "#F2E6D7", h=28, font_size=13, min_w=110, pad=30),
        pill_auto(248, 202, "cron", "#F2E6D7", h=28, font_size=13, min_w=82, pad=28),
        pill_auto(346, 202, "enable", "#F2E6D7", h=28, font_size=13, min_w=92, pad=28),
        card(76, 270, 248, 206, "调度规则", ["定义下一次执行时间。", "决定是否启用、暂停或删除。"], "amber", "规则"),
        card(392, 242, 284, 262, "调度器", ["轮询时间点。", "唤醒待跑任务。", "选择进入后台执行的对象。"], "teal", "主控"),
        card(744, 236, 212, 130, "主会话", "挂回主线程语义。", "sky", "执行"),
        card(744, 384, 212, 156, "隔离会话", "隔离线程运行，避免污染在线上下文。", "coral", "执行"),
        card(970, 278, 170, 214, "执行记录", ["下次执行时间。", "最近运行状态。", "运行日志与失败原因。"], "slate", "留痕"),
        line_arrow(324, 372, 392, 372),
        line_arrow(676, 322, 744, 300),
        line_arrow(676, 424, 744, 462),
        line_arrow(956, 300, 970, 332),
        line_arrow(956, 444, 970, 414),
        conclusion("定时任务是系统级后台能力。只有把时间、执行语义和留痕统一起来，它才不是一段脆弱脚本。"),
    ],
)

save(
    "core-multi-user-management.svg",
    [
        page_header("08", "接入与治理", "多用户管理不是后台附属功能，而是运行边界本身", "组织、单位、用户、权限、配额与工作区隔离必须前置，否则系统一扩容就会把线程、工具和数据边界全部冲散。"),
        card(82, 228, 222, 260, "身份层", ["组织、单位、用户分层。", "登录用户与虚拟 user_id 要区分。"], "amber", "身份"),
        card(346, 206, 242, 304, "治理层", ["权限控制决定能不能做。", "资源配额决定能做多少。", "管理员面板决定如何集中治理。"], "teal", "规则"),
        card(630, 206, 248, 304, "隔离层", ["工作区边界。", "线程归属。", "工具可见性与资源访问范围。"], "sky", "隔离"),
        card(920, 228, 222, 260, "并发运行结果", ["多用户并发访问仍保持清晰边界。", "能力可开放，也可按租户收口。"], "coral", "结果"),
        line_arrow(304, 358, 346, 358),
        line_arrow(588, 358, 630, 358),
        line_arrow(878, 358, 920, 358),
        f"<rect x='346' y='540' width='532' height='52' rx='20' ry='20' fill='{WHITE}' stroke='{LINE}' stroke-width='2'/>",
        text_block(612, 572, "边界前置比事后补权限更重要。", 460, "ribbon", 17, "middle"),
        conclusion("多用户系统真正难的不是多几张表，而是让身份、治理和隔离在运行前就已经决定资源边界。"),
    ],
)

save(
    "core-realtime.svg",
    [
        page_header("09", "交付保障", "实时性负责把线程事实持续投影到客户端", "关键不是 WebSocket 本身，而是线程变化如何变成可消费事件，以及断线后如何恢复到正确状态。"),
        card(84, 254, 238, 228, "线程事实", ["运行态变化。", "工具调用。", "终态收敛。"], "amber", "真相源"),
        card(388, 202, 334, 188, "事件流", "WebSocket / SSE 负责增量投影，thread_status、增量输出与工作流都从事实流派生。", "teal", "投影层"),
        f"<rect x='388' y='412' width='334' height='122' rx='28' ry='28' fill='{TONES['teal']['soft']}' stroke='{TONES['teal']['stroke']}' stroke-width='2.2' filter='url(#shadow)'/>",
        pill_auto(414, 432, "恢复机制", TONES["teal"]["accent"], "tag", WHITE, h=28, font_size=12, min_w=108, pad=28),
        text_block(414, 482, "resume、快照补偿、断线重连要把客户端重新拉回当前线程事实。", 270, "card-body", 16),
        card(796, 214, 160, 132, "列表页", ["看状态变化。"], "sky", "客户端"),
        card(980, 214, 160, 132, "详情页", ["看工作流与细节。"], "coral", "客户端"),
        card(888, 392, 168, 126, "终态收敛", ["回到一致终态。"], "slate", "一致"),
        line_arrow(322, 368, 388, 300),
        line_arrow(322, 368, 388, 470),
        line_arrow(722, 300, 796, 280),
        line_arrow(722, 300, 980, 280),
        curve_arrow("M722,470 C792,520 852,520 888,458", True, True),
        conclusion("实时性不是再造一套事实，而是让同一线程事实可以被持续消费、掉线补偿、最终收敛。"),
    ],
)

save(
    "core-stability.svg",
    [
        page_header("10", "交付保障", "稳定性不是少报错，而是错误出现时不扩散、可恢复、可复盘", "真正的稳定性来自结构：超时、重试、隔离、预算、事务与补偿要把风险消化在系统内部，而不是推给人盯着跑。"),
        card(72, 244, 230, 258, "风险源", ["模型超时。", "工具失败。", "网络抖动。", "并发积压。"], "coral", "输入"),
        f"<rect x='366' y='204' width='394' height='338' rx='42' ry='42' fill='{WHITE}' stroke='{TONES['slate']['stroke']}' stroke-width='2.4' filter='url(#shadow)'/>",
        f"<rect x='398' y='236' width='330' height='122' rx='28' ry='28' fill='{TONES['slate']['panel']}' stroke='{TONES['slate']['stroke']}' stroke-width='2'/>",
        pill_auto(424, 256, "隔离与治理", TONES["slate"]["accent"], "tag", WHITE, h=28, font_size=12, min_w=114, pad=28),
        text_block(424, 308, "超时、队列、资源预算把风险限制在边界内。", 270, "card-body", 16),
        f"<rect x='398' y='382' width='330' height='122' rx='28' ry='28' fill='{TONES['teal']['panel']}' stroke='{TONES['teal']['stroke']}' stroke-width='2'/>",
        pill_auto(424, 402, "恢复链路", TONES["teal"]["accent"], "tag", WHITE, h=28, font_size=12, min_w=98, pad=28),
        text_block(424, 454, "续跑、补发、原子写入、事务与 outbox 负责把任务重新带回收敛面。", 272, "card-body", 16),
        card(834, 258, 292, 230, "结果", ["故障不会一路级联。", "任务可以落回终态。", "问题还能被回归验证。"], "amber", "收敛"),
        line_arrow(302, 374, 366, 374),
        line_arrow(760, 374, 834, 374),
        conclusion("稳定性是系统结构消化风险的能力。没有隔离与恢复，再多人工值守也只是延后事故。"),
    ],
)

save(
    "core-observability.svg",
    [
        page_header("11", "交付保障", "可观测性要把“发生了什么”拆成事实、回放、画像三层", "如果把它们混成一锅日志，系统就既不能准确复盘，也不能稳定出报表，更无法沉淀统一口径。"),
        f"<rect x='90' y='220' width='1020' height='332' rx='38' ry='38' fill='{WHITE}' stroke='{LINE}' stroke-width='2.2' filter='url(#shadow)'/>",
        f"<rect x='126' y='428' width='948' height='94' rx='28' ry='28' fill='{TONES['amber']['panel']}' stroke='{TONES['amber']['stroke']}' stroke-width='2'/>",
        pill_auto(152, 448, "事实层", TONES["amber"]["accent"], "tag", WHITE, h=28, font_size=12, min_w=92, pad=28),
        text_block(270, 474, "线程事实流、真实事件顺序、请求与 observation 是最底层真相。", 760, "card-body", 16),
        f"<rect x='186' y='314' width='828' height='84' rx='26' ry='26' fill='{TONES['teal']['panel']}' stroke='{TONES['teal']['stroke']}' stroke-width='2'/>",
        pill_auto(212, 334, "回放层", TONES["teal"]["accent"], "tag", WHITE, h=28, font_size=12, min_w=92, pad=28),
        text_block(330, 360, "时间线重建、监控明细、导出与调试都基于事实层重放，而不是手工拼故事。", 640, "card-body", 16),
        f"<rect x='256' y='230' width='688' height='56' rx='22' ry='22' fill='{TONES['coral']['panel']}' stroke='{TONES['coral']['stroke']}' stroke-width='2'/>",
        pill_auto(282, 244, "画像层", TONES["coral"]["accent"], "tag", WHITE, h=28, font_size=12, min_w=92, pad=28),
        text_block(400, 266, "管理员面板、tool usage、throughput 与 benchmark 要共享同一指标口径。", 500, "card-body", 16),
        line_arrow(600, 428, 600, 398),
        line_arrow(600, 314, 600, 286),
        conclusion("可观测性的难点不在监控页数量，而在全链路口径统一。事实准了，回放和画像才不会各说各话。"),
    ],
)

print("generated core svg assets")
