#!/usr/bin/env python
"""Collect the server capacity scorecard's idle CPU and maintenance budget.

Reads Docker counters and two runtime snapshots; never changes containers or
data. The capacity PostgreSQL test supplies S1 and S3 correctness gates.
"""
from __future__ import annotations

import argparse
import http.client
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import urllib.parse
import urllib.request


class WindowsPipe(io.RawIOBase):
    """Use Win32 pipe IO; Python's CRT file handles reject some pipe reads."""

    def __init__(self, path: str) -> None:
        import ctypes
        from ctypes import wintypes
        self.api = ctypes.WinDLL("kernel32", use_last_error=True)
        self.api.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                                        wintypes.LPVOID, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
        self.api.CreateFileW.restype = wintypes.HANDLE
        self.api.ReadFile.argtypes = self.api.WriteFile.argtypes = [wintypes.HANDLE, wintypes.LPVOID,
                                                                  wintypes.DWORD, ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID]
        self.api.CloseHandle.argtypes = [wintypes.HANDLE]
        self.api.WaitNamedPipeW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD]
        self.api.WaitNamedPipeW(path, 10_000)
        self.handle = self.api.CreateFileW(path, 0xC0000000, 0, None, 3, 0, None)
        if self.handle == wintypes.HANDLE(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())

    def readable(self) -> bool:
        return True

    def writable(self) -> bool:
        return True

    def readinto(self, buffer) -> int:
        import ctypes
        from ctypes import wintypes
        count = wintypes.DWORD()
        target = (ctypes.c_char * len(buffer)).from_buffer(buffer)
        if not self.api.ReadFile(self.handle, target, len(buffer), ctypes.byref(count), None):
            error = ctypes.get_last_error()
            if error in (109, 232):
                return 0
            if error != 234:
                raise ctypes.WinError(error)
        return count.value

    def write(self, data) -> int:
        import ctypes
        from ctypes import wintypes
        count = wintypes.DWORD()
        source = ctypes.create_string_buffer(bytes(data))
        if not self.api.WriteFile(self.handle, source, len(data), ctypes.byref(count), None):
            raise ctypes.WinError(ctypes.get_last_error())
        return count.value

    def close(self) -> None:
        if not self.closed:
            self.api.CloseHandle(self.handle)
        super().close()


def runtime_snapshot(url: str | None) -> dict | None:
    if not url:
        return None
    headers = {}
    if key := os.environ.get("WUNDER_CAPACITY_API_KEY"):
        headers["X-API-Key"] = key
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.load(response)


class DockerEngine:
    """Read cumulative counters through the active Docker CLI context.

    No command runs inside either measured container. Point-in-time `stats`
    averages can miss short maintenance batches between samples.
    """

    def __init__(self) -> None:
        self.host = os.environ.get("DOCKER_HOST") if not os.environ.get("DOCKER_CONTEXT") else None
        if not self.host:
            result = subprocess.run(
                ["docker", "context", "inspect", "--format", "{{.Endpoints.docker.Host}}"],
                check=True, capture_output=True, text=True, timeout=20,
            )
            self.host = result.stdout.strip()
        if not self.host.startswith(("npipe:", "unix://")):
            raise ValueError("CPU sampling requires a local Docker named-pipe or Unix-socket context")

    def get(self, path: str) -> dict:
        # Pin a supported API rather than silently accepting changed shapes.
        request = f"GET /v1.47{path} HTTP/1.1\r\nHost: docker\r\nConnection: close\r\n\r\n".encode("ascii")
        if self.host.startswith("npipe:"):
            pipe = "\\\\" + self.host.removeprefix("npipe:").replace("/", "\\").lstrip("\\")
            stream = WindowsPipe(pipe)

            class PipeSocket:
                def makefile(self, *_args):
                    return io.BufferedReader(stream)

            connection = PipeSocket()
            stream.write(request)
        else:
            connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            connection.settimeout(20)
            connection.connect(self.host.removeprefix("unix://"))
            connection.sendall(request)
            stream = None
        try:
            with http.client.HTTPResponse(connection) as response:
                response.begin()
                if response.status != 200:
                    raise ValueError(f"Docker counter request failed: HTTP {response.status}")
                return json.load(response)
        finally:
            if stream is not None:
                stream.close()
            else:
                connection.close()

    def stats(self, name: str) -> dict:
        return self.get(f"/containers/{urllib.parse.quote(name, safe='')}/stats?stream=false&one-shot=true")

    def identity(self, name: str) -> tuple[str, str]:
        info = self.get(f"/containers/{urllib.parse.quote(name, safe='')}/json")
        if not info["State"]["Running"]:
            raise ValueError("A measured container stopped during sampling")
        return info["Id"], info["State"]["StartedAt"]


def cumulative_cpu(before: dict, after: dict) -> float:
    """One-core CPU percentage over the entire window, including all bursts."""
    from datetime import datetime
    elapsed = (datetime.fromisoformat(after["read"].replace("Z", "+00:00"))
               - datetime.fromisoformat(before["read"].replace("Z", "+00:00"))).total_seconds()
    delta = after["cpu_stats"]["cpu_usage"]["total_usage"] - before["cpu_stats"]["cpu_usage"]["total_usage"]
    if elapsed <= 0 or delta < 0:
        raise ValueError("Container CPU counters reset; repeat the window")
    return delta / (elapsed * 10_000_000)


