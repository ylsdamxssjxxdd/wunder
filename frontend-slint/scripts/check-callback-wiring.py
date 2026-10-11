#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""蜂窝 Slint 回调接线自检（棘轮）：声明并绑定了处理、却没人发的 callback 不许新增。

失效形状是这一类：根窗口 `callback x(...)` 声明了、Rust 侧 `app.on_x(..)` 也绑了，
但中间那一层的转发漏了 —— 编译通过、按钮看着能点、点了毫无反应，只有真机点击才暴露。
（互通「拉到本地」就是这么坏的：`interlink-cloud-pull` 在 main.slint 里只有声明行。）

判据：根窗口的 callback 名字，除了它自己的声明行之外，必须在 ui/main.slint 里再出现
一次（转发行 `x(..) => { root.x(..) }` 或调用），或在 Rust 里被显式 invoke；否则就是死线。

LEAKY_BASELINE 是 2026-10-11 实测到的历史欠账（16 条，全是别的类目留下的旧回调）：
它们不阻塞本次门禁，但删掉一条就应该从这份名单里划掉，名单只许缩短不许变长。

用法：python frontend-slint/scripts/check-callback-wiring.py
"""
import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UI_FILE = os.path.join(ROOT, "ui", "main.slint")

DECLARE = re.compile(r"^\s*callback\s+([a-z0-9][a-z0-9-]*)\s*\(", re.M)
APP_HANDLER = re.compile(r"\bapp\.on_([a-z0-9_]+)\s*\(")

LEAKY_BASELINE = {
    "agent-return-chat",
    "edit-agent-question",
    "new-thread",
    "open-active-workspace",
    "open-prompt-preview",
    "open-workspace-folder",
    "pick-attachments",
    "refresh-chat",
    "refresh-profile",
    "save-agent",
    "save-profile-avatar",
    "select-agent",
    "select-conversation",
    "timeline-counts",
    "timeline-last-answer",
    "toggle-agent-tool",
}


def occurrences(text, name):
    return len(
        re.findall(
            r"(?<![A-Za-z0-9_-])%s(?![A-Za-z0-9_-])" % re.escape(name),
            text,
        )
    )


def main():
    if not os.path.exists(UI_FILE):
        print("[FAIL] missing %s" % UI_FILE)
        return 1
    text = open(UI_FILE, encoding="utf-8").read()
    sources = [open(path, encoding="utf-8").read() for path in glob.glob(os.path.join(ROOT, "src", "*.rs"))]
    rust = "".join(sources)

    declared = set(DECLARE.findall(text))
    handled = set()
    for source in sources:
        for snake in APP_HANDLER.findall(source):
            handled.add(snake.replace("_", "-"))

    dead = []
    for name in sorted(declared & handled):
        snake = name.replace("-", "_")
        invoked = re.search(r"\bapp\.%s\s*\(" % re.escape(snake), rust) or re.search(
            r"\b%s\.call\s*\(" % re.escape(snake), rust
        )
        if occurrences(text, name) >= 2 or invoked:
            continue
        dead.append(name)

    failures = [name for name in dead if name not in LEAKY_BASELINE]
    stale = [name for name in sorted(LEAKY_BASELINE - set(dead)) if name not in declared]

    for name in failures:
        print(
            "[FAIL] callback `%s` is declared and bound but never emitted: the control "
            "is inert (add the missing forward from its panel component)" % name
        )
    for name in stale:
        print("[FAIL] `%s` is in LEAKY_BASELINE but no longer dead: shrink the list" % name)

    remaining = sorted(set(dead) & LEAKY_BASELINE)
    if failures or stale:
        print("[INFO] legacy debt still tracked: %d/%d" % (len(remaining), len(LEAKY_BASELINE)))
        return 1

    print(
        "[OK] every root callback is emitted; legacy debt unchanged at %d/%d"
        % (len(remaining), len(LEAKY_BASELINE))
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
