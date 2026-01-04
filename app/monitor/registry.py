from __future__ import annotations

import base64
import json
import os
import threading
import time
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any, Deque, Dict, List, Optional

import psutil

from app.core.config import get_config
from app.core.i18n import get_known_prefixes, t
from app.storage.sqlite import SQLiteStorage, get_storage

def _now_ts() -> float:
    return time.time()


def _format_ts(ts: float) -> str:
    return datetime.utcfromtimestamp(ts).isoformat() + "Z"


def _split_tool_summary_template(template: str) -> tuple[str, str]:
    """拆分工具调用摘要模板，提取前后缀。"""
    if "{tool}" not in template:
        return template, ""
    prefix, suffix = template.split("{tool}", 1)
    return prefix, suffix


_TOOL_SUMMARY_PARTS = [
    _split_tool_summary_template(item)
    for item in get_known_prefixes("monitor.summary.tool_call")
]
_SUMMARY_LOOKUP = {
    key: set(get_known_prefixes(key))
    for key in (
        "monitor.summary.restarted",
        "monitor.summary.finished",
        "monitor.summary.received",
        "monitor.summary.model_call",
        "monitor.summary.exception",
        "monitor.summary.cancelled",
        "monitor.summary.cancel_requested",
        "monitor.summary.user_deleted_cancel",
    )
}


def _localize_summary(summary: str) -> str:
    """根据当前语言转换已知摘要文案。"""
    if not summary:
        return summary
    for prefix, suffix in _TOOL_SUMMARY_PARTS:
        if not prefix:
            continue
        if not summary.startswith(prefix):
            continue
        if suffix and not summary.endswith(suffix):
            continue
        tool_name = summary[len(prefix) :]
        if suffix:
            tool_name = summary[len(prefix) : -len(suffix)]
        return t("monitor.summary.tool_call", tool=tool_name.strip())
    for key, candidates in _SUMMARY_LOOKUP.items():
        if summary in candidates:
            return t(key)
    return summary


@dataclass
class MonitorEvent:
    """监控事件记录。"""

    timestamp: float
    event_type: str
    data: Dict[str, Any]

    def to_storage(self) -> Dict[str, Any]:
        """转换为持久化存储结构。"""
        return {"timestamp": self.timestamp, "type": self.event_type, "data": self.data}

    @classmethod
    def from_storage(cls, payload: Dict[str, Any]) -> "MonitorEvent":
        """从持久化结构还原事件。"""
        timestamp = payload.get("timestamp")
        if not isinstance(timestamp, (int, float)):
            timestamp = _now_ts()
        event_type = str(payload.get("type", "unknown"))
        data = payload.get("data")
        if not isinstance(data, dict):
            data = {}
        return cls(timestamp=timestamp, event_type=event_type, data=data)

    def to_dict(self) -> Dict[str, Any]:
        data = self.data
        if isinstance(data, dict):
            summary = data.get("summary")
            if isinstance(summary, str):
                data = dict(data)
                data["summary"] = _localize_summary(summary)
        return {
            "timestamp": _format_ts(self.timestamp),
            "type": self.event_type,
            "data": data,
        }


