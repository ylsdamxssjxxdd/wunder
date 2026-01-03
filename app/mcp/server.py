from __future__ import annotations

import asyncio
from typing import Any, Callable, List, Optional

from app.api.deps import get_orchestrator
from app.core.i18n import t
from app.schemas.wunder import WunderRequest
from app.tools.availability import collect_available_tool_names

MCP_SERVER_NAME = "wunder"
MCP_TOOL_NAME = "run"
MCP_TOOL_FULL_NAME = f"{MCP_SERVER_NAME}@{MCP_TOOL_NAME}"
MCP_USER_ID = "wunder"


def _build_allowed_tool_names() -> List[str]:
    """基于管理员启用配置构建工具白名单，并剔除自调用工具。"""
    orchestrator = get_orchestrator()
    available = collect_available_tool_names(
        orchestrator.config,
        orchestrator.skills.list_specs(),
    )
    available.discard(MCP_TOOL_FULL_NAME)
    # a2ui 仅在显式勾选时开放，MCP 默认不注入该工具。
    available.discard("a2ui")
    return sorted(available)


async def run_wunder_task(task: str) -> dict:
    """执行 wunder 智能体任务并返回结果。"""
    cleaned = str(task or "").strip()
    if not cleaned:
        raise ValueError(t("error.task_required"))
    # 统一封装内部执行逻辑，供 MCP 工具与直连调用复用。
    tool_names = _build_allowed_tool_names()
    request = WunderRequest(
        user_id=MCP_USER_ID,
        question=cleaned,
        stream=False,
        tool_names=tool_names,
    )
    result = await get_orchestrator().run(request)
    return {
        "answer": result.answer,
        "session_id": result.session_id,
        "usage": result.usage,
        "uid": getattr(result, "uid", None),
        "a2ui": getattr(result, "a2ui", None),
    }


def _build_mcp_server():
    """延迟创建 FastMCP 服务，避免导入期加载重依赖。"""
    from fastmcp import FastMCP

    instructions = t("mcp.instructions")
    description = t("mcp.tool.run.description")
    mcp_server = FastMCP(
        name=MCP_SERVER_NAME,
        instructions=instructions,
    )

    @mcp_server.tool(
        name=MCP_TOOL_NAME,
        description=description,
    )
    async def wunder_run(task: str) -> dict:
        """执行 wunder 智能体任务。"""
        return await run_wunder_task(task)

    return mcp_server


def _build_mcp_app():
    """创建 Wunder MCP 的 ASGI 应用。"""
    server = _build_mcp_server()
    return server.http_app(path="/", transport="streamable-http")


async def _run_lifecycle(app: Any, action: str) -> None:
    """触发 ASGI 应用的启动/关闭钩子，兼容不同实现。"""
    target = getattr(app, action, None)
    if callable(target):
        result = target()
        if asyncio.iscoroutine(result):
            await result
        return
    router = getattr(app, "router", None)
    target = getattr(router, action, None) if router else None
    if callable(target):
        result = target()
        if asyncio.iscoroutine(result):
            await result


class LazyMCPApp:
    """惰性加载 MCP ASGI 应用，减少服务启动耗时。"""

    def __init__(self, factory: Callable[[], Any]) -> None:
        self._factory = factory
        self._app: Optional[Any] = None
        self._lock = asyncio.Lock()
        self._started = False
        self._closed = False
        self._lifespan_ctx: Optional[Any] = None

    async def _open_lifespan(self) -> None:
        """延迟打开 FastMCP 的 lifespan，确保流式会话管理器初始化。"""
        if self._app is None or self._lifespan_ctx is not None:
            return
        lifespan = getattr(self._app, "lifespan", None)
        if callable(lifespan):
            self._lifespan_ctx = lifespan(self._app)
            await self._lifespan_ctx.__aenter__()

    async def _ensure_app(self) -> Any:
        if self._app is not None:
            return self._app
        async with self._lock:
            if self._app is not None:
                return self._app
            self._app = self._factory()
            if not self._started:
                self._started = True
                # 初始化 lifespan 以激活 StreamableHTTP 会话管理，再触发生命周期钩子。
                await self._open_lifespan()
                # 首次请求时补上启动钩子，保证组件正常初始化。
                await _run_lifecycle(self._app, "startup")
            return self._app

    async def __call__(self, scope, receive, send) -> None:
        app = await self._ensure_app()
        await app(scope, receive, send)

    async def startup(self) -> None:
        """占位启动钩子，避免主服务启动时触发 MCP 初始化。"""
        return

    async def shutdown(self) -> None:
        """在主服务关闭时释放 MCP 资源。"""
        if self._app is None or self._closed:
            return
        self._closed = True
        await _run_lifecycle(self._app, "shutdown")
        if self._lifespan_ctx is not None:
            await self._lifespan_ctx.__aexit__(None, None, None)


def create_wunder_mcp_app():
    """创建 Wunder MCP 的延迟加载入口。"""
    return LazyMCPApp(_build_mcp_app)
