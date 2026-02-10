#!/usr/bin/env python3
"""Run standard backend_sim scenarios to build baseline and compare drift."""

from __future__ import annotations

import argparse
import json
import subprocess
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, List


@dataclass(frozen=True)
class Scenario:
    name: str
    requests: int
    concurrency: int
    stream: bool
    session_mode: str
    description: str
    max_error_rate_drift: float | None = None
    max_p95_latency_drift_ms: float | None = None
    max_final_miss_rate_drift: float | None = None
    p95_drift_ratio: float | None = None


STANDARD_SCENARIOS: List[Scenario] = [
    Scenario(
        name="stream_high_concurrency",
        requests=240,
        concurrency=48,
        stream=True,
        session_mode="unique",
        description="stream path under high concurrency with unique sessions",
        p95_drift_ratio=0.35,
    ),
    Scenario(
        name="stream_shared_session",
        requests=180,
        concurrency=24,
        stream=True,
        session_mode="shared",
        description="stream path under lock contention with shared session",
        max_error_rate_drift=0.12,
        max_p95_latency_drift_ms=900.0,
        max_final_miss_rate_drift=0.12,
        p95_drift_ratio=1.00,
    ),
    Scenario(
        name="run_non_stream",
        requests=200,
        concurrency=32,
        stream=False,
        session_mode="unique",
        description="non-stream run path control scenario",
        p95_drift_ratio=0.35,
    ),
]

QUICK_SCENARIOS: List[Scenario] = [
    Scenario(
        name="stream_high_concurrency",
        requests=40,
        concurrency=8,
        stream=True,
        session_mode="unique",
        description="quick stream unique-session smoke",
        p95_drift_ratio=0.35,
    ),
    Scenario(
        name="stream_shared_session",
        requests=30,
        concurrency=6,
        stream=True,
        session_mode="shared",
        description="quick stream shared-session smoke",
        max_error_rate_drift=0.20,
        max_p95_latency_drift_ms=700.0,
        max_final_miss_rate_drift=0.20,
        p95_drift_ratio=1.00,
    ),
    Scenario(
        name="run_non_stream",
        requests=36,
        concurrency=6,
        stream=False,
        session_mode="unique",
        description="quick non-stream smoke",
        p95_drift_ratio=0.35,
    ),
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run standard backend_sim scenarios for baseline/compare.",
    )
    parser.add_argument(
        "mode",
        choices=["baseline", "compare"],
        help="baseline writes new baseline files; compare checks drift against baseline files.",
    )
    parser.add_argument(
        "--quick",
        action="store_true",
        help="run quick scenarios for local smoke instead of standard workloads.",
    )
    parser.add_argument(
        "--base-dir",
        default="temp_dir/backend_sim",
        help="directory to store reports and baseline files.",
    )
    parser.add_argument(
        "--cargo-bin",
        default="cargo",
        help="cargo executable name/path.",
    )
    parser.add_argument(
        "--question",
        default="Please summarize this backend in one sentence.",
        help="question payload used in each scenario.",
    )
    parser.add_argument(
        "--max-error-rate-drift",
        type=float,
        default=0.02,
        help="default allowed increase of error rate in compare mode.",
    )
    parser.add_argument(
        "--max-p95-latency-drift-ms",
        type=float,
        default=200.0,
        help="default allowed increase of p95 latency in milliseconds in compare mode.",
    )
    parser.add_argument(
        "--max-final-miss-rate-drift",
        type=float,
        default=0.01,
        help="default allowed increase of final-event-miss rate in compare mode.",
    )
    parser.add_argument(
        "--p95-drift-ratio",
        type=float,
        default=0.25,
        help="default allowed p95 increase ratio relative to baseline p95 in compare mode.",
    )
    return parser.parse_args()


def read_baseline_signature(baseline_path: Path) -> dict[str, Any]:
    if not baseline_path.exists():
        return {}
    try:
        payload = json.loads(baseline_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}
    signature = payload.get("signature")
    if isinstance(signature, dict):
        return signature
    return {}


