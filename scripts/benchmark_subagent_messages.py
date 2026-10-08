"""Run isolated Release subagent experiments and record process resource peaks.

The test executable contains synthetic inputs and local mock models. PostgreSQL
tests require an isolated database supplied through WUNDER_SUBAGENT_TEST_DSN.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

import psutil


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--postgres", action="store_true")
    args = parser.parse_args()
    if args.postgres and not os.environ.get("WUNDER_SUBAGENT_TEST_DSN"):
        parser.error("--postgres requires WUNDER_SUBAGENT_TEST_DSN")
    args.output.mkdir(parents=True, exist_ok=True)
    tests = ["mailbox_pressure", "agent_message_sqlite_pressure", "child_pool_pressure"]
    if args.postgres:
        tests.insert(2, "agent_message_postgres_pressure")
    results = []
    for test in tests:
        started = time.perf_counter()
        peak_rss = peak_cpu_s = 0
        with (args.output / f"{test}.log").open("w", encoding="utf-8") as log:
            process = subprocess.Popen(
                [str(args.executable.resolve()), test, "--ignored", "--nocapture", "--test-threads=1"],
                stdout=log, stderr=subprocess.STDOUT,
            )
            metrics = psutil.Process(process.pid)
            while process.poll() is None:
                try:
                    peak_rss = max(peak_rss, metrics.memory_info().rss)
                    cpu = metrics.cpu_times()
                    peak_cpu_s = max(peak_cpu_s, cpu.user + cpu.system)
                except psutil.NoSuchProcess:
                    break
                if time.perf_counter() - started > 600:
                    process.kill()
                    process.wait()
                    raise TimeoutError(f"{test} exceeded 600 seconds")
                time.sleep(0.02)
            code = process.wait()
        result = dict(test=test, exit_code=code, elapsed_s=round(time.perf_counter()-started, 3),
                      sampled_peak_rss_mib=round(peak_rss/1024**2, 2), sampled_cpu_s=round(peak_cpu_s, 3))
        results.append(result)
        print(json.dumps(result), flush=True)
        print((args.output / f"{test}.log").read_text(encoding="utf-8"), flush=True)
        (args.output / "resources.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
        if code:
            raise SystemExit(code)


if __name__ == "__main__":
    main()
