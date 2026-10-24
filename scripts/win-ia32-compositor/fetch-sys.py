#!/usr/bin/env python3
"""只拉 rust-ffmpeg-sys 的源码（仓库 3.5GB，其中 zips/ 是各平台 FFmpeg 二进制包）。"""
import base64
import json
import os
import urllib.request

REV = "e7f864a5d93368a984cda5d73be52a13edb6cc4c"
ROOT = os.environ.get("SYS_ROOT", "/vendor/rust-ffmpeg-sys")
API = "https://api.github.com/repos/remotion-dev/rust-ffmpeg-sys"
SKIP = ("bindings/", "zips/", ".github/", ".agents/")

with urllib.request.urlopen(f"{API}/git/trees/{REV}?recursive=1", timeout=60) as resp:
    tree = json.load(resp)["tree"]

for blob in tree:
    if blob["type"] != "blob" or blob["path"].startswith(SKIP):
        continue
    dst = os.path.join(ROOT, blob["path"])
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    with urllib.request.urlopen(f"{API}/git/blobs/{blob['sha']}", timeout=60) as resp:
        open(dst, "wb").write(base64.b64decode(json.load(resp)["content"]))
print("vendored", len([b for b in tree if b["type"] == "blob"]), "files into", ROOT)
