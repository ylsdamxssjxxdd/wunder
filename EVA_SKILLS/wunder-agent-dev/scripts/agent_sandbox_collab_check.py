#!/usr/bin/env python3
"""智能体容器共享/隔离回归检查脚本。"""

from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request

BASE_URL = os.getenv("WUNDER_BASE_URL", "http://127.0.0.1:18000").rstrip("/")
USER_TOKEN = os.getenv("WUNDER_USER_TOKEN", "").strip()
USERNAME = os.getenv("WUNDER_USERNAME", "admin").strip() or "admin"
PASSWORD = os.getenv("WUNDER_PASSWORD", "admin").strip() or "admin"
TIMEOUT_S = float(os.getenv("WUNDER_TIMEOUT_S", "20") or "20")

SHARED_CONTAINER_ID = 9
ISOLATED_CONTAINER_ID = 10
EXPECTED_PRESET_CONTAINERS = {
    "文稿校对": 2,
    "数据分析": 3,
    "科学绘图": 4,
    "政策分析": 5,
    "公文写作": 6,
}


class ApiError(RuntimeError):
    pass


def request_json(method: str, path: str, token: str, payload=None, query=None):
    query = query or {}
    query_text = urllib.parse.urlencode(query)
    url = f"{BASE_URL}{path}"
    if query_text:
        url = f"{url}?{query_text}"
    data = None
    headers = {"Accept": "application/json"}
    if payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url=url, data=data, method=method.upper(), headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT_S) as response:
            status = response.status
            text = response.read().decode("utf-8", errors="replace")
            body = json.loads(text) if text.strip() else {}
            return status, body, response.headers
    except urllib.error.HTTPError as exc:
        text = exc.read().decode("utf-8", errors="replace")
        try:
            body = json.loads(text) if text.strip() else {}
        except Exception:
            body = {"raw": text}
        return exc.code, body, exc.headers


def format_error(status: int, body, headers) -> str:
    error_obj = body.get("error") if isinstance(body, dict) else None
    if isinstance(error_obj, dict):
        code = error_obj.get("code") or "UNKNOWN"
        message = error_obj.get("message") or error_obj.get("detail") or "request failed"
        hint = error_obj.get("hint")
        trace_id = error_obj.get("trace_id") or headers.get("x-trace-id")
        parts = [f"status={status}", f"code={code}", f"message={message}"]
        if hint:
            parts.append(f"hint={hint}")
        if trace_id:
            parts.append(f"trace_id={trace_id}")
        return ", ".join(parts)
    if isinstance(body, dict):
        detail = body.get("detail")
        if isinstance(detail, dict):
            code = detail.get("code")
            message = detail.get("message")
            if code or message:
                return f"status={status}, code={code or 'UNKNOWN'}, message={message or 'request failed'}"
        if detail:
            return f"status={status}, detail={detail}"
    return f"status={status}, body={body}"


def ensure_ok(status: int, body, headers, context: str):
    if 200 <= status < 300:
        return
    raise ApiError(f"{context} failed: {format_error(status, body, headers)}")


def login() -> str:
    if USER_TOKEN:
        return USER_TOKEN
    status, body, headers = request_json(
        "POST",
        "/wunder/auth/login",
        token="",
        payload={"username": USERNAME, "password": PASSWORD},
    )
    ensure_ok(status, body, headers, "login")
    token = body.get("data", {}).get("access_token") if isinstance(body, dict) else None
    if not token:
        raise ApiError("login failed: access_token missing in response")
    return str(token)


def list_agents(token: str):
    status, body, headers = request_json("GET", "/wunder/agents", token=token)
    ensure_ok(status, body, headers, "list agents")
    items = body.get("data", {}).get("items", []) if isinstance(body, dict) else []
    if not isinstance(items, list):
        return []
    return items


def create_agent(token: str, name: str, container_id: int) -> str:
    status, body, headers = request_json(
        "POST",
        "/wunder/agents",
        token=token,
        payload={"name": name, "sandbox_container_id": container_id},
    )
    ensure_ok(status, body, headers, f"create agent {name}")
    agent_id = body.get("data", {}).get("id") if isinstance(body, dict) else None
    if not agent_id:
        raise ApiError(f"create agent {name} failed: id missing")
    return str(agent_id)


def delete_agent(token: str, agent_id: str):
    status, body, headers = request_json(
        "DELETE", f"/wunder/agents/{urllib.parse.quote(agent_id, safe='')}", token=token
    )
    if 200 <= status < 300:
        return
    print(f"[WARN] cleanup delete agent {agent_id} failed: {format_error(status, body, headers)}")


