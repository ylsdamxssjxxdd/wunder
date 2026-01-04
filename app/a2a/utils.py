"""A2A JSON 对象构造与格式化工具。"""

from __future__ import annotations

import json
import uuid
from datetime import datetime
from typing import Any, Dict, Iterable, List, Optional


def utc_now() -> str:
    """生成 UTC ISO 8601 时间戳，满足 A2A 对时间格式的要求。"""
    return datetime.utcnow().isoformat() + "Z"


def format_timestamp(raw: Optional[float]) -> Optional[str]:
    """将秒级时间戳转换为 ISO 8601 字符串，空值返回 None。"""
    if raw is None:
        return None
    try:
        value = float(raw)
    except (TypeError, ValueError):
        return None
    if value <= 0:
        return None
    return datetime.utcfromtimestamp(value).isoformat() + "Z"


def build_text_part(text: str) -> Dict[str, Any]:
    """构造文本 Part，简化 A2A Message/Artifact 的内容拼装。"""
    return {"text": str(text or "")}


def build_data_part(data: Dict[str, Any]) -> Dict[str, Any]:
    """构造结构化 Data Part，适用于 A2UI 或扩展数据输出。"""
    return {"data": data}


def build_message(
    *,
    role: str,
    parts: Iterable[Dict[str, Any]],
    message_id: Optional[str] = None,
    context_id: Optional[str] = None,
    task_id: Optional[str] = None,
    metadata: Optional[Dict[str, Any]] = None,
    extensions: Optional[List[str]] = None,
    reference_task_ids: Optional[List[str]] = None,
) -> Dict[str, Any]:
    """构造 A2A Message 对象，自动补全必填字段与 ID。"""
    payload: Dict[str, Any] = {
        "messageId": message_id or uuid.uuid4().hex,
        "role": role,
        "parts": list(parts),
    }
    if context_id:
        payload["contextId"] = context_id
    if task_id:
        payload["taskId"] = task_id
    if metadata:
        payload["metadata"] = metadata
    if extensions:
        payload["extensions"] = extensions
    if reference_task_ids:
        payload["referenceTaskIds"] = reference_task_ids
    return payload


def build_status(
    *,
    state: str,
    message_text: Optional[str] = None,
    context_id: Optional[str] = None,
    task_id: Optional[str] = None,
    metadata: Optional[Dict[str, Any]] = None,
    timestamp: Optional[str] = None,
) -> Dict[str, Any]:
    """构造 TaskStatus，必要时附带一条 agent 侧消息。"""
    payload: Dict[str, Any] = {"state": state}
    if message_text:
        payload["message"] = build_message(
            role="agent",
            parts=[build_text_part(message_text)],
            context_id=context_id,
            task_id=task_id,
        )
    if metadata:
        payload["metadata"] = metadata
    payload["timestamp"] = timestamp or utc_now()
    return payload


def build_artifact(
    *,
    artifact_id: str,
    parts: Iterable[Dict[str, Any]],
    name: Optional[str] = None,
    description: Optional[str] = None,
    metadata: Optional[Dict[str, Any]] = None,
    extensions: Optional[List[str]] = None,
) -> Dict[str, Any]:
    """构造 Artifact 对象，支持文本与结构化 Part。"""
    payload: Dict[str, Any] = {
        "artifactId": artifact_id,
        "parts": list(parts),
    }
    if name:
        payload["name"] = name
    if description:
        payload["description"] = description
    if metadata:
        payload["metadata"] = metadata
    if extensions:
        payload["extensions"] = extensions
    return payload


def build_task(
    *,
    task_id: str,
    context_id: str,
    status: Dict[str, Any],
    artifacts: Optional[List[Dict[str, Any]]] = None,
    history: Optional[List[Dict[str, Any]]] = None,
    metadata: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """构造 Task 对象，默认包含必填字段。"""
    payload: Dict[str, Any] = {
        "id": task_id,
        "contextId": context_id,
        "status": status,
    }
    if artifacts is not None:
        payload["artifacts"] = artifacts
    if history is not None:
        payload["history"] = history
    if metadata:
        payload["metadata"] = metadata
    return payload


def build_task_status_update_event(
    *,
    task_id: str,
    context_id: str,
    status: Dict[str, Any],
    final: bool = False,
    metadata: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """构造 TaskStatusUpdateEvent 的 JSON 结构。"""
    payload: Dict[str, Any] = {
        "taskId": task_id,
        "contextId": context_id,
        "status": status,
        "final": bool(final),
    }
    if metadata:
        payload["metadata"] = metadata
    return {"statusUpdate": payload}


def build_task_artifact_update_event(
    *,
    task_id: str,
    context_id: str,
    artifact: Dict[str, Any],
    append: bool = False,
    last_chunk: bool = True,
    metadata: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """构造 TaskArtifactUpdateEvent 的 JSON 结构。"""
    payload: Dict[str, Any] = {
        "taskId": task_id,
        "contextId": context_id,
        "artifact": artifact,
        "append": bool(append),
        "lastChunk": bool(last_chunk),
    }
    if metadata:
        payload["metadata"] = metadata
    return {"artifactUpdate": payload}


def parse_task_name(name: str) -> str:
    """解析 tasks/{id} 资源名，返回 task_id。"""
    cleaned = str(name or "").strip()
    if not cleaned:
        return ""
    if cleaned.startswith("tasks/"):
        return cleaned[len("tasks/") :].strip()
    return cleaned


def safe_json_loads(raw: str) -> Optional[Dict[str, Any]]:
    """安全解析 JSON 字符串，异常时返回 None。"""
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError:
        return None
    if isinstance(payload, dict):
        return payload
    return None
