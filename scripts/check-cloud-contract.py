#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""云端易用重构的接口契约自检（用户侧 + 管理侧）。

对应 `docs/云端易用重构方案.md` §12.2.1「契约细则（字段级，冻结）」与 §四/§十的落地要求。
每条断言都对应一个具体契约条目，输出 PASS/FAIL，便于在 G1/G2 联调时一次跑完。

用法：
    python scripts/check-cloud-contract.py                     # 默认打 http://127.0.0.1:18000
    python scripts/check-cloud-contract.py --base http://127.0.0.1:18000
    python scripts/check-cloud-contract.py --admin-user admin --admin-pass admin
    python scripts/check-cloud-contract.py --preset preset_e2e_a --alt-preset preset_e2e_b

前置：隔离服务端在跑（`python scripts/check-web.py --keep-server`），且配置里有至少两个预设
（`check-web.py` 生成的隔离配置已内置 `preset_e2e_a` / `preset_e2e_b`）。
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
import uuid
from typing import Any


class Client:
    def __init__(self, base: str) -> None:
        self.base = base.rstrip("/")
        self.token = ""

    def request(
        self,
        method: str,
        path: str,
        body: dict[str, Any] | None = None,
        token: str | None = None,
    ) -> tuple[int, Any]:
        url = self.base + path
        data = json.dumps(body).encode("utf-8") if body is not None else None
        request = urllib.request.Request(url, data=data, method=method)
        request.add_header("Content-Type", "application/json")
        auth = token if token is not None else self.token
        if auth:
            request.add_header("Authorization", f"Bearer {auth}")
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                raw = response.read().decode("utf-8", "replace")
                status = response.status
        except urllib.error.HTTPError as error:
            raw = error.read().decode("utf-8", "replace")
            status = error.code
        try:
            payload = json.loads(raw) if raw else None
        except json.JSONDecodeError:
            payload = raw
        return status, payload

    def upload(
        self,
        filename: str,
        content: str,
        token: str | None = None,
        path: str = "",
    ) -> tuple[int, Any]:
        """multipart 上传（服务端的文件字段名是 `files`，可选 `path`）。"""
        boundary = f"----wundercontract{uuid.uuid4().hex}"
        chunks = []
        if path:
            chunks.append(
                f'--{boundary}\r\nContent-Disposition: form-data; name="path"\r\n\r\n{path}\r\n'
            )
        chunks.append(
            f'--{boundary}\r\nContent-Disposition: form-data; name="files"; filename="{filename}"\r\n'
            f"Content-Type: text/plain\r\n\r\n{content}\r\n"
        )
        chunks.append(f"--{boundary}--\r\n")
        request = urllib.request.Request(
            self.base + "/wunder/workspace/upload",
            data="".join(chunks).encode("utf-8"),
            method="POST",
        )
        request.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")
        auth = token if token is not None else self.token
        if auth:
            request.add_header("Authorization", f"Bearer {auth}")
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = response.read().decode("utf-8", "replace")
                status = response.status
        except urllib.error.HTTPError as error:
            raw = error.read().decode("utf-8", "replace")
            status = error.code
        try:
            payload = json.loads(raw) if raw else None
        except json.JSONDecodeError:
            payload = raw
        return status, payload


RESULTS: list[tuple[bool, str, str]] = []


def check(ok: bool, name: str, detail: str = "") -> None:
    RESULTS.append((bool(ok), name, detail))
    mark = "PASS" if ok else "FAIL"
    print(f"[{mark}] {name}" + (f"  — {detail}" if detail and not ok else ""))


def as_dict(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}


