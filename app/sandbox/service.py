from __future__ import annotations

import asyncio
import posixpath
from pathlib import PurePosixPath
from typing import List

from fastapi import FastAPI, HTTPException

from app.core.i18n import resolve_language, reset_language, set_language, t
from app.sandbox.runner import execute_payload
from app.sandbox.schemas import (
    SandboxReleaseRequest,
    SandboxReleaseResponse,
    SandboxToolRequest,
    SandboxToolResponse,
)


def _normalize_container_path(raw_path: str, container_root: PurePosixPath) -> PurePosixPath:
    """规范化容器内路径，确保位于 container_root 下。"""
    raw = str(raw_path).strip()
    if not raw:
        raise ValueError(t("sandbox.error.path_required"))
    path = PurePosixPath(raw)
    if not path.is_absolute():
        path = container_root / path
    normalized = PurePosixPath(posixpath.normpath(path.as_posix()))
    try:
        normalized.relative_to(container_root)
    except ValueError as exc:
        raise ValueError(t("sandbox.error.path_out_of_bounds")) from exc
    return normalized


def _filter_allow_paths(allow_paths: List[str], container_root: PurePosixPath) -> List[str]:
    """过滤不在容器根目录下的白名单路径，避免越界访问。"""
    results: List[str] = []
    for raw_path in allow_paths:
        text = str(raw_path).strip()
        if not text:
            continue
        try:
            normalized = _normalize_container_path(text, container_root)
        except ValueError:
            # 非容器根目录下的路径直接忽略
            continue
        results.append(normalized.as_posix())
    return results


def create_app() -> FastAPI:
    """创建共享沙盒服务应用实例。"""
    app = FastAPI(title="wunder-shared-sandbox", version="0.1.0")

    @app.get("/health")
    async def health() -> dict:
        """健康检查接口。"""
        return {"ok": True}

    @app.post("/sandboxes/execute_tool", response_model=SandboxToolResponse)
    async def execute_tool(request: SandboxToolRequest) -> SandboxToolResponse:
        """在共享沙盒中执行内置工具。"""
        language = resolve_language([request.language])
        token = set_language(language)
        try:
            container_root = PurePosixPath(request.container_root)
            try:
                workspace_root = _normalize_container_path(
                    request.workspace_root, container_root
                )
            except ValueError as exc:
                raise HTTPException(
                    status_code=400, detail={"message": str(exc)}
                ) from exc

            payload = request.model_dump()
            payload["workspace_root"] = workspace_root.as_posix()
            payload["allow_paths"] = _filter_allow_paths(request.allow_paths, container_root)

            result, debug_events = await asyncio.to_thread(execute_payload, payload)
            return SandboxToolResponse(
                ok=result.ok,
                data=result.data,
                error=result.error,
                debug_events=debug_events,
            )
        finally:
            reset_language(token)

    @app.post("/sandboxes/release", response_model=SandboxReleaseResponse)
    async def release_sandbox(request: SandboxReleaseRequest) -> SandboxReleaseResponse:
        """释放沙盒资源，共享沙盒下为幂等空操作。"""
        language = resolve_language([request.language])
        token = set_language(language)
        try:
            _ = request  # 共享沙盒不区分用户容器，释放为幂等空操作
            return SandboxReleaseResponse(
                ok=True, message=t("sandbox.message.release_not_required")
            )
        finally:
            reset_language(token)

    return app


app = create_app()
