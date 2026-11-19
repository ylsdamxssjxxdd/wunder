#!/usr/bin/env python3
"""Embed one or more GLB models into an HTML file as base64 data.

The delivered page must open from a single file, so a model file cannot travel
next to it: the binary is base64-encoded into the same document and decoded in
memory before ``THREE.GLTFLoader.parse()`` runs.

Two output modes:

- ``--js 片段.js``  writes one script that fills ``window.WUNDER_GLB``;
- ``--html 输入.html 输出.html`` inserts that script before ``</body>``.

Always run this last: ``inline_three.py`` first (so the loader exists), then the
GLB, then open the result.
"""

from __future__ import annotations

import argparse
import base64
import re
from pathlib import Path

# 单个 GLB 的体积建议上限（原始字节）。base64 会再增长约 1/3，整个 HTML 还
# 要装下 Three.js 与业务代码，所以超过它就先把网格减面或换程序化几何体。
SOFT_LIMIT_BYTES = 2 * 1024 * 1024
HARD_WARN_BYTES = 8 * 1024 * 1024


def _key(name: str) -> str:
    """JS 对象键：保留原名，只去掉换行与引号。"""
    return " ".join(name.replace('"', "").replace("\\", "").split())


def build_script(models: list[tuple[str, Path]]) -> str:
    lines = [
        "/* 内嵌 GLB 模型（由 scripts/inline_glb.py 生成，勿手工编辑） */",
        "window.WUNDER_GLB = window.WUNDER_GLB || {};",
    ]
    for name, path in models:
        payload = base64.b64encode(path.read_bytes()).decode("ascii")
        lines.append(
            'window.WUNDER_GLB["%s"] = { name: "%s", bytes: %d, base64: "%s" };'
            % (_key(name), _key(name), path.stat().st_size, payload)
        )
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description="Embed GLB models into an HTML file")
    parser.add_argument("models", nargs="+", type=Path, help="GLB 文件（可多个）")
    parser.add_argument("--js", type=Path, help="输出独立的 JS 片段")
    parser.add_argument(
        "--html",
        nargs=2,
        metavar=("输入.html", "输出.html"),
        help="把片段插入到输入 HTML 的 </body> 之前并写出新文件",
    )
    parser.add_argument(
        "--name",
        action="append",
        default=[],
        help="模型键名，按 models 顺序对应；缺省用文件名（不含扩展名）",
    )
    args = parser.parse_args()

    models: list[tuple[str, Path]] = []
    total = 0
    for index, path in enumerate(args.models):
        if not path.is_file():
            raise SystemExit("找不到模型文件：" + str(path))
        if path.suffix.lower() != ".glb":
            raise SystemExit("只支持 .glb（二进制 glTF）：" + str(path))
        raw = path.stat().st_size
        total += raw
        if raw > HARD_WARN_BYTES:
            print("警告：%s 有 %.1f MB，内嵌后页面会很难打开" % (path.name, raw / 1048576))
        elif raw > SOFT_LIMIT_BYTES:
            print("提示：%s 有 %.1f MB，超过建议上限 2 MB，先考虑减面" % (path.name, raw / 1048576))
        name = args.name[index] if index < len(args.name) else path.stem
        models.append((name, path))

    script = build_script(models)
    if args.js:
        args.js.parent.mkdir(parents=True, exist_ok=True)
        with open(args.js, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(script)
        print("写出片段：%s（模型 %.1f MB，base64 约 %.1f MB）" % (
            args.js, total / 1048576, len(script) / 1048576))
    if args.html:
        source, destination = Path(args.html[0]), Path(args.html[1])
        html = source.read_text(encoding="utf-8")
        block = "<script>\n" + script + "</script>\n"
        # 插在 </head> 之前：body 里的业务脚本必须能直接读到 window.WUNDER_GLB。
        # 没有 head 的页面退回到 body 开头；两者都没有才拒绝。
        if "</head>" in html:
            html = html.replace("</head>", block + "</head>", 1)
        elif re.search(r"<body[^>]*>", html):
            html = re.sub(r"(<body[^>]*>)", lambda m: m.group(1) + "\n" + block, html, count=1)
        else:
            raise SystemExit("输入 HTML 没有 <head> 或 <body>，无法插入模型")
        destination.parent.mkdir(parents=True, exist_ok=True)
        with open(destination, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(html)
        print("写出页面：%s（%.1f MB）" % (destination, destination.stat().st_size / 1048576))
    if not args.js and not args.html:
        raise SystemExit("请给出 --js 或 --html 之一")


if __name__ == "__main__":
    main()
