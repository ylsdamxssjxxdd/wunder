import json


def _parse_a2a_sse(text: str):
    """解析 A2A SSE 数据行，返回事件 payload 列表。"""
    events = []
    for line in text.splitlines():
        if not line.startswith("data:"):
            continue
        raw = line[len("data:") :].strip()
        events.append(json.loads(raw))
    return events


async def test_a2a_agent_card(client):
    """验证 AgentCard 发现入口可用。"""
    response = await client.get("/.well-known/agent-card.json")

    assert response.status_code == 200
    data = response.json()
    assert data.get("protocolVersion")
    assert any(
        iface.get("url", "").endswith("/a2a")
        for iface in data.get("supportedInterfaces", [])
    )


async def test_a2a_send_message_blocking(client):
    """验证 SendMessage 阻塞模式返回 Task。"""
    payload = {
        "jsonrpc": "2.0",
        "id": "req-1",
        "method": "SendMessage",
        "params": {
            "userId": "tester-a2a",
            "configuration": {"blocking": True, "historyLength": 2},
            "message": {
                "role": "user",
                "parts": [{"text": "你好"}],
            },
        },
    }
    response = await client.post("/a2a", json=payload)

    assert response.status_code == 200
    data = response.json().get("result", {})
    task = data.get("task", {})
    assert task.get("status", {}).get("state") == "completed"
    artifacts = task.get("artifacts", [])
    assert any(
        part.get("text") == "测试回复"
        for artifact in artifacts
        for part in artifact.get("parts", [])
        if isinstance(part, dict)
    )


async def test_a2a_send_streaming_message(client):
    """验证 SendStreamingMessage 输出 SSE 更新序列。"""
    payload = {
        "jsonrpc": "2.0",
        "id": "req-2",
        "method": "SendStreamingMessage",
        "params": {
            "userId": "tester-a2a-stream",
            "message": {"role": "user", "parts": [{"text": "hi"}]},
        },
    }
    response = await client.post(
        "/a2a",
        json=payload,
        headers={"Accept": "text/event-stream"},
    )

    assert response.status_code == 200
    events = _parse_a2a_sse(response.text)
    assert events
    assert "task" in events[0]

    status_updates = [
        event.get("statusUpdate") for event in events if "statusUpdate" in event
    ]
    assert status_updates
    assert status_updates[-1].get("final") is True
    assert status_updates[-1].get("status", {}).get("state") == "completed"

    artifact_updates = [
        event.get("artifactUpdate") for event in events if "artifactUpdate" in event
    ]
    assert any(
        part.get("text") == "测试回复"
        for update in artifact_updates
        for part in update.get("artifact", {}).get("parts", [])
        if isinstance(part, dict)
    )


async def test_a2a_get_task_and_list(client):
    """验证 GetTask/ListTasks 返回任务信息。"""
    send_payload = {
        "jsonrpc": "2.0",
        "id": "req-3",
        "method": "SendMessage",
        "params": {
            "userId": "tester-a2a-list",
            "configuration": {"blocking": True},
            "message": {"role": "user", "parts": [{"text": "list"}]},
        },
    }
    send_response = await client.post("/a2a", json=send_payload)
    task_id = send_response.json().get("result", {}).get("task", {}).get("id")

    assert task_id

    get_payload = {
        "jsonrpc": "2.0",
        "id": "req-4",
        "method": "GetTask",
        "params": {"name": f"tasks/{task_id}", "historyLength": 1},
    }
    get_response = await client.post("/a2a", json=get_payload)

    assert get_response.status_code == 200
    task = get_response.json().get("result", {})
    assert task.get("id") == task_id

    list_payload = {
        "jsonrpc": "2.0",
        "id": "req-5",
        "method": "ListTasks",
        "params": {"pageSize": 20},
    }
    list_response = await client.post("/a2a", json=list_payload)

    assert list_response.status_code == 200
    tasks = list_response.json().get("result", {}).get("tasks", [])
    assert any(item.get("id") == task_id for item in tasks)
