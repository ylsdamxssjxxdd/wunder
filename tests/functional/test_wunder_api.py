import io
import zipfile

from app.core.config import A2AServiceConfig
from tests.utils.sse import parse_sse_events


async def test_wunder_non_stream(client):
    """验证 /wunder 非流式响应的基本字段与内容。"""
    payload = {"user_id": "tester-non-stream", "question": "你好", "stream": False}
    # 使用非流式路径确保返回 JSON 结构，便于直接断言字段。
    response = await client.post("/wunder", json=payload)

    assert response.status_code == 200
    data = response.json()
    assert data["answer"] == "测试回复"
    assert data["session_id"]


async def test_wunder_stream_sse(client):
    """验证 /wunder 流式 SSE 协议能输出关键事件与最终结果。"""
    payload = {"user_id": "tester-stream", "question": "hi", "stream": True}
    response = await client.post("/wunder", json=payload)

    assert response.status_code == 200
    # 将 SSE 文本拆为事件列表，验证协议事件的完整性。
    events = parse_sse_events(response.text)
    event_types = [event.get("event") for event in events]

    # SSE 事件应该至少包含进度、LLM 输出与最终回复。
    assert "progress" in event_types
    assert "llm_output" in event_types
    assert "final" in event_types

    final_event = next(event for event in events if event.get("event") == "final")
    final_payload = final_event.get("data", {})
    assert final_payload.get("data", {}).get("answer") == "测试回复"


async def test_wunder_stream_event_order(client):
    """验证 SSE 事件顺序：最终事件应在输出序列末尾。"""
    payload = {"user_id": "tester-stream-order", "question": "测试顺序", "stream": True}
    response = await client.post("/wunder", json=payload)

    assert response.status_code == 200
    events = parse_sse_events(response.text)
    assert events

    event_types = [event.get("event") for event in events]
    assert event_types[-1] == "final"
    assert event_types.index("progress") < event_types.index("final")


async def test_wunder_busy_rejected(client, orchestrator, monkeypatch):
    """验证同一 user_id 并发时会收到 429 拒绝。"""
    async def _reject_acquire(*_args, **_kwargs):
        return False

    # 通过并发限制器模拟用户繁忙，确保返回 429。
    monkeypatch.setattr(orchestrator._request_limiter, "acquire", _reject_acquire)
    # 主动模拟用户被占用，触发后端限流分支。
    payload = {"user_id": "tester-busy", "question": "busy", "stream": False}
    response = await client.post("/wunder", json=payload)

    assert response.status_code == 429
    detail = response.json().get("detail", {})
    assert "message" in detail


async def test_wunder_missing_fields(client):
    """验证缺失必填字段会触发 422 参数错误。"""
    response = await client.post("/wunder", json={"question": "缺少用户"})
    assert response.status_code == 422

    response = await client.post("/wunder", json={"user_id": "tester-missing"})
    assert response.status_code == 422


async def test_system_prompt(client):
    """验证 /wunder/system_prompt 能正常返回提示词文本。"""
    payload = {"user_id": "tester-prompt"}
    response = await client.post("/wunder/system_prompt", json=payload)

    assert response.status_code == 200
    data = response.json()
    assert "WUNDER" in data["prompt"]


async def test_tools_list(client):
    """验证 /wunder/tools 返回结构完整。"""
    response = await client.get("/wunder/tools")

    assert response.status_code == 200
    data = response.json()
    assert "builtin_tools" in data
    assert "a2a_tools" in data
    assert isinstance(data["builtin_tools"], list)
    assert isinstance(data["a2a_tools"], list)


async def test_admin_a2a_update(client, monkeypatch):
    """验证 A2A 服务更新接口返回结构与配置写入流程。"""
    from app.api.routes import admin as admin_routes

    def _fake_apply_config_update(target_orchestrator, _config_path, _updater, *args, **_kwargs):
        """替换配置更新流程，避免写入真实配置文件。"""
        updated = target_orchestrator.config.model_copy(deep=True)
        services = args[0] if args else []
        updated.a2a.services = [A2AServiceConfig(**service) for service in services]
        target_orchestrator.apply_config(updated)
        return updated

    monkeypatch.setattr(admin_routes, "apply_config_update", _fake_apply_config_update)
    payload = {
        "services": [
            {
                "name": "demo-a2a",
                "endpoint": "http://example.com/a2a",
                "enabled": True,
                "description": "测试服务",
                "display_name": "Demo A2A",
            }
        ]
    }
    response = await client.post("/wunder/admin/a2a", json=payload)

    assert response.status_code == 200
    data = response.json()
    assert isinstance(data.get("services"), list)
    assert data["services"][0]["name"] == "demo-a2a"
    assert data["services"][0]["endpoint"] == "http://example.com/a2a"


async def test_workspace_roundtrip(client):
    """验证工作区上传/列表/下载/删除的闭环流程。"""
    user_id = "tester-workspace"
    files = {"files": ("hello.txt", b"hello", "text/plain")}
    data = {"user_id": user_id, "path": ""}

    # 先上传文件，再通过列表与下载验证落盘内容。
    upload_response = await client.post("/wunder/workspace/upload", data=data, files=files)
    assert upload_response.status_code == 200
    assert upload_response.json().get("ok") is True

    list_response = await client.get("/wunder/workspace", params={"user_id": user_id})
    assert list_response.status_code == 200
    entries = list_response.json().get("entries", [])
    assert any(entry.get("name") == "hello.txt" for entry in entries)

    download_response = await client.get(
        "/wunder/workspace/download", params={"user_id": user_id, "path": "hello.txt"}
    )
    assert download_response.status_code == 200
    assert download_response.content == b"hello"

    delete_response = await client.delete(
        "/wunder/workspace", params={"user_id": user_id, "path": "hello.txt"}
    )
    assert delete_response.status_code == 200
    assert delete_response.json().get("ok") is True


async def test_workspace_path_escape_rejected(client):
    """验证工作区路径越界会被拒绝。"""
    response = await client.get(
        "/wunder/workspace", params={"user_id": "tester-escape", "path": "../"}
    )

    assert response.status_code == 400
    detail = response.json().get("detail", {})
    assert "message" in detail


async def test_workspace_file_update_not_found(client):
    """验证更新不存在的工作区文件会返回 404。"""
    payload = {"user_id": "tester-file-missing", "path": "missing.txt", "content": "x"}
    response = await client.post("/wunder/workspace/file", json=payload)

    assert response.status_code == 404
    detail = response.json().get("detail", {})
    assert "message" in detail


async def test_monitor_list(client):
    """验证 /wunder/admin/monitor 返回系统资源字段。"""
    response = await client.get("/wunder/admin/monitor")

    assert response.status_code == 200
    data = response.json()
    assert "system" in data
    assert "cpu_percent" in data["system"]


async def test_workspace_archive_dir(client):
    """验证工作区目录可压缩下载。"""
    user_id = "tester-archive-dir"
    response = await client.post("/wunder/workspace/dir", json={"user_id": user_id, "path": "logs"})
    assert response.status_code == 200

    response = await client.post(
        "/wunder/workspace/file",
        json={
            "user_id": user_id,
            "path": "logs/hello.txt",
            "content": "hi",
            "create_if_missing": True,
        },
    )
    assert response.status_code == 200

    response = await client.get(
        "/wunder/workspace/archive",
        params={"user_id": user_id, "path": "logs"},
    )
    assert response.status_code == 200
    with zipfile.ZipFile(io.BytesIO(response.content)) as zipf:
        assert "logs/hello.txt" in zipf.namelist()
