# -*- coding: utf-8 -*-
"""Ensure every ui/*.slint that references I18n imports it."""
import glob
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SKIP = {"i18n.slint", "design_tokens.slint"}

for path in sorted(glob.glob(os.path.join(ROOT, "ui", "*.slint"))):
    base = os.path.basename(path)
    if base in SKIP:
        continue
    src = open(path, encoding="utf-8").read()
    if "I18n." in src and 'from "i18n.slint"' not in src:
        open(path, "w", encoding="utf-8", newline="").write(
            'import { I18n } from "i18n.slint";\n' + src
        )
        print("imported I18n into", base)
print("done")
