#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""Scenario stress probe for runtime boundary metrics.

The script calls selected runtime/admin/user endpoints concurrently, then compares
core::blocking/core::bounded_queue/core::long_task metric snapshots.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Any


READ_ONLY_SCENARIOS = ("monitor", "channels", "cron")
DEFAULT_SCENARIOS = ("monitor", "channels")
VALID_SCENARIOS = {
    "monitor",
    "channels",
    "cron",
    "performance",
    "channel-probe",
    "ws",
}


@dataclass(frozen=True)
class Operation:
    scenario: str
    method: str
    path: str
    body: Any | None = None


@dataclass(frozen=True)
class Config:
    base_url: str
    api_key: str
    auth_token: str
    user_id: str
    scenarios: tuple[str, ...]
    concurrency: int
    rounds: int
    timeout: float
    fail_on_alert: bool
    channel_limit: int
    channel_probe_channel: str
    channel_probe_account_id: str
    cron_job_id: str
    cron_runs_limit: int
    performance_concurrency: int
    performance_command: str
    ws_endpoint: str
    ws_connections: int
    ws_hold_seconds: float
    ws_pings: int


def parse_scenarios(raw_items: list[str] | None, include_performance: bool) -> tuple[str, ...]:
    expanded: list[str] = []
    for item in raw_items or DEFAULT_SCENARIOS:
        for part in item.split(","):
            scenario = part.strip().lower().replace("_", "-")
            if not scenario:
                continue
            if scenario == "all-readonly":
                expanded.extend(READ_ONLY_SCENARIOS)
                continue
            if scenario == "all":
                expanded.extend(["monitor", "channels", "cron", "performance", "ws"])
                continue
            if scenario not in VALID_SCENARIOS:
                raise SystemExit(f"unsupported scenario: {scenario}")
            expanded.append(scenario)
    if include_performance:
        expanded.append("performance")

    deduped = []
    seen = set()
    for scenario in expanded:
        if scenario not in seen:
            seen.add(scenario)
            deduped.append(scenario)
    return tuple(deduped)


def parse_args() -> Config:
    parser = argparse.ArgumentParser(description="Stress runtime boundary metrics.")
    parser.add_argument("--base-url", default="http://127.0.0.1:18001/wunder")
    parser.add_argument("--api-key", default="")
    parser.add_argument("--auth-token", default="")
    parser.add_argument("--user-id", default="")
    parser.add_argument(
        "--scenario",
        action="append",
        help=(
            "Scenario to run. Repeat or comma-separate. Supported: "
            "monitor, channels, cron, performance, channel-probe, ws, all-readonly, all. "
            "Default: monitor,channels."
        ),
    )
    parser.add_argument("--concurrency", type=int, default=8)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument(
        "--include-performance",
        action="store_true",
        help="Compatibility alias that adds the performance scenario.",
    )
    parser.add_argument("--fail-on-alert", action="store_true")
    parser.add_argument("--channel-limit", type=int, default=40)
    parser.add_argument("--channel-probe-channel", default="")
    parser.add_argument("--channel-probe-account-id", default="")
    parser.add_argument("--cron-job-id", default="")
    parser.add_argument("--cron-runs-limit", type=int, default=20)
    parser.add_argument("--performance-concurrency", type=int, default=1)
    parser.add_argument("--performance-command", default="")
    parser.add_argument("--ws-endpoint", choices=("core", "chat"), default="core")
    parser.add_argument("--ws-connections", type=int, default=0)
    parser.add_argument("--ws-hold-seconds", type=float, default=2.0)
    parser.add_argument("--ws-pings", type=int, default=2)
    args = parser.parse_args()
    scenarios = parse_scenarios(args.scenario, bool(args.include_performance))
    validate_scenario_config(args, scenarios)
    return Config(
        base_url=args.base_url.rstrip("/"),
        api_key=args.api_key.strip(),
        auth_token=args.auth_token.strip(),
        user_id=args.user_id.strip(),
        scenarios=scenarios,
        concurrency=max(1, args.concurrency),
        rounds=max(1, args.rounds),
        timeout=max(1.0, args.timeout),
        fail_on_alert=bool(args.fail_on_alert),
        channel_limit=max(1, min(200, args.channel_limit)),
        channel_probe_channel=args.channel_probe_channel.strip(),
        channel_probe_account_id=args.channel_probe_account_id.strip(),
        cron_job_id=args.cron_job_id.strip(),
        cron_runs_limit=max(1, min(200, args.cron_runs_limit)),
        performance_concurrency=max(1, args.performance_concurrency),
        performance_command=args.performance_command.strip(),
        ws_endpoint=args.ws_endpoint,
        ws_connections=max(0, args.ws_connections),
        ws_hold_seconds=max(0.0, args.ws_hold_seconds),
        ws_pings=max(0, args.ws_pings),
    )


