#!/usr/bin/env python3
"""Headless, external benchmark of the native desktop chat-open backend.

Requires Python 3.8+ and psutil. No desktop executable, Slint, browser, model
provider, HTTP bridge, UI automation or display is used.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import queue
import signal
import subprocess
import sys
import threading
import time

REPO = Path(__file__).resolve().parents[2]
CASES = ("first_open", "cached_reopen", "native_headers", "storage_page",
         "session_info", "change_cursor", "older_page")
PROFILE = "chat-open-actions-v1"
SCHEMA = 1
CAP = 10000


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def fixture_inventory(root):
    # Generated fixture only. Metadata/leases/settings may change on startup;
    # authoritative rollout bytes must remain identical after all reads.
    rollouts = {}
    sizes = {}
    for path in sorted((root / "home" / "data").rglob("*")):
        if not path.is_file():
            continue
        name = path.relative_to(root / "home" / "data").as_posix()
        sizes[name] = path.stat().st_size
        if name.endswith((".jsonl", ".jsonl.zst")):
            rollouts[name] = digest(path)
    if not rollouts:
        raise ValueError("fixture contains no authoritative rollouts")
    fingerprint = hashlib.sha256(json.dumps(rollouts, sort_keys=True).encode()).hexdigest()
    return {"rollout_sha256": fingerprint, "files_bytes": sizes,
            "total_bytes": sum(sizes.values())}


def percentile(values, quantile):
    """Nearest-rank percentile; failures are reported separately, never zero."""
    if not values:
        return None
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * quantile) - 1)]


def summarize(samples):
    result = {}
    for case in dict.fromkeys(row["case"] for row in samples):
        rows = [row for row in samples if row["case"] == case and not row["warmup"]]
        values = [v for row in rows if row["status"] == "ok" for v in row["samples_ms"]]
        result[case] = {
            "runs": len(rows), "successful_runs": sum(r["status"] == "ok" for r in rows),
            "failed_runs": sum(r["status"] != "ok" for r in rows), "measured_calls": len(values),
            "p50_ms": percentile(values, .5), "p95_ms": percentile(values, .95),
            "max_ms": max(values) if values else None,
            "max_cpu_ms": max((r.get("resources", {}).get("cpu_ms", 0) for r in rows), default=0),
            "max_rss_delta_bytes": max((r.get("resources", {}).get("rss_delta_bytes", 0) for r in rows), default=0),
            "max_read_bytes": max((r.get("resources", {}).get("read_bytes", 0) for r in rows), default=0),
            "max_read_count": max((r.get("resources", {}).get("read_count", 0) for r in rows), default=0),
        }
    return result


def compare(current, baseline, percent, absolute_ms):
    fields = ("schema", "fixture_key", "environment", "build", "measurement")
    incompatible = [key for key in fields if current.get(key) != baseline.get(key)]
    if baseline.get("passed") is False:
        incompatible.append("baseline failed")
    # Native artifact hash and source revision are expected to change when
    # optimizing, while toolchain/profile/features/target must stay comparable.
    if incompatible:
        return {"compatible": False, "mismatches": incompatible, "regressions": []}
    changes, regressions = [], []
    for case, now in current["summary"].items():
        old = baseline.get("summary", {}).get(case)
        if not old or old.get("p95_ms") is None or old.get("failed_runs"):
            return {"compatible": False, "mismatches": ["baseline coverage"], "regressions": []}
        for metric in ("p50_ms", "p95_ms"):
            if now[metric] is None:
                continue
            delta = now[metric] - old[metric]
            ratio = delta / old[metric] * 100 if old[metric] > 0 else None
            change = {"case": case, "metric": metric, "baseline_ms": old[metric],
                      "current_ms": now[metric], "delta_ms": delta, "delta_percent": ratio}
            changes.append(change)
            if delta > absolute_ms and (old[metric] <= 0 or ratio > percent):
                regressions.append(change)
    return {"compatible": True, "changes": changes, "regressions": regressions}


def read_events(stream, events):
    try:
        for line in stream:
            try:
                event = json.loads(line)
                if isinstance(event, dict) and "event" in event:
                    events.put(event)  # Worker emits a fixed, bounded event set.
            except (ValueError, TypeError):
                pass
    finally:
        events.put({"event": "eof"})


def wait_event(events, wanted, deadline):
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("worker deadline exceeded")
        try:
            event = events.get(timeout=min(remaining, 1.0))
        except queue.Empty:
            continue
        if event["event"] == wanted:
            return event
        if event["event"] in ("error", "eof"):
            raise RuntimeError("worker failed before " + wanted)


def process_sample(process):
    try:
        cpu = process.cpu_times()
        memory = process.memory_info()
        io = process.io_counters()
        return {
            "cpu_ms": (cpu.user + cpu.system) * 1000,
            "rss_bytes": memory.rss,
            "private_bytes": getattr(memory, "private", None),
            "read_bytes": io.read_bytes, "read_count": io.read_count,
            "write_bytes": io.write_bytes, "write_count": io.write_count,
            "handles": process.num_handles() if hasattr(process, "num_handles") else process.num_fds(),
            "threads": process.num_threads(),
        }
    except Exception:
        return None


def resource_delta(before, rows):
    if not before or not rows:
        return {"available": False}
    last = rows[-1]
    result = {"available": True, "samples": len(rows),
              "sampling_truncated": len(rows) >= CAP,
              "peak_rss_bytes": max(row["rss_bytes"] for row in rows),
              "rss_delta_bytes": max(row["rss_bytes"] for row in rows) - before["rss_bytes"]}
    for key in ("cpu_ms", "read_bytes", "read_count", "write_bytes", "write_count"):
        result[key] = max(0, last[key] - before[key])
    if before["private_bytes"] is not None:
        result["peak_private_bytes"] = max(row["private_bytes"] for row in rows)
        result["private_delta_bytes"] = result["peak_private_bytes"] - before["private_bytes"]
    for key in ("handles", "threads"):
        if key in before and all(key in row for row in rows):
            result["peak_" + key] = max(row[key] for row in rows)
            result["end_" + key + "_delta"] = last[key] - before[key]
    return result


def run_sample(binary, fixture, case, iterations, timeout, interval, psutil, trace=None, worker_args=None):
    command = [str(binary), "sample", "--root", str(fixture), "--case", case,
               "--iterations", str(iterations)]
    command.extend(worker_args or [])
    events = queue.Queue(maxsize=16)
    env = os.environ.copy()
    env["RUST_LOG"] = "off"
    for key in tuple(env):
        if key.startswith("WUNDER_"):
            del env[key]
    worker = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=subprocess.DEVNULL, text=True, encoding="utf-8", env=env)
    reader = threading.Thread(target=read_events, args=(worker.stdout, events), daemon=True)
    reader.start()
    deadline = time.monotonic() + timeout
    monitor = None
    profiler = None
    wpr_active = False
    rows = []
    stop = threading.Event()
    sampler = None
    trace_error = None
    try:
        monitor = psutil.Process(worker.pid)
        ready = wait_event(events, "ready", deadline)
        if ready["debug_assertions"]:
            raise RuntimeError("benchmark requires an optimized build")
        if trace:
            if trace["kind"] == "perf":
                profiler = subprocess.Popen(["perf", "record", "-g", "--call-graph", "dwarf",
                    "-p", str(worker.pid), "-o", str(trace["path"])],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                time.sleep(.3)
                if profiler.poll() is not None:
                    raise RuntimeError("perf could not attach")
            else:
                status = subprocess.run(["wpr", "-start", "GeneralProfile", "-filemode"],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20)
                if status.returncode:
                    raise RuntimeError("WPR could not start; check privileges or active recording")
                wpr_active = True
        setup_ms = (time.monotonic() - (deadline - timeout)) * 1000
        before = process_sample(monitor)

        def sample_resources():
            while not stop.is_set():
                row = process_sample(monitor)
                if row:
                    row["since_start_ms"] = (time.monotonic() - started) * 1000
                    rows.append(row)
                if len(rows) >= CAP:
                    break
                stop.wait(interval)

        started = time.monotonic()
        sampler = threading.Thread(target=sample_resources, daemon=True)
        sampler.start()
        worker.stdin.write("go\n")
        worker.stdin.flush()
        wait_event(events, "measurement_start", deadline)
        result = wait_event(events, "result", deadline)
        stop.set()
        sampler.join(timeout=1)
        final = process_sample(monitor)
        if final:
            final["since_start_ms"] = (time.monotonic() - started) * 1000
            rows.append(final)
        values = result.get("samples_ms", [])
        if len(values) != iterations or any(not isinstance(v, (int, float)) or not math.isfinite(v) or v < 0 for v in values):
            raise RuntimeError("invalid measured samples")
        external_window_ms = (time.monotonic() - started) * 1000
        worker.stdin.write("done\n")
        worker.stdin.flush()
        worker.stdin.close()
        worker.wait(timeout=max(.1, deadline-time.monotonic()))
        if worker.returncode:
            raise RuntimeError("worker did not exit successfully")
        return {"status": "ok", "samples_ms": values, "startup_ms": result["startup_ms"],
                "worker_pid": worker.pid,
                "external_setup_ms": setup_ms,
                "external_window_ms": external_window_ms,
                "resources": resource_delta(before, rows), "resource_series": rows,
                "check": result["check"]}
    finally:
        stop.set()
        if sampler:
            sampler.join(timeout=1)
        if worker.poll() is None:
            worker.kill()
            worker.wait(timeout=10)
        if profiler:
            if profiler.poll() is None:
                profiler.send_signal(signal.SIGINT)
                try:
                    profiler.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    profiler.kill()
                    profiler.wait(timeout=5)
            if not trace["path"].is_file() or trace["path"].stat().st_size == 0:
                trace_error = "perf recording is empty"
        if wpr_active:
            try:
                status = subprocess.run(["wpr", "-stop", str(trace["path"])],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
                if status.returncode:
                    trace_error = "WPR recording could not be saved"
            except (OSError, subprocess.TimeoutExpired):
                trace_error = "WPR recording could not be saved"
        for stream in (worker.stdin, worker.stdout):
            if stream and not stream.closed:
                stream.close()
        reader.join(timeout=1)
        if trace_error:
            raise RuntimeError(trace_error)


def build_probe(target, symbols, example="chat_open_probe"):
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target)
    env["CARGO_PROFILE_RELEASE_DEBUG"] = "1" if symbols else "0"
    env["CARGO_PROFILE_RELEASE_STRIP"] = "none" if symbols else "symbols"
    subprocess.run(["cargo", "build", "--release", "-p", "wunder-desktop", "--example", example,
                    "--no-default-features", "--features", "sqlite-storage,desktop-control", "-j", "8"],
                   cwd=REPO, env=env, check=True)
    return target / "release" / "examples" / (example + (".exe" if os.name == "nt" else ""))


def prepare(binary, fixture, args):
    fixture.parent.mkdir(parents=True, exist_ok=True)
    command = [str(binary), "prepare", "--root", str(fixture),
               "--user-rounds", str(args.user_rounds), "--actions", str(args.actions),
               "--seed", str(args.seed), "--text-bytes", str(args.text_bytes),
               "--reasoning-bytes", str(args.reasoning_bytes), "--tool-bytes", str(args.tool_bytes)]
    env = {key: value for key, value in os.environ.items() if not key.startswith("WUNDER_")}
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                               text=True, encoding="utf-8", env=env)
    events = queue.Queue(maxsize=512)
    reader = threading.Thread(target=read_events, args=(process.stdout, events), daemon=True)
    reader.start()
    deadline = time.monotonic() + args.prepare_timeout
    try:
        wait_event(events, "prepared", deadline)
        process.wait(timeout=max(.1, deadline-time.monotonic()))
        if process.returncode:
            raise RuntimeError("fixture preparation failed")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=10)
        process.stdout.close()
        reader.join(timeout=1)


def render(report):
    lines = ["# 蛹聊天打开后端基准", "", "直接调用 NativeDesktop 后端库；不启动蛹程序或前端。",
             "首屏指标截至 open_chat_snapshot 返回，不包含前端投影、布局、绘制或完整日志导出。", "",
             "| 场景 | 成功/运行 | P50 ms | P95 ms | 最大 ms |",
             "| --- | ---: | ---: | ---: | ---: |"]
    def number(value):
        return "—" if value is None else "{:.3f}".format(value)
    for case, summary in report["summary"].items():
        lines.append("| {} | {}/{} | {} | {} | {} |".format(case, summary["successful_runs"], summary["runs"],
                     number(summary["p50_ms"]), number(summary["p95_ms"]), number(summary["max_ms"])))
    lines.extend(["", "| 场景 | 最大 CPU ms | 最大 RSS 增量 MiB | 最大读取 MiB | 最大读取次数 |",
                  "| --- | ---: | ---: | ---: | ---: |"])
    for case, summary in report["summary"].items():
        lines.append("| {} | {:.3f} | {:.3f} | {:.3f} | {} |".format(case, summary["max_cpu_ms"],
            summary["max_rss_delta_bytes"] / 2**20, summary["max_read_bytes"] / 2**20, summary["max_read_count"]))
    lines.extend(["", "首次打开使用独立进程；应用启动和数据准备不计入接口耗时，操作系统文件缓存未清空。",
                  "分项在不同进程中独立测量，不能相加或相减推算函数独占时间。",
                  "资源采样覆盖外部握手到结果返回的窗口，包含后台任务与采样误差；极短调用以循环诊断辅助观察。",
                  "", "数据与调用校验：" + ("通过" if report.get("correctness_passed", report["passed"]) else "失败，查看 JSON 中的样本"),
                  "基准结果：" + ("通过" if report["passed"] else "未通过")])
    comparison = report.get("comparison")
    if comparison:
        regressions = {(row["case"], row["metric"]) for row in comparison.get("regressions", [])}
        lines.append("基线比较：" + ("条件不一致，拒绝比较" if not comparison["compatible"] else
                     "检测到 {} 项退化".format(len(regressions)) if regressions else "通过"))
        if comparison.get("changes"):
            lines.append("")
        for change in comparison.get("changes", []):
            lines.append("- {} {}: {:+.3f} ms{}".format(change["case"], change["metric"], change["delta_ms"],
                         "（退化）" if (change["case"], change["metric"]) in regressions else ""))
    return "\n".join(lines) + "\n"


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--fixture", type=Path, default=Path("target/desktop-chat-open/fixture"))
    result.add_argument("--output", type=Path, required=True, help="new output directory")
    result.add_argument("--probe", type=Path, help="already built headless test worker; never the desktop UI")
    result.add_argument("--target-dir", type=Path, default=Path("target"))
    result.add_argument("--symbols", action="store_true", help="build with symbols for stack profiling")
    result.add_argument("--prepare-only", action="store_true")
    result.add_argument("--user-rounds", type=int, default=100)
    result.add_argument("--actions", type=int, default=100, help="model requests plus tool actions per user round")
    result.add_argument("--seed", type=int, default=42)
    result.add_argument("--text-bytes", type=int, default=1024)
    result.add_argument("--reasoning-bytes", type=int, default=512)
    result.add_argument("--tool-bytes", type=int, default=4096)
    result.add_argument("--runs", type=int, default=10)
    result.add_argument("--warmups", type=int, default=1)
    result.add_argument("--cases", default=",".join(CASES))
    result.add_argument("--timeout", type=float, default=120)
    result.add_argument("--prepare-timeout", type=float, default=1800)
    result.add_argument("--sample-ms", type=float, default=10)
    result.add_argument("--iterations", type=int, default=1, help="diagnostic loop; first_open always requires 1")
    result.add_argument("--baseline", type=Path)
    result.add_argument("--regression-percent", type=float, default=20)
    result.add_argument("--regression-ms", type=float, default=5)
    result.add_argument("--profile", choices=("none", "perf", "wpr"), default="none")
    return result


def main():
    args = parser().parse_args()
    # Keep console reports readable in Windows shells without changing files'
    # UTF-8 format or inheriting an ambiguous legacy output code page.
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    import psutil
    cases = args.cases.split(",")
    if not cases or len(set(cases)) != len(cases) or any(case not in CASES for case in cases):
        raise ValueError("unknown or duplicate case")
    if not 1 <= args.runs <= 1000 or not 0 <= args.warmups <= 20 or not 1 <= args.iterations <= CAP:
        raise ValueError("invalid run count")
    if (args.runs + args.warmups) * len(cases) * args.iterations > 100000:
        raise ValueError("measurement call budget exceeded")
    if not 1 <= args.sample_ms <= 1000 or not 1 <= args.timeout <= 600 or not 1 <= args.prepare_timeout <= 7200:
        raise ValueError("invalid timeout or sampling interval")
    if not 1 <= args.user_rounds <= 2000 or not 1 <= args.actions <= 2000 or args.user_rounds * args.actions > 1000000:
        raise ValueError("invalid fixture size")
    if not 0 <= args.seed < 2**64 or not 1 <= args.text_bytes <= 65536 or not 0 <= args.reasoning_bytes <= 65536 or not 0 <= args.tool_bytes <= 65536:
        raise ValueError("invalid fixture body sizes or seed")
    if not math.isfinite(args.regression_percent) or args.regression_percent < 0 or not math.isfinite(args.regression_ms) or args.regression_ms < 0:
        raise ValueError("invalid regression threshold")
    if args.user_rounds <= 12 and "older_page" in cases and not args.prepare_only:
        raise ValueError("older_page requires more than 12 user rounds")
    if args.profile != "none" and args.baseline:
        raise ValueError("profiled timing cannot be used for baseline comparison")
    if args.profile != "none" and not args.symbols:
        raise ValueError("profiling requires --symbols for useful call stacks")
    if args.iterations != 1 and "first_open" in cases:
        raise ValueError("first_open requires one call in each fresh process")
    if args.profile == "wpr" and os.name != "nt" or args.profile == "perf" and sys.platform != "linux":
        raise ValueError("profiler is unsupported on this platform")
    if args.profile != "none" and args.runs > 5:
        raise ValueError("profiling is limited to five recordings per case")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binary = args.probe.resolve() if args.probe else build_probe(args.target_dir.resolve(), args.symbols)
    if not binary.is_file():
        raise ValueError("headless probe was not built")
    fixture = args.fixture.resolve()
    if not fixture.exists():
        print("Preparing synthetic fixture (not timed)...", flush=True)
        prepare(binary, fixture, args)
    manifest = json.loads((fixture / "fixture.json").read_text(encoding="utf-8"))
    owner = json.loads((fixture / "fixture-owner.json").read_text(encoding="utf-8"))
    if owner.get("profile") != PROFILE:
        raise ValueError("not an owned synthetic benchmark fixture")
    expected = {"profile": PROFILE, "user_rounds": args.user_rounds, "actions_per_round": args.actions,
                "seed": args.seed, "text_bytes": args.text_bytes,
                "reasoning_bytes": args.reasoning_bytes, "tool_bytes": args.tool_bytes}
    if any(manifest.get(key) != value for key, value in expected.items()):
        raise ValueError("existing fixture differs from requested workload; use a new fixture directory")
    inventory = fixture_inventory(fixture)
    if args.prepare_only:
        (output / "fixture.json").write_text(json.dumps({"fixture": manifest, "inventory": inventory}, indent=2), encoding="utf-8")
        print("Fixture ready.")
        return 0
    report = {
        "schema": SCHEMA, "fixture_key": {**expected, "rollout_sha256": inventory["rollout_sha256"]},
        "fixture": manifest, "inventory": inventory,
        "environment": {"system": platform.system(), "release": platform.release(), "machine": platform.machine(),
                        "processor": platform.processor(), "logical_cpus": os.cpu_count(),
                        "physical_memory_bytes": psutil.virtual_memory().total},
        "build": {"profile": "release", "symbols": args.symbols, "features": "sqlite-storage,desktop-control"},
        "measurement": {"sample_ms": args.sample_ms, "iterations": args.iterations, "profile": args.profile},
        "artifact_sha256": digest(binary), "captured_at_unix": time.time(), "samples": [],
    }
    overrides = {key: value for key, value in os.environ.items()
                 if key.startswith("CARGO_PROFILE_") or key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTUP_TOOLCHAIN", "CARGO_BUILD_TARGET")}
    report["build"]["environment_sha256"] = hashlib.sha256(json.dumps(overrides, sort_keys=True).encode()).hexdigest()
    if not args.probe:
        report["build"]["rustc"] = subprocess.check_output(["rustc", "-Vv"], text=True).strip()
    else:
        report["build"]["rustc"] = "external probe; verify matching toolchain and build flags"
    try:
        report["source"] = {"revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
            "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=REPO, text=True).strip())}
    except (OSError, subprocess.CalledProcessError):
        report["source"] = {"revision": None, "dirty": None}
    raw_file = output / "samples.jsonl"
    for case in cases:
        for index in range(args.warmups + args.runs):
            warmup = index < args.warmups
            print("{} {}/{}{}".format(case, index+1, args.warmups+args.runs, " warmup" if warmup else ""), flush=True)
            trace = None
            if args.profile != "none" and not warmup:
                trace = {"kind": args.profile, "path": output / ("{}-{}.{}".format(case, index, "etl" if args.profile == "wpr" else "perf.data"))}
            try:
                sample = run_sample(binary, fixture, case, args.iterations, args.timeout, args.sample_ms/1000, psutil, trace)
            except (TimeoutError, subprocess.TimeoutExpired):
                sample = {"status": "timeout", "samples_ms": [], "reason": "worker deadline exceeded"}
            except RuntimeError as error:
                sample = {"status": "error", "samples_ms": [], "reason": str(error)}
            except psutil.Error:
                sample = {"status": "error", "samples_ms": [], "reason": "worker exited before resource sampler attached"}
            except OSError:
                sample = {"status": "error", "samples_ms": [], "reason": "worker or profiler could not start"}
            sample.update(case=case, warmup=warmup, run=index)
            with raw_file.open("a", encoding="utf-8") as stream:
                stream.write(json.dumps(sample, ensure_ascii=False) + "\n")
            # Keep the full bounded resource series on disk, not in every
            # in-memory sample or duplicated in the aggregate report.
            report["samples"].append({key: value for key, value in sample.items() if key != "resource_series"})
    report["summary"] = summarize(report["samples"])
    report["diagnostic_hints"] = []
    first = report["summary"].get("first_open", {}).get("p50_ms")
    storage = report["summary"].get("storage_page", {}).get("p50_ms")
    native = report["summary"].get("native_headers", {}).get("p50_ms")
    if first and storage is not None and storage >= first * .6:
        report["diagnostic_hints"].append("Storage-page and first-open timings are close: profile indexed reads, JSON decoding and connection/history validation; independent cases do not establish exclusive costs.")
    if native and storage is not None and native > max(storage * 1.5, storage + 5):
        report["diagnostic_hints"].append("Native-header reads are substantially slower than storage-page reads: profile conversion and fallback item reads; confirm with call stacks.")
    report["rollout_unchanged"] = fixture_inventory(fixture)["rollout_sha256"] == inventory["rollout_sha256"]
    report["correctness_passed"] = report["rollout_unchanged"] and all(s["status"] == "ok" for s in report["samples"])
    report["passed"] = report["correctness_passed"]
    if args.baseline:
        baseline = json.loads(args.baseline.read_text(encoding="utf-8"))
        report["comparison"] = compare(report, baseline, args.regression_percent, args.regression_ms)
        report["passed"] &= report["comparison"]["compatible"] and not report["comparison"]["regressions"]
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    (output / "report.md").write_text(render(report), encoding="utf-8")
    with (output / "summary.csv").open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=["case", "runs", "successful_runs", "failed_runs", "measured_calls",
            "p50_ms", "p95_ms", "max_ms", "max_cpu_ms", "max_rss_delta_bytes", "max_read_bytes", "max_read_count"])
        writer.writeheader()
        writer.writerows({"case": case, **summary} for case, summary in report["summary"].items())
    print(render(report))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError) as error:
        # File contents, settings and absolute paths are never persisted.
        print("Benchmark setup failed: " + type(error).__name__ + "; check arguments, fixture, build and permissions.", file=sys.stderr)
        sys.exit(2)
