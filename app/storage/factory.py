from __future__ import annotations

import threading
from pathlib import Path
from typing import Any, Dict, TYPE_CHECKING, Union

from app.core.config import StorageConfig
from app.storage.sqlite import SQLiteStorage

if TYPE_CHECKING:
    from app.storage.postgres import PostgresStorage

Storage = Union[SQLiteStorage, "PostgresStorage"] if TYPE_CHECKING else Any

_STORAGE_LOCK = threading.Lock()
_STORAGE_CACHE: Dict[str, Storage] = {}


def _normalize_backend(value: str) -> str:
    """规范化存储后端名称，统一大小写与别名。"""
    return str(value or "").strip().lower()


def get_storage(config: StorageConfig) -> Storage:
    """根据配置返回存储实例，避免重复初始化连接配置。"""
    backend = _normalize_backend(config.backend or "sqlite")
    if backend in ("sqlite", "sqlite3"):
        db_path = config.db_path or "./data/wunder.db"
        key = f"sqlite:{Path(db_path).resolve()}"
        with _STORAGE_LOCK:
            storage = _STORAGE_CACHE.get(key)
            if storage is None:
                storage = SQLiteStorage(db_path)
                _STORAGE_CACHE[key] = storage
            return storage
    if backend in ("postgres", "postgresql", "pg"):
        dsn = config.postgres.dsn
        timeout = config.postgres.connect_timeout_s
        key = f"postgres:{dsn}|timeout={timeout}"
        with _STORAGE_LOCK:
            storage = _STORAGE_CACHE.get(key)
            if storage is None:
                from app.storage.postgres import PostgresStorage

                storage = PostgresStorage(dsn, connect_timeout_s=timeout)
                _STORAGE_CACHE[key] = storage
            return storage
    raise ValueError(f"Unsupported storage backend: {config.backend}")
