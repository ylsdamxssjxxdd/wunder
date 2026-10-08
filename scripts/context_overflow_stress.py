#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""Context overflow recovery stress runner for /wunder chat sessions.

Usage examples:
  python scripts/context_overflow_stress.py --base-url http://127.0.0.1:8080 --token <token>
  python scripts/context_overflow_stress.py --base-url http://127.0.0.1:8080 --username admin --password 123456
  python scripts/context_overflow_stress.py --token <token> --rounds 50 --repeat 700 --sleep-ms 50
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Dict, Optional, Tuple


DEFAULT_BASE_URL = "http://127.0.0.1:8080"
DEFAULT_TIMEOUT_S = 60
DEFAULT_ROUNDS = 50
DEFAULT_REPEAT = 600
DEFAULT_SESSION_TITLE = "MindIE Context Overflow Stress"


@dataclass
class RoundResult:
    round_index: int
    status: int
    ok: bool
    code: str
    message: str
    answer_len: int
    elapsed_ms: float


def normalize_base_url(base_url: str) -> str:
    return base_url.rstrip("/")


def http_json(
    method: str,
    url: str,
    payload: Optional[Dict[str, Any]],
    token: Optional[str],
    timeout_s: int,
) -> Tuple[int, Dict[str, Any]]:
    headers = {
        "Accept": "application/json",
    }
    body = None
    if payload is not None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url=url, method=method.upper(), headers=headers, data=body)
    try:
        with urllib.request.urlopen(req, timeout=timeout_s) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
            data = json.loads(raw) if raw.strip() else {}
            return int(resp.status), data
    except urllib.error.HTTPError as err:
        raw = err.read().decode("utf-8", errors="replace")
        try:
            data = json.loads(raw) if raw.strip() else {}
        except json.JSONDecodeError:
            data = {"raw": raw}
        return int(err.code), data


def login_and_get_token(base_url: str, username: str, password: str, timeout_s: int) -> str:
    status, payload = http_json(
        "POST",
        f"{base_url}/wunder/auth/login",
        {"username": username, "password": password},
        token=None,
        timeout_s=timeout_s,
    )
    if status != 200:
        raise RuntimeError(f"login failed ({status}): {payload}")
    token = (
        payload.get("data", {}).get("access_token")
        if isinstance(payload.get("data"), dict)
        else None
    )
    if not token:
        raise RuntimeError(f"login succeeded but access_token missing: {payload}")
    return token


def create_session(base_url: str, token: str, title: str, timeout_s: int) -> str:
    status, payload = http_json(
        "POST",
        f"{base_url}/wunder/chat/sessions",
        {"title": title},
        token=token,
        timeout_s=timeout_s,
    )
    if status != 200:
        raise RuntimeError(f"create session failed ({status}): {payload}")
    session_id = payload.get("data", {}).get("id")
    if not session_id:
        raise RuntimeError(f"create session missing session id: {payload}")
    return str(session_id)


def build_pressure_question(round_index: int, repeat: int) -> str:
    payload = "mindie-context-payload " * repeat
    return (
        f"[mindie-overflow-stress] round={round_index}\n"
        "目标：即使触发上下文压缩也要持续对话，回复任意简短确认。\n"
        f"{payload}"
    )


def send_round(
    base_url: str,
    token: str,
    session_id: str,
    round_index: int,
    repeat: int,
    timeout_s: int,
) -> RoundResult:
    started = time.perf_counter()
    status, payload = http_json(
        "POST",
        f"{base_url}/wunder/chat/sessions/{session_id}/messages",
        {
            "content": build_pressure_question(round_index, repeat),
            "stream": False,
        },
        token=token,
        timeout_s=timeout_s,
    )
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    ok = status == 200
    code = ""
    message = ""
    answer_len = 0

    if ok:
        answer = payload.get("data", {}).get("answer")
        if isinstance(answer, str):
            answer_len = len(answer.strip())
        else:
            answer_len = 0
        if answer_len == 0:
            ok = False
            code = "EMPTY_ANSWER"
            message = "data.answer is empty"
    else:
        code = str(payload.get("code") or payload.get("error", {}).get("code") or "")
        message = str(
            payload.get("message")
            or payload.get("error", {}).get("message")
            or payload.get("detail")
            or payload
        )

    return RoundResult(
        round_index=round_index,
        status=status,
        ok=ok,
        code=code,
        message=message,
        answer_len=answer_len,
        elapsed_ms=elapsed_ms,
    )