def runtime_calls(snapshot: dict | None) -> dict[str, int]:
    """Observe non-probe API calls without recording users or payloads."""
    if snapshot is None:
        return {}
    runtime = snapshot["runtime"]
    calls = {row["label"]: row["calls"] for row in runtime["blocking"] if row["label"].startswith("api.")}
    calls.update({row["label"]: row["ticks"] for row in runtime["loop_ticks"]
                  if row["label"].startswith("api.") and row["label"] != "api.admin.runtime_metrics"})
    return calls


def docker_stats(server: str, postgres: str) -> dict[str, float]:
    result = subprocess.run(
        ["docker", "stats", "--no-stream", "--format", "{{json .}}", server, postgres],
        check=True, capture_output=True, text=True, timeout=20,
    )
    values = {}
    for line in result.stdout.splitlines():
        row = json.loads(line)
        values[row["Name"]] = float(row["CPUPerc"].rstrip("%"))
    if set(values) != {server, postgres}:
        raise ValueError("Both requested containers must remain running")
    return values


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, type=Path, help="capacity test JSON input")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--server-container", required=True)
    parser.add_argument("--postgres-container", required=True)
    parser.add_argument("--runtime-url", help="admin runtime_metrics endpoint")
    parser.add_argument("--seconds", type=float, default=900)
    parser.add_argument("--interval", type=float, default=5)
    parser.add_argument("--idle-confirmed", action="store_true", help="operator confirms no requests or active tasks during this window")
    args = parser.parse_args()
    if args.seconds <= 0 or args.interval < 1:
        parser.error("seconds must be positive and interval must be at least 1s")
    report = json.loads(args.input.read_text(encoding="utf-8-sig"))
    engine = DockerEngine()
    containers = (args.server_container, args.postgres_container)
    identities = {name: engine.identity(name) for name in containers}
    before = runtime_snapshot(args.runtime_url)
    initial = {name: engine.stats(name) for name in containers}
    latest = initial
    started = time.monotonic()
    samples = []
    invalid_reason = None
    while time.monotonic()-started < args.seconds:
        time.sleep(min(args.interval, max(0, args.seconds-(time.monotonic()-started))))
        try:
            candidate = {name: engine.stats(name) for name in containers}
            if any(engine.identity(name) != identities[name] for name in containers):
                invalid_reason = "container restarted or replaced during sampling"
                break
            sample = {name: cumulative_cpu(initial[name], candidate[name]) for name in containers}
        except Exception as error:
            invalid_reason = f"counter sampling failed: {type(error).__name__}"
            break
        latest = candidate
        samples.append({"elapsed_s": time.monotonic()-started, "server_cpu_percent": sample[args.server_container], "postgres_cpu_percent": sample[args.postgres_container]})
    try:
        after = runtime_snapshot(args.runtime_url)
    except Exception as error:
        after = None
        invalid_reason = invalid_reason or f"runtime snapshot failed: {type(error).__name__}"
    observed_calls = runtime_calls(after)
    prior_calls = runtime_calls(before)
    if any(count > prior_calls.get(label, 0) for label, count in observed_calls.items()):
        invalid_reason = invalid_reason or "interactive API activity observed during sampling"
    measured_seconds = time.monotonic()-started
    scenarios = report.setdefault("scenarios", {})
    metrics = {}
    if samples:
        metrics = {"server_idle_cpu_percent": samples[-1]["server_cpu_percent"], "postgres_idle_cpu_percent": samples[-1]["postgres_cpu_percent"]}
    scenarios["S2"] = {
        "metrics": metrics,
        "gates": {"idle_window_valid": args.idle_confirmed and measured_seconds >= 900 and len(samples) >= 30 and invalid_reason is None},
    }
    duty = None
    if before is not None and after is not None:
        before = before["thread_view_maintenance"]
        after = after["thread_view_maintenance"]
        wall = after["elapsed_ms"]-before["elapsed_ms"]
        work = after["work_ms"]-before["work_ms"]
        if invalid_reason is None and wall > 0 and work >= 0:
            duty = work/wall
            scenarios.setdefault("S3", {}).setdefault("metrics", {})["maintenance_duty_ratio"] = duty
        else:
            invalid_reason = invalid_reason or "maintenance counters reset during sampling"
            scenarios["S2"]["gates"]["idle_window_valid"] = False
    report.setdefault("raw", {})["idle_samples"] = samples
    report["raw"]["cpu_window"] = {
        "method": "Docker Engine cumulative CPU nanoseconds / elapsed seconds; single-core percent",
        "counters": [{"role": role, "begin_read": initial[name]["read"], "end_read": latest[name]["read"], "begin_cpu_ns": initial[name]["cpu_stats"]["cpu_usage"]["total_usage"], "end_cpu_ns": latest[name]["cpu_stats"]["cpu_usage"]["total_usage"]} for role, name in zip(("server", "postgres"), containers)],
        "invalid_reason": invalid_reason,
    }
    report.setdefault("environment", {})["idle_window_s"] = measured_seconds
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2)+"\n", encoding="utf-8")
    print(f"Recorded {len(samples)} CPU samples; maintenance duty={duty}; output={args.output}")
    if invalid_reason:
        print(f"Invalid idle window: {invalid_reason}")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