def write_file(token: str, agent_id: str, path: str, content: str):
    status, body, headers = request_json(
        "POST",
        "/wunder/workspace/file",
        token=token,
        payload={
            "agent_id": agent_id,
            "path": path,
            "content": content,
            "create_if_missing": True,
        },
    )
    ensure_ok(status, body, headers, f"write file via agent {agent_id}")


def read_file(token: str, agent_id: str, path: str) -> str:
    status, body, headers = request_json(
        "GET",
        "/wunder/workspace/content",
        token=token,
        query={
            "agent_id": agent_id,
            "path": path,
            "include_content": "true",
            "max_bytes": "1048576",
        },
    )
    ensure_ok(status, body, headers, f"read file via agent {agent_id}")
    content = body.get("content") if isinstance(body, dict) else None
    if content is None:
        raise ApiError(f"read file via agent {agent_id} failed: content missing")
    return str(content)


def delete_file(token: str, agent_id: str, path: str):
    status, body, headers = request_json(
        "DELETE",
        "/wunder/workspace",
        token=token,
        query={"agent_id": agent_id, "path": path},
    )
    if 200 <= status < 300 or status == 404:
        return
    print(f"[WARN] cleanup delete file via agent {agent_id} failed: {format_error(status, body, headers)}")


def verify_preset_layout(items) -> bool:
    ok = True
    by_name = {str(item.get("name", "")).strip(): item for item in items}
    for name, expected_container in EXPECTED_PRESET_CONTAINERS.items():
        item = by_name.get(name)
        if not item:
            ok = False
            print(f"[FAIL] missing preset agent: {name}")
            continue
        actual_container = item.get("sandbox_container_id")
        if actual_container != expected_container:
            ok = False
            print(
                f"[FAIL] preset {name} sandbox_container_id mismatch: "
                f"expected={expected_container}, actual={actual_container}"
            )
        else:
            print(f"[OK] preset {name} sandbox_container_id={actual_container}")
    return ok


def verify_workspace_collab(token: str) -> bool:
    probe_id = int(time.time())
    probe_path = f".skill_sandbox_probe_{probe_id}.txt"
    shared_content = f"shared-{probe_id}"
    isolated_content = f"isolated-{probe_id}"
    created_agents = []
    ok = True

    try:
        agent_a = create_agent(token, f"skill-sandbox-shared-a-{probe_id}", SHARED_CONTAINER_ID)
        created_agents.append(agent_a)
        agent_b = create_agent(token, f"skill-sandbox-shared-b-{probe_id}", SHARED_CONTAINER_ID)
        created_agents.append(agent_b)
        agent_c = create_agent(token, f"skill-sandbox-isolated-{probe_id}", ISOLATED_CONTAINER_ID)
        created_agents.append(agent_c)

        write_file(token, agent_a, probe_path, shared_content)
        content_from_b = read_file(token, agent_b, probe_path)
        if content_from_b != shared_content:
            ok = False
            print(
                "[FAIL] same-container sharing check failed: "
                f"expected={shared_content}, actual={content_from_b}"
            )
        else:
            print("[OK] same-container sharing check passed")

        write_file(token, agent_c, probe_path, isolated_content)
        content_from_a = read_file(token, agent_a, probe_path)
        content_from_c = read_file(token, agent_c, probe_path)

        if content_from_a != shared_content:
            ok = False
            print(
                "[FAIL] cross-container isolation check failed on shared container: "
                f"expected={shared_content}, actual={content_from_a}"
            )
        if content_from_c != isolated_content:
            ok = False
            print(
                "[FAIL] cross-container isolation check failed on isolated container: "
                f"expected={isolated_content}, actual={content_from_c}"
            )
        if content_from_a == shared_content and content_from_c == isolated_content:
            print("[OK] cross-container isolation check passed")

        delete_file(token, agent_a, probe_path)
        delete_file(token, agent_c, probe_path)

    finally:
        for agent_id in reversed(created_agents):
            delete_agent(token, agent_id)

    return ok


def main() -> int:
    try:
        token = login()
        agents = list_agents(token)
        print(f"[INFO] loaded {len(agents)} agents")
        preset_ok = verify_preset_layout(agents)
        collab_ok = verify_workspace_collab(token)
        if preset_ok and collab_ok:
            print("[OK] sandbox container checks passed")
            return 0
        print("[FAIL] sandbox container checks failed")
        return 1
    except ApiError as exc:
        print(f"[FAIL] {exc}")
        return 1
    except Exception as exc:
        print(f"[FAIL] unexpected error: {exc}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