@dataclass
class SessionRecord:
    """智能体会话状态记录。"""

    session_id: str
    user_id: str
    question: str
    status: str
    stage: str
    summary: str
    start_time: float
    updated_time: float
    cancel_requested: bool = False
    ended_time: Optional[float] = None
    rounds: int = 1
    token_usage: int = 0
    events: Deque[MonitorEvent] = field(default_factory=deque)

    def elapsed_s(self) -> float:
        end = self.ended_time if self.ended_time is not None else _now_ts()
        return max(0.0, end - self.start_time)

    def to_summary(self) -> Dict[str, Any]:
        return {
            "session_id": self.session_id,
            "user_id": self.user_id,
            "question": self.question,
            "status": self.status,
            "stage": self.stage,
            "summary": _localize_summary(self.summary),
            "start_time": _format_ts(self.start_time),
            "updated_time": _format_ts(self.updated_time),
            "elapsed_s": round(self.elapsed_s(), 2),
            "cancel_requested": self.cancel_requested,
            "token_usage": self.token_usage,
        }

    def to_detail(self) -> Dict[str, Any]:
        payload = self.to_summary()
        return payload

    def to_storage(self) -> Dict[str, Any]:
        """转换为持久化结构，保存历史线程。"""
        return {
            "session_id": self.session_id,
            "user_id": self.user_id,
            "question": self.question,
            "status": self.status,
            "stage": self.stage,
            "summary": self.summary,
            "start_time": self.start_time,
            "updated_time": self.updated_time,
            "ended_time": self.ended_time,
            "cancel_requested": self.cancel_requested,
            "rounds": self.rounds,
            "token_usage": self.token_usage,
            "events": [event.to_storage() for event in self.events],
        }

    @classmethod
    def from_storage(cls, payload: Dict[str, Any]) -> "SessionRecord":
        """从持久化结构还原线程记录。"""
        events = deque()
        for item in payload.get("events", []) or []:
            if isinstance(item, dict):
                events.append(MonitorEvent.from_storage(item))
        ended_time = payload.get("ended_time")
        if not isinstance(ended_time, (int, float)):
            ended_time = None
        return cls(
            session_id=str(payload.get("session_id", "")),
            user_id=str(payload.get("user_id", "")),
            question=str(payload.get("question", "")),
            status=str(payload.get("status", SessionMonitor.STATUS_FINISHED)),
            stage=str(payload.get("stage", "")),
            summary=str(payload.get("summary", "")),
            start_time=float(payload.get("start_time", _now_ts())),
            updated_time=float(payload.get("updated_time", _now_ts())),
            cancel_requested=bool(payload.get("cancel_requested", False)),
            ended_time=ended_time,
            rounds=int(payload.get("rounds", 1)),
            token_usage=int(payload.get("token_usage", 0)),
            events=events,
        )


