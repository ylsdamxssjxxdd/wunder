#!/usr/bin/env python3
"""管理端导航改造后的快速一致性检查。"""

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[3]
INDEX = ROOT / "web/index.html"
ELEMENTS = ROOT / "web/modules/elements.js"
APP = ROOT / "web/app.js"
I18N = ROOT / "web/modules/i18n.js"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def assert_contains(text: str, needle: str, errors: list[str], label: str) -> None:
    if needle not in text:
        errors.append(f"缺少 {label}: {needle}")


def assert_regex(text: str, pattern: str, errors: list[str], label: str) -> None:
    if not re.search(pattern, text, re.S):
        errors.append(f"未匹配 {label}: {pattern}")


def main() -> int:
    errors: list[str] = []
    index = read(INDEX)
    elements = read(ELEMENTS)
    app = read(APP)
    i18n = read(I18N)

    for group in ("system", "agent", "debug", "docs"):
        assert_contains(index, f'data-group="{group}"', errors, "侧栏分组")

    for nav_id in (
        "navLlm",
        "navUsers",
        "navMemory",
        "navChannels",
        "navBuiltin",
    ):
        assert_contains(index, f'id="{nav_id}"', errors, "智能体分组入口")

    for removed in ("navMcp", "navKnowledge", "navA2aServices", "navSkills"):
        if f'id="{removed}"' in index:
            errors.append(f"旧侧栏入口仍存在: {removed}")

    for shortcut in (
        "toolManagerOpenBuiltin",
        "toolManagerOpenMcp",
        "toolManagerOpenKnowledge",
        "toolManagerOpenA2aServices",
        "toolManagerOpenSkills",
    ):
        assert_contains(index, f'id="{shortcut}"', errors, "工具管理快捷按钮")
        assert_contains(elements, shortcut, errors, "elements 映射")
        assert_contains(app, shortcut, errors, "app 事件绑定")

    assert_regex(app, r"mcp\s*:\s*\{[^\}]*nav\s*:\s*elements\.navBuiltin", errors, "mcp 共享导航")
    assert_regex(app, r"knowledge\s*:\s*\{[^\}]*nav\s*:\s*elements\.navBuiltin", errors, "knowledge 共享导航")
    assert_regex(app, r"skills\s*:\s*\{[^\}]*nav\s*:\s*elements\.navBuiltin", errors, "skills 共享导航")
    assert_regex(app, r"a2aServices\s*:\s*\{[^\}]*nav\s*:\s*elements\.navBuiltin", errors, "a2a 共享导航")
    assert_contains(app, 'const navButtons = new Set();', errors, "switchPanel 清理逻辑")

    for key in (
        "sidebar.group.agent",
        "panel.toolManager",
        "toolManager.tip",
        "toolManager.shortcutHint",
    ):
        if i18n.count(f'"{key}"') < 2:
            errors.append(f"i18n 键未同时出现在中英文: {key}")

    if errors:
        print("[FAIL] 管理端导航一致性检查未通过")
        for item in errors:
            print(f" - {item}")
        return 1

    print("[OK] 管理端导航一致性检查通过")
    print(f" - checked files: {INDEX}, {ELEMENTS}, {APP}, {I18N}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
