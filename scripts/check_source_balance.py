#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""批量删除/大改之后的括号配平自检。

用途：一次性找出「函数头被删、尾部语句与 `}` 留下」这类残片——它们会让编译器停在
`unexpected closing delimiter` / `unclosed delimiter`，而逐个错误来回跑 cargo check 很慢。
本脚本只做词法级的 `()[]{}` 配平，不解析语法，因此足够快，可用于大范围扫描。

支持：Rust（含 `r#"..."#` 原始字符串、字符字面量、`//`、`/* */`）、
JS/TS/Vue（含模板字符串 `` ` ``、正则字面量的保守处理由「斜杠后跟非空白」启发式跳过）。

用法：
    python scripts/check_source_balance.py crates frontend/src web
    python scripts/check_source_balance.py --changed          # 只看 git 改动的文件
    python scripts/check_source_balance.py --changed --quiet   # 只输出有问题的文件

退出码：0 = 全部配平；1 = 存在不平衡文件。

注意：本脚本是**启发式分诊工具**，不是解析器。已知的误报来源：
- 正则字面量前面不是 `( , = : [ ! & | ? { } ;` 的写法（如 `return x / y` 之外的除法边界）；
- 内嵌 HTML/JS 的模板字符串（`workspaceHtmlPreview.ts`）、Vue 模板里的复杂表达式；
- HTML 里的 `//`（未加引号的 URL）。
因此命中项只是**候选**，最终仍以 `cargo check` / `node --check` / `vue-tsc` 为准；反之，
`crates` 全量扫描 0 命中说明不存在「删掉函数头、尾部语句与 `}` 留下」这类残片。
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

EXTS = {".rs", ".js", ".mjs", ".cjs", ".ts", ".tsx", ".vue", ".html", ".css"}
SKIP_DIRS = {
    ".git",
    "target",
    "node_modules",
    "node_modules-linux-arm",
    "node_modules-linux-x86",
    "dist",
    "dist-desktop",
    "docs",
    "third",
}

PAIRS = {")": "(", "]": "[", "}": "{"}
OPENERS = set("([{")


class Scanner:
    def __init__(self, text: str, is_rust: bool) -> None:
        self.text = text
        self.is_rust = is_rust
        self.depth = {"(": 0, "[": 0, "{": 0}
        self.line = 1
        self.problems: list[str] = []
        self.stack: list[tuple[str, int]] = []

    def fail(self, message: str) -> None:
        self.problems.append(f"line {self.line}: {message}")

    def run(self) -> None:
        i = 0
        n = len(self.text)
        while i < n:
            ch = self.text[i]
            nxt = self.text[i + 1] if i + 1 < n else ""

            if ch == "\n":
                self.line += 1
                i += 1
                continue

            if ch == "/" and nxt == "/":
                while i < n and self.text[i] != "\n":
                    i += 1
                continue

            if ch == "/" and nxt == "*":
                i += 2
                while i < n and not (self.text[i] == "*" and i + 1 < n and self.text[i + 1] == "/"):
                    if self.text[i] == "\n":
                        self.line += 1
                    i += 1
                i += 2
                continue

            if self.is_rust and ch == "r" and nxt in ('"', "#"):
                consumed = self._skip_rust_raw_string(i)
                if consumed:
                    i = consumed
                    continue

            if ch in ('"', "'"):
                if ch == "'" and self.is_rust and self._looks_like_rust_lifetime(i):
                    i += 1
                    continue
                if ch == "'" and not self.is_rust and self._looks_like_js_apostrophe(i):
                    i += 1
                    continue
                i = self._skip_quoted(i, ch)
                continue

            if ch == "/" and not self.is_rust and self._looks_like_regex_start(i):
                i = self._skip_regex(i)
                continue

            if ch == "`":
                i = self._skip_template(i)
                continue

            if ch in OPENERS:
                self.depth[ch] += 1
                self.stack.append((ch, self.line))
                i += 1
                continue

            if ch in PAIRS:
                opener = PAIRS[ch]
                self.depth[opener] -= 1
                if self.depth[opener] < 0:
                    self.fail(f"unexpected closing delimiter `{ch}`")
                    return
                for index in range(len(self.stack) - 1, -1, -1):
                    if self.stack[index][0] == opener:
                        del self.stack[index]
                        break
                i += 1
                continue

            i += 1

        for opener, opener_line in self.stack:
            self.fail(f"unclosed delimiter `{opener}` opened at line {opener_line}")

    def _skip_rust_raw_string(self, start: int) -> int:
        i = start + 1
        hashes = 0
        while i < len(self.text) and self.text[i] == "#":
            hashes += 1
            i += 1
        if i >= len(self.text) or self.text[i] != '"':
            return 0
        i += 1
        closing = '"' + "#" * hashes
        while i < len(self.text):
            if self.text[i] == "\n":
                self.line += 1
            if self.text.startswith(closing, i):
                return i + len(closing)
            i += 1
        self.fail("unterminated raw string")
        return len(self.text)

    def _looks_like_rust_lifetime(self, index: int) -> bool:
        """`'a` / `'_` / `'static` 是生命周期；`'a'` 是字符字面量。"""
        i = index + 1
        if i >= len(self.text) or not (self.text[i].isalpha() or self.text[i] == "_"):
            return False
        start = i
        while i < len(self.text) and (self.text[i].isalnum() or self.text[i] == "_"):
            i += 1
        identifier = self.text[start:i]
        if len(identifier) == 1 and i < len(self.text) and self.text[i] == "'":
            return False  # 单字符 + 收尾引号 = 字符字面量
        return True

    def _looks_like_regex_start(self, index: int) -> bool:
        """保守判定 `/` 是否为正则起始：看前一个有效字符/关键字。"""
        i = index - 1
        while i >= 0 and self.text[i] in " \t":
            i -= 1
        if i < 0:
            return True
        prev = self.text[i]
        # 只保留最不容易与 HTML 标签（`</div>`）、除号、泛型尖括号撞车的起始符
        if prev in "(,=:[!&|?{};":
            return True
        if prev.isalnum() or prev in "_$":
            word_end = i + 1
            word_start = i
            while word_start >= 0 and (self.text[word_start].isalnum() or self.text[word_start] in "_$"):
                word_start -= 1
            word = self.text[word_start + 1 : word_end]
            return word in {"return", "typeof", "instanceof", "in", "of", "case", "delete", "void", "new"}
        return False

    def _skip_regex(self, start: int) -> int:
        i = start + 1
        in_class = False
        while i < len(self.text):
            ch = self.text[i]
            if ch == "\\":
                i += 2
                continue
            if ch == "\n":
                self.line += 1
                self.fail("unterminated regex literal")
                return i
            if ch == "[":
                in_class = True
            elif ch == "]":
                in_class = False
            elif ch == "/" and not in_class:
                i += 1
                while i < len(self.text) and self.text[i].isalpha():
                    i += 1
                return i
            i += 1
        self.fail("unterminated regex literal")
        return len(self.text)

    def _looks_like_js_apostrophe(self, index: int) -> bool:
        # 英文注释/文案里的撇号（don't、's）不应被当成字符串起始
        prev = self.text[index - 1] if index > 0 else ""
        return prev.isalnum()

    def _skip_quoted(self, start: int, quote: str) -> int:
        i = start + 1
        while i < len(self.text):
            ch = self.text[i]
            if ch == "\\":
                i += 2
                continue
            if ch == "\n":
                self.line += 1
                if quote == "'":
                    self.fail("unterminated character literal")
                    return i
            if ch == quote:
                return i + 1
            i += 1
        self.fail("unterminated string literal")
        return len(self.text)

    def _skip_template(self, start: int) -> int:
        i = start + 1
        while i < len(self.text):
            ch = self.text[i]
            if ch == "\\":
                i += 2
                continue
            if ch == "\n":
                self.line += 1
                i += 1
                continue
            if ch == "`":
                return i + 1
            if ch == "$" and i + 1 < len(self.text) and self.text[i + 1] == "{":
                i = self._skip_template_interpolation(i + 1)
                continue
            i += 1
        self.fail("unterminated template literal")
        return len(self.text)

    def _skip_template_interpolation(self, brace_index: int) -> int:
        """从 `${` 的 `{` 扫到配对的 `}`，期间正确处理嵌套字符串/模板/注释。"""
        depth = 0
        i = brace_index
        while i < len(self.text):
            ch = self.text[i]
            nxt = self.text[i + 1] if i + 1 < len(self.text) else ""
            if ch == "\n":
                self.line += 1
                i += 1
                continue
            if ch == "/" and nxt == "/":
                while i < len(self.text) and self.text[i] != "\n":
                    i += 1
                continue
            if ch == "/" and nxt == "*":
                i += 2
                while i < len(self.text) and not (
                    self.text[i] == "*" and i + 1 < len(self.text) and self.text[i + 1] == "/"
                ):
                    if self.text[i] == "\n":
                        self.line += 1
                    i += 1
                i += 2
                continue
            if ch in ('"', "'"):
                i = self._skip_quoted(i, ch)
                continue
            if ch == "`":
                i = self._skip_template(i)
                continue
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return i + 1
            i += 1
        self.fail("unterminated template interpolation")
        return len(self.text)


def iter_files(targets: list[str], changed_only: bool) -> list[Path]:
    if changed_only:
        output = subprocess.run(
            ["git", "status", "--porcelain"],
            capture_output=True,
            text=True,
            check=False,
        ).stdout
        files: list[Path] = []
        for raw in output.splitlines():
            entry = raw[3:].strip().strip('"')
            if " -> " in entry:
                entry = entry.split(" -> ", 1)[1]
            path = Path(entry)
            if path.suffix in EXTS and path.exists():
                files.append(path)
        return files

    files = []
    for target in targets:
        root = Path(target)
        if root.is_file():
            files.append(root)
            continue
        for path in root.rglob("*"):
            if not path.is_file() or path.suffix not in EXTS:
                continue
            if any(part in SKIP_DIRS for part in path.parts):
                continue
            files.append(path)
    return files


def main() -> int:
    parser = argparse.ArgumentParser(description="括号配平自检")
    parser.add_argument("targets", nargs="*", default=["crates", "frontend/src", "web"])
    parser.add_argument("--changed", action="store_true", help="只检查 git 改动/新增的文件")
    parser.add_argument("--quiet", action="store_true", help="只输出有问题的文件")
    args = parser.parse_args()

    files = iter_files(args.targets or ["crates", "frontend/src", "web"], args.changed)
    bad = 0
    for path in files:
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError as error:
            print(f"[skip] {path}: {error}")
            continue
        scanner = Scanner(text, is_rust=path.suffix == ".rs")
        scanner.run()
        if scanner.problems:
            bad += 1
            print(f"[BAD] {path}")
            for problem in scanner.problems[:6]:
                print(f"      {problem}")

    if not args.quiet:
        print(f"checked {len(files)} files, {bad} unbalanced")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
