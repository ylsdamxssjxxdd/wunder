import asyncio
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, HTTPException, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles

from app.api.deps import get_orchestrator
from app.api.wunder import router as wunder_router
from app.core.auth import extract_api_key, normalize_api_key
from app.core.config import get_config
from app.core.http_client import close_async_client
from app.core.i18n import resolve_language, reset_language, set_language, t
from app.core.logging import setup_logging
from app.mcp.server import create_wunder_mcp_app
from app.monitor.registry import set_app_start_time, warm_monitor_history


async def warm_admin_cache() -> None:
    """后台预热管理端依赖数据，降低首次打开页面卡顿。"""
    # 先触发监控历史加载，避免首次访问内部状态阻塞
    warm_monitor_history()
    try:
        orchestrator = await asyncio.to_thread(get_orchestrator)
    except Exception:
        return
    try:
        await asyncio.to_thread(orchestrator.workspace_manager.get_user_usage_stats)
    except Exception:
        return
    try:
        await asyncio.to_thread(orchestrator.workspace_manager.get_tool_usage_stats)
    except Exception:
        return


def build_lifespan(mcp_app) -> callable:
    """组装 FastAPI 与 MCP 的生命周期，按需初始化组件。"""

    @asynccontextmanager
    async def _core_lifespan():
        set_app_start_time()
        warm_task = asyncio.create_task(warm_admin_cache())
        try:
            yield
        finally:
            if not warm_task.done():
                warm_task.cancel()
            # 关闭全局连接池，避免资源泄漏。
            await close_async_client()

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        if hasattr(mcp_app, "startup"):
            await mcp_app.startup()
            try:
                async with _core_lifespan():
                    yield
            finally:
                if hasattr(mcp_app, "shutdown"):
                    await mcp_app.shutdown()
        else:
            async with mcp_app.lifespan(app):
                async with _core_lifespan():
                    yield

    return lifespan


def _is_protected_path(path: str) -> bool:
    """判断当前请求路径是否需要 API Key 鉴权。"""
    if path.startswith("/.well-known/agent-card.json"):
        return False
    if path.startswith("/a2a"):
        return True
    if not path.startswith("/wunder"):
        return False
    # 静态调试页不走接口鉴权，避免浏览器直接访问被拦截。
    if path.startswith("/wunder/web"):
        return False
    # 系统介绍 PPT 作为静态页面开放访问。
    if path.startswith("/wunder/ppt"):
        return False
    # i18n 配置用于前端初始化，不要求携带 API Key。
    if path.startswith("/wunder/i18n"):
        return False
    return True


def create_app() -> FastAPI:
    """创建 FastAPI 应用实例。"""
    config = get_config()
    setup_logging(config)

    mcp_app = create_wunder_mcp_app()
    app = FastAPI(title="wunder", version="0.1.0", lifespan=build_lifespan(mcp_app))
    # 统一保存 API Key，避免每次请求重复读取配置文件。
    app.state.api_key = normalize_api_key(config.security.api_key)

    @app.middleware("http")
    async def api_key_guard(request: Request, call_next):
        """统一拦截 /wunder 与 /wunder/mcp 请求，校验 API Key。"""
        # 放行 CORS 预检请求，避免影响浏览器跨域调用。
        if request.method == "OPTIONS" or not _is_protected_path(request.url.path):
            return await call_next(request)
        expected_api_key = getattr(request.app.state, "api_key", "")
        if not expected_api_key:
            raise HTTPException(
                status_code=500, detail={"message": t("error.api_key_missing")}
            )
        provided_api_key = extract_api_key(request.headers)
        if provided_api_key != expected_api_key:
            raise HTTPException(
                status_code=401, detail={"message": t("error.api_key_invalid")}
            )
        return await call_next(request)

    @app.middleware("http")
    async def language_context(request: Request, call_next):
        """为每个请求设置语言上下文，并写回响应头。"""
        language = resolve_language(
            [
                request.headers.get("X-Wunder-Language"),
                request.headers.get("Accept-Language"),
                request.query_params.get("lang"),
                request.query_params.get("language"),
            ]
        )
        token = set_language(language)
        try:
            response = await call_next(request)
        finally:
            reset_language(token)
        response.headers.setdefault("Content-Language", language)
        return response
    # 调试期默认开放跨域，避免前端测试被浏览器拦截。
    app.add_middleware(
        CORSMiddleware,
        allow_origins=config.cors.allow_origins,
        allow_credentials=config.cors.allow_credentials,
        allow_methods=config.cors.allow_methods,
        allow_headers=config.cors.allow_headers,
    )
    # 通过 /wunder/web 暴露前端静态资源，便于远程调试访问。
    web_root = Path(__file__).resolve().parents[1] / "web"
    if web_root.exists():
        app.mount("/wunder/web", StaticFiles(directory=str(web_root), html=True), name="wunder-web")
        simple_chat_path = web_root / "simple-chat" / "index.html"

        @app.get("/", include_in_schema=False)
        async def simple_chat_root():
            if simple_chat_path.exists():
                return FileResponse(simple_chat_path)
            raise HTTPException(status_code=404, detail={"message": "Not Found"})
    # 通过 /wunder/ppt 暴露系统介绍 PPT，便于前端嵌入。
    ppt_root = Path(__file__).resolve().parents[1] / "docs" / "ppt"
    if ppt_root.exists():
        app.mount("/wunder/ppt", StaticFiles(directory=str(ppt_root), html=True), name="wunder-ppt")
    # 通过 /wunder/ppt-en 暴露英文版系统介绍 PPT。
    ppt_en_root = Path(__file__).resolve().parents[1] / "docs" / "ppt-en"
    if ppt_en_root.exists():
        app.mount(
            "/wunder/ppt-en",
            StaticFiles(directory=str(ppt_en_root), html=True),
            name="wunder-ppt-en",
        )
    app.include_router(wunder_router)
    # 挂载自托管 MCP 服务，供外部或自调用使用。
    app.mount("/wunder/mcp", mcp_app, name="wunder-mcp")

    return app


app = create_app()
