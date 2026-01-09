#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Run k6 performance tests and extract latency percentiles from the summary export.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run k6 load tests for Wunder.")
    parser.add_argument(
        "--profile",
        default="quick",
        help="k6 profile name: quick/load/spike/soak.",
    )
    parser.add_argument(
        "--base-url",
        default="http://127.0.0.1:8000/wunder",
        help="Base URL for the Wunder endpoint.",
    )
    parser.add_argument(
        "--api-key",
        default="",
        help="API key for X-API-Key header (falls back to WUNDER_API_KEY).",
    )
    parser.add_argument(
        "--stream",
        action="store_true",
        help="Enable streaming mode in the load test payload.",
    )
    parser.add_argument(
        "--question",
        default="",
        help="Override question payload.",
    )
    parser.add_argument(
        "--user-prefix",
        default="",
        help="Override user id prefix.",
    )
    parser.add_argument(
        "--user-id",
        default="",
        help="Force a fixed user id (only used in quick profile).",
    )
    parser.add_argument(
        "--summary-path",
        default="data/perf/k6_summary.json",
        help="Path to save the k6 summary JSON.",
    )
    parser.add_argument(
        "--k6-script",
        default="tests/performance/k6_wunder.js",
        help="Path to the k6 script.",
    )
    return parser.parse_args()


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def run_k6(args: argparse.Namespace, summary_path: Path) -> None:
    env = os.environ.copy()
    env["WUNDER_PROFILE"] = args.profile
    env["WUNDER_BASE_URL"] = args.base_url
    env["WUNDER_STREAM"] = "true" if args.stream else "false"
    if args.question:
        env["WUNDER_QUESTION"] = args.question
    if args.user_prefix:
        env["WUNDER_USER_PREFIX"] = args.user_prefix
    if args.user_id:
        env["WUNDER_USER_ID"] = args.user_id
    api_key = args.api_key or env.get("WUNDER_API_KEY", "")
    if api_key:
        env["WUNDER_API_KEY"] = api_key

    ensure_parent(summary_path)
    script_path = Path(args.k6_script)
    if not script_path.exists():
        raise FileNotFoundError(f"k6 script not found: {script_path}")

    cmd = [
        "k6",
        "run",
        "--summary-export",
        str(summary_path),
        "--summary-trend-stats",
        "avg,min,med,max,p(50),p(95),p(99)",
        str(script_path),
    ]
    subprocess.run(cmd, env=env, check=True)


def pick_value(values: dict, *keys: str) -> float | None:
    for key in keys:
        if key in values:
            return values[key]
    return None


def load_summary(summary_path: Path) -> dict:
    raw = summary_path.read_text(encoding="utf-8")
    return json.loads(raw)


def extract_values(metric: dict) -> dict:
    if not isinstance(metric, dict):
        return {}
    values = metric.get("values")
    if isinstance(values, dict):
        return values
    return metric


def print_metrics(summary: dict) -> dict:
    metrics = summary.get("metrics", {})
    duration = extract_values(metrics.get("http_req_duration", {}))
    failed = extract_values(metrics.get("http_req_failed", {}))
    requests = extract_values(metrics.get("http_reqs", {}))

    p50 = pick_value(duration, "p(50)", "med")
    p95 = pick_value(duration, "p(95)")
    p99 = pick_value(duration, "p(99)")
    avg = pick_value(duration, "avg")
    max_v = pick_value(duration, "max")
    fail_rate = pick_value(failed, "rate", "value")
    reqs = pick_value(requests, "count")

    output = {
        "http_req_duration": {
            "p50_ms": p50,
            "p95_ms": p95,
            "p99_ms": p99,
            "avg_ms": avg,
            "max_ms": max_v,
        },
        "http_req_failed": {"rate": fail_rate},
        "http_reqs": {"count": reqs},
    }

    def fmt(value: float | None) -> str:
        if value is None:
            return "n/a"
        return f"{value:.2f}"

    print("k6 summary:")
    print(f"  http_req_duration p50={fmt(p50)}ms p95={fmt(p95)}ms p99={fmt(p99)}ms")
    print(f"  http_req_duration avg={fmt(avg)}ms max={fmt(max_v)}ms")
    print(f"  http_req_failed rate={fmt(fail_rate)}")
    if reqs is not None:
        print(f"  http_reqs count={int(reqs)}")
    return output


def main() -> int:
    args = parse_args()
    summary_path = Path(args.summary_path)
    try:
        run_k6(args, summary_path)
    except (subprocess.CalledProcessError, FileNotFoundError) as exc:
        print(f"load test failed: {exc}", file=sys.stderr)
        return 1

    summary = load_summary(summary_path)
    metrics = print_metrics(summary)
    metrics_path = summary_path.with_suffix(".metrics.json")
    metrics_path.write_text(json.dumps(metrics, indent=2), encoding="utf-8")
    print(f"metrics written: {metrics_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
