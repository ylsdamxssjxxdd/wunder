#!/usr/bin/env python3
"""External, headless desktop backend experience suite. Python 3.8 + psutil."""
from __future__ import annotations

import argparse
import csv
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

SPEC = importlib.util.spec_from_file_location("chat_open_benchmark", Path(__file__).with_name("measure-chat-open.py"))
CHAT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHAT)
CASES = ("startup", "recovery", "submit", "stream", "slow_consumer", "cancel_resume",
         "queue", "switch", "background", "workspace", "soak")
PROFILES = {
    "smoke": dict(runs=1, warmups=0, chunks=8, interval_ms=2, switches=4, soak_seconds=3, files=600),
    "daily": dict(runs=10, warmups=1, chunks=32, interval_ms=8, switches=50, soak_seconds=60, files=2000),
    "release": dict(runs=30, warmups=2, chunks=32, interval_ms=8, switches=200, soak_seconds=300, files=10000),
    "soak": dict(runs=1, warmups=0, chunks=32, interval_ms=8, switches=50, soak_seconds=7200, files=2000),
}
REQUIRED = {
    "startup": {"startup_ready_ms"}, "recovery": {"recovery_ready_ms"},
    "queue": {"queue_accept_ms", "queue_reorder_ms", "queue_interject_ms", "queue_withdraw_ms", "queue_dispatch_complete_ms"},
    "switch": {"switch_cycle_ms"}, "background": {"switch_cycle_ms"},
    "workspace": {"workspace_page_ms", "workspace_refresh_ms"},
    "soak": {"soak_elapsed_ms", "soak_cycles", "cycle_delta_gap_p95_ms"},
}
STREAM_METRICS = {"accepted_ms", "submit_return_ms", "first_delta_ms", "delta_gap_ms",
                  "provider_request_ms", "first_source_delivery_ms", "settlement_after_source_ms", "pending_events_peak"}
for _case in ("submit", "stream", "slow_consumer", "recovery_seed", "cancel_resume"):
    REQUIRED[_case] = STREAM_METRICS | ({"cancel_settled_ms"} if _case == "cancel_resume" else set())


def unit(name):
    return "ms" if name.endswith("_ms") else {"pending_events_peak": "events", "soak_cycles": "cycles"}[name]


def validate(sample, case):
    check = sample.get("check", {})
    metrics = check.get("metrics", {})
    if (check.get("scope") != "backend" or check.get("model") != "loopback-fixed-v1"
            or not isinstance(check.get("assertions"), int) or check["assertions"] < 1
            or not isinstance(metrics, dict) or len(metrics) > 40
            or not REQUIRED[case].issubset(metrics)):
        raise RuntimeError("missing correctness evidence or scenario metrics")
    for name, values in metrics.items():
        unit(name)
        if not isinstance(values, list) or not 1 <= len(values) <= 10000:
            raise RuntimeError("invalid metric sample count")
        if any(isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v) or v < 0 for v in values):
            raise RuntimeError("invalid metric sample value")
    if not sample.get("resources", {}).get("available") or sample["resources"].get("sampling_truncated"):
        raise RuntimeError("resource sampling unavailable or exhausted")


def trends(series):
    """Post-warmup growth; diagnostic evidence, not a leak verdict."""
    if len(series) < 5:
        return {"available": False}
    end = series[-1]["since_start_ms"]
    rows = [r for r in series if r["since_start_ms"] >= end * .2]
    result = {"available": len(rows) >= 3, "duration_ms": end,
              "long_enough": end >= 60000, "excluded_fraction": .2}
    if len(rows) < 3:
        return result
    for key in ("rss_bytes", "private_bytes", "handles", "threads"):
        pairs = [(r["since_start_ms"] / 60000, r[key]) for r in rows if r.get(key) is not None]
        if len(pairs) < 3:
            continue
        center = sum(x for x, _ in pairs) / len(pairs)
        mean = sum(y for _, y in pairs) / len(pairs)
        denominator = sum((x-center)**2 for x, _ in pairs)
        slope = sum((x-center)*(y-mean) for x, y in pairs) / denominator if denominator else 0
        result[key] = {"per_minute": slope, "end_minus_start": pairs[-1][1]-pairs[0][1]}
    return result


