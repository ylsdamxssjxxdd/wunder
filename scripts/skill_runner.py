#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
技能执行入口：加载技能脚本中的 run(payload) 并输出 JSON 结果。
"""

from __future__ import annotations

import asyncio
import importlib.util
import json
import sys
from pathlib import Path


def _load_module_from_path(path: Path):
    """从指定路径加载模块，避免污染全局导入环境。"""
    spec = importlib.util.spec_from_file_location(path.stem, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"无法加载技能模块: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


async def _run_skill(entry: Path, payload: dict) -> object:
    """执行技能 run 函数，兼容同步/异步实现。"""
    module = _load_module_from_path(entry)
    run_func = getattr(module, "run", None)
    if run_func is None:
        raise AttributeError("技能入口缺少 run 函数")
    if asyncio.iscoroutinefunction(run_func):
        return await run_func(payload)
    return await asyncio.to_thread(run_func, payload)


def main() -> int:
    """主入口：读取输入 JSON 并输出执行结果。"""
    if len(sys.argv) < 2:
        print("缺少技能入口路径", file=sys.stderr)
        return 2
    entry = Path(sys.argv[1]).resolve()
    if not entry.exists():
        print(f"技能入口不存在: {entry}", file=sys.stderr)
        return 2
    raw = sys.stdin.read().strip()
    payload = {}
    if raw:
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError as exc:
            print(f"输入 JSON 解析失败: {exc}", file=sys.stderr)
            return 3
    try:
        result = asyncio.run(_run_skill(entry, payload))
    except Exception as exc:  # noqa: BLE001
        print(f"技能执行失败: {exc}", file=sys.stderr)
        return 4
    print(json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
