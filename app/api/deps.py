from pathlib import Path
import threading
from typing import Optional

from app.core.config import resolve_config_path
from app.orchestrator.engine import WunderOrchestrator


_CONFIG_PATH = resolve_config_path(Path("config/wunder.yaml"))
_ORCHESTRATOR: Optional[WunderOrchestrator] = None
_ORCHESTRATOR_LOCK = threading.Lock()


def get_config_path() -> Path:
    """获取当前生效的配置文件路径。"""
    return _CONFIG_PATH


def get_orchestrator() -> WunderOrchestrator:
    """获取全局调度器实例（惰性初始化）。"""
    global _ORCHESTRATOR
    if _ORCHESTRATOR is None:
        with _ORCHESTRATOR_LOCK:
            if _ORCHESTRATOR is None:
                # 惰性创建调度器，避免导入阶段阻塞启动。
                _ORCHESTRATOR = WunderOrchestrator(str(_CONFIG_PATH))
    return _ORCHESTRATOR


def get_orchestrator_if_ready() -> Optional[WunderOrchestrator]:
    """仅在已初始化时返回调度器，避免触发冷启动。"""
    return _ORCHESTRATOR
