import asyncio
import time
from dataclasses import dataclass

from app.storage.sqlite import SQLiteStorage


@dataclass
class LimiterSnapshot:
    """并发限制器状态快照，用于监控排队与占用情况。"""

    max_active: int
    active: int
    waiting: int
    last_wait_ms: float


class RequestLimiter:
    """基于 SQLite 的跨进程并发控制与单用户互斥。"""

    def __init__(
        self,
        storage: SQLiteStorage,
        max_active: int,
        *,
        poll_interval_s: float = 0.2,
        lock_ttl_s: float = 60.0,
    ) -> None:
        self._storage = storage
        self._max_active = max(1, int(max_active))
        self._poll_interval_s = max(0.05, float(poll_interval_s))
        self._lock_ttl_s = max(1.0, float(lock_ttl_s))
        self._lock = asyncio.Lock()
        self._active = 0
        self._waiting = 0
        self._last_wait_ms = 0.0

    @property
    def lock_ttl_s(self) -> float:
        """返回锁的存活时间，供心跳续租使用。"""
        return self._lock_ttl_s

    async def acquire(self, *, session_id: str, user_id: str) -> bool:
        """获取会话执行许可，返回是否成功。"""
        if not user_id or not session_id:
            return False
        waiting_decremented = False
        async with self._lock:
            self._waiting += 1
        start = time.perf_counter()
        try:
            while True:
                result = await asyncio.to_thread(
                    self._storage.try_acquire_session_lock,
                    session_id,
                    user_id,
                    self._max_active,
                    self._lock_ttl_s,
                )
                if result == "acquired":
                    wait_ms = (time.perf_counter() - start) * 1000
                    async with self._lock:
                        self._waiting = max(0, self._waiting - 1)
                        waiting_decremented = True
                        self._active += 1
                        self._last_wait_ms = round(wait_ms, 2)
                    return True
                if result == "user_busy":
                    async with self._lock:
                        self._waiting = max(0, self._waiting - 1)
                        waiting_decremented = True
                    return False
                await asyncio.sleep(self._poll_interval_s)
        finally:
            if not waiting_decremented:
                async with self._lock:
                    self._waiting = max(0, self._waiting - 1)

    async def touch(self, *, session_id: str) -> None:
        """续租会话锁，避免长任务被误判过期。"""
        if not session_id:
            return
        await asyncio.to_thread(
            self._storage.touch_session_lock, session_id, self._lock_ttl_s
        )

    async def release(self, *, session_id: str) -> None:
        """释放会话锁，允许新的请求进入。"""
        if not session_id:
            return
        await asyncio.to_thread(self._storage.release_session_lock, session_id)
        async with self._lock:
            self._active = max(0, self._active - 1)

    def snapshot(self) -> LimiterSnapshot:
        """返回当前状态快照，主要用于本进程内排队观测。"""
        return LimiterSnapshot(
            max_active=self._max_active,
            active=self._active,
            waiting=self._waiting,
            last_wait_ms=self._last_wait_ms,
        )