def resolve_thresholds(
    scenario: Scenario,
    args: argparse.Namespace,
    mode: str,
    baseline_path: Path,
) -> dict[str, float | None]:
    error_rate = (
        scenario.max_error_rate_drift
        if scenario.max_error_rate_drift is not None
        else args.max_error_rate_drift
    )
    final_miss = (
        scenario.max_final_miss_rate_drift
        if scenario.max_final_miss_rate_drift is not None
        else args.max_final_miss_rate_drift
    )
    static_p95_ms = (
        scenario.max_p95_latency_drift_ms
        if scenario.max_p95_latency_drift_ms is not None
        else args.max_p95_latency_drift_ms
    )
    ratio = scenario.p95_drift_ratio if scenario.p95_drift_ratio is not None else args.p95_drift_ratio

    baseline_p95_ms: float | None = None
    ratio_p95_ms = 0.0
    effective_p95_ms = static_p95_ms

    if mode == "compare":
        signature = read_baseline_signature(baseline_path)
        raw_baseline_p95 = signature.get("p95_latency_ms")
        if isinstance(raw_baseline_p95, (int, float)) and raw_baseline_p95 > 0:
            baseline_p95_ms = float(raw_baseline_p95)
            ratio_p95_ms = baseline_p95_ms * max(ratio, 0.0)
            effective_p95_ms = max(static_p95_ms, ratio_p95_ms)

    return {
        "max_error_rate_drift": error_rate,
        "max_final_miss_rate_drift": final_miss,
        "max_p95_latency_drift_ms": effective_p95_ms,
        "static_p95_latency_drift_ms": static_p95_ms,
        "ratio_p95_latency_drift_ms": ratio_p95_ms,
        "baseline_p95_latency_ms": baseline_p95_ms,
        "p95_drift_ratio": ratio,
    }


def build_command(
    cargo_bin: str,
    scenario: Scenario,
    report_path: Path,
    baseline_path: Path,
    mode: str,
    args: argparse.Namespace,
    thresholds: dict[str, float | None],
) -> List[str]:
    cmd = [
        cargo_bin,
        "run",
        "--bin",
        "backend_sim",
        "--",
        "--requests",
        str(scenario.requests),
        "--concurrency",
        str(scenario.concurrency),
        "--stream",
        "true" if scenario.stream else "false",
        "--session-mode",
        scenario.session_mode,
        "--question",
        args.question,
        "--baseline",
        str(baseline_path),
        "--report",
        str(report_path),
    ]

    if mode == "baseline":
        cmd.append("--write-baseline")
    else:
        cmd.extend(
            [
                "--fail-on-drift",
                "--max-error-rate-drift",
                str(thresholds["max_error_rate_drift"]),
                "--max-p95-latency-drift-ms",
                str(thresholds["max_p95_latency_drift_ms"]),
                "--max-final-miss-rate-drift",
                str(thresholds["max_final_miss_rate_drift"]),
            ]
        )

    return cmd


def load_report_metrics(report_path: Path) -> dict[str, Any]:
    if not report_path.exists():
        return {}
    try:
        payload = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}

    summary = payload.get("summary", {})
    latency = payload.get("latency_ms", {})
    first_event = payload.get("first_event_latency_ms", {}) or {}

    return {
        "success_rate": summary.get("success_rate"),
        "error_rate": summary.get("error_rate"),
        "p95_ms": latency.get("p95"),
        "p99_ms": latency.get("p99"),
        "first_event_p95_ms": first_event.get("p95"),
        "final_event_missing_rate": summary.get("final_event_missing_rate"),
        "throughput_rps": summary.get("throughput_rps"),
        "drift": payload.get("drift"),
    }


def format_percent(value: object) -> str:
    if isinstance(value, (int, float)):
        return f"{value * 100:.2f}%"
    return "n/a"


def format_ms(value: object) -> str:
    if isinstance(value, (int, float)):
        return f"{value:.2f}ms"
    return "n/a"


def format_rps(value: object) -> str:
    if isinstance(value, (int, float)):
        return f"{value:.2f}"
    return "n/a"