def fetch_session_context_tokens(
    base_url: str, token: str, session_id: str, timeout_s: int
) -> Optional[int]:
    status, payload = http_json(
        "GET",
        f"{base_url}/wunder/chat/sessions/{session_id}",
        None,
        token=token,
        timeout_s=timeout_s,
    )
    if status != 200:
        return None
    value = payload.get("data", {}).get("context_tokens")
    if isinstance(value, int):
        return value
    return None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Stress-test context overflow recovery over /wunder chat sessions."
    )
    parser.add_argument("--base-url", default=DEFAULT_BASE_URL, help="Wunder server base URL.")
    parser.add_argument("--token", default="", help="Bearer access token.")
    parser.add_argument("--username", default="", help="Login username (if token omitted).")
    parser.add_argument("--password", default="", help="Login password (if token omitted).")
    parser.add_argument("--session-id", default="", help="Reuse existing session id.")
    parser.add_argument("--title", default=DEFAULT_SESSION_TITLE, help="Session title.")
    parser.add_argument("--rounds", type=int, default=DEFAULT_ROUNDS, help="Round count.")
    parser.add_argument(
        "--repeat",
        type=int,
        default=DEFAULT_REPEAT,
        help="Repeated payload chunks per message.",
    )
    parser.add_argument(
        "--timeout-s",
        type=int,
        default=DEFAULT_TIMEOUT_S,
        help="HTTP timeout seconds.",
    )
    parser.add_argument(
        "--sleep-ms",
        type=int,
        default=0,
        help="Optional sleep between rounds.",
    )
    parser.add_argument(
        "--no-fail-fast",
        action="store_true",
        help="Continue remaining rounds even if one round fails.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    base_url = normalize_base_url(args.base_url)
    rounds = max(1, int(args.rounds))
    repeat = max(1, int(args.repeat))
    timeout_s = max(1, int(args.timeout_s))
    sleep_ms = max(0, int(args.sleep_ms))
    fail_fast = not args.no_fail_fast

    token = args.token.strip()
    if not token:
        username = args.username.strip()
        password = args.password.strip()
        if not username or not password:
            print("error: provide either --token or --username + --password", file=sys.stderr)
            return 2
        try:
            token = login_and_get_token(base_url, username, password, timeout_s)
            print(f"[auth] login success for {username}")
        except Exception as err:  # noqa: BLE001
            print(f"[auth] login failed: {err}", file=sys.stderr)
            return 2

    session_id = args.session_id.strip()
    if not session_id:
        try:
            session_id = create_session(base_url, token, args.title, timeout_s)
            print(f"[session] created: {session_id}")
        except Exception as err:  # noqa: BLE001
            print(f"[session] create failed: {err}", file=sys.stderr)
            return 2
    else:
        print(f"[session] reuse: {session_id}")

    print(
        f"[stress] start rounds={rounds} repeat={repeat} fail_fast={fail_fast} "
        f"base_url={base_url}"
    )

    results: list[RoundResult] = []
    failures = 0
    started = time.perf_counter()

    for idx in range(1, rounds + 1):
        result = send_round(base_url, token, session_id, idx, repeat, timeout_s)
        results.append(result)
        if result.ok:
            print(
                f"[round {idx:02d}] ok status={result.status} "
                f"answer_len={result.answer_len} latency_ms={result.elapsed_ms:.1f}"
            )
        else:
            failures += 1
            print(
                f"[round {idx:02d}] fail status={result.status} code={result.code or '-'} "
                f"latency_ms={result.elapsed_ms:.1f} message={result.message}"
            )
            if fail_fast:
                break
        if sleep_ms > 0 and idx < rounds:
            time.sleep(sleep_ms / 1000.0)

    elapsed_s = time.perf_counter() - started
    passed = len(results) - failures
    context_tokens = fetch_session_context_tokens(base_url, token, session_id, timeout_s)

    print(
        f"[summary] session={session_id} passed={passed} failed={failures} "
        f"executed={len(results)} elapsed_s={elapsed_s:.2f} "
        f"context_tokens={context_tokens if context_tokens is not None else 'n/a'}"
    )

    if failures > 0:
        print("[summary] context overflow recovery stress FAILED", file=sys.stderr)
        return 1
    print("[summary] context overflow recovery stress PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
