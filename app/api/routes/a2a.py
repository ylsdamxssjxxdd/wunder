from __future__ import annotations

import json
from typing import Any, AsyncGenerator, Dict, Optional

from fastapi import APIRouter, Request
from fastapi.responses import StreamingResponse

from app.a2a.constants import (
    A2A_ERROR_CODES,
    A2A_PROTOCOL_VERSION,
    JSONRPC_ERROR_CODES,
    JSONRPC_VERSION,
)
from app.a2a.service import A2AError, A2AService
from app.api.deps import get_orchestrator
from app.api.responses import json_response
from app.core.i18n import t


router = APIRouter()


def _build_jsonrpc_result(request_id: Any, result: Any) -> Dict[str, Any]:
    """构造 JSON-RPC 成功响应体。"""
    return {"jsonrpc": JSONRPC_VERSION, "id": request_id, "result": result}


def _build_jsonrpc_error(
    request_id: Any,
    code: int,
    message: str,
    data: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """构造 JSON-RPC 错误响应体。"""
    payload: Dict[str, Any] = {"code": int(code), "message": message}
    if data:
        payload["data"] = data
    return {"jsonrpc": JSONRPC_VERSION, "id": request_id, "error": payload}


def _resolve_base_url(request: Request) -> str:
    """从请求中解析基础 URL，用于构造 AgentCard。"""
    return str(request.base_url).rstrip("/")


def _ensure_a2a_version(request: Request) -> None:
    """校验 A2A-Version 请求头，避免协议版本不匹配。"""
    version = (request.headers.get("a2a-version") or "").strip()
    if not version:
        return
    normalized = version.lower().lstrip("v")
    supported = {A2A_PROTOCOL_VERSION, A2A_PROTOCOL_VERSION.lower().lstrip("v")}
    if normalized not in supported:
        raise A2AError(
            A2A_ERROR_CODES["VersionNotSupportedError"],
            "不支持的 A2A 协议版本",
            {"version": version},
        )


async def _stream_as_sse(
    source: AsyncGenerator[Dict[str, Any], None]
) -> AsyncGenerator[str, None]:
    """将 A2A 事件流转换为 SSE data 行输出。"""
    async for payload in source:
        data = json.dumps(payload, ensure_ascii=False)
        yield f"data: {data}\n\n"


@router.get("/.well-known/agent-card.json")
async def a2a_agent_card(request: Request):
    """公开 AgentCard，用于 A2A 服务发现。"""
    service = A2AService(get_orchestrator())
    base_url = _resolve_base_url(request)
    return json_response(service.build_agent_card(base_url))


@router.get("/a2a/extendedAgentCard")
async def a2a_extended_agent_card(request: Request):
    """返回扩展 AgentCard，当前与基础版一致。"""
    service = A2AService(get_orchestrator())
    base_url = _resolve_base_url(request)
    result = await service.get_extended_agent_card(base_url)
    return json_response(result)


@router.post("/a2a")
async def a2a_jsonrpc(request: Request):
    """A2A JSON-RPC 入口，支持标准请求与 SSE 流式响应。"""
    try:
        payload = await request.json()
    except json.JSONDecodeError:
        error_payload = _build_jsonrpc_error(
            None,
            JSONRPC_ERROR_CODES["ParseError"],
            "Parse error",
        )
        return json_response(error_payload)

    if not isinstance(payload, dict):
        error_payload = _build_jsonrpc_error(
            None,
            JSONRPC_ERROR_CODES["InvalidRequest"],
            "Invalid Request",
        )
        return json_response(error_payload)

    request_id = payload.get("id")
    if payload.get("jsonrpc") != JSONRPC_VERSION:
        error_payload = _build_jsonrpc_error(
            request_id,
            JSONRPC_ERROR_CODES["InvalidRequest"],
            "Invalid Request",
        )
        return json_response(error_payload)

    method = payload.get("method")
    if not isinstance(method, str) or not method.strip():
        error_payload = _build_jsonrpc_error(
            request_id,
            JSONRPC_ERROR_CODES["InvalidRequest"],
            "Invalid Request",
        )
        return json_response(error_payload)

    params = payload.get("params") or {}
    if not isinstance(params, dict):
        error_payload = _build_jsonrpc_error(
            request_id,
            JSONRPC_ERROR_CODES["InvalidParams"],
            "Invalid params",
        )
        return json_response(error_payload)

    service = A2AService(get_orchestrator())
    try:
        _ensure_a2a_version(request)
        method_name = method.strip()
        if method_name == "SendMessage":
            result = await service.send_message(params)
            return json_response(_build_jsonrpc_result(request_id, result))
        if method_name == "SendStreamingMessage":
            stream = service.send_streaming_message(params)
            return StreamingResponse(
                _stream_as_sse(stream),
                media_type="text/event-stream; charset=utf-8",
            )
        if method_name == "SubscribeToTask":
            stream = service.subscribe_to_task(params)
            return StreamingResponse(
                _stream_as_sse(stream),
                media_type="text/event-stream; charset=utf-8",
            )
        if method_name == "GetTask":
            result = await service.get_task(params)
            return json_response(_build_jsonrpc_result(request_id, result))
        if method_name == "ListTasks":
            result = await service.list_tasks(params)
            return json_response(_build_jsonrpc_result(request_id, result))
        if method_name == "CancelTask":
            result = await service.cancel_task(params)
            return json_response(_build_jsonrpc_result(request_id, result))
        if method_name == "GetExtendedAgentCard":
            base_url = _resolve_base_url(request)
            result = await service.get_extended_agent_card(base_url)
            return json_response(_build_jsonrpc_result(request_id, result))
        if method_name in {
            "SetTaskPushNotificationConfig",
            "GetTaskPushNotificationConfig",
            "ListTaskPushNotificationConfig",
            "DeleteTaskPushNotificationConfig",
        }:
            raise A2AError(
                A2A_ERROR_CODES["PushNotificationNotSupportedError"],
                "暂不支持推送通知",
            )
        error_payload = _build_jsonrpc_error(
            request_id,
            JSONRPC_ERROR_CODES["MethodNotFound"],
            "Method not found",
        )
        return json_response(error_payload)
    except A2AError as exc:
        error_payload = _build_jsonrpc_error(request_id, exc.code, exc.message, exc.data)
        return json_response(error_payload)
    except Exception as exc:  # noqa: BLE001
        error_payload = _build_jsonrpc_error(
            request_id,
            JSONRPC_ERROR_CODES["InternalError"],
            t("error.internal_error", detail=str(exc)),
            {"detail": str(exc)},
        )
        return json_response(error_payload)
