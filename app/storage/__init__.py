"""持久化存储模块。"""

from app.storage.factory import Storage, get_storage
from app.storage.sqlite import StorageLogHandler

__all__ = ["Storage", "get_storage", "StorageLogHandler"]
