from pathlib import Path

from app.monitor.registry import monitor
from app.memory.workspace import WorkspaceContext
from app.tools import builtin
from app.tools.types import ToolContext


def _build_tool_context(session_id: str, user_id: str, root: Path) -> ToolContext:
    """构造 A2A 工具所需上下文，避免引入额外依赖。"""
    workspace = WorkspaceContext(user_id=user_id, session_id=session_id, root=root)
    return ToolContext(
        workspace=workspace,
        config={"a2a_services": [], "a2a_timeout_s": 1},
        emit_event=None,
    )


def test_a2a_observe_collects_monitor_events(orchestrator, tmp_path: Path):
    """验证 a2a_observe 可汇总监控中的 A2A 任务状态。"""
    session_id = "a2a-observe-1"
    monitor.register(session_id, "user-1", "a2a observe")
    monitor.record_event(
        session_id,
        "a2a_task",
        {
            "task_id": "task-1",
            "context_id": "ctx-1",
            "endpoint": "http://example.local/a2a",
            "method": "SendMessage",
            "service_name": "demo",
        },
    )
    monitor.record_event(
        session_id,
        "a2a_status",
        {"task_id": "task-1", "context_id": "ctx-1", "state": "completed"},
    )
    monitor.record_event(
        session_id,
        "a2a_result",
        {"task_id": "task-1", "context_id": "ctx-1", "status": "completed", "ok": True},
    )

    tool_ctx = _build_tool_context(session_id, "user-1", tmp_path)
    result = builtin.a2a_observe(tool_ctx, {"refresh": False})

    assert result.ok is True
    tasks = result.data.get("tasks", [])
    assert len(tasks) == 1
    assert tasks[0].get("task_id") == "task-1"
    assert result.data.get("done") is True


def test_a2a_wait_times_out_when_pending(orchestrator, tmp_path: Path):
    """验证 a2a_wait 在任务未完成时按超时返回。"""
    session_id = "a2a-wait-1"
    monitor.register(session_id, "user-2", "a2a wait")
    monitor.record_event(
        session_id,
        "a2a_task",
        {
            "task_id": "task-2",
            "context_id": "ctx-2",
            "endpoint": "http://example.local/a2a",
            "method": "SendMessage",
        },
    )
    monitor.record_event(
        session_id,
        "a2a_status",
        {"task_id": "task-2", "context_id": "ctx-2", "state": "working"},
    )

    tool_ctx = _build_tool_context(session_id, "user-2", tmp_path)
    result = builtin.a2a_wait(tool_ctx, {"wait_s": 0.1, "refresh": False})

    assert result.ok is True
    assert result.data.get("timeout") is True
    assert result.data.get("pending")
