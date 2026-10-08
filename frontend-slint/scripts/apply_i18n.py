# -*- coding: utf-8 -*-
"""One-off migration: replace CJK string literals in ui/*.slint with I18n.<key>.

Uses the shared quote/comment state machine so strings containing `//`
(URLs) and real comments are both handled correctly.
"""
import glob
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
from build_i18n import SKIP_FILES, load_catalog  # noqa: E402
from slint_scan import has_cjk, scan  # noqa: E402


def main():
    entries = load_catalog()
    zh_map = {}
    for key, zh, _en in entries:
        zh_map.setdefault(zh, key)
    total = 0
    for path in sorted(glob.glob(os.path.join(ROOT, "ui", "*.slint"))):
        base = os.path.basename(path)
        if base in SKIP_FILES:
            continue
        source = open(path, encoding="utf-8").read()
        pieces = []
        cursor = 0
        replaced_here = 0
        for start, end, text in scan(source):
            if not has_cjk(text):
                continue
            key = zh_map.get(text)
            if key is None:
                raise SystemExit(f"{base}: no catalog entry for {text!r}")
            pieces.append(source[cursor:start])
            pieces.append(f"I18n.{key}")
            cursor = end
            replaced_here += 1
        pieces.append(source[cursor:])
        total += replaced_here
        if replaced_here:
            content = "".join(pieces)
            if "I18n." in content and 'from "i18n.slint"' not in content:
                content = 'import { I18n } from "i18n.slint";\n' + content
            open(path, "w", encoding="utf-8", newline="").write(content)
            print(f"rewrote {base}: {replaced_here} literals")
    print("replaced literals:", total)


if __name__ == "__main__":
    main()