def write_summary(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(summary, ensure_ascii=False, indent=2)
    path.write_text(f"{text}\n", encoding="utf-8")


def log(message: str) -> None:
    print(message, flush=True)


def main() -> int:
    args = parse_args()
    repo_root = Path(__file__).resolve().parents[1]
    base_dir = (repo_root / args.base_dir).resolve()
    baseline_dir = base_dir / "baselines"
    report_dir = base_dir / "reports"
    scenario_list = QUICK_SCENARIOS if args.quick else STANDARD_SCENARIOS

    baseline_dir.mkdir(parents=True, exist_ok=True)
    report_dir.mkdir(parents=True, exist_ok=True)

    log(f"[backend_sim_workflow] mode={args.mode} quick={args.quick} scenarios={len(scenario_list)}")

    results = []
    overall_success = True

    for scenario in scenario_list:
        report_path = report_dir / f"{scenario.name}.{args.mode}.json"
        baseline_path = baseline_dir / f"{scenario.name}.json"
        thresholds = resolve_thresholds(scenario, args, args.mode, baseline_path)

        cmd = build_command(
            args.cargo_bin,
            scenario,
            report_path,
            baseline_path,
            args.mode,
            args,
            thresholds,
        )

        log("")
        log(f"[backend_sim_workflow] scenario={scenario.name}")
        log(f"[backend_sim_workflow] desc={scenario.description}")
        log(
            "[backend_sim_workflow] thresholds="
            f"error={thresholds['max_error_rate_drift']:.4f} "
            f"p95_effective={thresholds['max_p95_latency_drift_ms']:.2f}ms "
            f"p95_static={thresholds['static_p95_latency_drift_ms']:.2f}ms "
            f"p95_ratio={thresholds['p95_drift_ratio']:.3f} "
            f"p95_ratio_abs={thresholds['ratio_p95_latency_drift_ms']:.2f}ms "
            f"final_miss={thresholds['max_final_miss_rate_drift']:.4f}"
        )
        log(f"[backend_sim_workflow] cmd={' '.join(cmd)}")

        completed = subprocess.run(cmd, cwd=repo_root)
        succeeded = completed.returncode == 0
        overall_success = overall_success and succeeded

        metrics = load_report_metrics(report_path)
        result = {
            "scenario": scenario.name,
            "description": scenario.description,
            "succeeded": succeeded,
            "return_code": completed.returncode,
            "report": str(report_path),
            "baseline": str(baseline_path),
            "thresholds": thresholds,
            "metrics": metrics,
        }
        results.append(result)

        log(
            "[backend_sim_workflow] result="
            f"{'PASS' if succeeded else 'FAIL'} "
            f"success_rate={format_percent(metrics.get('success_rate'))} "
            f"error_rate={format_percent(metrics.get('error_rate'))} "
            f"p95={format_ms(metrics.get('p95_ms'))} "
            f"p99={format_ms(metrics.get('p99_ms'))} "
            f"first_event_p95={format_ms(metrics.get('first_event_p95_ms'))} "
            f"rps={format_rps(metrics.get('throughput_rps'))}"
        )

        if args.mode == "compare":
            drift = metrics.get("drift") or {}
            if drift:
                log(
                    "[backend_sim_workflow] drift="
                    f"{'PASS' if drift.get('passed') else 'FAIL'} "
                    f"violations={len(drift.get('violations', []))}"
                )

    summary = {
        "generated_at": datetime.utcnow().isoformat() + "Z",
        "mode": args.mode,
        "quick": args.quick,
        "base_dir": str(base_dir),
        "overall_success": overall_success,
        "results": results,
    }

    summary_path = base_dir / (
        f"summary.{args.mode}.{datetime.utcnow().strftime('%Y%m%dT%H%M%SZ')}.json"
    )
    write_summary(summary_path, summary)
    log("")
    log(f"[backend_sim_workflow] summary={summary_path}")

    return 0 if overall_success else 1


if __name__ == "__main__":
    raise SystemExit(main())

