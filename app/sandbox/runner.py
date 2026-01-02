from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any, Dict, List, Tuple

from app.core.i18n import reset_language, set_language, t
from app.memory.workspace import WorkspaceContext
from app.tools.catalog import build_sandbox_tool_handlers
from app.tools.types import ToolContext, ToolResult


BUILTIN_TOOL_MAP = build_sandbox_tool_handlers()


def _read_payload() -> Dict[str, Any]:
    """从标准输入读取沙盒执行请求。"""
    raw = sys.stdin.read()
    if not raw:
        return {}
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return {}


def _build_context(payload: Dict[str, Any]) -> Tuple[ToolContext, List[Dict[str, Any]]]:
    """构建工具上下文，并收集工具内部调试事件。"""
    user_id = str(payload.get("user_id", "") or "")
    session_id = str(payload.get("session_id", "") or "")
    workspace_root = str(payload.get("workspace_root", "") or "")
    root_path = Path(workspace_root).resolve()

    debug_events: List[Dict[str, Any]] = []

    def _emit_event(event_type: str, data: Dict[str, Any]) -> None:
        # 将工具内部上报事件收集起来，返回给 orchestrator 处理
        debug_events.append({"type": event_type, "data": data})

    context = ToolContext(
        workspace=WorkspaceContext(user_id=user_id, session_id=session_id, root=root_path),
        config={
            "allow_commands": payload.get("allow_commands", []) or [],
            "allow_paths": payload.get("allow_paths", []) or [],
            "deny_globs": payload.get("deny_globs", []) or [],
        },
        emit_event=_emit_event,
    )
    return context, debug_events


def _execute_tool(payload: Dict[str, Any]) -> Tuple[ToolResult, List[Dict[str, Any]]]:
    """根据工具名称执行内置工具。"""
    tool_name = str(payload.get("tool", "") or "")
    args = payload.get("args", {}) or {}
    context, debug_events = _build_context(payload)
    func = BUILTIN_TOOL_MAP.get(tool_name)
    if not func:
        return (
            ToolResult(ok=False, data={}, error=t("sandbox.error.unsupported_tool")),
            debug_events,
        )
    try:
        return func(context, args), debug_events
    except Exception as exc:  # noqa: BLE001
        return (
            ToolResult(
                ok=False,
                data={},
                error=t("sandbox.error.tool_failed", detail=str(exc)),
            ),
            debug_events,
        )


def execute_payload(payload: Dict[str, Any]) -> Tuple[ToolResult, List[Dict[str, Any]]]:
    """供共享沙盒服务调用的入口，直接执行工具并返回结果。"""
    language = str(payload.get("language") or "").strip()
    token = set_language(language) if language else None
    try:
        return _execute_tool(payload)
    finally:
        if token is not None:
            reset_language(token)


def _write_response(result: ToolResult, debug_events: List[Dict[str, Any]]) -> None:
    """将工具执行结果输出为 JSON。"""
    payload = {
        "ok": result.ok,
        "data": result.data,
        "error": result.error,
        "debug_events": debug_events,
    }
    sys.stdout.write(json.dumps(payload, ensure_ascii=False))
    sys.stdout.flush()


def main() -> None:
    """入口函数：读取请求、执行工具并输出结果。"""
    payload = _read_payload()
    if not payload:
        _write_response(
            ToolResult(ok=False, data={}, error=t("sandbox.error.payload_invalid")),
            [],
        )
        return
    result, debug_events = _execute_tool(payload)
    _write_response(result, debug_events)


if __name__ == "__main__":
    main()