def validate_scenario_config(args: argparse.Namespace, scenarios: tuple[str, ...]) -> None:
    user_id = args.user_id.strip()
    auth_token = args.auth_token.strip()
    api_key = args.api_key.strip()
    if "cron" in scenarios and not user_id and not auth_token:
        raise SystemExit("cron scenario requires --user-id or --auth-token")
    if "channel-probe" in scenarios and not args.channel_probe_channel.strip():
        raise SystemExit("channel-probe scenario requires --channel-probe-channel")
    if "ws" in scenarios and not auth_token and not (api_key and user_id):
        raise SystemExit("ws scenario requires --auth-token, or --api-key with --user-id")
    if "ws" in scenarios and args.ws_endpoint == "chat" and not auth_token:
        raise SystemExit("chat ws scenario requires --auth-token")


def request_headers(config: Config) -> dict[str, str]:
    headers = {"accept": "application/json"}
    if config.api_key:
        headers["x-api-key"] = config.api_key
    if config.auth_token:
        headers["authorization"] = f"Bearer {config.auth_token}"
    return headers


def request_json(config: Config, method: str, path: str, body: Any | None = None) -> Any:
    url = f"{config.base_url}{path}"
    data = None
    headers = request_headers(config)
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["content-type"] = "application/json"
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    with urllib.request.urlopen(request, timeout=config.timeout) as response:
        raw = response.read()
    if not raw:
        return None
    return json.loads(raw.decode("utf-8"))


async def request_json_async(config: Config, operation: Operation) -> tuple[bool, str]:
    started = time.perf_counter()
    try:
        await asyncio.to_thread(
            request_json, config, operation.method, operation.path, operation.body
        )
        return (
            True,
            f"{operation.scenario}: {operation.method} {operation.path} "
            f"{time.perf_counter() - started:.3f}s",
        )
    except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, OSError) as exc:
        return (
            False,
            f"{operation.scenario}: {operation.method} {operation.path} failed: {exc}",
        )


def cron_query(config: Config, extra: dict[str, str] | None = None) -> str:
    params: dict[str, str] = {}
    if config.user_id:
        params["user_id"] = config.user_id
    if extra:
        params.update(extra)
    encoded = urllib.parse.urlencode(params)
    return f"?{encoded}" if encoded else ""


def performance_body(config: Config) -> dict[str, Any]:
    body: dict[str, Any] = {"concurrency": config.performance_concurrency}
    if config.performance_command:
        body["command"] = config.performance_command
    return body


def build_http_operations(config: Config) -> list[Operation]:
    operations: list[Operation] = []
    if "monitor" in config.scenarios:
        operations.extend(
            [
                Operation("monitor", "GET", "/admin/runtime_metrics"),
                Operation("monitor", "GET", "/admin/monitor?active_only=false&tool_hours=0.1"),
            ]
        )
    if "channels" in config.scenarios:
        limit = config.channel_limit
        operations.extend(
            [
                Operation("channels", "GET", "/admin/channels/accounts"),
                Operation("channels", "GET", f"/admin/channels/runtime_logs?limit={limit}"),
                Operation("channels", "GET", "/admin/channels/bindings"),
                Operation("channels", "GET", f"/admin/channels/user_bindings?limit={limit}"),
                Operation("channels", "GET", f"/admin/channels/sessions?limit={limit}"),
            ]
        )
    if "cron" in config.scenarios:
        operations.extend(
            [
                Operation("cron", "GET", f"/cron/status{cron_query(config)}"),
                Operation("cron", "GET", f"/cron/list{cron_query(config)}"),
            ]
        )
        if config.cron_job_id:
            operations.append(
                Operation(
                    "cron",
                    "GET",
                    f"/cron/runs{cron_query(config, {'job_id': config.cron_job_id, 'limit': str(config.cron_runs_limit)})}",
                )
            )
    if "performance" in config.scenarios:
        operations.append(
            Operation(
                "performance",
                "POST",
                "/admin/performance/sample",
                performance_body(config),
            )
        )
    if "channel-probe" in config.scenarios:
        body: dict[str, Any] = {
            "channel": config.channel_probe_channel,
            "message": "runtime boundary stress probe",
        }
        if config.channel_probe_account_id:
            body["account_id"] = config.channel_probe_account_id
        operations.append(
            Operation("channel-probe", "POST", "/admin/channels/runtime_logs/probe", body)
        )
    return operations


