#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Initialize a minimal research workspace tree.

Usage:
    python init_research_tree.py <output_dir>
"""

from __future__ import annotations

import sys
from pathlib import Path


FILES = {
    "00-研究计划.md": "# 研究计划\n\n## 研究题目\n\n## 核心问题\n\n## 资料来源计划\n\n## 预期产物\n",
    "01-资料盘点.md": "# 资料盘点\n\n## 已发现资料\n\n## 来源分层\n\n## 明显缺口\n",
    "02-证据台账.md": "# 证据台账\n\n## 问题 1\n\n### 证据\n\n### 初步判断\n\n### 缺口\n",
    "03-分析归纳.md": "# 分析归纳\n\n## 关键事实\n\n## 核心矛盾\n\n## 趋势或阶段\n\n## 不确定性\n",
    "最终研究报告.md": "# 深度研究报告\n\n## 摘要\n",
}


def main() -> int:
    if len(sys.argv) != 2:
        print("用法: python init_research_tree.py <output_dir>")
        return 1

    root = Path(sys.argv[1]).expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)

    for name, content in FILES.items():
        path = root / name
        if not path.exists():
            path.write_text(content, encoding="utf-8")

    print(str(root))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