class SessionMonitor:
    """智能体会话监控与系统资源采集。"""

    STATUS_RUNNING = "running"
    STATUS_FINISHED = "finished"
    STATUS_ERROR = "error"
    STATUS_CANCELLED = "cancelled"
    STATUS_CANCELLING = "cancelling"

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._sessions: Dict[str, SessionRecord] = {}
        # 记录被强制取消的会话，避免删除后仍需终止执行
        self._forced_cancelled_sessions: set[str] = set()
        self._proc = psutil.Process(os.getpid())
        # 记录应用启动时间，用于计算运行时长
        self._app_start_ts = _now_ts()
        # SQLite 持久化存储，统一记录监控与日志
        config = get_config()
        self._event_limit = self._resolve_event_limit(
            getattr(config.observability, "monitor_event_limit", 500)
        )
        self._payload_limit = self._resolve_payload_limit(
            getattr(config.observability, "monitor_payload_max_chars", 4000)
        )
        drop_items = getattr(config.observability, "monitor_drop_event_types", []) or []
        self._drop_event_types = {
            str(item).strip() for item in drop_items if str(item).strip()
        }
        self._storage: SQLiteStorage = get_storage(config.storage.db_path)
        self._storage.ensure_initialized()
        # 旧版监控历史目录，保留用于迁移
        self._history_dir = Path("data/historys/monitor")
        self._history_ready = threading.Event()
        self._history_lock = threading.Lock()
        self._history_loading = False
        # 后台开始加载历史记录，避免管理页首次调用阻塞
        self.warm_history(background=True)

    @staticmethod
    def _resolve_event_limit(raw: Any) -> Optional[int]:
        """解析监控事件保留上限，<= 0 表示不限制。"""
        if raw is None:
            return 500
        try:
            limit = int(raw)
        except (TypeError, ValueError):
            return 500
        if limit <= 0:
            return None
        return max(1, limit)

    @staticmethod
    def _resolve_payload_limit(raw: Any) -> Optional[int]:
        """解析监控事件内容长度上限，<= 0 表示不截断。"""
        if raw is None:
            return 4000
        try:
            limit = int(raw)
        except (TypeError, ValueError):
            return 4000
        if limit <= 0:
            return None
        return max(256, limit)

    @staticmethod
    def _safe_session_filename(session_id: str) -> str:
        """生成安全的文件名，避免 session_id 包含非法路径字符。"""
        encoded = base64.urlsafe_b64encode(session_id.encode("utf-8")).decode("ascii").rstrip("=")
        return f"{encoded}.json"

    def _save_record(self, record: SessionRecord) -> None:
        """持久化保存历史线程记录。"""
        payload = record.to_storage()
        try:
            self._storage.upsert_monitor_record(payload)
        except Exception:
            # 持久化失败时不影响主流程
            return

    def _load_history(self) -> None:
        """加载已持久化的历史线程。"""
        self._migrate_legacy_history()
        records: Dict[str, SessionRecord] = {}
        for payload in self._storage.load_monitor_records():
            if not isinstance(payload, dict):
                continue
            record = SessionRecord.from_storage(payload)
            # 进程重启后，未结束的线程视为异常结束
            if record.status in {self.STATUS_RUNNING, self.STATUS_CANCELLING}:
                record.status = self.STATUS_ERROR
                record.summary = t("monitor.summary.restarted")
                record.ended_time = record.updated_time or _now_ts()
                self._append_event(
                    record,
                    "restart",
                    {"summary": record.summary},
                    record.ended_time,
                )
            # 重启后按最终状态修正阶段与摘要，避免历史记录显示为中间阶段
            if record.status == self.STATUS_FINISHED:
                record.stage = "final"
                record.summary = t("monitor.summary.finished")
            elif record.status == self.STATUS_ERROR:
                record.stage = "error"
            elif record.status == self.STATUS_CANCELLED:
                record.stage = "cancelled"
            elif record.status == self.STATUS_CANCELLING:
                record.stage = "cancelling"
            if self._event_limit is not None and len(record.events) > self._event_limit:
                record.events = deque(list(record.events)[-self._event_limit :])
            records[record.session_id] = record
        if not records:
            return
        with self._lock:
            for session_id, record in records.items():
                current = self._sessions.get(session_id)
                if current and current.status in {
                    self.STATUS_RUNNING,
                    self.STATUS_CANCELLING,
                }:
                    continue
                if current and current.updated_time >= record.updated_time:
                    continue
                self._sessions[session_id] = record

    def _load_history_worker(self) -> None:
        """后台执行历史加载，完成后标记就绪。"""
        try:
            self._load_history()
        finally:
            with self._history_lock:
                self._history_loading = False
                self._history_ready.set()

    def warm_history(self, background: bool = True) -> bool:
        """触发历史加载，默认后台执行避免阻塞。"""
        if self._history_ready.is_set():
            return True
        with self._history_lock:
            if self._history_ready.is_set():
                return True
            if self._history_loading:
                return False
            self._history_loading = True
        if background:
            thread = threading.Thread(target=self._load_history_worker, daemon=True)
            thread.start()
            return False
        self._load_history_worker()
        return True

    def _trim_text(self, text: str) -> str:
        """裁剪超长文本，避免监控事件占用过多内存。"""
        if self._payload_limit is None:
            return text
        if len(text) <= self._payload_limit:
            return text
        return text[: self._payload_limit] + "...(truncated)"

    def _trim_string_fields(self, data: Dict[str, Any]) -> Dict[str, Any]:
        """裁剪事件数据中的顶层字符串字段，控制监控详情大小。"""
        if not data:
            return {}
        trimmed: Dict[str, Any] = {}
        for key, value in data.items():
            if isinstance(value, str):
                trimmed[key] = self._trim_text(value)
            else:
                trimmed[key] = value
        return trimmed

    def _summarize_messages(self, messages: Any) -> List[Dict[str, Any]]:
        """提取模型请求消息摘要，避免保存完整上下文。"""
        summary: List[Dict[str, Any]] = []
        if not isinstance(messages, list):
            return summary
        for item in messages:
            if not isinstance(item, dict):
                continue
            content = item.get("content")
            reasoning = item.get("reasoning_content") or item.get("reasoning")
            summary.append(
                {
                    "role": str(item.get("role", "")),
                    "content_len": len(str(content or "")),
                    "has_reasoning": bool(reasoning),
                }
            )
        return summary

    def _sanitize_event_data(self, event_type: str, data: Dict[str, Any]) -> Dict[str, Any]:
        """根据事件类型裁剪数据内容，降低监控内存压力。"""
        if not isinstance(data, dict):
            return {}
        if event_type == "llm_request":
            # 调试面板刷新后需要完整请求体，因此保留 payload 不做摘要裁剪
            # 仅裁剪顶层字符串字段，避免额外日志字段过长
            sanitized = dict(data)
            return self._trim_string_fields(sanitized)
        if event_type == "llm_output":
            sanitized = dict(data)
            if "content" in sanitized:
                sanitized["content"] = self._trim_text(str(sanitized.get("content") or ""))
            if "reasoning" in sanitized:
                sanitized["reasoning"] = self._trim_text(str(sanitized.get("reasoning") or ""))
            return self._trim_string_fields(sanitized)
        return self._trim_string_fields(data)

    def _append_event(
        self, record: SessionRecord, event_type: str, data: Dict[str, Any], timestamp: float
    ) -> None:
        """统一追加监控事件，执行裁剪与容量控制。"""
        if event_type in self._drop_event_types:
            return
        sanitized = self._sanitize_event_data(event_type, data)
        record.events.append(
            MonitorEvent(timestamp=timestamp, event_type=event_type, data=sanitized)
        )
        if self._event_limit is None:
            return
        while len(record.events) > self._event_limit:
            record.events.popleft()

    def _migrate_legacy_history(self) -> None:
        """迁移旧版监控 JSON 文件到 SQLite。"""
        migration_key = "monitor_migrated"
        if self._storage.get_meta(migration_key) == "1":
            return
        if not self._history_dir.exists():
            self._storage.set_meta(migration_key, "1")
            return
        for path in self._history_dir.glob("*.json"):
            try:
                payload = json.loads(path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                continue
            if not isinstance(payload, dict):
                continue
            self._storage.upsert_monitor_record(payload)
        self._storage.set_meta(migration_key, "1")

    def _has_active_session_locked(self, user_id: str) -> bool:
        """在锁内判断用户是否存在运行中的会话。"""
        if not user_id:
            return False
        for record in self._sessions.values():
            if record.user_id != user_id:
                continue
            if record.status in {self.STATUS_RUNNING, self.STATUS_CANCELLING}:
                return True
        return False

    def has_active_session(self, user_id: str) -> bool:
        """判断用户是否有仍在执行的会话。"""
        if not user_id:
            return False
        with self._lock:
            return self._has_active_session_locked(user_id)

    def _register_locked(
        self, session_id: str, user_id: str, question: str, now: float
    ) -> None:
        """在锁内注册会话，避免并发导致状态错乱。"""
        # 新会话注册时清理强制取消标记，避免复用 session_id 受影响
        self._forced_cancelled_sessions.discard(session_id)
        record = self._sessions.get(session_id)
        if record:
            record.rounds += 1
            record.question = question
            record.status = self.STATUS_RUNNING
            record.stage = "received"
            record.summary = t("monitor.summary.received")
            record.start_time = now
            record.updated_time = now
            record.ended_time = None
            record.cancel_requested = False
            record.token_usage = 0
            self._append_event(
                record,
                "round_start",
                {"summary": record.summary, "round": record.rounds, "question": question},
                now,
            )
            return
        record = SessionRecord(
            session_id=session_id,
            user_id=user_id,
            question=question,
            status=self.STATUS_RUNNING,
            stage="received",
            summary=t("monitor.summary.received"),
            start_time=now,
            updated_time=now,
        )
        self._append_event(
            record,
            "received",
            {"summary": record.summary, "round": record.rounds, "question": question},
            now,
        )
        self._sessions[session_id] = record

    def register(self, session_id: str, user_id: str, question: str) -> None:
        """注册新会话。"""
        now = _now_ts()
        with self._lock:
            self._register_locked(session_id, user_id, question, now)

    def try_register(self, session_id: str, user_id: str, question: str) -> bool:
        """尝试注册新会话，如已有运行中的会话则拒绝。"""
        now = _now_ts()
        with self._lock:
            # 同一用户已有运行中的会话时直接拒绝，避免并发挤占服务资源
            if self._has_active_session_locked(user_id):
                return False
            self._register_locked(session_id, user_id, question, now)
            return True

    def record_event(self, session_id: str, event_type: str, data: Dict[str, Any]) -> None:
        """记录事件并刷新会话状态。"""
        now = _now_ts()
        with self._lock:
            record = self._sessions.get(session_id)
            if not record:
                return
            record.updated_time = now
            if event_type == "token_usage":
                try:
                    record.token_usage = int(data.get("total_tokens", 0))
                except (TypeError, ValueError):
                    pass
            if event_type == "progress":
                record.stage = str(data.get("stage", record.stage))
                record.summary = str(data.get("summary", record.summary))
            elif event_type == "tool_call":
                record.stage = "tool_call"
                record.summary = t(
                    "monitor.summary.tool_call", tool=str(data.get("tool", "") or "")
                )
            elif event_type == "llm_request":
                record.stage = "llm_request"
                record.summary = t("monitor.summary.model_call")
            elif event_type == "final":
                record.stage = "final"
                record.summary = t("monitor.summary.finished")
            elif event_type == "error":
                record.stage = "error"
                record.summary = str(
                    data.get("message") or t("monitor.summary.exception")
                )

            self._append_event(record, event_type, data, now)

    def mark_finished(self, session_id: str) -> None:
        """标记会话完成。"""
        self._mark_status(session_id, self.STATUS_FINISHED)

    def mark_error(self, session_id: str, message: str) -> None:
        """标记会话异常。"""
        self._mark_status(session_id, self.STATUS_ERROR, summary=message)

    def mark_cancelled(self, session_id: str) -> None:
        """标记会话被终止。"""
        self._mark_status(
            session_id, self.STATUS_CANCELLED, summary=t("monitor.summary.cancelled")
        )

    def _mark_status(self, session_id: str, status: str, summary: Optional[str] = None) -> None:
        now = _now_ts()
        record: Optional[SessionRecord] = None
        with self._lock:
            record = self._sessions.get(session_id)
            if not record:
                return
            record.status = status
            record.updated_time = now
            record.ended_time = now
            # 根据最终状态修正阶段与摘要，确保落盘后仍显示正确阶段
            if status == self.STATUS_FINISHED:
                record.stage = "final"
                record.summary = summary or t("monitor.summary.finished")
            elif status == self.STATUS_ERROR:
                record.stage = "error"
                if summary:
                    record.summary = summary
            elif status == self.STATUS_CANCELLED:
                record.stage = "cancelled"
                if summary:
                    record.summary = summary
            elif status == self.STATUS_CANCELLING:
                record.stage = "cancelling"
                if summary:
                    record.summary = summary
            elif summary:
                record.summary = summary
            self._append_event(record, status, {"summary": record.summary}, now)
        if record:
            self._save_record(record)

    def list_sessions(self, active_only: bool = True) -> List[Dict[str, Any]]:
        """列出会话摘要。"""
        with self._lock:
            result: List[Dict[str, Any]] = []
            for session_id, record in self._sessions.items():
                if active_only and record.status not in {
                    self.STATUS_RUNNING,
                    self.STATUS_CANCELLING,
                }:
                    continue
                result.append(record.to_summary())
        return result

    def get_detail(self, session_id: str) -> Optional[Dict[str, Any]]:
        """获取会话详情。"""
        with self._lock:
            record = self._sessions.get(session_id)
            if not record:
                return None
            return {
                "session": record.to_detail(),
                "events": [event.to_dict() for event in record.events],
            }

    def cancel(self, session_id: str) -> bool:
        """请求终止指定会话。"""
        with self._lock:
            record = self._sessions.get(session_id)
            if not record:
                return False
            if record.status not in {self.STATUS_RUNNING, self.STATUS_CANCELLING}:
                return False
            record.cancel_requested = True
            record.status = self.STATUS_CANCELLING
            record.updated_time = _now_ts()
            self._append_event(
                record,
                "cancel",
                {"summary": t("monitor.summary.cancel_requested")},
                record.updated_time,
            )
            return True

    def set_app_start_time(self, timestamp: Optional[float] = None) -> None:
        """设置应用启动时间，便于监控面板显示运行时长。"""
        with self._lock:
            self._app_start_ts = timestamp if timestamp is not None else _now_ts()

    def delete_session(self, session_id: str) -> bool:
        """删除历史线程，运行中线程禁止删除。"""
        with self._lock:
            record = self._sessions.get(session_id)
            if not record:
                return False
            if record.status in {self.STATUS_RUNNING, self.STATUS_CANCELLING}:
                return False
            self._sessions.pop(session_id, None)
        self._storage.delete_monitor_record(session_id)
        return True

    def purge_user_sessions(self, user_id: str) -> Dict[str, int]:
        """清理指定用户的会话记录，并对运行中会话发起终止请求。"""
        cleaned = user_id.strip()
        if not cleaned:
            return {"cancelled": 0, "deleted": 0, "deleted_storage": 0}
        cancelled = 0
        session_ids: List[str] = []
        active_ids: List[str] = []
        with self._lock:
            for session_id, record in self._sessions.items():
                if record.user_id != cleaned:
                    continue
                session_ids.append(session_id)
                if record.status in {self.STATUS_RUNNING, self.STATUS_CANCELLING}:
                    record.cancel_requested = True
                    record.status = self.STATUS_CANCELLING
                    record.updated_time = _now_ts()
                    self._append_event(
                        record,
                        "cancel",
                        {"summary": t("monitor.summary.user_deleted_cancel")},
                        record.updated_time,
                    )
                    cancelled += 1
                    active_ids.append(session_id)
            # 标记强制取消，确保删除后仍可终止执行
            self._forced_cancelled_sessions.update(active_ids)
            for session_id in session_ids:
                self._sessions.pop(session_id, None)
        try:
            deleted_storage = self._storage.delete_monitor_records_by_user(cleaned)
        except Exception:
            deleted_storage = 0
        return {"cancelled": cancelled, "deleted": len(session_ids), "deleted_storage": deleted_storage}

    def is_cancelled(self, session_id: str) -> bool:
        """判断会话是否已请求终止。"""
        with self._lock:
            if session_id in self._forced_cancelled_sessions:
                return True
            record = self._sessions.get(session_id)
            return bool(record and record.cancel_requested)

    def get_system_metrics(self) -> Dict[str, Any]:
        """获取系统与进程资源占用。"""
        cpu_percent = psutil.cpu_percent(interval=None)
        mem = psutil.virtual_memory()
        proc_mem = self._proc.memory_info()
        proc_cpu = self._proc.cpu_percent(interval=None)
        # 系统负载仅在类 Unix 平台可用，Windows 环境返回 None
        load_avg_1 = load_avg_5 = load_avg_15 = None
        try:
            load_avg_1, load_avg_5, load_avg_15 = os.getloadavg()
        except (AttributeError, OSError):
            pass
        # 以当前工作目录所在磁盘为基准统计磁盘空间
        disk_total = disk_used = disk_free = 0
        disk_percent = 0.0
        try:
            disk_usage = psutil.disk_usage(os.getcwd())
            disk_total = disk_usage.total
            disk_used = disk_usage.used
            disk_free = disk_usage.free
            disk_percent = float(disk_usage.percent)
        except (OSError, RuntimeError, ValueError):
            pass
        # IO 与网络统计为系统累计值，适合趋势观察
        disk_read_bytes = disk_write_bytes = 0
        disk_io = psutil.disk_io_counters()
        if disk_io:
            disk_read_bytes = disk_io.read_bytes
            disk_write_bytes = disk_io.write_bytes
        net_sent_bytes = net_recv_bytes = 0
        net_io = psutil.net_io_counters()
        if net_io:
            net_sent_bytes = net_io.bytes_sent
            net_recv_bytes = net_io.bytes_recv
        uptime_s = max(0.0, _now_ts() - self._app_start_ts)
        return {
            "cpu_percent": cpu_percent,
            "memory_total": mem.total,
            "memory_used": mem.used,
            "memory_available": mem.available,
            "process_rss": proc_mem.rss,
            "process_cpu_percent": proc_cpu,
            "load_avg_1": load_avg_1,
            "load_avg_5": load_avg_5,
            "load_avg_15": load_avg_15,
            "disk_total": disk_total,
            "disk_used": disk_used,
            "disk_free": disk_free,
            "disk_percent": disk_percent,
            "disk_read_bytes": disk_read_bytes,
            "disk_write_bytes": disk_write_bytes,
            "net_sent_bytes": net_sent_bytes,
            "net_recv_bytes": net_recv_bytes,
            "uptime_s": uptime_s,
        }

    def get_service_metrics(
        self,
        recent_window_s: Optional[float] = None,
        now_ts: Optional[float] = None,
    ) -> Dict[str, Any]:
        """统计服务层线程指标，便于前端展示运行概况。"""
        now = (
            float(now_ts)
            if isinstance(now_ts, (int, float)) and now_ts > 0
            else _now_ts()
        )
        window_s = (
            float(recent_window_s)
            if isinstance(recent_window_s, (int, float)) and recent_window_s > 0
            else 3600.0
        )
        with self._lock:
            records = list(self._sessions.values())
        active_sessions = 0
        finished_sessions = 0
        error_sessions = 0
        cancelled_sessions = 0
        recent_completed = 0
        elapsed_total = 0.0
        elapsed_count = 0
        for record in records:
            if record.status in {self.STATUS_RUNNING, self.STATUS_CANCELLING}:
                active_sessions += 1
                continue
            if record.status == self.STATUS_FINISHED:
                finished_sessions += 1
            elif record.status == self.STATUS_ERROR:
                error_sessions += 1
            elif record.status == self.STATUS_CANCELLED:
                cancelled_sessions += 1
            end_ts = record.ended_time if record.ended_time is not None else record.updated_time
            if isinstance(end_ts, (int, float)):
                if now - end_ts <= window_s:
                    recent_completed += 1
                elapsed_total += max(0.0, end_ts - record.start_time)
                elapsed_count += 1
        history_sessions = len(records) - active_sessions
        avg_elapsed_s = round(elapsed_total / elapsed_count, 2) if elapsed_count else 0.0
        return {
            "active_sessions": active_sessions,
            "history_sessions": history_sessions,
            "finished_sessions": finished_sessions,
            "error_sessions": error_sessions,
            "cancelled_sessions": cancelled_sessions,
            "total_sessions": len(records),
            "recent_completed": recent_completed,
            "avg_elapsed_s": avg_elapsed_s,
        }

    def get_sandbox_metrics(
        self,
        since_time: Optional[float] = None,
        until_time: Optional[float] = None,
    ) -> Dict[str, Any]:
        """获取沙盒配置与调用统计，供内部状态页面展示。"""
        config = get_config()
        sandbox = config.sandbox
        since_ts = since_time if isinstance(since_time, (int, float)) else None
        if since_ts is not None and since_ts <= 0:
            since_ts = None
        until_ts = until_time if isinstance(until_time, (int, float)) else None
        if until_ts is not None and until_ts <= 0:
            until_ts = None
        call_count = 0
        session_ids: set[str] = set()
        with self._lock:
            records = list(self._sessions.values())
        for record in records:
            for event in record.events:
                if event.event_type != "tool_result":
                    continue
                if not event.data.get("sandbox"):
                    continue
                if since_ts is not None and event.timestamp < since_ts:
                    continue
                if until_ts is not None and event.timestamp > until_ts:
                    continue
                call_count += 1
                session_ids.add(record.session_id)
        return {
            "mode": str(sandbox.mode or ""),
            "network": str(sandbox.network or ""),
            "readonly_rootfs": bool(sandbox.readonly_rootfs),
            "idle_ttl_s": int(sandbox.idle_ttl_s or 0),
            "timeout_s": int(sandbox.timeout_s or 0),
            "endpoint": str(sandbox.endpoint or ""),
            "image": str(sandbox.image or ""),
            "resources": {
                "cpu": float(sandbox.resources.cpu or 0),
                "memory_mb": int(sandbox.resources.memory_mb or 0),
                "pids": int(sandbox.resources.pids or 0),
            },
            "recent_calls": call_count,
            "recent_sessions": len(session_ids),
        }


_MONITOR: Optional[SessionMonitor] = None
_MONITOR_LOCK = threading.Lock()
_APP_START_TS: Optional[float] = None


def get_monitor() -> SessionMonitor:
    """获取监控实例（惰性初始化）。"""
    global _MONITOR
    if _MONITOR is None:
        with _MONITOR_LOCK:
            if _MONITOR is None:
                _MONITOR = SessionMonitor()
                if _APP_START_TS is not None:
                    _MONITOR.set_app_start_time(_APP_START_TS)
    return _MONITOR


def warm_monitor_history() -> None:
    """后台预热监控历史加载，避免管理页面首次阻塞。"""
    try:
        get_monitor().warm_history(background=True)
    except Exception:
        return


def set_app_start_time(timestamp: Optional[float] = None) -> None:
    """记录应用启动时间，必要时同步到监控实例。"""
    global _APP_START_TS
    _APP_START_TS = timestamp if timestamp is not None else _now_ts()
    if _MONITOR is not None:
        _MONITOR.set_app_start_time(_APP_START_TS)


class _LazyMonitorProxy:
    """惰性加载监控实例的代理对象。"""

    def __getattr__(self, name):
        return getattr(get_monitor(), name)

    def __setattr__(self, name, value):
        setattr(get_monitor(), name, value)

    def __repr__(self) -> str:
        return repr(get_monitor())


monitor = _LazyMonitorProxy()
