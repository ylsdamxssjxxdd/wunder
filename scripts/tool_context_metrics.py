#!/usr/bin/env python3
"""Summarise model-facing tool-result context cost from an exported JSONL log.

The report deliberately contains only aggregates.  It never copies session ids,
arguments, paths, URLs, result text, or error messages from the input log.  The
same fixture can therefore be used for local regression tests and CI checks.

This is a lightweight budget meter, not a provider tokenizer.  Token estimates
use the runtime's conservative ``UTF-8 bytes / 4`` density, matching
``wunder-core::token_utils::approx_token_count``.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from collections import OrderedDict
from pathlib import Path
from typing import Any, Iterable, Mapping


REPORT_VERSION = 1
BYTES_PER_TOKEN = 4

# Browser sub-actions are one model-visible surface.  The aliases below are
# intentionally generic and do not expose arguments or values from the log.
TOOL_ALIASES = {
    # Goal management was formerly exposed as separate tool names.  The
    # current model surface is one `goal(action=...)` tool, so a historical
    # export must aggregate these calls instead of making the before/after
    # report look like three unrelated tools.
    "目标": "goal",
    "goal": "goal",
    "get_goal": "goal",
    "create_goal": "goal",
    "update_goal": "goal",
    "计划面板": "update_plan",
    "问询面板": "question_panel",
    "ask_panel": "question_panel",
    "记忆管理": "memory_manager",
    "memory_manage": "memory_manager",
    "子智能体控制": "subagent_control",
    "会话线程控制": "thread_control",
    "session_thread": "thread_control",
    "桌面控制": "desktop_control",
    "desktop": "desktop_control",
    "自我状态": "self_status",
    "自身状态": "self_status",
    "浏览器": "browser",
    "browser": "browser",
    "读取文件": "read_file",
    "读文件": "read_file",
    "read_file": "read_file",
    "读取": "read_file",
    "搜索内容": "search_content",
    "search_content": "search_content",
    "列出文件": "list_files",
    "list_files": "list_files",
    "文件名匹配": "glob",
    "glob": "glob",
    "执行命令": "execute_command",
    "execute_command": "execute_command",
    "命令会话": "command_session",
    "command_session": "command_session",
    "write_command_stdin": "command_session",
    "定时任务": "schedule_task",
    "schedule_task": "schedule_task",
    "技能调用": "skill_call",
    "skill_call": "skill_call",
    "skill_get": "skill_call",
    "skill_manifest": "skill_call",
    "skill_read": "skill_call",
    "写入文件": "write_file",
    "write_file": "write_file",
    "文本编辑": "edit",
    "edit": "edit",
    "编辑": "edit",
    "应用补丁": "edit",
    "apply_patch": "edit",
    "patch": "edit",
    "edit_file": "edit",
    "str_replace_editor": "edit",
    "读图工具": "read_image",
    "view_image": "read_image",
    "网页搜索": "web_search",
    "网页抓取": "web_fetch",
    "语音生成": "generate_speech",
    "声转文": "transcribe_speech",
    "图像生成": "generate_image",
    "绘图生成": "generate_image",
    "视频生成": "generate_video",
}


def _json_compact(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)


def _token_count(text: str) -> int:
    return math.ceil(len(text.encode("utf-8")) / BYTES_PER_TOKEN) if text else 0


def _value_size(value: Any) -> tuple[int, int, int]:
    text = _json_compact(value)
    return len(text), len(text.encode("utf-8")), _token_count(text)


def _first_string(mapping: Mapping[str, Any], keys: Iterable[str]) -> str | None:
    for key in keys:
        value = mapping.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return None


def canonical_tool_name(payload: Mapping[str, Any]) -> str:
    raw = _first_string(payload, ("tool_function_name", "tool_runtime_name", "tool", "tool_name"))
    if raw is None:
        return "unknown"
    # Collapse all browser sub-actions even when an old export has no compact
    # model catalog entry.  Keep unknown names bounded and control-free.
    if raw.startswith("browser_"):
        return "browser"
    normalized = TOOL_ALIASES.get(raw, raw)
    return "".join(ch for ch in normalized if ch.isprintable())[:96] or "unknown"


def _parse_observation(value: Any) -> tuple[Mapping[str, Any] | None, bool]:
    if isinstance(value, Mapping):
        return value, False
    if not isinstance(value, str) or not value.strip():
        return None, False
    try:
        parsed = json.loads(value)
    except (TypeError, ValueError):
        return None, True
    return (parsed, False) if isinstance(parsed, Mapping) else (None, True)


def _is_truncated(value: Any) -> bool:
    if not isinstance(value, Mapping):
        return False
    if value.get("truncated") is True:
        return True
    data = value.get("data")
    return isinstance(data, Mapping) and data.get("truncated") is True


def _empty_bucket(name: str) -> dict[str, Any]:
    return {
        "tool": name,
        "calls": 0,
        "observed": 0,
        "errors": 0,
        "truncated": 0,
        "raw_chars": 0,
        "raw_bytes": 0,
        "raw_tokens": 0,
        "raw_observed_tokens": 0,
        "observation_chars": 0,
        "observation_bytes": 0,
        "observation_tokens": 0,
        "observation_data_tokens": 0,
        "observation_parse_errors": 0,
        "status_mismatches": 0,
    }


def _finish_bucket(bucket: dict[str, Any]) -> dict[str, Any]:
    calls = bucket["calls"]
    observed = bucket["observed"]
    raw_tokens = bucket["raw_observed_tokens"]
    observation_tokens = bucket["observation_data_tokens"]
    bucket["error_rate"] = bucket["errors"] / calls if calls else 0.0
    bucket["truncation_rate"] = bucket["truncated"] / calls if calls else 0.0
    bucket["observation_coverage"] = observed / calls if calls else 0.0
    bucket["token_savings"] = raw_tokens - observation_tokens
    bucket["token_reduction_ratio"] = (
        bucket["token_savings"] / raw_tokens if raw_tokens else 0.0
    )
    bucket["status_mismatch_rate"] = bucket["status_mismatches"] / observed if observed else 0.0
    return bucket


def analyze_records(records: Iterable[Mapping[str, Any]]) -> dict[str, Any]:
    """Aggregate tool-result records without retaining their contents."""
    buckets: OrderedDict[str, dict[str, Any]] = OrderedDict()
    input_records = 0
    skipped_records = 0

    for record in records:
        if not isinstance(record, Mapping):
            skipped_records += 1
            continue
        # Exports place the event under item.payload.  Accept a bare payload as
        # well so unit tests and small fixtures need no export boilerplate.
        item = record.get("item")
        payload = item.get("payload") if isinstance(item, Mapping) else record
        if not isinstance(payload, Mapping):
            continue
        event_type = payload.get("event_type")
        kind = payload.get("kind")
        # Exported result records historically used kind=tool_call.  An
        # explicit event_type is authoritative; only use kind as a fallback
        # for bare/legacy result payloads.
        if event_type not in (None, "tool_result"):
            continue
        if event_type is None and kind not in (None, "tool_result", "tool_call"):
            continue
        if "data" not in payload and "model_observation" not in payload:
            continue
        input_records += 1
        name = canonical_tool_name(payload)
        bucket = buckets.setdefault(name, _empty_bucket(name))
        bucket["calls"] += 1

        raw_data = payload.get("data", {})
        chars, byte_count, tokens = _value_size(raw_data)
        bucket["raw_chars"] += chars
        bucket["raw_bytes"] += byte_count
        bucket["raw_tokens"] += tokens
        if payload.get("ok") is False:
            bucket["errors"] += 1

        observation, parse_error = _parse_observation(payload.get("model_observation"))
        if parse_error:
            bucket["observation_parse_errors"] += 1
        meta = payload.get("meta")
        if _is_truncated(payload) or _is_truncated(meta) or _is_truncated(observation):
            bucket["truncated"] += 1
        if observation is None:
            continue
        bucket["observed"] += 1
        bucket["raw_observed_tokens"] += tokens
        obs_chars, obs_bytes, obs_tokens = _value_size(observation)
        bucket["observation_chars"] += obs_chars
        bucket["observation_bytes"] += obs_bytes
        bucket["observation_tokens"] += obs_tokens
        _, _, obs_data_tokens = _value_size(observation.get("data", {}))
        bucket["observation_data_tokens"] += obs_data_tokens
        if isinstance(observation.get("ok"), bool) and isinstance(payload.get("ok"), bool):
            if observation["ok"] != payload["ok"]:
                bucket["status_mismatches"] += 1

    finished = [_finish_bucket(bucket) for bucket in buckets.values()]
    totals = _empty_bucket("__all__")
    for bucket in finished:
        for key in (
            "calls", "observed", "errors", "truncated", "raw_chars", "raw_bytes",
            "raw_tokens", "raw_observed_tokens", "observation_chars", "observation_bytes", "observation_tokens", "observation_data_tokens",
            "observation_parse_errors", "status_mismatches",
        ):
            totals[key] += bucket[key]
    totals = _finish_bucket(totals)
    totals.pop("tool", None)
    return {
        "report_version": REPORT_VERSION,
        "meter": {"method": "utf8_bytes_per_4_tokens", "bytes_per_token": BYTES_PER_TOKEN},
        "input_records": input_records,
        "skipped_records": skipped_records,
        "totals": totals,
        "tools": finished,
    }


def read_jsonl(path: Path) -> list[Mapping[str, Any]]:
    records: list[Mapping[str, Any]] = []
    with path.open("r", encoding="utf-8-sig") as stream:
        for line_number, line in enumerate(stream, start=1):
            if not line.strip():
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError as exc:
                raise ValueError(f"JSONL 第 {line_number} 行无法解析") from exc
            if isinstance(value, Mapping):
                records.append(value)
    return records


def render_markdown(report: Mapping[str, Any]) -> str:
    totals = report["totals"]
    lines = [
        "# 工具上下文收益量化报告",
        "",
        "仅包含聚合指标；token 为 UTF-8 字节数除以 4 的近似值。",
        "",
        f"- 工具结果：{totals['calls']} 条；模型观察覆盖：{totals['observed'] / totals['calls']:.1%}" if totals["calls"] else "- 工具结果：0 条",
        f"- 错误率：{totals['error_rate']:.2%}；截断率：{totals['truncation_rate']:.2%}",
        f"- 原始 data：{totals['raw_tokens']} token；已观测样本原始/观察 data：{totals['raw_observed_tokens']}/{totals['observation_data_tokens']} token；data 节省：{totals['token_reduction_ratio']:.2%}",
        f"- 已观测样本的完整模型结果信封：{totals['observation_tokens']} token；该值包含状态和恢复字段，data 节省率不代表整个请求的节省率。",
        "",
        "| 工具 | 调用 | 观察覆盖 | 错误率 | 截断率 | 已观测原始 data token | 观察 data token | data token 节省 | 状态不一致 |",
    ]
    lines.append("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for bucket in report["tools"]:
        observed_tokens = bucket["observation_data_tokens"] if bucket["observed"] else "—"
        savings = f"{bucket['token_reduction_ratio']:.2%}" if bucket["observed"] else "—"
        lines.append(
            f"| {bucket['tool']} | {bucket['calls']} | {bucket['observation_coverage']:.1%} | "
            f"{bucket['error_rate']:.2%} | {bucket['truncation_rate']:.2%} | "
            f"{bucket['raw_observed_tokens']} | {observed_tokens} | {savings} | "
            f"{bucket['status_mismatches']} |"
        )
    return "\n".join(lines) + "\n"


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="统计工具结果的上下文收益与错误率。")
    parser.add_argument("--input", type=Path, required=True, help="工具事件 JSONL")
    parser.add_argument("--output", type=Path, help="JSON 报告路径")
    parser.add_argument("--markdown", type=Path, help="Markdown 报告路径")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        report = analyze_records(read_jsonl(args.input))
    except (OSError, ValueError) as exc:
        print(f"错误：{exc}", file=sys.stderr)
        return 2
    encoded = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    else:
        print(encoded, end="")
    if args.markdown:
        args.markdown.parent.mkdir(parents=True, exist_ok=True)
        args.markdown.write_text(render_markdown(report), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