async def run_http_load(config: Config) -> list[tuple[bool, str]]:
    operations = build_http_operations(config)
    if not operations:
        return []

    semaphore = asyncio.Semaphore(config.concurrency)
    results: list[tuple[bool, str]] = []

    async def one_call(index: int) -> None:
        operation = operations[index % len(operations)]
        async with semaphore:
            results.append(await request_json_async(config, operation))

    tasks = [
        asyncio.create_task(one_call(index))
        for index in range(config.concurrency * config.rounds)
    ]
    await asyncio.gather(*tasks)
    return results


def ws_path(config: Config) -> str:
    return "/chat/ws" if config.ws_endpoint == "chat" else "/ws"


def websocket_url(config: Config, index: int) -> str:
    parsed = urllib.parse.urlsplit(config.base_url)
    scheme = "wss" if parsed.scheme == "https" else "ws"
    path = f"{parsed.path.rstrip('/')}{ws_path(config)}"
    params: dict[str, str] = {}
    token = config.auth_token or config.api_key
    if token:
        params["token"] = token
    if config.user_id:
        params["user_id"] = config.user_id
    params["stress_id"] = str(index)
    query = urllib.parse.urlencode(params)
    return urllib.parse.urlunsplit((scheme, parsed.netloc, path, query, ""))


async def recv_ws_json(ws: Any, timeout: float) -> dict[str, Any]:
    raw = await asyncio.wait_for(ws.recv(), timeout=timeout)
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8")
    payload = json.loads(raw)
    if not isinstance(payload, dict):
        raise RuntimeError("websocket message is not a JSON object")
    return payload


async def wait_ws_type(ws: Any, kind: str, timeout: float, max_messages: int = 8) -> dict[str, Any]:
    for _ in range(max_messages):
        payload = await recv_ws_json(ws, timeout)
        if str(payload.get("type") or "").lower() == kind:
            return payload
    raise RuntimeError(f"websocket did not receive {kind}")


async def one_ws_connection(config: Config, index: int) -> tuple[bool, str]:
    started = time.perf_counter()
    try:
        import websockets  # type: ignore[import-not-found]
    except ImportError:
        return (
            False,
            "ws: optional dependency missing; install with `python -m pip install websockets`",
        )

    url = websocket_url(config, index)
    try:
        async with websockets.connect(
            url,
            subprotocols=["wunder"],
            open_timeout=config.timeout,
            close_timeout=min(5.0, config.timeout),
            ping_interval=None,
        ) as ws:
            await wait_ws_type(ws, "ready", config.timeout)
            request_id = f"stress_connect_{index}"
            await ws.send(
                json.dumps(
                    {
                        "type": "connect",
                        "request_id": request_id,
                        "payload": {
                            "protocol_version": 1,
                            "client": {
                                "name": "runtime_boundary_stress",
                                "version": "1",
                                "platform": "script",
                                "mode": "probe",
                            },
                        },
                    }
                )
            )
            await wait_ws_type(ws, "ready", config.timeout)
            for ping_index in range(config.ws_pings):
                await ws.send(
                    json.dumps(
                        {
                            "type": "ping",
                            "request_id": f"stress_ping_{index}_{ping_index}",
                        }
                    )
                )
                await wait_ws_type(ws, "pong", config.timeout)
            if config.ws_hold_seconds > 0:
                await asyncio.sleep(config.ws_hold_seconds)
        elapsed = time.perf_counter() - started
        return True, f"ws: {config.ws_endpoint} connection {index} {elapsed:.3f}s"
    except (OSError, TimeoutError, RuntimeError, asyncio.TimeoutError, ValueError) as exc:
        return False, f"ws: {config.ws_endpoint} connection {index} failed: {exc}"


async def run_ws_load(config: Config) -> list[tuple[bool, str]]:
    if "ws" not in config.scenarios:
        return []
    connection_count = config.ws_connections or config.concurrency
    tasks = [
        asyncio.create_task(one_ws_connection(config, index))
        for index in range(max(1, connection_count))
    ]
    return list(await asyncio.gather(*tasks))


