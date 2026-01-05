import logging
from typing import Optional

from app.core.config import WunderConfig
from app.storage import StorageLogHandler, get_storage


def setup_logging(config: WunderConfig) -> None:
    """按配置初始化日志输出。"""
    formatter = logging.Formatter("%(asctime)s | %(levelname)s | %(name)s | %(message)s")
    storage = get_storage(config.storage)
    storage_handler = StorageLogHandler(storage)
    storage_handler.setLevel(getattr(logging, config.observability.log_level.upper(), logging.INFO))
    storage_handler.setFormatter(formatter)

    logging.basicConfig(
        level=getattr(logging, config.observability.log_level.upper(), logging.INFO),
        format="%(asctime)s | %(levelname)s | %(name)s | %(message)s",
        handlers=[
            logging.StreamHandler(),
            storage_handler,
        ],
    )


def get_logger(name: Optional[str] = None) -> logging.Logger:
    """获取指定名称的日志器。"""
    return logging.getLogger(name or "wunder")
