from __future__ import annotations

import json
import logging
import sqlite3
import threading
import time
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List, Optional


_STORAGE_LOCK = threading.Lock()
_STORAGE_CACHE: Dict[str, "SQLiteStorage"] = {}


def get_storage(db_path: str) -> "SQLiteStorage":
    """获取 SQLite 存储实例，避免重复初始化连接配置。"""
    key = str(Path(db_path).resolve())
    with _STORAGE_LOCK:
        storage = _STORAGE_CACHE.get(key)
        if storage is None:
            storage = SQLiteStorage(key)
            _STORAGE_CACHE[key] = storage
        return storage


class SQLiteStorage:
    """基于 SQLite 的持久化存储。"""

    def __init__(self, db_path: str) -> None:
        self._db_path = Path(db_path).resolve()
        self._init_lock = threading.Lock()
        self._initialized = False

    def ensure_initialized(self) -> None:
        """初始化数据库表结构，保证幂等。"""
        if self._initialized:
            return
        with self._init_lock:
            if self._initialized:
                return
            self._db_path.parent.mkdir(parents=True, exist_ok=True)
            with self._connect() as conn:
                conn.executescript(
                    """
                    CREATE TABLE IF NOT EXISTS meta (
                      key TEXT PRIMARY KEY,
                      value TEXT NOT NULL,
                      updated_time REAL NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS chat_history (
                      id INTEGER PRIMARY KEY AUTOINCREMENT,
                      user_id TEXT NOT NULL,
                      session_id TEXT NOT NULL,
                      role TEXT NOT NULL,
                      content TEXT,
                      timestamp TEXT,
                      meta TEXT,
                      payload TEXT NOT NULL,
                      created_time REAL NOT NULL
                    );
                    CREATE INDEX IF NOT EXISTS idx_chat_history_session
                      ON chat_history (user_id, session_id, id);
                    CREATE TABLE IF NOT EXISTS tool_logs (
                      id INTEGER PRIMARY KEY AUTOINCREMENT,
                      user_id TEXT NOT NULL,
                      session_id TEXT NOT NULL,
                      tool TEXT,
                      ok INTEGER,
                      error TEXT,
                      args TEXT,
                      data TEXT,
                      timestamp TEXT,
                      payload TEXT NOT NULL,
                      created_time REAL NOT NULL
                    );
                    CREATE INDEX IF NOT EXISTS idx_tool_logs_session
                      ON tool_logs (user_id, session_id, id);
                    CREATE TABLE IF NOT EXISTS artifact_logs (
                      id INTEGER PRIMARY KEY AUTOINCREMENT,
                      user_id TEXT NOT NULL,
                      session_id TEXT NOT NULL,
                      kind TEXT NOT NULL,
                      name TEXT,
                      payload TEXT NOT NULL,
                      created_time REAL NOT NULL
                    );
                    CREATE INDEX IF NOT EXISTS idx_artifact_logs_session
                      ON artifact_logs (user_id, session_id, id);
                    CREATE TABLE IF NOT EXISTS monitor_sessions (
                      session_id TEXT PRIMARY KEY,
                      user_id TEXT,
                      status TEXT,
                      updated_time REAL,
                      payload TEXT NOT NULL
                    );
                    CREATE INDEX IF NOT EXISTS idx_monitor_sessions_status
                      ON monitor_sessions (status);
                    CREATE TABLE IF NOT EXISTS system_logs (
                      id INTEGER PRIMARY KEY AUTOINCREMENT,
                      created_at TEXT NOT NULL,
                      level TEXT NOT NULL,
                      logger TEXT NOT NULL,
                      message TEXT NOT NULL,
                      payload TEXT
                    );
                    CREATE INDEX IF NOT EXISTS idx_system_logs_created
                      ON system_logs (created_at);
                    CREATE TABLE IF NOT EXISTS session_locks (
                      session_id TEXT PRIMARY KEY,
                      user_id TEXT NOT NULL,
                      created_time REAL NOT NULL,
                      updated_time REAL NOT NULL,
                      expires_at REAL NOT NULL
                    );
                    CREATE UNIQUE INDEX IF NOT EXISTS idx_session_locks_user
                      ON session_locks (user_id);
                    CREATE INDEX IF NOT EXISTS idx_session_locks_expires
                      ON session_locks (expires_at);
                    CREATE TABLE IF NOT EXISTS stream_events (
                      session_id TEXT NOT NULL,
                      event_id INTEGER NOT NULL,
                      user_id TEXT NOT NULL,
                      payload TEXT NOT NULL,
                      created_time REAL NOT NULL,
                      PRIMARY KEY (session_id, event_id)
                    );
                    CREATE INDEX IF NOT EXISTS idx_stream_events_user
                      ON stream_events (user_id);
                    CREATE INDEX IF NOT EXISTS idx_stream_events_time
                      ON stream_events (created_time);
                    CREATE TABLE IF NOT EXISTS memory_settings (
                      user_id TEXT PRIMARY KEY,
                      enabled INTEGER NOT NULL,
                      updated_time REAL NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS memory_records (
                      id INTEGER PRIMARY KEY AUTOINCREMENT,
                      user_id TEXT NOT NULL,
                      session_id TEXT NOT NULL,
                      summary TEXT NOT NULL,
                      created_time REAL NOT NULL,
                      updated_time REAL NOT NULL,
                      UNIQUE(user_id, session_id)
                    );
                    CREATE INDEX IF NOT EXISTS idx_memory_records_user_time
                      ON memory_records (user_id, updated_time);
                    CREATE TABLE IF NOT EXISTS memory_task_logs (
                      id INTEGER PRIMARY KEY AUTOINCREMENT,
                      task_id TEXT NOT NULL,
                      user_id TEXT NOT NULL,
                      session_id TEXT NOT NULL,
                      status TEXT NOT NULL,
                      queued_time REAL NOT NULL,
                      started_time REAL NOT NULL,
                      finished_time REAL NOT NULL,
                      elapsed_s REAL NOT NULL,
                      request_payload TEXT,
                      result TEXT,
                      error TEXT,
                      updated_time REAL NOT NULL,
                      UNIQUE(user_id, session_id)
                    );
                    CREATE INDEX IF NOT EXISTS idx_memory_task_logs_updated
                      ON memory_task_logs (updated_time);
                    CREATE INDEX IF NOT EXISTS idx_memory_task_logs_task_id
                      ON memory_task_logs (task_id);
                    """
                )
            self._initialized = True

    def get_meta(self, key: str) -> Optional[str]:
        """读取元信息，用于迁移与版本控制。"""
        self.ensure_initialized()
        with self._connect() as conn:
            row = conn.execute("SELECT value FROM meta WHERE key = ?", (key,)).fetchone()
            return row["value"] if row else None

    def set_meta(self, key: str, value: str) -> None:
        """写入元信息，避免重复迁移。"""
        self.ensure_initialized()
        with self._connect() as conn:
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value, updated_time) VALUES (?, ?, ?)",
                (key, value, time.time()),
            )

    def incr_meta(self, key: str, delta: int) -> int:
        """原子递增元信息数值并返回最新结果。"""
        self.ensure_initialized()
        if not isinstance(delta, int):
            try:
                delta = int(delta)
            except (TypeError, ValueError):
                delta = 0
        now = time.time()
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO meta (key, value, updated_time)
                VALUES (?, ?, ?)
                ON CONFLICT(key) DO UPDATE SET
                  value = CAST(value AS INTEGER) + CAST(excluded.value AS INTEGER),
                  updated_time = excluded.updated_time
                """,
                (key, str(delta), now),
            )
            row = conn.execute(
                "SELECT value FROM meta WHERE key = ?", (key,)
            ).fetchone()
        if not row:
            return 0
        try:
            return int(row["value"])
        except (TypeError, ValueError):
            return 0

    def delete_meta_prefix(self, prefix: str) -> int:
        """按前缀删除元信息记录，用于清理会话级缓存。"""
        self.ensure_initialized()
        cleaned = str(prefix or "").strip()
        if not cleaned:
            return 0
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM meta WHERE key LIKE ?", (f"{cleaned}%",)
            )
        return int(cursor.rowcount or 0)

    def cleanup_retention(self, retention_days: int) -> Dict[str, int]:
        """按 retention_days 清理过期记录，小于等于 0 则不做清理。"""
        try:
            days = int(retention_days)
        except (TypeError, ValueError):
            return {}
        if days <= 0:
            return {}
        cutoff = time.time() - (days * 86400)
        if cutoff <= 0:
            return {}
        cutoff_int = int(cutoff)

        results: Dict[str, int] = {}
        with self._connect() as conn:
            # 使用 created_time/updated_time 做时间判断，避免清理影响正在运行的会话
            results["chat_history"] = int(
                (conn.execute("DELETE FROM chat_history WHERE created_time < ?", (cutoff,))).rowcount
                or 0
            )
            results["tool_logs"] = int(
                (conn.execute("DELETE FROM tool_logs WHERE created_time < ?", (cutoff,))).rowcount
                or 0
            )
            results["artifact_logs"] = int(
                (conn.execute("DELETE FROM artifact_logs WHERE created_time < ?", (cutoff,))).rowcount
                or 0
            )
            results["monitor_sessions"] = int(
                (
                    conn.execute(
                        "DELETE FROM monitor_sessions WHERE COALESCE(updated_time, 0) < ?",
                        (cutoff,),
                    )
                ).rowcount
                or 0
            )
            results["stream_events"] = int(
                (conn.execute("DELETE FROM stream_events WHERE created_time < ?", (cutoff,))).rowcount
                or 0
            )
            # 使用 strftime 解析 ISO 时间，避免因字符串格式差异导致比较错误
            results["system_logs"] = int(
                (
                    conn.execute(
                        """
                        DELETE FROM system_logs
                        WHERE CAST(strftime('%s', replace(created_at, 'Z', '')) AS INTEGER) < ?
                        """,
                        (cutoff_int,),
                    )
                ).rowcount
                or 0
            )
        return results

    def try_acquire_session_lock(
        self,
        session_id: str,
        user_id: str,
        max_active: int,
        ttl_s: float,
        now_ts: Optional[float] = None,
    ) -> str:
        """尝试抢占会话锁，返回 acquired/user_busy/global_busy。"""
        self.ensure_initialized()
        cleaned_session = str(session_id or "").strip()
        cleaned_user = str(user_id or "").strip()
        if not cleaned_session or not cleaned_user:
            return "user_busy"
        max_active = max(1, int(max_active))
        ttl_s = max(1.0, float(ttl_s))
        now = float(now_ts if now_ts is not None else time.time())
        expires_at = now + ttl_s
        with self._connect() as conn:
            try:
                # 采用 IMMEDIATE 锁避免并发下计数与写入交叉
                conn.execute("BEGIN IMMEDIATE")
                conn.execute(
                    "DELETE FROM session_locks WHERE expires_at <= ?", (now,)
                )
                row = conn.execute(
                    "SELECT session_id FROM session_locks WHERE user_id = ?",
                    (cleaned_user,),
                ).fetchone()
                if row:
                    return "user_busy"
                total = conn.execute(
                    "SELECT COUNT(*) AS total FROM session_locks"
                ).fetchone()
                if total and int(total["total"] or 0) >= max_active:
                    return "global_busy"
                conn.execute(
                    """
                    INSERT INTO session_locks
                      (session_id, user_id, created_time, updated_time, expires_at)
                    VALUES (?, ?, ?, ?, ?)
                    """,
                    (cleaned_session, cleaned_user, now, now, expires_at),
                )
                return "acquired"
            except sqlite3.IntegrityError:
                return "user_busy"
            except sqlite3.OperationalError as exc:
                message = str(exc).lower()
                if "locked" in message or "busy" in message:
                    return "global_busy"
                raise

    def touch_session_lock(
        self, session_id: str, ttl_s: float, now_ts: Optional[float] = None
    ) -> None:
        """续租会话锁，避免长任务被误判过期。"""
        self.ensure_initialized()
        cleaned_session = str(session_id or "").strip()
        if not cleaned_session:
            return
        ttl_s = max(1.0, float(ttl_s))
        now = float(now_ts if now_ts is not None else time.time())
        expires_at = now + ttl_s
        with self._connect() as conn:
            conn.execute(
                """
                UPDATE session_locks
                SET updated_time = ?, expires_at = ?
                WHERE session_id = ?
                """,
                (now, expires_at, cleaned_session),
            )

    def release_session_lock(self, session_id: str) -> None:
        """释放会话锁，允许新的请求进入。"""
        self.ensure_initialized()
        cleaned_session = str(session_id or "").strip()
        if not cleaned_session:
            return
        with self._connect() as conn:
            conn.execute(
                "DELETE FROM session_locks WHERE session_id = ?",
                (cleaned_session,),
            )

    def delete_session_locks_by_user(self, user_id: str) -> int:
        """按用户清理会话锁，防止遗留锁阻塞新请求。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        if not cleaned_user:
            return 0
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM session_locks WHERE user_id = ?",
                (cleaned_user,),
            )
        return int(cursor.rowcount or 0)

    def append_stream_event(
        self,
        session_id: str,
        event_id: int,
        user_id: str,
        payload: Dict[str, Any],
        created_time: Optional[float] = None,
    ) -> None:
        """记录 SSE 溢出事件，避免队列满时丢失。"""
        self.ensure_initialized()
        cleaned_session = str(session_id or "").strip()
        cleaned_user = str(user_id or "").strip()
        if not cleaned_session or not cleaned_user:
            return
        try:
            event_seq = int(event_id)
        except (TypeError, ValueError):
            return
        now = float(created_time if created_time is not None else time.time())
        payload_json = self._json_dumps(payload)
        with self._connect() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO stream_events
                  (session_id, event_id, user_id, payload, created_time)
                VALUES (?, ?, ?, ?, ?)
                """,
                (cleaned_session, event_seq, cleaned_user, payload_json, now),
            )

    def load_stream_events(
        self, session_id: str, after_event_id: int, limit: int
    ) -> List[Dict[str, Any]]:
        """读取指定会话的 SSE 溢出事件。"""
        self.ensure_initialized()
        cleaned_session = str(session_id or "").strip()
        if not cleaned_session or limit <= 0:
            return []
        try:
            after_id = int(after_event_id)
        except (TypeError, ValueError):
            after_id = 0
        with self._connect() as conn:
            rows = conn.execute(
                """
                SELECT event_id, payload
                FROM stream_events
                WHERE session_id = ? AND event_id > ?
                ORDER BY event_id ASC
                LIMIT ?
                """,
                (cleaned_session, after_id, int(limit)),
            ).fetchall()
        records: List[Dict[str, Any]] = []
        for row in rows:
            payload = self._json_loads(row["payload"])
            if not payload:
                continue
            payload["event_id"] = int(row["event_id"] or 0)
            records.append(payload)
        return records

    def delete_stream_events_before(self, before_time: float) -> int:
        """清理指定时间之前的 SSE 溢出事件。"""
        self.ensure_initialized()
        try:
            cutoff = float(before_time)
        except (TypeError, ValueError):
            return 0
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM stream_events WHERE created_time < ?",
                (cutoff,),
            )
        return int(cursor.rowcount or 0)

    def delete_stream_events_by_user(self, user_id: str) -> int:
        """按用户清理 SSE 溢出事件。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        if not cleaned_user:
            return 0
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM stream_events WHERE user_id = ?",
                (cleaned_user,),
            )
        return int(cursor.rowcount or 0)

    def append_chat(self, user_id: str, payload: Dict[str, Any]) -> None:
        """写入对话历史记录。"""
        self.ensure_initialized()
        session_id = str(payload.get("session_id", ""))
        role = str(payload.get("role", ""))
        content = payload.get("content")
        timestamp = payload.get("timestamp")
        meta = payload.get("meta")
        payload_json = self._json_dumps(payload)
        meta_json = self._json_dumps(meta) if meta is not None else None
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO chat_history
                  (user_id, session_id, role, content, timestamp, meta, payload, created_time)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    user_id,
                    session_id,
                    role,
                    None if content is None else str(content),
                    None if timestamp is None else str(timestamp),
                    meta_json,
                    payload_json,
                    time.time(),
                ),
            )

    def append_tool_log(self, user_id: str, payload: Dict[str, Any]) -> None:
        """写入工具调用日志。"""
        self.ensure_initialized()
        session_id = str(payload.get("session_id", ""))
        ok = payload.get("ok")
        ok_value = None if ok is None else 1 if bool(ok) else 0
        payload_json = self._json_dumps(payload)
        args_json = (
            self._json_dumps(payload.get("args"))
            if "args" in payload
            else None
        )
        data_json = (
            self._json_dumps(payload.get("data"))
            if "data" in payload
            else None
        )
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO tool_logs
                  (user_id, session_id, tool, ok, error, args, data, timestamp, payload, created_time)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    user_id,
                    session_id,
                    str(payload.get("tool", "")),
                    ok_value,
                    None if payload.get("error") is None else str(payload.get("error")),
                    args_json,
                    data_json,
                    None if payload.get("timestamp") is None else str(payload.get("timestamp")),
                    payload_json,
                    time.time(),
                ),
            )

    def append_artifact_log(self, user_id: str, payload: Dict[str, Any]) -> None:
        """写入产物索引日志，供上下文索引与审计使用。"""
        self.ensure_initialized()
        session_id = str(payload.get("session_id", "")).strip()
        kind = str(payload.get("kind", "")).strip()
        name = payload.get("name")
        if not user_id or not session_id or not kind:
            return
        payload_json = self._json_dumps(payload)
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO artifact_logs
                  (user_id, session_id, kind, name, payload, created_time)
                VALUES (?, ?, ?, ?, ?, ?)
                """,
                (
                    user_id,
                    session_id,
                    kind,
                    None if name is None else str(name),
                    payload_json,
                    time.time(),
                ),
            )

    def load_artifact_logs(
        self, user_id: str, session_id: str, limit: int
    ) -> List[Dict[str, Any]]:
        """读取会话产物索引日志，按时间顺序返回。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        cleaned_session = str(session_id or "").strip()
        if not cleaned_user or not cleaned_session or limit <= 0:
            return []
        with self._connect() as conn:
            rows = conn.execute(
                """
                SELECT id, payload
                FROM artifact_logs
                WHERE user_id = ? AND session_id = ?
                ORDER BY id DESC
                LIMIT ?
                """,
                (cleaned_user, cleaned_session, int(limit)),
            ).fetchall()
        records: List[Dict[str, Any]] = []
        for row in reversed(rows):
            payload = self._json_loads(row["payload"])
            if not payload:
                continue
            payload["artifact_id"] = int(row["id"] or 0)
            records.append(payload)
        return records

    def load_chat_history(
        self, user_id: str, session_id: str, limit: Optional[int]
    ) -> List[Dict[str, Any]]:
        """读取指定会话的历史记录。"""
        self.ensure_initialized()
        limit_value: Optional[int]
        if limit is None:
            limit_value = None
        else:
            try:
                limit_value = int(limit)
            except (TypeError, ValueError):
                limit_value = None
        # 数量小于等于 0 视为不限制，读取完整历史记录
        if limit_value is not None and limit_value <= 0:
            limit_value = None

        with self._connect() as conn:
            if limit_value is None:
                rows = conn.execute(
                    """
                    SELECT payload
                    FROM chat_history
                    WHERE user_id = ? AND session_id = ?
                    ORDER BY id ASC
                    """,
                    (user_id, session_id),
                ).fetchall()
            else:
                rows = conn.execute(
                    """
                    SELECT payload
                    FROM chat_history
                    WHERE user_id = ? AND session_id = ?
                    ORDER BY id DESC
                    LIMIT ?
                    """,
                    (user_id, session_id, limit_value),
                ).fetchall()
                rows = reversed(rows)

        records: List[Dict[str, Any]] = []
        for row in rows:
            payload = self._json_loads(row["payload"])
            if payload:
                records.append(payload)
        return records

    def get_session_system_prompt(self, user_id: str, session_id: str) -> Optional[str]:
        """读取指定会话固定系统提示词。"""
        self.ensure_initialized()
        with self._connect() as conn:
            rows = conn.execute(
                """
                SELECT payload
                FROM chat_history
                WHERE user_id = ? AND session_id = ? AND role = 'system'
                ORDER BY id ASC
                """,
                (user_id, session_id),
            ).fetchall()
        for row in rows:
            payload = self._json_loads(row["payload"])
            if not payload:
                continue
            meta = payload.get("meta")
            if not isinstance(meta, dict):
                continue
            if meta.get("type") != "system_prompt":
                continue
            content = payload.get("content")
            if isinstance(content, str) and content.strip():
                return content
        return None

    def get_user_chat_stats(self) -> Dict[str, Dict[str, Any]]:
        """按用户统计对话记录数量与最近写入时间。"""
        self.ensure_initialized()
        with self._connect() as conn:
            rows = conn.execute(
                """
                SELECT user_id,
                       COUNT(*) as chat_records,
                       MAX(created_time) as last_time
                FROM chat_history
                GROUP BY user_id
                """
            ).fetchall()
        stats: Dict[str, Dict[str, Any]] = {}
        for row in rows:
            user_id = str(row["user_id"] or "").strip()
            if not user_id:
                continue
            stats[user_id] = {
                "chat_records": int(row["chat_records"] or 0),
                "last_time": float(row["last_time"] or 0),
            }
        return stats

    def get_user_tool_stats(self) -> Dict[str, Dict[str, Any]]:
        """按用户统计工具调用数量与最近写入时间。"""
        self.ensure_initialized()
        with self._connect() as conn:
            rows = conn.execute(
                """
                SELECT user_id,
                       COUNT(*) as tool_records,
                       MAX(created_time) as last_time
                FROM tool_logs
                GROUP BY user_id
                """
            ).fetchall()
        stats: Dict[str, Dict[str, Any]] = {}
        for row in rows:
            user_id = str(row["user_id"] or "").strip()
            if not user_id:
                continue
            stats[user_id] = {
                "tool_records": int(row["tool_records"] or 0),
                "last_time": float(row["last_time"] or 0),
            }
        return stats

    def get_tool_usage_stats(
        self,
        since_time: Optional[float] = None,
        until_time: Optional[float] = None,
    ) -> Dict[str, int]:
        """按工具统计调用次数，可按时间窗口过滤。"""
        self.ensure_initialized()
        query = """
            SELECT tool,
                   COUNT(*) as tool_records
            FROM tool_logs
        """
        params: List[Any] = []
        filters: List[str] = []
        if isinstance(since_time, (int, float)) and since_time > 0:
            filters.append("created_time >= ?")
            params.append(float(since_time))
        if isinstance(until_time, (int, float)) and until_time > 0:
            filters.append("created_time <= ?")
            params.append(float(until_time))
        if filters:
            query += " WHERE " + " AND ".join(filters)
        query += " GROUP BY tool ORDER BY tool_records DESC"
        with self._connect() as conn:
            rows = conn.execute(query, params).fetchall()
        stats: Dict[str, int] = {}
        for row in rows:
            tool = str(row["tool"] or "").strip()
            if not tool:
                continue
            stats[tool] = int(row["tool_records"] or 0)
        return stats

    def get_tool_session_usage(
        self,
        tool: str,
        since_time: Optional[float] = None,
        until_time: Optional[float] = None,
    ) -> List[Dict[str, Any]]:
        """按工具统计使用该工具的会话列表，支持时间窗口过滤。"""
        self.ensure_initialized()
        cleaned = str(tool or "").strip()
        if not cleaned:
            return []
        query = """
            SELECT session_id,
                   user_id,
                   COUNT(*) as tool_calls,
                   MAX(created_time) as last_time
            FROM tool_logs
            WHERE tool = ?
        """
        params: List[Any] = [cleaned]
        filters: List[str] = []
        if isinstance(since_time, (int, float)) and since_time > 0:
            filters.append("created_time >= ?")
            params.append(float(since_time))
        if isinstance(until_time, (int, float)) and until_time > 0:
            filters.append("created_time <= ?")
            params.append(float(until_time))
        if filters:
            query += " AND " + " AND ".join(filters)
        query += " GROUP BY session_id, user_id ORDER BY last_time DESC"
        with self._connect() as conn:
            rows = conn.execute(query, params).fetchall()
        sessions: List[Dict[str, Any]] = []
        for row in rows:
            session_id = str(row["session_id"] or "").strip()
            if not session_id:
                continue
            sessions.append(
                {
                    "session_id": session_id,
                    "user_id": str(row["user_id"] or "").strip(),
                    "tool_calls": int(row["tool_calls"] or 0),
                    "last_time": float(row["last_time"] or 0),
                }
            )
        return sessions

    def upsert_monitor_record(self, payload: Dict[str, Any]) -> None:
        """保存监控会话记录。"""
        self.ensure_initialized()
        session_id = str(payload.get("session_id", ""))
        if not session_id:
            return
        with self._connect() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO monitor_sessions
                  (session_id, user_id, status, updated_time, payload)
                VALUES (?, ?, ?, ?, ?)
                """,
                (
                    session_id,
                    str(payload.get("user_id", "")),
                    str(payload.get("status", "")),
                    float(payload.get("updated_time", 0) or 0),
                    self._json_dumps(payload),
                ),
            )

    def load_monitor_records(self) -> List[Dict[str, Any]]:
        """读取全部监控会话记录。"""
        self.ensure_initialized()
        with self._connect() as conn:
            rows = conn.execute("SELECT payload FROM monitor_sessions").fetchall()
        records: List[Dict[str, Any]] = []
        for row in rows:
            payload = self._json_loads(row["payload"])
            if payload:
                records.append(payload)
        return records

    def delete_monitor_record(self, session_id: str) -> None:
        """删除指定监控会话记录。"""
        self.ensure_initialized()
        with self._connect() as conn:
            conn.execute("DELETE FROM monitor_sessions WHERE session_id = ?", (session_id,))

    def delete_monitor_records_by_user(self, user_id: str) -> int:
        """按用户清理监控会话记录。"""
        self.ensure_initialized()
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM monitor_sessions WHERE user_id = ?", (user_id,)
            )
        return int(cursor.rowcount or 0)

    def delete_chat_history(self, user_id: str) -> int:
        """清理指定用户的对话记录。"""
        self.ensure_initialized()
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM chat_history WHERE user_id = ?", (user_id,)
            )
        return int(cursor.rowcount or 0)

    def delete_tool_logs(self, user_id: str) -> int:
        """清理指定用户的工具调用日志。"""
        self.ensure_initialized()
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM tool_logs WHERE user_id = ?", (user_id,)
            )
        return int(cursor.rowcount or 0)

    def delete_artifact_logs(self, user_id: str) -> int:
        """清理指定用户的产物索引日志。"""
        self.ensure_initialized()
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM artifact_logs WHERE user_id = ?", (user_id,)
            )
        return int(cursor.rowcount or 0)

    def get_memory_enabled(self, user_id: str) -> Optional[bool]:
        """获取用户长期记忆开关状态。"""
        self.ensure_initialized()
        cleaned = str(user_id or "").strip()
        if not cleaned:
            return None
        with self._connect() as conn:
            row = conn.execute(
                "SELECT enabled FROM memory_settings WHERE user_id = ?",
                (cleaned,),
            ).fetchone()
        if not row:
            return None
        return bool(row["enabled"])

    def set_memory_enabled(
        self,
        user_id: str,
        enabled: bool,
        now_ts: Optional[float] = None,
    ) -> None:
        """更新用户长期记忆开关状态。"""
        self.ensure_initialized()
        cleaned = str(user_id or "").strip()
        if not cleaned:
            return
        now = float(now_ts if now_ts is not None else time.time())
        enabled_value = 1 if bool(enabled) else 0
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO memory_settings (user_id, enabled, updated_time)
                VALUES (?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE SET
                  enabled = excluded.enabled,
                  updated_time = excluded.updated_time
                """,
                (cleaned, enabled_value, now),
            )

    def load_memory_settings(self) -> Dict[str, Dict[str, Any]]:
        """批量读取所有用户的记忆开关配置。"""
        self.ensure_initialized()
        settings: Dict[str, Dict[str, Any]] = {}
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT user_id, enabled, updated_time FROM memory_settings"
            ).fetchall()
        for row in rows:
            user_id = str(row["user_id"] or "").strip()
            if not user_id:
                continue
            settings[user_id] = {
                "enabled": bool(row["enabled"]),
                "updated_time": float(row["updated_time"] or 0),
            }
        return settings

    def upsert_memory_record(
        self,
        user_id: str,
        session_id: str,
        summary: str,
        max_records: int,
        now_ts: Optional[float] = None,
    ) -> None:
        """插入或覆盖会话记忆，并维护用户记录上限。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        cleaned_session = str(session_id or "").strip()
        cleaned_summary = str(summary or "").strip()
        if not cleaned_user or not cleaned_session or not cleaned_summary:
            return
        now = float(now_ts if now_ts is not None else time.time())
        safe_limit = max(1, int(max_records))
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO memory_records
                  (user_id, session_id, summary, created_time, updated_time)
                VALUES (?, ?, ?, ?, ?)
                ON CONFLICT(user_id, session_id) DO UPDATE SET
                  summary = excluded.summary,
                  updated_time = excluded.updated_time
                """,
                (cleaned_user, cleaned_session, cleaned_summary, now, now),
            )
            conn.execute(
                """
                DELETE FROM memory_records
                WHERE user_id = ?
                  AND id NOT IN (
                    SELECT id
                    FROM memory_records
                    WHERE user_id = ?
                    ORDER BY updated_time DESC, id DESC
                    LIMIT ?
                  )
                """,
                (cleaned_user, cleaned_user, safe_limit),
            )
            conn.execute(
                """
                DELETE FROM memory_task_logs
                WHERE user_id = ?
                  AND session_id NOT IN (
                    SELECT session_id
                    FROM memory_records
                    WHERE user_id = ?
                  )
                """,
                (cleaned_user, cleaned_user),
            )

    def load_memory_records(
        self,
        user_id: str,
        limit: int,
        *,
        order_desc: bool = True,
    ) -> List[Dict[str, Any]]:
        """读取用户记忆记录列表。"""
        self.ensure_initialized()
        cleaned = str(user_id or "").strip()
        if not cleaned or limit <= 0:
            return []
        direction = "DESC" if order_desc else "ASC"
        with self._connect() as conn:
            rows = conn.execute(
                f"""
                SELECT session_id, summary, created_time, updated_time
                FROM memory_records
                WHERE user_id = ?
                ORDER BY updated_time {direction}, id {direction}
                LIMIT ?
                """,
                (cleaned, int(limit)),
            ).fetchall()
        records: List[Dict[str, Any]] = []
        for row in rows:
            records.append(
                {
                    "session_id": str(row["session_id"] or ""),
                    "summary": str(row["summary"] or ""),
                    "created_time": float(row["created_time"] or 0),
                    "updated_time": float(row["updated_time"] or 0),
                }
            )
        return records

    def get_memory_record_stats(self) -> Dict[str, Dict[str, Any]]:
        """汇总每个用户的记忆条数与最新时间。"""
        self.ensure_initialized()
        with self._connect() as conn:
            rows = conn.execute(
                """
                SELECT user_id,
                       COUNT(*) as record_count,
                       MAX(updated_time) as last_time
                FROM memory_records
                GROUP BY user_id
                """
            ).fetchall()
        stats: Dict[str, Dict[str, Any]] = {}
        for row in rows:
            user_id = str(row["user_id"] or "").strip()
            if not user_id:
                continue
            stats[user_id] = {
                "record_count": int(row["record_count"] or 0),
                "last_time": float(row["last_time"] or 0),
            }
        return stats

    def delete_memory_record(self, user_id: str, session_id: str) -> int:
        """删除指定会话的记忆条目。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        cleaned_session = str(session_id or "").strip()
        if not cleaned_user or not cleaned_session:
            return 0
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM memory_records WHERE user_id = ? AND session_id = ?",
                (cleaned_user, cleaned_session),
            )
        return int(cursor.rowcount or 0)

    def delete_memory_records_by_user(self, user_id: str) -> int:
        """清理用户所有记忆条目。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        if not cleaned_user:
            return 0
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM memory_records WHERE user_id = ?",
                (cleaned_user,),
            )
        return int(cursor.rowcount or 0)


    def upsert_memory_task_log(
        self,
        user_id: str,
        session_id: str,
        task_id: str,
        status: str,
        queued_time: float,
        started_time: float,
        finished_time: float,
        elapsed_s: float,
        request_payload: Optional[Dict[str, Any]],
        result: str,
        error: str,
        *,
        updated_time: Optional[float] = None,
    ) -> None:
        """写入或覆盖长期记忆任务日志，按会话覆盖保留最新一次记录。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        cleaned_session = str(session_id or "").strip()
        cleaned_task = str(task_id or "").strip()
        if not cleaned_user or not cleaned_session or not cleaned_task:
            return
        status_text = str(status or "").strip()
        payload_text = ""
        if request_payload:
            try:
                payload_text = json.dumps(request_payload, ensure_ascii=False)
            except TypeError:
                payload_text = json.dumps(str(request_payload), ensure_ascii=False)
        result_text = str(result or "").strip()
        error_text = str(error or "").strip()
        now = float(updated_time if updated_time is not None else time.time())
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO memory_task_logs
                  (task_id, user_id, session_id, status, queued_time, started_time,
                   finished_time, elapsed_s, request_payload, result, error, updated_time)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(user_id, session_id) DO UPDATE SET
                  task_id = excluded.task_id,
                  status = excluded.status,
                  queued_time = excluded.queued_time,
                  started_time = excluded.started_time,
                  finished_time = excluded.finished_time,
                  elapsed_s = excluded.elapsed_s,
                  request_payload = excluded.request_payload,
                  result = excluded.result,
                  error = excluded.error,
                  updated_time = excluded.updated_time
                """,
                (
                    cleaned_task,
                    cleaned_user,
                    cleaned_session,
                    status_text,
                    float(queued_time or 0),
                    float(started_time or 0),
                    float(finished_time or 0),
                    float(elapsed_s or 0),
                    payload_text,
                    result_text,
                    error_text,
                    now,
                ),
            )

    def load_memory_task_logs(self, limit: Optional[int] = None) -> List[Dict[str, Any]]:
        """读取长期记忆任务日志列表，默认按时间降序。"""
        self.ensure_initialized()
        sql = (
            "SELECT task_id, user_id, session_id, status, queued_time, started_time, "
            "finished_time, elapsed_s, updated_time "
            "FROM memory_task_logs ORDER BY updated_time DESC, id DESC"
        )
        params: List[Any] = []
        if limit is not None:
            safe_limit = max(1, int(limit))
            sql += " LIMIT ?"
            params.append(safe_limit)
        with self._connect() as conn:
            rows = conn.execute(sql, params).fetchall()
        logs: List[Dict[str, Any]] = []
        for row in rows:
            logs.append(
                {
                    "task_id": str(row["task_id"] or ""),
                    "user_id": str(row["user_id"] or ""),
                    "session_id": str(row["session_id"] or ""),
                    "status": str(row["status"] or ""),
                    "queued_time": float(row["queued_time"] or 0),
                    "started_time": float(row["started_time"] or 0),
                    "finished_time": float(row["finished_time"] or 0),
                    "elapsed_s": float(row["elapsed_s"] or 0),
                    "updated_time": float(row["updated_time"] or 0),
                }
            )
        return logs

    def load_memory_task_log_by_task_id(self, task_id: str) -> Optional[Dict[str, Any]]:
        """读取指定 task_id 的长期记忆任务日志详情。"""
        self.ensure_initialized()
        cleaned_task = str(task_id or "").strip()
        if not cleaned_task:
            return None
        with self._connect() as conn:
            row = conn.execute(
                """
                SELECT task_id, user_id, session_id, status, queued_time, started_time,
                       finished_time, elapsed_s, request_payload, result, error, updated_time
                FROM memory_task_logs
                WHERE task_id = ?
                ORDER BY updated_time DESC, id DESC
                LIMIT 1
                """,
                (cleaned_task,),
            ).fetchone()
        if not row:
            return None
        return {
            "task_id": str(row["task_id"] or ""),
            "user_id": str(row["user_id"] or ""),
            "session_id": str(row["session_id"] or ""),
            "status": str(row["status"] or ""),
            "queued_time": float(row["queued_time"] or 0),
            "started_time": float(row["started_time"] or 0),
            "finished_time": float(row["finished_time"] or 0),
            "elapsed_s": float(row["elapsed_s"] or 0),
            "request_payload": str(row["request_payload"] or ""),
            "result": str(row["result"] or ""),
            "error": str(row["error"] or ""),
            "updated_time": float(row["updated_time"] or 0),
        }

    def delete_memory_task_log(self, user_id: str, session_id: str) -> int:
        """删除指定会话的长期记忆任务日志。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        cleaned_session = str(session_id or "").strip()
        if not cleaned_user or not cleaned_session:
            return 0
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM memory_task_logs WHERE user_id = ? AND session_id = ?",
                (cleaned_user, cleaned_session),
            )
        return int(cursor.rowcount or 0)

    def delete_memory_task_logs_by_user(self, user_id: str) -> int:
        """删除用户所有长期记忆任务日志。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        if not cleaned_user:
            return 0
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM memory_task_logs WHERE user_id = ?",
                (cleaned_user,),
            )
        return int(cursor.rowcount or 0)

    def delete_memory_settings_by_user(self, user_id: str) -> int:
        """删除用户长期记忆开关配置。"""
        self.ensure_initialized()
        cleaned_user = str(user_id or "").strip()
        if not cleaned_user:
            return 0
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM memory_settings WHERE user_id = ?",
                (cleaned_user,),
            )
        return int(cursor.rowcount or 0)

    def write_system_log(
        self,
        created_at: str,
        level: str,
        logger: str,
        message: str,
        payload: Optional[Dict[str, Any]] = None,
    ) -> None:
        """写入系统日志。"""
        self.ensure_initialized()
        payload_json = self._json_dumps(payload) if payload else None
        with self._connect() as conn:
            conn.execute(
                """
                INSERT INTO system_logs (created_at, level, logger, message, payload)
                VALUES (?, ?, ?, ?, ?)
                """,
                (created_at, level, logger, message, payload_json),
            )

    def _connect(self) -> sqlite3.Connection:
        """创建 SQLite 连接并开启 WAL 模式。"""
        conn = sqlite3.connect(self._db_path, timeout=5, check_same_thread=False)
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA journal_mode=WAL;")
        conn.execute("PRAGMA synchronous=NORMAL;")
        conn.execute("PRAGMA foreign_keys=ON;")
        conn.execute("PRAGMA busy_timeout=3000;")
        return conn

    @staticmethod
    def _json_dumps(value: Any) -> str:
        """JSON 序列化，保证中文可读。"""
        return json.dumps(value, ensure_ascii=False, separators=(",", ":"))

    @staticmethod
    def _json_loads(raw: Optional[str]) -> Optional[Dict[str, Any]]:
        """JSON 反序列化，失败时返回 None。"""
        if not raw:
            return None
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError:
            return None
        if isinstance(payload, dict):
            return payload
        return None