async def run_load(config: Config) -> list[tuple[bool, str]]:
    http_task = asyncio.create_task(run_http_load(config))
    ws_task = asyncio.create_task(run_ws_load(config))
    http_results, ws_results = await asyncio.gather(http_task, ws_task)
    return [*http_results, *ws_results]


def runtime_snapshot(config: Config) -> dict[str, Any]:
    payload = request_json(config, "GET", "/admin/runtime_metrics")
    runtime = payload.get("runtime") if isinstance(payload, dict) else None
    if not isinstance(runtime, dict):
        raise RuntimeError("runtime metrics response missing runtime object")
    return runtime


def indexed(items: Any, key: str) -> dict[str, dict[str, Any]]:
    if not isinstance(items, list):
        return {}
    result = {}
    for item in items:
        if isinstance(item, dict):
            name = str(item.get(key) or "")
            if name:
                result[name] = item
    return result


def delta_sum(
    before: dict[str, dict[str, Any]], after: dict[str, dict[str, Any]], field: str
) -> int:
    total = 0
    for name, item in after.items():
        current = int(item.get(field) or 0)
        previous = int(before.get(name, {}).get(field) or 0)
        total += max(0, current - previous)
    return total


def top_deltas(
    before: dict[str, dict[str, Any]],
    after: dict[str, dict[str, Any]],
    field: str,
    name_field: str,
    limit: int = 8,
) -> list[dict[str, Any]]:
    items = []
    for name, item in after.items():
        current = int(item.get(field) or 0)
        previous = int(before.get(name, {}).get(field) or 0)
        delta = max(0, current - previous)
        if delta > 0:
            items.append({name_field: name, field: delta})
    items.sort(key=lambda value: int(value[field]), reverse=True)
    return items[:limit]


def summarize_delta(before: dict[str, Any], after: dict[str, Any]) -> dict[str, Any]:
    blocking_before = indexed(before.get("blocking"), "label")
    blocking_after = indexed(after.get("blocking"), "label")
    queue_before = indexed(before.get("queues"), "name")
    queue_after = indexed(after.get("queues"), "name")
    long_before = indexed(before.get("long_tasks"), "label")
    long_after = indexed(after.get("long_tasks"), "label")
    return {
        "blocking_calls": delta_sum(blocking_before, blocking_after, "calls"),
        "blocking_queue_timeouts": delta_sum(
            blocking_before, blocking_after, "queue_timeouts"
        ),
        "blocking_exec_timeouts": delta_sum(blocking_before, blocking_after, "exec_timeouts"),
        "queue_enqueued": delta_sum(queue_before, queue_after, "enqueued"),
        "queue_busy": delta_sum(queue_before, queue_after, "busy"),
        "long_task_started": delta_sum(long_before, long_after, "started"),
        "long_task_warnings": delta_sum(long_before, long_after, "warnings"),
        "top_blocking_calls": top_deltas(blocking_before, blocking_after, "calls", "label"),
        "top_queue_enqueued": top_deltas(queue_before, queue_after, "enqueued", "name"),
        "top_long_tasks": top_deltas(long_before, long_after, "started", "label"),
        "alerts": after.get("alerts") if isinstance(after.get("alerts"), list) else [],
    }


async def main() -> int:
    config = parse_args()
    before = runtime_snapshot(config)
    results = await run_load(config)
    after = runtime_snapshot(config)
    summary = summarize_delta(before, after)
    failures = [message for ok, message in results if not ok]
    output = {
        "config": {
            "base_url": config.base_url,
            "scenarios": list(config.scenarios),
            "concurrency": config.concurrency,
            "rounds": config.rounds,
            "user_id": config.user_id,
            "channel_limit": config.channel_limit,
            "cron_job_id": config.cron_job_id,
            "performance_concurrency": config.performance_concurrency,
            "ws_endpoint": config.ws_endpoint,
            "ws_connections": config.ws_connections or config.concurrency,
            "ws_hold_seconds": config.ws_hold_seconds,
            "ws_pings": config.ws_pings,
        },
        "requests": {
            "total": len(results),
            "failed": len(failures),
            "failures": failures[:10],
        },
        "delta": summary,
    }
    print(json.dumps(output, ensure_ascii=False, indent=2))
    if failures:
        return 2
    if config.fail_on_alert and summary["alerts"]:
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