def summarize(samples):
    summary = {}
    for case in dict.fromkeys(s["case"] for s in samples if not s.get("setup")):
        rows = [s for s in samples if s["case"] == case and not s["warmup"] and not s.get("setup")]
        good = [s for s in rows if s["status"] == "ok"]
        metrics = {}
        names = sorted({name for row in good for name in row["check"]["metrics"]})
        for name in names:
            values = [v for row in good for v in row["check"]["metrics"].get(name, [])]
            metrics[name] = dict(unit=unit(name), count=len(values), p50=CHAT.percentile(values, .5),
                                 p95=CHAT.percentile(values, .95), maximum=max(values))
        summary[case] = dict(runs=len(rows), successful_runs=len(good), failed_runs=len(rows)-len(good), metrics=metrics)
    return summary


def compare(now, old, percent, absolute):
    fields = ("schema", "environment", "build", "workload", "measurement", "cases", "chat_fixture_key")
    mismatch = [key for key in fields if now.get(key) != old.get(key)]
    if not old.get("correctness_passed") or old.get("passed") is False:
        mismatch.append("baseline failed")
    changes = []
    regressions = []
    for case, summary in now["summary"].items():
        before = old.get("summary", {}).get(case, {})
        if not before or before.get("failed_runs") or before.get("runs", 0) < 1:
            mismatch.append("coverage:" + case)
            continue
        for name, values in summary["metrics"].items():
            previous = before.get("metrics", {}).get(name)
            if not previous or previous.get("unit") != values["unit"]:
                mismatch.append("metric:" + case + "/" + name)
                continue
            if values["unit"] != "ms":
                continue
            for rank in ("p50", "p95"):
                a, b = values[rank], previous.get(rank)
                if b is None or not math.isfinite(b) or b < 0:
                    mismatch.append("baseline metric:" + case + "/" + name)
                    continue
                delta = a-b
                ratio = delta/b*100 if b else None
                row = dict(case=case, metric=name, rank=rank, before=b, after=a,
                           delta_ms=delta, delta_percent=ratio)
                changes.append(row)
                # Source cadence/total soak duration are workload evidence,
                # not local speed gates. They remain in the report.
                if name not in ("source_span_ms", "soak_elapsed_ms") and delta > absolute and (not b or ratio > percent):
                    regressions.append(row)
    if "chat_open" in (now.get("cases") or []) and not now.get("chat_comparison"):
        mismatch.append("chat baseline coverage")
    if now.get("chat_comparison"):
        comparison = now["chat_comparison"]
        if not comparison["compatible"]:
            mismatch.extend("chat:" + s for s in comparison["mismatches"])
        regressions.extend(comparison.get("regressions", []))
    return dict(compatible=not mismatch, mismatches=sorted(set(mismatch)), changes=changes, regressions=regressions)


def render(report):
    lines = ["# 蛹后端体验测试", "", "独立进程调用正式 NativeDesktop 库；结果不含界面布局、绘制及交互响应时间。", "",
             "- 配置：`{}`；正确性：{}；总结果：{}。".format(report["profile"],
                  "通过" if report["correctness_passed"] else "失败", "通过" if report["passed"] else "失败"),
             "- 启动恢复为正常退出后重启；工具阻塞取消、故障中断恢复和真正并发执行尚待扩充。",
             "", "| 场景 | 成功/运行 | 指标 | 单位 | 样本数 | P50 | P95 | 最大 |",
             "| --- | ---: | --- | --- | ---: | ---: | ---: | ---: |"]
    for case, summary in report["summary"].items():
        if not summary["metrics"]:
            lines.append("| {} | {}/{} | 失败，无有效指标 | | | | | |".format(case, summary["successful_runs"], summary["runs"]))
        for name, metric in summary["metrics"].items():
            lines.append("| {} | {}/{} | {} | {} | {} | {:.3f} | {:.3f} | {:.3f} |".format(
                case, summary["successful_runs"], summary["runs"], name, metric["unit"], metric["count"],
                metric["p50"], metric["p95"], metric["maximum"]))
    if report.get("chat_report"):
        lines.extend(["", "100×100 日志打开的七项耗时与资源数据见 [聊天打开报告](chat-open/report.md)。"])
    if any(s.get("check", {}).get("diagnostics", {}).get("blocking") for s in report.get("samples", [])):
        lines.extend(["", "分段诊断见 report.json 的 samples[].check.diagnostics.blocking：记录测量窗口内各后台操作的调用次数、排队及执行累计毫秒。",
                      "该窗口包含正确性读取与并发后台操作，累计时间可能重叠，不是单请求的独占阶段耗时；原计数器按整数毫秒累计，亚毫秒操作可能记为 0。"])
    if report.get("comparison"):
        c = report["comparison"]
        lines.extend(["", "基线兼容：{}；退化项：{}。".format(c["compatible"], len(c["regressions"]))])
        if c["mismatches"]:
            lines.append("不可比较字段：" + ", ".join(c["mismatches"]))
    lines.extend(["", "逐次资源计数和持续运行趋势见 report.json；完整有界采样序列见 samples.jsonl。",
                  "短时运行仅验证测试链路；持续增长须结合长时趋势、调用栈和不同日志规模复核。"])
    return "\n".join(lines) + "\n"