class SQLiteLogHandler(logging.Handler):
    """将日志写入 SQLite 的日志处理器。"""

    def __init__(self, storage: SQLiteStorage) -> None:
        super().__init__()
        self._storage = storage
        self._lock = threading.Lock()

    def emit(self, record: logging.LogRecord) -> None:
        """落库日志，失败时静默降级。"""
        try:
            created_at = datetime.utcfromtimestamp(record.created).isoformat() + "Z"
            payload = {
                "pathname": record.pathname,
                "module": record.module,
                "func_name": record.funcName,
                "lineno": record.lineno,
                "process": record.process,
                "thread": record.thread,
                "thread_name": record.threadName,
            }
            if record.exc_info:
                payload["exception"] = self._format_exception(record)
            if record.stack_info:
                payload["stack"] = record.stack_info
            with self._lock:
                self._storage.write_system_log(
                    created_at=created_at,
                    level=record.levelname,
                    logger=record.name,
                    message=record.getMessage(),
                    payload=payload,
                )
        except Exception:
            return

    def _format_exception(self, record: logging.LogRecord) -> str:
        """格式化异常堆栈，避免丢失关键信息。"""
        if self.formatter:
            return self.formatter.formatException(record.exc_info)
        return logging.Formatter().formatException(record.exc_info)