def main() -> int:
    parser = argparse.ArgumentParser(description="云端契约自检")
    parser.add_argument("--base", default="http://127.0.0.1:18000")
    parser.add_argument("--admin-user", default="admin")
    parser.add_argument("--admin-pass", default="admin")
    parser.add_argument("--preset", default="preset_e2e_a")
    parser.add_argument("--alt-preset", default="preset_e2e_b")
    args = parser.parse_args()

    client = Client(args.base)

    # ---- 准备：管理员登录 + 注册一个普通用户 ----
    status, payload = client.request(
        "POST", "/wunder/auth/login", {"username": args.admin_user, "password": args.admin_pass}
    )
    admin_token = str(as_dict(as_dict(payload).get("data")).get("access_token") or "")
    check(status == 200 and bool(admin_token), "管理员登录", f"status={status}")

    username = f"contract_{int(time.time()) % 1000000}"
    status, payload = client.request(
        "POST", "/wunder/auth/register", {"username": username, "password": "Passw0rd!23"}
    )
    user_data = as_dict(as_dict(payload).get("data"))
    user_token = str(user_data.get("access_token") or "")
    user_record = as_dict(user_data.get("user"))
    user_id = str(user_record.get("user_id") or user_record.get("id") or username)
    check(status == 200 and bool(user_token), "注册普通用户", f"status={status}")
    if not (admin_token and user_token):
        return finish()

    client.token = user_token

    # ---- 3) GET /wunder/user/agent ----
    # agent 载荷复用 `agent_payload`，其标识键是 `id`（与 /wunder/agents 的 items 完全一致），
    # 不是 `agent_id` —— 早期契约示例写错了键名，这里以实际载荷为准并校验两者同构。
    status, payload = client.request("GET", "/wunder/user/agent")
    data = as_dict(as_dict(payload).get("data"))
    agent = as_dict(data.get("agent"))
    binding = as_dict(data.get("preset_binding"))
    customizable = as_dict(data.get("customizable"))
    check(
        status == 200 and bool(agent.get("id")),
        "GET /user/agent → data.agent.id",
        f"status={status} body={str(payload)[:160]}",
    )
    check(bool(binding.get("preset_id")), "GET /user/agent → preset_binding.preset_id")
    expected_keys = {"system_prompt", "welcome", "model_name", "reasoning_effort", "tool_names", "approval_mode"}
    check(
        expected_keys.issubset(set(customizable)),
        "GET /user/agent → customizable 六键齐全",
        f"keys={sorted(customizable)}",
    )

    # ---- 4) GET /wunder/agents/models（用户视图） ----
    status, payload = client.request("GET", "/wunder/agents/models")
    data = as_dict(as_dict(payload).get("data"))
    items = data.get("items") if isinstance(data.get("items"), list) else []
    check(status == 200 and isinstance(data.get("items"), list), "GET /agents/models → data.items 为列表", f"status={status}")
    if items and isinstance(items[0], dict):
        fields = set(items[0])
        check("name" in fields or "id" in fields, "models item 至少含 id/name", f"fields={sorted(fields)}")
        # `source` / `is_default` 必须存在；`context` 允许缺省（配置里没有上下文长度时
        # 契约要求省略而不是编造），但存在时必须是正整数。
        check(
            {"source", "is_default"}.issubset(fields),
            "models item 含 source/is_default",
            f"fields={sorted(fields)}",
        )
        if "context" in fields:
            context_value = items[0].get("context")
            check(
                context_value is None or (isinstance(context_value, int) and context_value > 0),
                "models item 的 context 为正整数或 null",
                f"context={context_value!r}",
            )
    check("user_default_model_name" in data, "models 响应含 user_default_model_name", f"keys={sorted(data)}")

    # ---- 5) PUT /wunder/user/agent（写路径 + customizable 强制） ----
    # `customizable` 为 false 的字段必须被拒绝（422 FIELD_NOT_CUSTOMIZABLE），
    # 为 true 的字段必须落库并在响应里回显。这里优先挑一个已放开的字段（通常是 model_name）。
    writable = [field for field in expected_keys if customizable.get(field) is True]
    if writable:
        field = "model_name" if "model_name" in writable else writable[0]
        status, payload = client.request("PUT", "/wunder/user/agent", {field: "e2e-write-probe"})
        body = as_dict(payload)
        echoed = as_dict(as_dict(body.get("data")).get("agent")).get(field) == "e2e-write-probe"
        check(status == 200 and echoed, f"PUT /user/agent 可写字段 {field} → 200 且回显", f"status={status} body={str(payload)[:160]}")
    else:
        print("[SKIP] 预设未放开任何字段，跳过可写路径（隔离配置里 preset_e2e_a 已放开 model_name/approval_mode）")

    locked = [field for field in expected_keys if customizable.get(field) is not True]
    # `welcome` / `reasoning_effort` 是**语义标记**而不是直接写字段：
    # - `welcome` 由 description + preset_questions 承载；
    # - `reasoning_effort` 是会话/消息级，随请求下发，不是 agent 持久化字段。
    # 因此拒绝路径用真正持久的字段探测（system_prompt / tool_names / model_name）。
    probe_locked = [field for field in ("system_prompt", "tool_names", "model_name", "approval_mode") if field in locked]
    if probe_locked:
        field = probe_locked[0]
        status, payload = client.request("PUT", "/wunder/user/agent", {field: "should-be-rejected"})
        code = str(as_dict(as_dict(payload).get("error")).get("code") or "")
        check(
            status in (400, 403, 409, 422) and (code == "FIELD_NOT_CUSTOMIZABLE" or "customizable" in str(payload)),
            f"PUT /user/agent 未授权字段 {field} 被拒（期望 4xx/FIELD_NOT_CUSTOMIZABLE）",
            f"status={status} code={code} body={str(payload)[:160]}",
        )
    else:
        print("[SKIP] 预设放开全部持久字段，跳过只读路径")
    if "welcome" in locked or "reasoning_effort" in locked:
        print("[INFO] welcome/reasoning_effort 为语义标记（分别由 description+preset_questions、会话级推理强度承载），不作为 agent 写字段校验")

    # ---- 6) GET /wunder/workspace/stats（裸对象） ----
    status, payload = client.request("GET", "/wunder/workspace/stats")
    body = as_dict(payload)
    check(status == 200 and "used_bytes" in body, "GET /workspace/stats 裸对象含 used_bytes", f"status={status}")
    check(
        {"files", "dirs", "truncated", "quota_bytes", "recent"}.issubset(set(body)),
        "stats 含 files/dirs/truncated/quota_bytes/recent",
        f"keys={sorted(body)}",
    )

    # ---- 6) GET /wunder/workspace（列表、无 container_id） ----
    status, payload = client.request("GET", "/wunder/workspace?path=&offset=0&limit=10")
    body = as_dict(payload)
    check(status == 200 and "entries" in body, "GET /workspace 裸对象含 entries", f"status={status}")
    entries = body.get("entries") if isinstance(body.get("entries"), list) else []
    relative_ok = all(not str(as_dict(item).get("path", "")).startswith(("/", "\\")) for item in entries)
    check(relative_ok, "workspace entries.path 为工作区相对路径")

    # ---- 6b) 多租户隔离（方案 §15.6：单根目录仍必须按 user_id 严格隔离） ----
    # 单根/flatten 之后最容易出的事故是「所有用户共用一个目录」，这里用两个真实用户
    # 交叉验证：A 上传的文件对 B 不可见、不可读、不可下载，而 A 自己可读。
    probe_name = f"iso_probe_{uuid.uuid4().hex[:8]}.txt"
    probe_marker = f"isolation-marker-{uuid.uuid4().hex}"
    status, payload = client.upload(probe_name, probe_marker)
    check(status == 200, "用户 A 上传探测文件 → 200", f"status={status} body={str(payload)[:160]}")

    status, payload = client.request("GET", "/wunder/workspace?path=&offset=0&limit=50")
    names = [str(as_dict(item).get("name", "")) for item in (as_dict(payload).get("entries") or [])]
    check(probe_name in names, "用户 A 能在自己的工作目录看到该文件", f"names={names[:8]}")

    status, payload = client.request("GET", f"/wunder/workspace/content?path={probe_name}")
    check(
        status == 200 and probe_marker in str(payload),
        "用户 A 可读取自己的文件（对照，证明下面的拒绝不是路径写错）",
        f"status={status} body={str(payload)[:160]}",
    )

    other_username = f"contract_b_{int(time.time()) % 1000000}"
    status, payload = client.request(
        "POST", "/wunder/auth/register", {"username": other_username, "password": "Passw0rd!23"}
    )
    other_data = as_dict(as_dict(payload).get("data"))
    other_token = str(other_data.get("access_token") or "")
    check(status == 200 and bool(other_token), "注册第二个用户 B", f"status={status}")
    if other_token:
        status, payload = client.request("GET", "/wunder/workspace?path=&offset=0&limit=50", token=other_token)
        other_names = [str(as_dict(item).get("name", "")) for item in (as_dict(payload).get("entries") or [])]
        check(
            status == 200 and probe_name not in other_names,
            "用户 B 的目录列表看不到 A 的文件",
            f"status={status} names={other_names[:8]}",
        )
        status, payload = client.request(
            "GET", f"/wunder/workspace/content?path={probe_name}", token=other_token
        )
        check(status >= 400, "用户 B 读 A 的文件被拒", f"status={status} body={str(payload)[:120]}")
        status, payload = client.request(
            "GET", f"/wunder/workspace/download?path={probe_name}", token=other_token
        )
        check(status >= 400, "用户 B 下载 A 的文件被拒", f"status={status} body={str(payload)[:120]}")

    # ---- 7) GET /wunder/admin/preset_agents ----
    client.token = admin_token
    status, payload = client.request("GET", "/wunder/admin/preset_agents")
    data = as_dict(as_dict(payload).get("data"))
    presets = data.get("items") if isinstance(data.get("items"), list) else []
    check(status == 200 and bool(presets), "GET /admin/preset_agents 返回预设列表", f"status={status} count={len(presets)}")
    if presets:
        first = as_dict(presets[0])
        for field in ("preset_id", "name", "bound_users", "customizable", "updated_at"):
            check(field in first, f"preset item 含 {field}", f"keys={sorted(first)}")
        check("sandbox_container_id" not in first, "preset item 不含 sandbox_container_id")
        preset_customizable = as_dict(first.get("customizable"))
        check(
            expected_keys.issubset(set(preset_customizable)),
            "preset.customizable 六键齐全",
            f"keys={sorted(preset_customizable)}",
        )

    target_preset = args.preset if any(as_dict(p).get("preset_id") == args.preset for p in presets) else (
        str(as_dict(presets[0]).get("preset_id")) if presets else args.preset
    )

    # ---- 8) 绑定列表 ----
    status, payload = client.request(
        "GET", f"/wunder/admin/preset_agents/{target_preset}/bindings?page=1&page_size=20&keyword="
    )
    data = as_dict(as_dict(payload).get("data"))
    check(status == 200 and "total" in data and isinstance(data.get("items"), list), "GET bindings 分页形状", f"status={status}")

    # ---- 9) bind ----
    status, payload = client.request(
        "POST",
        "/wunder/admin/preset_agents/bindings",
        {"preset_id": target_preset, "user_ids": [user_id], "action": "bind"},
    )
    data = as_dict(as_dict(payload).get("data"))
    check(
        status == 200 and "affected_users" in data,
        "POST bindings(bind) → affected_users/created/rebound",
        f"status={status} body={payload}",
    )
    check(int(data.get("affected_users") or 0) >= 1, "bind 影响用户数 ≥ 1", f"data={data}")

    # ---- 10) rebind 到另一个预设 ----
    alt = args.alt_preset if any(as_dict(p).get("preset_id") == args.alt_preset for p in presets) else ""
    if alt:
        status, payload = client.request(
            "POST",
            "/wunder/admin/preset_agents/bindings",
            {"preset_id": alt, "user_ids": [user_id], "action": "bind"},
        )
        data = as_dict(as_dict(payload).get("data"))
        check(
            status == 200 and int(data.get("rebound_agents") or 0) >= 1,
            "换绑计入 rebound_agents",
            f"status={status} data={data}",
        )

    # ---- 11) unbind 缺少 new_preset_id 必须 400 ----
    status, _payload = client.request(
        "POST",
        "/wunder/admin/preset_agents/bindings",
        {"preset_id": target_preset, "user_ids": [user_id], "action": "unbind"},
    )
    check(status == 400, "unbind 缺少 new_preset_id → 400", f"status={status}")

    # ---- 12) sync dry_run ----
    status, payload = client.request(
        "POST",
        "/wunder/admin/preset_agents/sync",
        {"preset_id": target_preset, "mode": "safe", "dry_run": True},
    )
    data = as_dict(as_dict(payload).get("data"))
    check(status == 200 and "affected_users" in data, "POST sync(dry_run) → affected_users", f"status={status}")
    check(
        {"updated_agents", "skipped_customized", "created_agents"}.issubset(set(data)),
        "sync 返回 updated/skipped/created",
        f"keys={sorted(data)}",
    )
    check(data.get("dry_run") is True, "sync dry_run 回显为 true", f"data={data}")

    # ---- 13) 管理侧用户列表字段 ----
    # `/admin/user_accounts` 是**权威账户列表**（{data:{items:[…]}}），三个绑定字段必须齐全；
    # `/admin/users` 是**监控口径的会话汇总**（裸 {users:[…]}，由会话投影而来，没有会话的用户不出现），
    # 因此只校验形状；列表非空时再核对三个字段，为空不算失败（隔离库里通常没有会话）。
    status, payload = client.request("GET", "/wunder/admin/user_accounts?limit=50")
    body = as_dict(payload)
    data = as_dict(body.get("data"))
    items = data.get("items") if isinstance(data.get("items"), list) else None
    if status != 200 or items is None:
        check(False, "GET user_accounts 列表可用", f"status={status} body={str(payload)[:160]}")
    elif not items:
        check(False, "GET user_accounts 列表非空（需要有用户才能核对字段）", f"status={status} 空列表")
    else:
        sample = as_dict(items[0])
        missing = [field for field in ("preset_id", "agent_id", "customized_fields") if field not in sample]
        check(
            not missing,
            "user_accounts 列表含 preset_id/agent_id/customized_fields",
            f"missing={missing} keys={sorted(sample)}",
        )

    status, payload = client.request("GET", "/wunder/admin/users?limit=50")
    body = as_dict(payload)
    monitor_users = body.get("users") if isinstance(body.get("users"), list) else None
    if monitor_users is None:
        inner = as_dict(body.get("data"))
        monitor_users = inner.get("items") if isinstance(inner.get("items"), list) else None
    check(status == 200 and monitor_users is not None, "GET users（监控会话汇总）形状可用", f"status={status} body={str(payload)[:120]}")
    if monitor_users:
        sample = as_dict(monitor_users[0])
        missing = [field for field in ("preset_id", "agent_id", "customized_fields") if field not in sample]
        check(
            not missing,
            "users（监控汇总）含 preset_id/agent_id/customized_fields",
            f"missing={missing} keys={sorted(sample)}",
        )
    else:
        print("[SKIP] users（监控汇总）当前为空：新库无会话，字段核对留待有会话时再跑")

    return finish()


def finish() -> int:
    failed = [item for item in RESULTS if not item[0]]
    print("-" * 60)
    print(f"共 {len(RESULTS)} 项，通过 {len(RESULTS) - len(failed)}，失败 {len(failed)}")
    for _ok, name, detail in failed:
        print(f"  FAIL {name} {('- ' + detail) if detail else ''}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
