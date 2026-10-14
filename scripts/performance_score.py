#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Calculate the 蜂巢/蛹/舵机 performance scorecard.

The collector is intentionally separate from product-specific profilers.  A
profiler, Playwright run, desktop harness, or CLI harness writes the small JSON
contract accepted here; this script turns those observations into a comparable
score and a reviewable report.

Examples:
  python scripts/performance_score.py --shape 蜂巢 --template --output target/beehive-template.json
  python scripts/performance_score.py --input target/beehive-run.json \
      --baseline target/beehive-previous/performance-score.json \
      --out-dir target/beehive-score --require-complete
"""

from __future__ import annotations

import argparse
import csv
import json
import math
import platform
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence


SCRIPT_VERSION = "1"


@dataclass(frozen=True)
class MetricSpec:
    key: str
    label: str
    unit: str
    target: float
    boundary: float
    direction: str  # lower or higher


@dataclass(frozen=True)
class GateSpec:
    key: str
    label: str
    kind: str = "true"  # true, zero, one


@dataclass(frozen=True)
class ScenarioSpec:
    key: str
    name: str
    weight: float
    metrics: tuple[MetricSpec, ...]
    gates: tuple[GateSpec, ...]


def lower(key: str, label: str, unit: str, target: float, limit: float) -> MetricSpec:
    return MetricSpec(key, label, unit, target, limit, "lower")


def higher(key: str, label: str, unit: str, target: float, floor: float) -> MetricSpec:
    return MetricSpec(key, label, unit, target, floor, "higher")


def gate(key: str, label: str, kind: str = "true") -> GateSpec:
    return GateSpec(key, label, kind)


SCORECARDS: dict[str, tuple[ScenarioSpec, ...]] = {
    # Backend capacity is scored independently from browser interaction.
    "server": (
        ScenarioSpec("S1", "大库日常启动", 40, (lower("storage_startup_p95_ms", "存储就绪 p95", "ms", 2000, 5000), lower("capacity_added_p95_ms", "容量放大附加耗时", "ms", 100, 500)), (gate("index_oids_stable", "已有索引保持"), gate("no_history_scan", "启动不扫描历史"), gate("no_row_rewrite", "启动不重写数据"))),
        ScenarioSpec("S2", "大库空闲驻留", 25, (lower("server_idle_cpu_percent", "舰体空闲 CPU", "percent", 1, 2), lower("postgres_idle_cpu_percent", "数据库空闲 CPU", "percent", 1, 2)), (gate("idle_window_valid", "无请求空闲窗口有效"),)),
        ScenarioSpec("S3", "历史重建预算", 20, (lower("maintenance_duty_ratio", "维护工作时间占比", "ratio", 0.01, 0.02),), (gate("discovery_page_bounded", "发现每批不超过 64 线程"), gate("pending_rotates", "待迁移线程轮转"), gate("durable_resume", "游标重启续跑"))),
        ScenarioSpec("S4", "未完成投影的大线程窗口", 15, (lower("chat_window_p95_ms", "聊天窗口 p95", "ms", 500, 1500), lower("trajectory_window_p95_ms", "轨迹窗口 p95", "ms", 500, 1500)), (gate("opens_without_projection", "不等待历史投影即可打开"), gate("seek_pages_disjoint", "游标分页不重复"), gate("response_bytes_bounded", "响应体有界"), gate("read_does_not_write", "展示读取不写投影"), gate("body_integrity", "原始正文分块完整"))),
    ),
    "beehive": (
        ScenarioSpec("H1", "冷启动到工作台可交互", 8, (lower("tti_p95_ms", "TTI p95", "ms", 1200, 2000),), (gate("no_uncaught_errors", "无未捕获错误"),)),
        ScenarioSpec("H2", "线程目录打开", 7, (lower("first_action_p95_ms", "首个可操作线程", "ms", 600, 1200), lower("duplicate_request_rate_ratio", "接口重复率", "ratio", 0.02, 0.05)), (gate("virtualized_list", "列表已虚拟化"),)),
        ScenarioSpec("H3", "长线程首个正文", 12, (lower("first_paint_p95_ms", "首个正文 paint", "ms", 800, 1500),), (gate("real_snapshot_ws_path", "使用真实快照/WS入口"),)),
        ScenarioSpec("H4", "长线程窗口补水", 8, (lower("complete_window_p95_ms", "完整窗口可读", "ms", 2500, 5000), lower("response_compressed_mb", "压缩响应体积", "mb", 1, 2)), (gate("verified_empty_state", "空态来自权威读取"),)),
        ScenarioSpec("H5", "流式首个增量", 14, (lower("first_delta_p95_ms", "首个增量可见", "ms", 500, 900),), (gate("stream_integrity", "流式内容完整"),)),
        ScenarioSpec("H6", "流式期间交互", 12, (lower("input_paint_p95_ms", "输入到绘制", "ms", 100, 200), lower("frame_p95_ms", "帧 p95", "ms", 33, 50)), (gate("input_not_blocked", "输出期间输入不阻塞"),)),
        ScenarioSpec("H7", "长轨迹滚动", 8, (lower("scroll_frame_p95_ms", "滚动帧 p95", "ms", 33, 50), lower("long_task_ratio", "长任务占比", "ratio", 0.01, 0.03)), (gate("scroll_position_stable", "滚动位置稳定"),)),
        ScenarioSpec("H8", "工作区文件操作", 8, (lower("file_list_p95_ms", "文件列表", "ms", 500, 1000), lower("file_op_p95_ms", "单次文件操作", "ms", 500, 1200)), (gate("permission_errors_visible", "权限错误可见"),)),
        ScenarioSpec("H9", "断线恢复", 8, (lower("recovery_p95_ms", "恢复到可读", "ms", 1500, 3000),), (gate("event_integrity_ratio", "事件完整性", "one"),)),
        ScenarioSpec("H10", "多用户活动流", 10, (higher("success_rate", "成功率", "ratio", 0.995, 0.98), lower("api_p95_ms", "接口 p95", "ms", 500, 1000)), (gate("bounded_queue", "队列有界"),)),
        ScenarioSpec("H11", "空闲驻留", 5, (lower("heap_growth_ratio", "heap 增长", "ratio", 0.05, 0.10),), (gate("no_crash", "无崩溃"),)),
    ),
    "desktop": (
        ScenarioSpec("C1", "冷启动首帧", 8, (lower("first_frame_p95_ms", "首帧", "ms", 350, 700),), (gate("no_heavy_work_before_frame", "首帧前无重任务"),)),
        ScenarioSpec("C2", "冷启动可交互", 8, (lower("interactive_p95_ms", "可交互", "ms", 1200, 2000),), (gate("no_ui_blocking", "无 UI 线程阻塞"),)),
        ScenarioSpec("C3", "长线程打开", 12, (lower("first_paint_p95_ms", "首个正文", "ms", 300, 600), lower("complete_window_p95_ms", "完整窗口", "ms", 1500, 3000)), (gate("history_complete", "历史不丢段"),)),
        ScenarioSpec("C4", "SQLite 线程目录", 10, (lower("query_projection_p95_ms", "查询/投影", "ms", 150, 300),), (gate("bounded_query", "查询有界"),)),
        ScenarioSpec("C5", "NativeDesktop 流式首显", 14, (lower("first_delta_p95_ms", "首个增量", "ms", 300, 600),), (gate("bounded_stream_channel", "通道有界且不丢字"),)),
        ScenarioSpec("C6", "流式期间交互", 14, (lower("input_response_p95_ms", "输入响应", "ms", 100, 200), lower("frame_p95_ms", "帧 p95", "ms", 33, 50)), (gate("ui_thread_free", "UI 线程不做重解析"),)),
        ScenarioSpec("C7", "线程切换", 8, (lower("thread_switch_p95_ms", "切换完成", "ms", 150, 300),), (gate("thread_state_isolated", "线程状态隔离"),)),
        ScenarioSpec("C8", "本地文件操作", 8, (lower("file_list_p95_ms", "文件列表", "ms", 500, 1000), lower("file_op_p95_ms", "文件操作", "ms", 500, 1200)), (gate("path_boundary_enforced", "路径边界有效"),)),
        ScenarioSpec("C9", "离线与恢复", 6, (lower("offline_ready_p95_ms", "离线可用", "ms", 1200, 2000), lower("recovery_p95_ms", "恢复连接", "ms", 2000, 4000)), (gate("offline_history_available", "离线历史可用"),)),
        ScenarioSpec("C10", "空闲与持续流驻留", 7, (lower("idle_cpu_percent", "空闲 CPU", "percent", 1, 2), lower("working_set_delta_mb", "工作集增量", "mb", 10, 20)), (gate("no_timer_leak", "无定时器泄漏"),)),
        ScenarioSpec("C11", "取消轮次", 5, (lower("cancel_persist_p95_ms", "取消持久化", "ms", 500, 1000),), (gate("no_write_after_cancel", "取消后不继续写入"),)),
    ),
    "cli": (
        ScenarioSpec("D1", "帮助或版本命令", 5, (lower("first_line_p95_ms", "首行输出", "ms", 300, 600),), (gate("exit_code_zero", "退出码正确"),)),
        ScenarioSpec("D2", "配置和运行时冷加载", 8, (lower("ready_p95_ms", "命令就绪", "ms", 800, 1500),), (gate("config_error_visible", "配置错误可见"),)),
        ScenarioSpec("D3", "线程列表分页", 8, (lower("first_page_p95_ms", "首页", "ms", 300, 600), higher("page_throughput", "后续页吞吐", "pages/s", 50, 20)), (gate("paged_output", "输出分页"),)),
        ScenarioSpec("D4", "长线程首屏", 10, (lower("first_flush_p95_ms", "首屏 flush", "ms", 1000, 2000),), (gate("output_bounded_cancelable", "输出有界可取消"),)),
        ScenarioSpec("D5", "消息到首个运行事件", 15, (lower("first_event_p95_ms", "首个运行事件", "ms", 500, 1000),), (gate("execution_observed", "已执行而非仅受理"),)),
        ScenarioSpec("D6", "流式终端输出", 12, (lower("flush_p95_ms", "事件到 flush", "ms", 100, 200), lower("cpu_peak_percent", "CPU 峰值", "percent", 25, 50)), (gate("stream_integrity", "输出完整"),)),
        ScenarioSpec("D7", "工具调用额外开销", 10, (lower("cli_overhead_p95_ms", "CLI 额外开销", "ms", 200, 400),), (gate("tool_errors_visible", "工具错误可见"),)),
        ScenarioSpec("D8", "并行任务调度", 10, (lower("scheduling_overhead_p95_ms", "调度额外延迟", "ms", 200, 500),), (gate("parallel_output_isolated", "输出按任务隔离"),)),
        ScenarioSpec("D9", "断网恢复待处理任务", 7, (lower("recovery_p95_ms", "恢复", "ms", 1000, 2000),), (gate("no_duplicate_execution", "不重复执行"),)),
        ScenarioSpec("D10", "长时间运行", 10, (lower("working_set_growth_ratio", "工作集增长", "ratio", 0.10, 0.20), lower("cpu_average_percent", "CPU 平均", "percent", 2, 4)), (gate("no_leaked_tasks", "无未回收任务"),)),
        ScenarioSpec("D11", "退出状态 flush", 5, (lower("exit_flush_p95_ms", "退出 flush", "ms", 500, 1000),), (gate("restart_state_complete", "重启后状态完整"),)),
    ),
}


SHAPE_ALIASES = {
    "server": "server",
    "舰体": "server",
    "beehive": "beehive",
    "web": "beehive",
    "cloud": "beehive",
    "蜂巢": "beehive",
    "desktop": "desktop",
    "local": "desktop",
    "pupa": "desktop",
    "蛹": "desktop",
    "cli": "cli",
    "terminal": "cli",
    "舵机": "cli",
}
SHAPE_LABELS = {"beehive": "蜂巢", "desktop": "蛹", "cli": "舵机", "server": "舰体"}


def shape_key(raw: str) -> str:
    key = SHAPE_ALIASES.get(str(raw).strip().lower())
    if key is None:
        raise ValueError(f"未知形态 {raw!r}，可选：蜂巢、蛹、舵机、舰体")
    return key


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def json_number(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)) and math.isfinite(float(value)):
        return float(value)
    if isinstance(value, Mapping) and "value" in value:
        return json_number(value["value"])
    return None


def read_json(path: Path) -> dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"无法读取 JSON：{path}：{exc}") from exc
    if not isinstance(data, dict):
        raise ValueError(f"输入必须是 JSON 对象：{path}")
    return data


def read_csv(path: Path, shape: str | None) -> dict[str, Any]:
    """Read the stable row contract emitted by profilers.

    Required columns are ``scenario`` and either ``metric``/``value`` or
    ``gate``/``passed``.  Extra columns are retained only as metadata by the
    producer; this keeps the scoring input small and deterministic.
    """
    scenarios: dict[str, dict[str, dict[str, Any]]] = {}
    try:
        with path.open("r", encoding="utf-8-sig", newline="") as handle:
            rows = csv.DictReader(handle)
            if not rows.fieldnames:
                raise ValueError("CSV 缺少表头")
            for index, row in enumerate(rows, start=2):
                scenario = str(row.get("scenario") or row.get("场景") or "").strip()
                if not scenario:
                    raise ValueError(f"CSV 第 {index} 行缺少 scenario")
                bucket = scenarios.setdefault(scenario, {"metrics": {}, "gates": {}})
                metric = str(row.get("metric") or row.get("指标") or "").strip()
                gate_name = str(row.get("gate") or row.get("门禁") or "").strip()
                if metric:
                    raw = row.get("value") if "value" in row else row.get("值")
                    if raw is None or str(raw).strip() == "":
                        bucket["metrics"][metric] = None
                    else:
                        try:
                            bucket["metrics"][metric] = float(raw)
                        except ValueError as exc:
                            raise ValueError(f"CSV 第 {index} 行指标值不是数字：{raw!r}") from exc
                elif gate_name:
                    raw = row.get("passed") if "passed" in row else row.get("通过")
                    if raw is None or str(raw).strip() == "":
                        bucket["gates"][gate_name] = None
                    else:
                        bucket["gates"][gate_name] = str(raw).strip().lower() in {"1", "true", "yes", "pass", "passed", "ok", "通过"}
                else:
                    raise ValueError(f"CSV 第 {index} 行必须填写 metric 或 gate")
    except OSError as exc:
        raise ValueError(f"无法读取 CSV：{path}：{exc}") from exc
    return {"shape": shape or "", "scenarios": scenarios}


def read_input(path: Path, shape: str | None) -> dict[str, Any]:
    if path.suffix.lower() == ".csv":
        return read_csv(path, shape)
    return read_json(path)


def scenario_payload(data: Mapping[str, Any], scenario: str) -> Mapping[str, Any]:
    scenarios = data.get("scenarios", {})
    if not isinstance(scenarios, Mapping):
        return {}
    payload = scenarios.get(scenario, {})
    return payload if isinstance(payload, Mapping) else {}


def metric_values(payload: Mapping[str, Any]) -> Mapping[str, Any]:
    metrics = payload.get("metrics")
    if isinstance(metrics, Mapping):
        return metrics
    return payload


def gate_values(payload: Mapping[str, Any]) -> Mapping[str, Any]:
    gates = payload.get("gates")
    if isinstance(gates, Mapping):
        return gates
    if isinstance(gates, Sequence) and not isinstance(gates, (str, bytes)):
        result: dict[str, Any] = {}
        for item in gates:
            if isinstance(item, Mapping) and item.get("name"):
                result[str(item["name"])] = item
        return result
    return {}


def score_metric(value: float, spec: MetricSpec) -> float:
    if spec.direction == "lower":
        if spec.boundary == spec.target:
            return 100.0 if value <= spec.target else 0.0
        raw = 100.0 * (spec.boundary - value) / (spec.boundary - spec.target)
    else:
        if spec.boundary == spec.target:
            return 100.0 if value >= spec.target else 0.0
        raw = 100.0 * (value - spec.boundary) / (spec.target - spec.boundary)
    return max(0.0, min(100.0, raw))


def gate_passed(value: Any, spec: GateSpec) -> bool | None:
    if isinstance(value, Mapping) and "passed" in value:
        value = value["passed"]
    if value is None:
        return None
    # A collector may report a correctness gate as either a boolean or a
    # numeric ratio (for example event_integrity_ratio = 1). Accept both
    # representations so JSON and CSV producers share the same contract.
    if value is True:
        return True
    if value is False:
        return False
    if spec.kind == "true":
        return value is True or (isinstance(value, str) and value.strip().lower() in {"true", "pass", "passed", "ok"})
    number = json_number(value)
    if number is None:
        return False
    if spec.kind == "zero":
        return number == 0
    if spec.kind == "one":
        return number >= 1.0
    return False


def template(shape: str) -> dict[str, Any]:
    key = shape_key(shape)
    scenarios: dict[str, Any] = {}
    for spec in SCORECARDS[key]:
        scenarios[spec.key] = {
            "metrics": {metric.key: None for metric in spec.metrics},
            "gates": {item.key: None for item in spec.gates},
        }
    return {
        "schema_version": SCRIPT_VERSION,
        "shape": SHAPE_LABELS[key],
        "version": "",
        "environment": {"os": platform.platform(), "notes": ""},
        "scenarios": scenarios,
    }


def evaluate(data: Mapping[str, Any], requested_shape: str | None = None) -> dict[str, Any]:
    raw_shape = requested_shape or str(data.get("shape") or data.get("形态") or "")
    key = shape_key(raw_shape)
    specs = SCORECARDS[key]
    scenario_results: list[dict[str, Any]] = []
    measured_weight = 0.0
    weighted_score = 0.0
    hard_failures: list[str] = []

    for spec in specs:
        payload = scenario_payload(data, spec.key)
        values = metric_values(payload)
        gates = gate_values(payload)
        metric_results: list[dict[str, Any]] = []
        missing = False
        metric_scores: list[float] = []
        for metric in spec.metrics:
            value = json_number(values.get(metric.key))
            if value is None:
                missing = True
                metric_results.append({"key": metric.key, "label": metric.label, "value": None, "score": None, "unit": metric.unit, "status": "missing"})
                continue
            score = score_metric(value, metric)
            metric_scores.append(score)
            metric_results.append({"key": metric.key, "label": metric.label, "value": value, "score": round(score, 2), "unit": metric.unit, "status": "measured"})

        gate_results: list[dict[str, Any]] = []
        gate_failed = False
        for item in spec.gates:
            passed = gate_passed(gates.get(item.key), item)
            if passed is None:
                missing = True
            elif not passed:
                gate_failed = True
                hard_failures.append(f"{spec.key}.{item.key}: {item.label}")
            gate_results.append({"key": item.key, "label": item.label, "passed": passed, "status": "missing" if passed is None else ("passed" if passed else "failed")})

        complete = not missing
        score = round(sum(metric_scores) / len(metric_scores), 2) if complete and metric_scores else None
        if score is not None:
            measured_weight += spec.weight
            weighted_score += spec.weight * score
        status = "missing" if missing else ("failed" if gate_failed else "measured")
        scenario_results.append({
            "key": spec.key,
            "name": spec.name,
            "weight": spec.weight,
            "score": score,
            "weighted_contribution": round(spec.weight * score / 100.0, 2) if score is not None else None,
            "status": status,
            "metrics": metric_results,
            "gates": gate_results,
        })

    expected_weight = sum(item.weight for item in specs)
    # A partial run must never look like a perfect score. Keep the measured
    # scenario scores and coverage, but publish a total only after all weights
    # are present.
    total = round(weighted_score / expected_weight, 2) if measured_weight == expected_weight else None
    missing_scenarios = [item["key"] for item in scenario_results if item["score"] is None]
    return {
        "schema_version": SCRIPT_VERSION,
        "benchmark": "wunder-performance-score",
        "shape": SHAPE_LABELS[key],
        "shape_key": key,
        "version": data.get("version", ""),
        "generated_at": utc_now(),
        "score": total,
        "coverage_percent": round(measured_weight / expected_weight * 100.0, 2),
        "measured_weight": measured_weight,
        "expected_weight": expected_weight,
        "missing_scenarios": missing_scenarios,
        "hard_failures": hard_failures,
        "scenario_results": scenario_results,
        "environment": data.get("environment", {}),
    }


def baseline_scores(data: Mapping[str, Any], shape: str) -> dict[str, float]:
    if isinstance(data.get("scenario_results"), list):
        return {str(item.get("key")): float(item["score"]) for item in data["scenario_results"] if isinstance(item, Mapping) and item.get("score") is not None}
    report = evaluate(data, shape)
    return {str(item["key"]): float(item["score"]) for item in report["scenario_results"] if item["score"] is not None}


def apply_baseline(report: dict[str, Any], baseline_data: Mapping[str, Any], shape: str) -> None:
    previous = baseline_scores(baseline_data, shape)
    regressions: list[dict[str, Any]] = []
    for item in report["scenario_results"]:
        if item["score"] is None or item["key"] not in previous:
            continue
        delta = round(float(item["score"]) - previous[item["key"]], 2)
        item["previous_score"] = round(previous[item["key"]], 2)
        item["delta"] = delta
        if delta < -10.0:
            regressions.append({"key": item["key"], "previous": item["previous_score"], "current": item["score"], "delta": delta})
    previous_total = baseline_data.get("score")
    if previous_total is None and isinstance(baseline_data.get("scenario_results"), list):
        weighted = [(float(item.get("weight", 0)), float(item["score"])) for item in baseline_data["scenario_results"] if item.get("score") is not None]
        previous_total = sum(weight * score for weight, score in weighted) / sum(weight for weight, _ in weighted) if weighted else None
    report["previous_score"] = round(float(previous_total), 2) if previous_total is not None else None
    report["score_delta"] = round(report["score"] - float(previous_total), 2) if report["score"] is not None and previous_total is not None else None
    if report["score_delta"] is not None and report["score_delta"] < -3.0:
        regressions.append({"key": "total", "previous": report["previous_score"], "current": report["score"], "delta": report["score_delta"]})
    report["regressions"] = regressions


def format_value(value: Any, unit: str) -> str:
    if value is None:
        return "待测"
    if unit == "ratio":
        return f"{float(value) * 100:.2f}%"
    if unit == "percent":
        return f"{float(value):.2f}%"
    return f"{float(value):.2f} {unit}"


def status_for(report: Mapping[str, Any], require_complete: bool) -> str:
    if report.get("hard_failures"):
        return "不通过"
    if report.get("regressions"):
        return "回退"
    if require_complete and report.get("missing_scenarios"):
        return "待测"
    if report.get("missing_scenarios"):
        return "部分完成"
    return "通过"


def markdown(report: Mapping[str, Any], status: str) -> str:
    lines = [f"# {report['shape']} 性能评分", "", f"- 状态：{status}", f"- 总分：{report.get('score') if report.get('score') is not None else '待测'}", f"- 覆盖率：{report.get('coverage_percent', 0):.2f}%", f"- 版本：{report.get('version') or '未填写'}"]
    if report.get("previous_score") is not None:
        lines.append(f"- 上一版本：{report['previous_score']:.2f}（变化 {report.get('score_delta', 0):+.2f}）")
    lines += ["", "## 场景分数", "", "| 场景 | 权重 | 本次 | 上次 | 变化 | 状态 |", "| --- | ---: | ---: | ---: | ---: | --- |"]
    for item in report["scenario_results"]:
        current = "待测" if item["score"] is None else f"{item['score']:.2f}"
        previous = "" if item.get("previous_score") is None else f"{item['previous_score']:.2f}"
        delta = "" if item.get("delta") is None else f"{item['delta']:+.2f}"
        lines.append(f"| {item['key']} {item['name']} | {item['weight']:.0f}% | {current} | {previous} | {delta} | {item['status']} |")
    lines += ["", "## 指标明细", ""]
    for item in report["scenario_results"]:
        lines += [f"### {item['key']} {item['name']}", "", "| 指标 | 值 | 分数 | 状态 |", "| --- | ---: | ---: | --- |"]
        for metric in item["metrics"]:
            score = "" if metric["score"] is None else f"{metric['score']:.2f}"
            lines.append(f"| {metric['label']} | {format_value(metric['value'], metric['unit'])} | {score} | {metric['status']} |")
        for gate_item in item["gates"]:
            gate_status = "待测" if gate_item["passed"] is None else ("通过" if gate_item["passed"] else "失败")
            lines.append(f"| 门禁：{gate_item['label']} |  |  | {gate_status} |")
        lines.append("")
    if report.get("missing_scenarios"):
        lines += ["## 待测场景", "", "- " + "、".join(report["missing_scenarios"]), ""]
    if report.get("hard_failures"):
        lines += ["## 硬门禁失败", "", *[f"- {item}" for item in report["hard_failures"]], ""]
    if report.get("regressions"):
        lines += ["## 性能回退", "", *[f"- {item['key']}：{item['previous']:.2f} → {item['current']:.2f}（{item['delta']:+.2f}）" for item in report["regressions"]], ""]
    return "\n".join(lines).rstrip() + "\n"


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="按性能评分体系计算工作台、舵机或舰体容量评分。")
    parser.add_argument("--shape", choices=sorted(set(SHAPE_ALIASES)), help="形态：蜂巢、蛹、舵机、舰体，也可用 web/desktop/cli/server")
    parser.add_argument("--input", type=Path, help="实测 JSON 输入")
    parser.add_argument("--baseline", type=Path, help="上一版本原始输入或 performance-score.json")
    parser.add_argument("--out-dir", type=Path, default=Path("target/performance-score"), help="报告输出目录")
    parser.add_argument("--template", action="store_true", help="生成指定形态的 JSON 输入模板")
    parser.add_argument("--output", type=Path, help="模板输出路径；未指定时写到标准输出")
    parser.add_argument("--require-complete", action="store_true", help="缺少场景或门禁时返回失败")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    if args.template:
        if not args.shape:
            raise SystemExit("--template 必须同时指定 --shape")
        payload = json.dumps(template(args.shape), ensure_ascii=False, indent=2) + "\n"
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(payload, encoding="utf-8")
            print(f"已写入模板：{args.output}")
        else:
            print(payload, end="")
        return 0
    if not args.input:
        raise SystemExit("评分时必须指定 --input；需要输入模板时使用 --template")
    data = read_input(args.input, args.shape)
    report = evaluate(data, args.shape)
    if args.baseline:
        baseline_data = read_input(args.baseline, report["shape_key"])
        apply_baseline(report, baseline_data, report["shape_key"])
    status = status_for(report, args.require_complete)
    report["status"] = status
    args.out_dir.mkdir(parents=True, exist_ok=True)
    json_path = args.out_dir / "performance-score.json"
    md_path = args.out_dir / "performance-score.md"
    json_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    md_path.write_text(markdown(report, status), encoding="utf-8")
    print(f"形态：{report['shape']}；状态：{status}；总分：{report['score'] if report['score'] is not None else '待测'}；覆盖率：{report['coverage_percent']:.2f}%")
    print(f"JSON：{json_path}")
    print(f"Markdown：{md_path}")
    if report["hard_failures"] or report.get("regressions") or (args.require_complete and report["missing_scenarios"]):
        return 1
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as exc:
        print(f"错误：{exc}", file=sys.stderr)
        raise SystemExit(2)
