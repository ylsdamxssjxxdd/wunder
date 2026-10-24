#!/usr/bin/env python3
"""把 bindgen 产物改造成 compositor 可编译的形态。

1. AVRational 需要 PartialEq（rust-ffmpeg 里直接用 == 比较）；
2. 老工具链（< 1.82）不认识 `unsafe extern`，需要降级为 `extern`。
   与 wunder 一致的 nightly 不需要这步，用 LEGACY_BINDINGS=1 显式开启。
"""
import os
import re
import sys

PATH = sys.argv[1] if len(sys.argv) > 1 else "/vendor/rust-ffmpeg-sys/bindings/i686-win7-windows-gnu.rs"
LEGACY = os.environ.get("LEGACY_BINDINGS") == "1"

src = open(PATH, encoding="utf-8").read()

replaced = 0
if LEGACY:
    src, replaced = re.subn(r'unsafe extern ("[^"]*") \{', r'extern \1 {', src)

old = "#[derive(Debug, Copy, Clone)]\npub struct AVRational {"
new = "#[derive(Debug, Copy, Clone, PartialEq)]\npub struct AVRational {"
patched = old in src
if patched:
    src = src.replace(old, new, 1)

open(PATH, "w", encoding="utf-8", newline="\n").write(src)
print(f"extern blocks rewritten: {replaced}, AVRational PartialEq: {patched}")
