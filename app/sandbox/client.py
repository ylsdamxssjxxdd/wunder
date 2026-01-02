from __future__ import annotations

import json
from typing import Any, Dict, List, Tuple

import httpx

from app.core.config import SandboxConfig
from app.core.http_client import get_async_client
from app.core.i18n import get_language, t
from app.memory.workspace import WorkspaceContext
from app.tools.types import ToolResult


class SandboxClientError(RuntimeError):
    """沙盒调用异常。"""


class SandboxClient:
    """沙盒客户端，封装沙盒服务调用。"""

    def __init__(self, config: SandboxConfig) -> None:
        self._config = config
        self._base_url = config.endpoint.rstrip("/")

    def _build_payload(
        self,
        tool_name: str,
        args: Dict[str, Any],
        workspace: WorkspaceContext,
        allow_paths: List[str],
        deny_globs: List[str],
        allow_commands: List[str],
    ) -> Dict[str, Any]:
        """拼装沙盒执行请求，保持工具参数格式不变。"""
        if not self._config.image:
            raise SandboxClientError(t("sandbox.error.image_missing"))
        return {
            "user_id": workspace.user_id,
            "session_id": workspace.session_id,
            "language": get_language(),
            "tool": tool_name,
            "args": args,
            "workspace_root": str(workspace.root),
            "allow_paths": allow_paths,
            "deny_globs": deny_globs,
            "allow_commands": allow_commands,
            "container_root": self._config.container_root,
            "image": self._config.image,
            "network": self._config.network,
            "readonly_rootfs": self._config.readonly_rootfs,
            "idle_ttl_s": self._config.idle_ttl_s,
            "resources": {
                "cpu": self._config.resources.cpu,
                "memory_mb": self._config.resources.memory_mb,
                "pids": self._config.resources.pids,
            },
        }

    async def execute_tool(
        self,
        tool_name: str,
        args: Dict[str, Any],
        workspace: WorkspaceContext,
        allow_paths: List[str],
        deny_globs: List[str],
        allow_commands: List[str],
    ) -> Tuple[ToolResult, List[Dict[str, Any]]]:
        """请求沙盒执行内置工具，返回工具结果与调试事件列表。"""
        payload = self._build_payload(
            tool_name,
            args,
            workspace,
            allow_paths,
            deny_globs,
            allow_commands,
        )
        timeout = httpx.Timeout(self._config.timeout_s)
        try:
            client = await get_async_client()
            response = await client.post(
                f"{self._base_url}/sandboxes/execute_tool",
                json=payload,
                timeout=timeout,
            )
        except Exception as exc:  # noqa: BLE001
            raise SandboxClientError(
                t("sandbox.error.request_failed", detail=str(exc))
            ) from exc

        if response.status_code != 200:
            detail = response.text.strip()
            raise SandboxClientError(
                t(
                    "sandbox.error.response_error",
                    status=response.status_code,
                    detail=detail,
                )
            )

        try:
            body = response.json()
        except json.JSONDecodeError as exc:
            raise SandboxClientError(
                t("sandbox.error.response_not_json", detail=str(exc))
            ) from exc

        ok = bool(body.get("ok"))
        data = body.get("data") if isinstance(body.get("data"), dict) else {}
        error = str(body.get("error", "") or "")
        debug_events = (
            body.get("debug_events") if isinstance(body.get("debug_events"), list) else []
        )
        return ToolResult(ok=ok, data=data, error=error), debug_events

    async def release_sandbox(self, user_id: str, session_id: str = "") -> None:
        """请求释放沙盒资源，兼容共享沙盒的幂等释放。"""
        timeout = httpx.Timeout(self._config.timeout_s)
        payload = {
            "user_id": user_id,
            "session_id": session_id,
            "language": get_language(),
        }
        try:
            client = await get_async_client()
            response = await client.post(
                f"{self._base_url}/sandboxes/release",
                json=payload,
                timeout=timeout,
            )
        except Exception as exc:  # noqa: BLE001
            raise SandboxClientError(
                t("sandbox.error.release_failed", detail=str(exc))
            ) from exc

        if response.status_code != 200:
            detail = response.text.strip()
            raise SandboxClientError(
                t(
                    "sandbox.error.release_response_error",
                    status=response.status_code,
                    detail=detail,
                )
            )
