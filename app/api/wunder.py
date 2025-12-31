from fastapi import APIRouter

from app.api.deps import get_orchestrator
from app.api.routes import admin, core, user_tools, workspace


router = APIRouter()
router.include_router(core.router)
router.include_router(user_tools.router)
router.include_router(admin.router)
router.include_router(workspace.router)


class _LazyOrchestratorProxy:
    """懒加载调度器代理，避免模块导入阶段初始化。"""

    def __getattr__(self, name):
        return getattr(get_orchestrator(), name)

    def __setattr__(self, name, value):
        setattr(get_orchestrator(), name, value)

    def __repr__(self) -> str:
        return repr(get_orchestrator())


# 兼容测试与外部引用，保留全局调度器入口。
_orchestrator = _LazyOrchestratorProxy()
