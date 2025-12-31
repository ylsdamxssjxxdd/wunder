from __future__ import annotations

import asyncio
import os
from typing import Any, Callable, Optional


def _load_fastapi_app() -> Any:
    """懒加载主应用，避免进程启动阶段导入重依赖。"""
    from app.main import app as fastapi_app

    return fastapi_app


def _resolve_warmup_delay() -> Optional[float]:
    """解析后台预热延迟，支持通过环境变量关闭。"""
    raw = os.getenv("WUNDER_LAZY_WARMUP_S", "").strip().lower()
    if not raw:
        return 1.0
    if raw in {"off", "disable", "false", "none"}:
        return None
    try:
        value = float(raw)
    except ValueError:
        return 0.0
    if value < 0:
        return None
    return value


async def _maybe_await(result: Any) -> None:
    """统一处理同步/异步返回值，避免判断分支散落。"""
    if asyncio.iscoroutine(result):
        await result


class LazyASGIApp:
    """惰性加载 ASGI 应用，确保秒起并在后台完成初始化。"""

    def __init__(self, factory: Callable[[], Any], warmup_delay_s: Optional[float]) -> None:
        self._factory = factory
        self._app: Optional[Any] = None
        self._lock = asyncio.Lock()
        self._lifespan_cm: Optional[Any] = None
        self._started = False
        self._closed = False
        self._warmup_task: Optional[asyncio.Task] = None
        self._warmup_delay_s = warmup_delay_s

    async def _ensure_app(self) -> Any:
        if self._app is not None:
            return self._app
        async with self._lock:
            if self._app is not None:
                return self._app
            app = self._factory()
            self._app = app
            await self._startup_app(app)
            return self._app

    async def _startup_app(self, app: Any) -> None:
        if self._started:
            return
        self._started = True
        router = getattr(app, "router", None)
        lifespan_factory = getattr(router, "lifespan_context", None) if router else None
        if callable(lifespan_factory):
            try:
                lifespan_cm = lifespan_factory(app)
            except TypeError:
                lifespan_cm = lifespan_factory
            if hasattr(lifespan_cm, "__aenter__"):
                try:
                    self._lifespan_cm = lifespan_cm
                    await lifespan_cm.__aenter__()
                    return
                except Exception:
                    self._lifespan_cm = None
        if router and hasattr(router, "startup"):
            await _maybe_await(router.startup())

    async def _shutdown_app(self) -> None:
        if self._closed:
            return
        self._closed = True
        if self._app is None:
            return
        if self._lifespan_cm is not None:
            await self._lifespan_cm.__aexit__(None, None, None)
            return
        router = getattr(self._app, "router", None)
        if router and hasattr(router, "shutdown"):
            await _maybe_await(router.shutdown())

    async def _warmup(self) -> None:
        if self._warmup_delay_s is None:
            return
        delay = self._warmup_delay_s
        if delay > 0:
            await asyncio.sleep(delay)
        try:
            await self._ensure_app()
        except Exception:
            return

    async def _handle_lifespan(self, receive, send) -> None:
        while True:
            message = await receive()
            if message["type"] == "lifespan.startup":
                if self._warmup_task is None and self._warmup_delay_s is not None:
                    # 启动后后台预热，避免阻塞进程启动。
                    self._warmup_task = asyncio.create_task(self._warmup())
                await send({"type": "lifespan.startup.complete"})
                continue
            if message["type"] == "lifespan.shutdown":
                if self._warmup_task is not None:
                    self._warmup_task.cancel()
                try:
                    await self._shutdown_app()
                    await send({"type": "lifespan.shutdown.complete"})
                except Exception:
                    await send({"type": "lifespan.shutdown.failed"})
                return

    async def __call__(self, scope, receive, send) -> None:
        if scope["type"] == "lifespan":
            await self._handle_lifespan(receive, send)
            return
        app = await self._ensure_app()
        await app(scope, receive, send)


# 秒起入口：通过 uvicorn app.asgi:app 启动，并支持后台预热。
app = LazyASGIApp(_load_fastapi_app, _resolve_warmup_delay())
