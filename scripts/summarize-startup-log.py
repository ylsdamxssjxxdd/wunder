#!/usr/bin/env python3
"""把 <home>/logs/startup.log 的最后一次启动摘要成两张表（UI 阶段 / 引擎阶段）。

用法:
    python3 scripts/summarize-startup-log.py ~/.wunder/logs/startup.log
    python3 scripts/summarize-startup-log.py ~/.wunder/logs/startup.log --runs 3

判读要点:
    * 「本段」是这一步自己花的毫秒数，看启动慢只需要看这一列。
    * UI 阶段的累计从进程入口算；引擎阶段的累计从运行时初始化算，两者不同源。
    * 老机器上先看引擎阶段：ensure_runtime_dirs（资产释放）、config_store_update、
      app_state_init（数据库/技能/服务装配）、ensure_desktop_identity。
    * 再看 UI 阶段：font_decompressed（内嵌字体解压）、main_window_new、
      first_frame_presented（首帧）、sessions_ready（线程列表可用）。
"""
from __future__ import annotations

import argparse
import re
import signal
import sys

STAGE = re.compile(r"\bstage=([\w:.\-]+)")
SEGMENT = re.compile(r"\bsegment=([\w:.\-]+)")
POINT = re.compile(r"\bpoint=([\w:.\-]+)")
MS = re.compile(r"(?:elapsed_ms|stage_ms|total_ms)=([0-9.]+)")
RUN_START = {"process_begin", "runtime_init_begin"}


def classify(raw: str):
    """返回 (kind, name, source)。stage/segment 自带本段耗时，point 只有累计。"""
    if "desktop-ui" in raw:
        source = "ui"
    elif "bridge-runtime" in raw:
        source = "engine"
    else:
        source = "other"
    match = STAGE.search(raw)
    if match:
        return "stage", match.group(1), source
    match = SEGMENT.search(raw)
    if match:
        return "segment", match.group(1), source
    match = POINT.search(raw)
    if match:
        return "point", match.group(1), source
    return None, None, source


def parse(path: str):
    runs = []
    last_total = {}
    with open(path, encoding="utf-8", errors="replace") as handle:
        for raw in handle:
            kind, name, source = classify(raw)
            if not name:
                continue
            values = MS.findall(raw)
            if not values:
                continue
            elapsed = float(values[0]) if len(values) > 1 else 0.0
            total = float(values[-1])
            if name in RUN_START and runs and runs[-1]:
                # 同一次启动里 UI 先记 process_begin，引擎随后记 runtime_init_begin；
                # 只有再次出现 process_begin（或空文件）才算新的一次启动。
                if name == "runtime_init_begin" and any(
                    entry[1] == "process_begin" for entry in runs[-1]
                ):
                    pass
                else:
                    runs.append([])
            if not runs:
                runs.append([])
            if name == "process_begin" or len(runs[-1]) == 0:
                last_total = {}
            if kind == "point":
                # 里程碑只有累计值：用同一来源的相邻点求差，得到它自己花的时间。
                previous = last_total.get(source)
                if previous is not None and total >= previous:
                    elapsed = total - previous
                last_total[source] = total
            runs[-1].append((kind, name, elapsed, total, source))
    return runs


def section(title: str, entries, unit: str) -> None:
    if not entries:
        return
    total = max(entry[3] for entry in entries)
    print(f"\n-- {title}（共 {len(entries)} 个点，走到 {total:.0f} ms）--")
    print(f"{'名称':<36}{'本段 ms':>10}{'累计 ms':>10}  类别")
    for kind, name, elapsed, point_total, _ in entries:
        print(f"{name:<36}{elapsed:>10.1f}{point_total:>10.1f}  {kind}")
    ranked = sorted(
        (entry for entry in entries if entry[2] > 0), key=lambda entry: entry[2], reverse=True
    )
    if ranked:
        print("\n  按本段耗时排序（%s）:" % unit)
        for kind, name, elapsed, _, _ in ranked[:8]:
            print(f"  {elapsed:>9.1f} ms  {name}  ({kind})")


def show_run(index: int, entries) -> None:
    ui = [entry for entry in entries if entry[4] == "ui"]
    engine = [entry for entry in entries if entry[4] == "engine"]
    print(f"\n===== 第 {index} 次启动 =====")
    section("引擎阶段（UI 等它才装配工作台）", engine, "引擎")
    section("UI 阶段（进程入口到可交互）", ui, "UI")
    if not ui and not engine:
        print("未识别到启动记录")


def main() -> int:
    try:
        signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    except (AttributeError, ValueError):
        pass
    parser = argparse.ArgumentParser()
    parser.add_argument("log", help="startup.log 路径")
    parser.add_argument("--runs", type=int, default=1, help="输出最近几次启动，默认 1")
    args = parser.parse_args()
    runs = [run for run in parse(args.log) if run]
    if not runs:
        print("没有找到启动记录", file=sys.stderr)
        return 1
    selected = runs[-max(1, args.runs) :]
    offset = len(runs) - len(selected)
    for index, entries in enumerate(selected, start=offset + 1):
        show_run(index, entries)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
