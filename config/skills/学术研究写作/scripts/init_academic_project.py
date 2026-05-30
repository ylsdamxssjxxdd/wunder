#!/usr/bin/env python3
# -*- coding: utf-8 -*-
from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_ROOT = SCRIPT_DIR.parent

TEMPLATE_MAP = {
    "plan": "templates/01-研究计划.md",
    "review": "templates/02-文献综述.md",
    "report": "templates/03-研究报告.md",
    "empirical": "templates/04-实证论文.md",
    "theory": "templates/05-理论分析论文.md",
    "response": "templates/06-审稿意见回复.md",
    "proposal": "templates/07-开题报告.md",
    "peer-review": "templates/08-同行评审报告.md",
    "chart": "templates/12-图表工作表.md",
    "evidence-matrix": "templates/13-综述证据矩阵.md",
    "reference-log": "templates/14-参考文献整理表.md",
}

STARTER_FILES = {
    "notes/00-任务说明.md": """研究任务说明

## 文稿类型

## 当前目标

## 已有材料

## 不得超出的边界

## 交付要求
""",
    "notes/01-研究计划.md": """研究计划

## 研究主题

## 核心问题

## 资料范围

## 计划方法

## 风险与限制

## 预计产出
""",
    "notes/02-证据摘录.md": """证据摘录

## 来源一

### 摘录

### 可支撑的判断

### 暂时不能确定的部分
""",
    "notes/03-研究问题拆解.md": """研究问题拆解

## 原始题目

## 研究对象

## 核心问题

## 子问题

## 不研究什么

## 现有材料是否足以回答

## 暂定研究路径
""",
    "notes/04-文献矩阵.md": """文献矩阵

| 来源 | 年份 | 类型 | 主题一 | 主题二 | 主题三 | 主要结论 | 可用等级 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 待补充 | 待补充 | 待补充 |  |  |  |  |  |
""",
    "notes/05-证据评估卡.md": """证据评估卡

## 来源名称

## 来源类型

## 核心结论

## 证据等级

## 方法是否清楚

## 时效性

## 可能偏差

## 建议使用方式
""",
    "notes/06-图表清单.md": """图表清单

## 图表一

### 用途

### 类型

### 所需信息

### 是否需要用户补充

## 图表二

### 用途

### 类型

### 所需信息

### 是否需要用户补充
""",
    "notes/07-参考文献整理表.md": """参考文献整理表

| 引用标识 | 作者/机构 | 年份 | 标题 | 来源类型 | 载体信息 | 当前状态 | 备注 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [1] | 待补充 | 待补充 | 待补充 | 待补充 | 待补充 | 待核对 |  |
""",
    "output/README.md": """# 输出目录

将最终交付稿、DOCX、PDF 或其他导出文件放在这里。
""",
}


def create_file_if_missing(path: Path, content: str) -> None:
    if not path.exists():
        path.write_text(content, encoding="utf-8")


def resolve_template(template_key: str) -> Path:
    relative_path = TEMPLATE_MAP.get(template_key)
    if relative_path is None:
        valid = ", ".join(TEMPLATE_MAP)
        raise ValueError(f"未知模板键: {template_key}。可选值: {valid}")
    return SKILL_ROOT / relative_path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="初始化离线学术研究写作工作目录。",
    )
    parser.add_argument("output_dir", help="输出目录")
    parser.add_argument(
        "--template",
        default="report",
        choices=sorted(TEMPLATE_MAP),
        help="主稿模板类型，默认 report",
    )
    parser.add_argument(
        "--draft-name",
        default="主稿.md",
        help="主稿文件名，默认 主稿.md",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = Path(args.output_dir).expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)

    for directory in ("materials", "notes", "drafts", "output"):
        (root / directory).mkdir(parents=True, exist_ok=True)

    for relative_path, content in STARTER_FILES.items():
        create_file_if_missing(root / relative_path, content)

    template_path = resolve_template(args.template)
    if not template_path.is_file():
        print(f"未找到模板文件: {template_path}", file=sys.stderr)
        return 1

    draft_path = root / "drafts" / args.draft_name
    if not draft_path.exists():
        shutil.copyfile(template_path, draft_path)

    print(str(root))
    print(f"主稿模板: {template_path.name}")
    print(f"主稿文件: {draft_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