def parser():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--output", type=Path, required=True, help="new report directory")
    p.add_argument("--profile", choices=PROFILES, default="smoke")
    p.add_argument("--cases", help="comma-separated cases; chat_open also accepted")
    p.add_argument("--probe", type=Path, help="prebuilt release experience_probe, never the final application")
    p.add_argument("--chat-probe", type=Path)
    p.add_argument("--fixture", type=Path, default=Path("target/desktop-chat-open/experience-fixture-v1"),
                   help="owned chat-open fixture, generated once and reused for comparable hashes")
    p.add_argument("--target-dir", type=Path, default=Path("target"))
    p.add_argument("--baseline", type=Path, help="prior suite report.json")
    for name in ("runs", "warmups", "chunks", "interval-ms", "switches", "soak-seconds", "files"):
        p.add_argument("--" + name, type=int)
    p.add_argument("--timeout", type=float, default=120)
    p.add_argument("--regression-percent", type=float, default=20)
    p.add_argument("--regression-ms", type=float, default=5)
    return p


def main():
    args = parser().parse_args()
    import psutil
    config = PROFILES[args.profile].copy()
    for key in config:
        if getattr(args, key) is not None:
            config[key] = getattr(args, key)
    bounds = dict(runs=(1, 100), warmups=(0, 10), chunks=(2, 1024), interval_ms=(1, 100),
                  switches=(1, 2000), soak_seconds=(1, 7200), files=(600, 100000))
    for key, (low, high) in bounds.items():
        if not low <= config[key] <= high:
            raise ValueError("invalid workload: " + key)
    if not 1 <= args.timeout <= 86400 or not 0 <= args.regression_percent <= 1000 or not 0 <= args.regression_ms <= 60000:
        raise ValueError("invalid deadline or regression threshold")
    defaults = ("soak",) if args.profile == "soak" else ("chat_open",) + CASES
    cases = list(dict.fromkeys(args.cases.split(",") if args.cases else defaults))
    if not cases or any(c not in ("chat_open",) + CASES for c in cases):
        raise ValueError("unknown scenario")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "workers").mkdir()
    target = args.target_dir.resolve()
    binary = (args.probe.resolve() if args.probe else CHAT.build_probe(target, False, "experience_probe")) if any(c != "chat_open" for c in cases) else None
    build_env = {k: v for k, v in os.environ.items() if k.startswith("CARGO_PROFILE_") or k in (
        "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTUP_TOOLCHAIN", "CARGO_BUILD_TARGET")}
    report = dict(schema=1, profile=args.profile, cases=cases, workload=config,
                  measurement=dict(timeout_seconds=args.timeout, sample_ms=10,
                                   soak_sample_ms=max(10, max(args.timeout, config["soak_seconds"]+90)/9)),
                  environment=dict(system=platform.system(), release=platform.release(), machine=platform.machine(),
                                   processor=platform.processor(), logical_cpus=os.cpu_count(), physical_memory_bytes=psutil.virtual_memory().total),
                  build=dict(profile="release", features="sqlite-storage,desktop-control",
                             rustc=subprocess.check_output(["rustc", "-Vv"], text=True).strip(),
                             environment_sha256=hashlib.sha256(json.dumps(build_env, sort_keys=True).encode()).hexdigest()),
                  probe_sha256=CHAT.digest(binary) if binary else None, captured_at_unix=time.time(), samples=[])
    report["build"]["artifact_origin"] = "external" if args.probe or args.chat_probe else "built-by-runner"
    try:
        report["source"] = dict(revision=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=CHAT.REPO, text=True).strip(),
                                dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=CHAT.REPO, text=True).strip()))
    except (OSError, subprocess.SubprocessError):
        report["source"] = dict(revision=None, dirty=None)
    baseline = json.loads(args.baseline.read_text(encoding="utf-8")) if args.baseline else None
    if "chat_open" in cases:
        command = [sys.executable, str(Path(__file__).with_name("measure-chat-open.py")),
                   "--output", str(output / "chat-open"), "--fixture", str(args.fixture.resolve()),
                   "--runs", str(config["runs"]), "--warmups", str(config["warmups"]), "--target-dir", str(target)]
        if args.chat_probe:
            command.extend(["--probe", str(args.chat_probe.resolve())])
        child = subprocess.run(command, cwd=CHAT.REPO, check=False)
        child_report = output / "chat-open/report.json"
        if child_report.is_file():
            chat = json.loads(child_report.read_text(encoding="utf-8"))
            report["chat_report"] = {k: v for k, v in chat.items() if k != "samples"}
            report["chat_fixture_key"] = chat["fixture_key"]
            if baseline and baseline.get("chat_report"):
                report["chat_comparison"] = CHAT.compare(chat, baseline["chat_report"], args.regression_percent, args.regression_ms)
        report["chat_passed"] = child.returncode == 0 and report.get("chat_report", {}).get("passed", False)
    worker_args = []
    for key in ("chunks", "interval_ms", "switches", "soak_seconds", "files"):
        worker_args.extend(["--" + key.replace("_", "-"), str(config[key])])

    def execute(case, root, warmup, index, setup=False):
        timeout = max(args.timeout, config["soak_seconds"] + 90) if case == "soak" else args.timeout
        interval = max(.01, timeout / 9000) if case == "soak" else .01
        print("{} {}{}".format(case, index + 1, " warmup" if warmup else ""), flush=True)
        try:
            sample = CHAT.run_sample(binary, root, case, 1, timeout, interval, psutil, worker_args=worker_args)
            validate(sample, case)
            if case == "soak":
                sample["resource_trends"] = trends(sample["resource_series"])
        except (TimeoutError, subprocess.TimeoutExpired):
            sample = dict(status="timeout", reason="worker deadline exceeded")
        except (RuntimeError, OSError, psutil.Error, ValueError, KeyError):
            sample = dict(status="error", reason="worker, protocol or correctness check failed")
        sample.update(case=case, warmup=warmup, run=index, setup=setup)
        with (output / "samples.jsonl").open("a", encoding="utf-8") as stream:
            stream.write(json.dumps(sample, ensure_ascii=False) + "\n")
        report["samples"].append({k: v for k, v in sample.items() if k != "resource_series"})
        return sample["status"] == "ok"

    for case in (c for c in cases if c != "chat_open"):
        # Soak duration is already its repetition budget. Do not multiply a
        # release soak by the ordinary 30 process samples.
        runs = 1 if case == "soak" else config["runs"]
        warmups = 0 if case == "soak" else config["warmups"]
        for index in range(warmups + runs):
            root = output / "workers" / (case + "-" + str(index))
            if case == "recovery" and not execute("recovery_seed", root, True, index, True):
                report["samples"].append(dict(case=case, warmup=index < warmups, run=index, status="error", reason="recovery seed failed"))
                continue
            execute(case, root, index < warmups, index)
    report["summary"] = summarize(report["samples"])
    report["correctness_passed"] = report.get("chat_passed", True) and all(s["status"] == "ok" for s in report["samples"])
    report["passed"] = report["correctness_passed"]
    if baseline:
        report["comparison"] = compare(report, baseline, args.regression_percent, args.regression_ms)
        report["passed"] &= report["comparison"]["compatible"] and not report["comparison"]["regressions"]
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    (output / "report.md").write_text(render(report), encoding="utf-8")
    with (output / "summary.csv").open("w", encoding="utf-8", newline="") as stream:
        writer = csv.writer(stream)
        writer.writerow(["case", "runs", "failures", "metric", "unit", "count", "p50", "p95", "maximum"])
        for case, summary in report["summary"].items():
            for name, values in summary["metrics"].items():
                writer.writerow([case, summary["runs"], summary["failed_runs"], name] + [values[k] for k in ("unit", "count", "p50", "p95", "maximum")])
    print("Suite {}. Report: {}".format("passed" if report["passed"] else "FAILED", args.output / "report.md"))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError, ImportError):
        print("Suite setup failed; check arguments, new output directory, dependencies and release probes.", file=sys.stderr)
        sys.exit(2)
