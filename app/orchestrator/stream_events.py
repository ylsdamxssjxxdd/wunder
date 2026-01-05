import asyncio
import threading
import time
from typing import Dict, List

from app.schemas.wunder import StreamEvent
from app.storage import Storage


class StreamEventStore:
    """SSE 溢出事件存储：仅在队列满时落库，避免丢失。"""

    def __init__(
        self,
        storage: Storage,
        *,
        ttl_s: float,
        cleanup_interval_s: float,
    ) -> None:
        self._storage = storage
        self._ttl_s = max(60.0, float(ttl_s))
        self._cleanup_interval_s = max(5.0, float(cleanup_interval_s))
        self._cleanup_lock = threading.Lock()
        self._last_cleanup_ts = 0.0

    def record_overflow_event(self, user_id: str, event: StreamEvent) -> None:
        """记录溢出事件，保证队列满时仍可回放。"""
        event_id = getattr(event, "event_id", None)
        if event_id is None:
            return
        payload = event.model_dump(exclude={"event_id"})
        self._storage.append_stream_event(
            session_id=event.session_id,
            event_id=event_id,
            user_id=user_id,
            payload=payload,
        )
        self._maybe_cleanup()

    async def load_overflow_events(
        self, session_id: str, after_event_id: int, limit: int
    ) -> List[StreamEvent]:
        """读取溢出事件并转换为 StreamEvent 列表。"""
        records = await asyncio.to_thread(
            self._storage.load_stream_events, session_id, after_event_id, limit
        )
        events: List[StreamEvent] = []
        for payload in records:
            event_id = payload.pop("event_id", None)
            try:
                event = StreamEvent.model_validate(payload)
            except Exception:
                continue
            event.event_id = event_id
            events.append(event)
        return events

    def _maybe_cleanup(self) -> None:
        """按时间节流清理过期溢出事件，避免表增长过快。"""
        now = time.time()
        with self._cleanup_lock:
            if now - self._last_cleanup_ts < self._cleanup_interval_s:
                return
            self._last_cleanup_ts = now
        cutoff = now - self._ttl_s
        try:
            self._storage.delete_stream_events_before(cutoff)
        except Exception:
            return
