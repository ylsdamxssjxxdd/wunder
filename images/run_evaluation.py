#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
评估脚本：读取用例集，调用 /wunder 接口并输出结构化报告。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional

import httpx
import yaml


@dataclass
class CaseResult:
    """单条用例评估结果，用于汇总报告。"""

    case_id: str
    ok: bool
    stream: bool
    status_code: int
    duration_ms: float
    answer: str
    session_id: str
    errors: List[str]
    event_types: List[str]


def parse_args() -> argparse.Namespace:
    """解析命令行参数，支持指定用例文件、目标地址与输出路径。"""
    parser = argparse.ArgumentParser(description="运行 wunder 评估用例并输出报告")
    parser.add_argument(
        "--cases",
        default="tests/evaluation/cases/basic.yaml",
        help="评估用例 YAML 路径",
    )
    parser.add_argument(
        "--base-url",
        default="http://127.0.0.1:8000/wunder",
        help="目标接口地址（默认 /wunder）",
    )
    parser.add_argument(
        "--output",
        default="",
        help="评估报告输出路径（默认写入 data/eval_reports）",
    )
    return parser.parse_args()


def _read_yaml(path: Path) -> Dict[str, Any]:
    """读取 YAML 文件并返回字典结构。"""
    if not path.exists():
        raise FileNotFoundError(f"用例文件不存在：{path}")
    data = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("用例文件格式错误：根节点必须是字典")
    return data


def _merge_dicts(base: Dict[str, Any], override: Dict[str, Any]) -> Dict[str, Any]:
    """浅递归合并字典，优先使用 override 中的配置。"""
    merged: Dict[str, Any] = dict(base)
    for key, value in override.items():
        if isinstance(value, dict) and isinstance(merged.get(key), dict):
            merged[key] = _merge_dicts(merged[key], value)
        else:
            merged[key] = value
    return merged


def _parse_sse_events(text: str) -> List[Dict[str, Any]]:
    """解析 SSE 文本为事件列表，兼容多行 data。"""
    events: List[Dict[str, Any]] = []
    current: Dict[str, Any] = {}
    data_lines: List[str] = []

    for line in text.splitlines():
        if not line.strip():
            if data_lines:
                raw = "\n".join(data_lines)
                current["data"] = _decode_event_data(raw)
            if current:
                events.append(current)
            current = {}
            data_lines = []
            continue
        if line.startswith("event:"):
            current["event"] = line[len("event:") :].strip()
            continue
        if line.startswith("data:"):
            data_lines.append(line[len("data:") :].strip())
            continue

    if data_lines:
        raw = "\n".join(data_lines)
        current["data"] = _decode_event_data(raw)
    if current:
        events.append(current)
    return events


def _decode_event_data(raw: str) -> Any:
    """解析事件 data 字段，优先按 JSON 解析。"""
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return raw


def _extract_final_answer(events: Iterable[Dict[str, Any]]) -> tuple[str, str]:
    """从 SSE 事件中提取最终 answer 与 session_id。"""
    answer = ""
    session_id = ""
    for event in events:
        if event.get("event") != "final":
            continue
        payload = event.get("data", {})
        if isinstance(payload, dict):
            session_id = str(payload.get("session_id", session_id))
            data = payload.get("data", {})
            if isinstance(data, dict):
                answer = str(data.get("answer", answer))
        break
    return answer, session_id


def _build_user_id(prefix: str, case_id: str) -> str:
    """构造用于评估的 user_id，避免与真实用户冲突。"""
    suffix = uuid.uuid4().hex[:8]
    return f"{prefix}-{case_id}-{suffix}"


def _check_contains(answer: str, patterns: List[str], errors: List[str]) -> None:
    """检查答案包含指定子串。"""
    for item in patterns:
        if item not in answer:
            errors.append(f"答案缺少关键字：{item}")


def _check_regex(answer: str, patterns: List[str], errors: List[str]) -> None:
    """检查答案匹配正则表达式。"""
    for pattern in patterns:
        if not re.search(pattern, answer):
            errors.append(f"答案未匹配正则：{pattern}")


