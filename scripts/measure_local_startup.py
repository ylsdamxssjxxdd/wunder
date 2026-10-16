#!/usr/bin/env python3
"""Measure local startup in isolated homes without recording paths or credentials.

Fresh-home runs include first-time resource publication and SQLite creation.
They do not flush the operating system's executable or filesystem caches.
Desktop first_frame_presented means a successful software-surface present;
show() and event-loop entry alone are not proof of a painted window.
The optional Windows TUI probe requires pywinpty on PYTHONPATH. "passed" means
milestones and shutdown succeeded, not that every sample met the latency target.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys
import tempfile
import threading
import time


CLI_TIMING = re.compile(
    r"\[startup\]\[cli\] (?:segment|point)=([a-z_]+) "
    r"(?:elapsed_ms=([\d.]+) )?total_ms=([\d.]+)"
)
DESKTOP_TIMING = re.compile(
    r"\[startup\]\[desktop-ui\] point=([a-z_]+) total_ms=([\d.]+)"
)
RUNTIME_TIMING = re.compile(
    r"\[startup\]\[(app-state|bridge-runtime)\] (?:segment|point)=([a-z_]+) "
    r"(?:elapsed_ms=([\d.]+) )?total_ms=([\d.]+)"
)
DESKTOP_SEGMENTS = {
    "platform": ("process_begin", "slint_platform_ready"),
    "font_collection": ("slint_platform_ready", "font_collection_ready"),
    "font_decompression": ("font_collection_ready", "font_decompressed"),
    "font_registration": ("font_decompressed", "font_registered"),
    "font_finalize": ("font_registered", "font_ready"),
    "window_creation": ("font_ready", "main_window_new"),
    "window_setup_and_show": ("main_window_new", "window_show_return"),
    "first_present_after_show": ("window_show_return", "first_frame_presented"),
    "runtime_initialization": ("runtime_begin", "runtime_initialized"),
    "runtime_ui_install": ("runtime_initialized", "runtime_ui_ready"),
}


def isolated_environment(home):
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(("WUNDER_", "RCHO_", "SLINT_"))}
    environment.update(WUNDER_HOME=str(home), WUNDER_STARTUP_TIMING="1", RUST_LOG="info")
    return environment


def cli_run(executable, home):
    started = time.perf_counter()
    process = subprocess.run(
        [str(executable), "config", "show"], cwd=home.parent,
        env=isolated_environment(home), stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE, text=True, encoding="utf-8", errors="replace",
        timeout=45, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
    )
    command_ms = round((time.perf_counter() - started) * 1000, 1)
    segments = {}
    points = {}
    for name, elapsed, total in CLI_TIMING.findall(process.stderr):
        points[name] = float(total)
        if elapsed:
            segments[name] = float(elapsed)
    runtime_points = {f"{scope}.{name}": float(total)
                      for scope, name, _, total in RUNTIME_TIMING.findall(process.stderr)}
    return {"passed": process.returncode == 0 and "init_done" in points,
            "command_ms": command_ms,
            "points_ms": points, "segments_ms": segments,
            "runtime_points_ms": runtime_points}


def desktop_run(executable, home):
    # Popen uses inherited pipes without a console window on Windows; the
    # application still displays its normal native window for real painting.
    points = {}
    observed = {}
    runtime_points, runtime_segments = {}, {}
    ready = threading.Event()
    started = time.perf_counter()
    process = subprocess.Popen(
        [str(executable), "--native"], cwd=home.parent,
        env=isolated_environment(home), stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE, text=True, encoding="utf-8", errors="replace",
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
    )

    def read_points():
        for line in process.stderr:
            for scope, name, elapsed, total in RUNTIME_TIMING.findall(line):
                runtime_points[f"{scope}.{name}"] = float(total)
                if elapsed:
                    runtime_segments[f"{scope}.{name}"] = float(elapsed)
            match = DESKTOP_TIMING.search(line)
            if match:
                points[match[1]] = float(match[2])
                observed[match[1]] = round((time.perf_counter() - started) * 1000, 1)
                if match[1] == "runtime_failed" or all(
                    name in points for name in ("first_frame_presented", "runtime_ui_ready")
                ):
                    ready.set()
        ready.set()

    reader = threading.Thread(target=read_points, daemon=True)
    reader.start()
    try:
        ready.wait(timeout=30)
        passed = (process.poll() is None and "runtime_failed" not in points
                  and all(name in points for name in ("first_frame_presented", "runtime_ui_ready")))
    finally:
        if process.poll() is None:
            process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        reader.join(timeout=5)
        process.stderr.close()
    segments = {name: round(points[end] - points[begin], 1)
                for name, (begin, end) in DESKTOP_SEGMENTS.items()
                if begin in points and end in points}
    return {"passed": passed, "points_ms": points, "segments_ms": segments,
            "parent_observed_ms": observed,
            "runtime_points_ms": runtime_points, "runtime_segments_ms": runtime_segments,
            "shutdown": "terminated isolated benchmark process after measurement"}


def cli_tui_run(executable, home):
    # Optional Windows probe: pywinpty creates a real TTY for all three streams.
    # Install it in a separate tools directory; it is not an app dependency.
    from winpty import PtyProcess

    points, segments, observed = {}, {}, {}
    ready = threading.Event()
    started = time.perf_counter()
    process = PtyProcess.spawn([str(executable)], cwd=str(home.parent),
                               env=isolated_environment(home), dimensions=(40, 240))

    def read_points():
        tail = ""
        try:
            while process.isalive():
                chunk = process.read(8192)
                if "\x1b[6n" in chunk:
                    process.write("\x1b[1;1R")
                tail += chunk
                complete, separator, tail = tail.rpartition("\n")
                if not separator:
                    tail = tail[-8192:]
                    continue
                complete = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", complete)
                for name, elapsed, total in CLI_TIMING.findall(complete):
                    if name not in points:
                        points[name] = float(total)
                        observed[name] = round((time.perf_counter() - started) * 1000, 1)
                        if elapsed:
                            segments[name] = float(elapsed)
                tail = tail[-8192:]
                if "tui_first_frame_submitted" in points:
                    ready.set()
        except (EOFError, OSError):
            pass
        finally:
            ready.set()

    reader = threading.Thread(target=read_points, daemon=True)
    reader.start()
    try:
        ready.wait(timeout=30)
        passed = "tui_first_frame_submitted" in points and process.isalive()
        if passed:
            process.write("/exit\r")
            deadline = time.perf_counter() + 5
            while process.isalive() and time.perf_counter() < deadline:
                time.sleep(0.02)
        clean_exit = not process.isalive() and process.exitstatus == 0
    finally:
        if process.isalive():
            process.terminate(force=True)
        process.close(force=True)
        reader.join(timeout=2)
    return {"passed": passed and clean_exit, "points_ms": points,
            "segments_ms": segments, "parent_observed_ms": observed,
            "shutdown": "exit command" if clean_exit else "terminated isolated benchmark process"}


def distribution(values):
    return {"min": min(values), "median": round(statistics.median(values), 1), "max": max(values)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path)
    parser.add_argument("--desktop", type=Path)
    parser.add_argument("--cli-tui", type=Path,
                        help="Windows real-TTY measurement; requires optional pywinpty")
    parser.add_argument("--runs", type=int, default=3, choices=range(1, 11))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not any((args.cli, args.desktop, args.cli_tui)):
        parser.error("provide --cli, --desktop and/or --cli-tui")
    if args.cli_tui:
        if sys.platform != "win32":
            parser.error("--cli-tui currently requires Windows")
        try:
            import winpty  # noqa: F401
        except ImportError:
            parser.error("--cli-tui requires optional pywinpty on PYTHONPATH")
    report = {"home_cold_means": "fresh isolated home; OS caches are not flushed",
              "clock": "desktop points relative to Rust main; CLI points relative to runtime init; parent clock includes process launch",
              "passed_means": "startup milestones and shutdown succeeded; latency is reported separately",
              "forms": {}}
    with tempfile.TemporaryDirectory(prefix="local-startup-") as temporary:
        root = Path(temporary)
        for name, executable, runner in (("cli", args.cli, cli_run),
                                          ("desktop", args.desktop, desktop_run),
                                          ("cli_tui", args.cli_tui, cli_tui_run)):
            if executable is None:
                continue
            executable = executable.resolve(strict=True)
            before = executable.stat()
            form = {"fresh_home": [], "existing_home": []}
            for index in range(args.runs):
                home = root / f"{name}-{index}"
                form["fresh_home"].append(runner(executable, home))
                form["existing_home"].append(runner(executable, home))
            # Identify the measured artifact afterwards: hashing it first would
            # deliberately warm its executable pages before the first sample.
            digest = hashlib.sha256()
            with executable.open("rb") as binary:
                for chunk in iter(lambda: binary.read(1024 * 1024), b""):
                    digest.update(chunk)
            form["binary_sha256"] = digest.hexdigest()
            after = executable.stat()
            form["binary_bytes"] = after.st_size
            form["artifact_unchanged"] = (before.st_size, before.st_mtime_ns) == (
                after.st_size, after.st_mtime_ns)
            form["summary_ms"] = {}
            for mode in ("fresh_home", "existing_home"):
                values = form[mode]
                summary = {}
                if name == "cli":
                    summary["command"] = distribution([item["command_ms"] for item in values])
                for key in ("init_done", "first_frame_presented", "runtime_initialized", "runtime_ui_ready",
                            "tui_first_frame_submitted"):
                    samples = [item["points_ms"][key] for item in values if key in item["points_ms"]]
                    if samples:
                        summary[key] = distribution(samples)
                    observed = [item["parent_observed_ms"][key] for item in values
                                if key in item.get("parent_observed_ms", {})]
                    if observed:
                        summary[f"parent_observed_{key}"] = distribution(observed)
                segment_names = sorted({key for item in values for key in item["segments_ms"]})
                summary["segments"] = {
                    key: distribution([item["segments_ms"][key] for item in values
                                       if key in item["segments_ms"]])
                    for key in segment_names
                }
                form["summary_ms"][mode] = summary
            milestones = {"cli": ("command",),
                          "desktop": ("parent_observed_first_frame_presented",
                                      "parent_observed_runtime_ui_ready"),
                          "cli_tui": ("parent_observed_tui_first_frame_submitted",)}[name]
            form["latency_budget_ms"] = 1000
            form["all_samples_within_budget"] = {
                mode: all(form["summary_ms"][mode].get(key, {}).get("max", float("inf")) < 1000
                          for key in milestones)
                for mode in ("fresh_home", "existing_home")
            }
            report["forms"][name] = form
    report["passed"] = all(form["artifact_unchanged"]
                           and all(item["passed"] for mode in ("fresh_home", "existing_home")
                                   for item in form[mode])
                           for form in report["forms"].values())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"passed": report["passed"], "forms": {
        name: form["summary_ms"] for name, form in report["forms"].items()
    }}))
    raise SystemExit(0 if report["passed"] else 1)


if __name__ == "__main__":
    main()
