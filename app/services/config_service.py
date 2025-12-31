from pathlib import Path
from typing import Any, Callable

from app.core.config import WunderConfig
from app.orchestrator.engine import WunderOrchestrator


def apply_config_update(
    orchestrator: WunderOrchestrator,
    config_path: Path,
    updater: Callable[..., WunderConfig],
    *args: Any,
    **kwargs: Any,
) -> WunderConfig:
    """统一应用配置更新并同步到运行时调度器。"""
    updated = updater(config_path, *args, **kwargs)
    orchestrator.apply_config(updated)
    return updated