def _evaluate_case(
    client: httpx.Client,
    base_url: str,
    case: Dict[str, Any],
    defaults: Dict[str, Any],
) -> CaseResult:
    """执行单条用例并返回评估结果。"""
    case_id = str(case.get("id", "")).strip() or "unknown"
    expect = _merge_dicts(defaults.get("expect", {}), case.get("expect", {}))
    stream = bool(case.get("stream", defaults.get("stream", False)))
    timeout_s = float(case.get("timeout_s", defaults.get("timeout_s", 120)))

    user_id = str(case.get("user_id", "")).strip()
    if not user_id:
        user_id = _build_user_id(str(defaults.get("user_id_prefix", "eval")), case_id)

    payload: Dict[str, Any] = {
        "user_id": user_id,
        "question": str(case.get("question", "")).strip(),
        "stream": stream,
    }
    payload.update(case.get("payload", {}) if isinstance(case.get("payload"), dict) else {})

    errors: List[str] = []
    event_types: List[str] = []
    answer = ""
    session_id = ""

    start = time.perf_counter()
    try:
        response = client.post(base_url, json=payload, timeout=timeout_s)
    except Exception as exc:  # noqa: BLE001
        duration_ms = (time.perf_counter() - start) * 1000
        return CaseResult(
            case_id=case_id,
            ok=False,
            stream=stream,
            status_code=0,
            duration_ms=duration_ms,
            answer="",
            session_id="",
            errors=[f"请求失败：{exc}"],
            event_types=[],
        )
    duration_ms = (time.perf_counter() - start) * 1000

    status_code = int(response.status_code)
    expected_status = int(expect.get("status_code", 200))
    if status_code != expected_status:
        errors.append(f"状态码不匹配：实际 {status_code}，期望 {expected_status}")

    if stream:
        events = _parse_sse_events(response.text)
        event_types = [event.get("event") for event in events if event.get("event")]
        required_events = expect.get("required_events") or []
        missing = [item for item in required_events if item not in event_types]
        if missing:
            errors.append(f"SSE 缺少事件：{', '.join(missing)}")
        answer, session_id = _extract_final_answer(events)
    else:
        try:
            payload_json = response.json()
        except Exception as exc:  # noqa: BLE001
            errors.append(f"JSON 解析失败：{exc}")
            payload_json = {}
        if isinstance(payload_json, dict):
            answer = str(payload_json.get("answer", "") or "")
            session_id = str(payload_json.get("session_id", "") or "")

    min_answer_len = int(expect.get("min_answer_length", 0))
    if min_answer_len > 0 and len(answer.strip()) < min_answer_len:
        errors.append("答案长度不足")

    if expect.get("require_session_id", False) and not session_id:
        errors.append("缺少 session_id")

    if isinstance(expect.get("answer_contains"), list):
        _check_contains(answer, expect.get("answer_contains"), errors)
    if isinstance(expect.get("answer_regex"), list):
        _check_regex(answer, expect.get("answer_regex"), errors)

    return CaseResult(
        case_id=case_id,
        ok=not errors,
        stream=stream,
        status_code=status_code,
        duration_ms=round(duration_ms, 2),
        answer=answer,
        session_id=session_id,
        errors=errors,
        event_types=event_types,
    )


def _build_report(results: List[CaseResult]) -> Dict[str, Any]:
    """汇总用例结果，生成可落盘的报告结构。"""
    total = len(results)
    passed = sum(1 for item in results if item.ok)
    avg_ms = round(
        sum(item.duration_ms for item in results) / total, 2
    ) if total else 0.0
    return {
        "summary": {
            "total": total,
            "passed": passed,
            "failed": total - passed,
            "pass_rate": round(passed / total, 4) if total else 0.0,
            "avg_duration_ms": avg_ms,
        },
        "cases": [
            {
                "id": item.case_id,
                "ok": item.ok,
                "stream": item.stream,
                "status_code": item.status_code,
                "duration_ms": item.duration_ms,
                "session_id": item.session_id,
                "answer": item.answer,
                "event_types": item.event_types,
                "errors": item.errors,
            }
            for item in results
        ],
    }


def _resolve_output_path(output: str) -> Path:
    """生成评估报告的输出路径，默认落到 data/eval_reports。"""
    if output:
        return Path(output).expanduser().resolve()
    root = Path(__file__).resolve().parents[1]
    target_dir = root / "data" / "eval_reports"
    target_dir.mkdir(parents=True, exist_ok=True)
    timestamp = time.strftime("%Y%m%d_%H%M%S")
    return target_dir / f"eval_report_{timestamp}.json"


def main() -> int:
    """主入口：加载用例、逐条执行并输出报告。"""
    args = parse_args()
    try:
        suite = _read_yaml(Path(args.cases))
    except Exception as exc:  # noqa: BLE001
        print(f"读取用例失败：{exc}", file=sys.stderr)
        return 1

    defaults = suite.get("defaults", {}) if isinstance(suite.get("defaults"), dict) else {}
    cases = suite.get("cases", [])
    if not isinstance(cases, list) or not cases:
        print("未找到有效用例列表", file=sys.stderr)
        return 1

    results: List[CaseResult] = []
    with httpx.Client() as client:
        for case in cases:
            if not isinstance(case, dict):
                continue
            result = _evaluate_case(client, args.base_url, case, defaults)
            results.append(result)

    report = _build_report(results)
    output_path = _resolve_output_path(args.output)
    output_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")

    summary = report.get("summary", {})
    print(
        "评估完成：总数 {total}，通过 {passed}，失败 {failed}，通过率 {pass_rate}".format(
            **summary
        )
    )
    print(f"报告已写入：{output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
