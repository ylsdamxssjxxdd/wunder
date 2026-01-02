from __future__ import annotations

import asyncio
import json
import re
import time
from dataclasses import dataclass
from datetime import datetime
from typing import Any, Dict, List, Optional

from app.core.i18n import t
from app.storage.sqlite import SQLiteStorage


@dataclass
class MemoryRecord:
    """用户会话级长期记忆记录。"""

    session_id: str
    summary: str
    created_time: float
    updated_time: float

    def to_dict(self) -> Dict[str, Any]:
        """转换为接口输出结构，补齐 ISO 时间。"""
        return {
            "session_id": self.session_id,
            "summary": self.summary,
            "created_time": _format_ts(self.created_time),
            "updated_time": _format_ts(self.updated_time),
            "created_time_ts": self.created_time,
            "updated_time_ts": self.updated_time,
        }


def _format_ts(ts: float) -> str:
    """将时间戳转为 ISO 字符串，便于前端展示。"""
    if not isinstance(ts, (int, float)) or ts <= 0:
        return ""
    return datetime.utcfromtimestamp(ts).isoformat() + "Z"


class MemoryStore:
    """长期记忆存储管理器，封装 SQLite 读写与格式化逻辑。"""

    def __init__(self, storage: SQLiteStorage, max_records: int = 30) -> None:
        self._storage = storage
        self._max_records = max(1, int(max_records))

    @property
    def max_records(self) -> int:
        """返回单用户长期记忆记录上限。"""
        return self._max_records

    @staticmethod
    def normalize_summary(text: str) -> str:
        """规范化记忆摘要为纯文本段落格式。"""
        raw = str(text or "").strip()
        if not raw:
            return ""
        # 如果模型返回包含 <memory_summary> 标签，则优先抽取标签内容
        tagged = MemoryStore._extract_tagged_summary(raw)
        if tagged is not None:
            parsed = MemoryStore._parse_summary_payload(tagged)
            if parsed is not None:
                return parsed
            raw = tagged
        parsed = MemoryStore._parse_summary_payload(raw)
        if parsed is not None:
            return parsed
        lines = [line.strip() for line in raw.splitlines() if line.strip()]
        segments: List[str] = []
        for line in lines:
            cleaned = re.sub(r"^[-*\u2022]\s*", "", line).strip()
            if cleaned:
                segments.append(cleaned)
        if not segments:
            return ""
        if len(segments) == 1:
            return segments[0]
        return "；".join(segments).strip()

    @staticmethod
    def _extract_tagged_summary(text: str) -> Optional[str]:
        """从 <memory_summary> 标签中抽取总结内容，未命中时返回 None。"""
        if not text:
            return None
        matches = re.findall(
            r"<memory_summary>(.*?)</memory_summary>",
            text,
            flags=re.IGNORECASE | re.DOTALL,
        )
        if not matches:
            return None
        parts = [part.strip() for part in matches if part and part.strip()]
        if not parts:
            return ""
        return "\n".join(parts).strip()

    @staticmethod
    def _parse_summary_payload(text: str) -> Optional[str]:
        """尝试从 JSON 摘要中组合为段落文本，失败时返回 None。"""
        raw = str(text or "").strip()
        if not raw:
            return ""
        try:
            data = json.loads(raw)
        except json.JSONDecodeError:
            return None
        segments: List[str] = []
        if isinstance(data, dict):
            values = list(data.values())
        elif isinstance(data, list):
            values = data
        else:
            return None
        for value in values:
            if isinstance(value, str):
                cleaned = value.strip()
            else:
                cleaned = json.dumps(value, ensure_ascii=False).strip()
            if cleaned and cleaned != "null":
                segments.append(cleaned)
        if not segments:
            return ""
        if len(segments) == 1:
            return segments[0]
        return "；".join(segments).strip()

    def build_prompt_block(self, records: List[MemoryRecord]) -> str:
        """构建系统提示词追加的长期记忆文本块。"""
        if not records:
            return ""
        chunks: List[str] = []
        for record in records:
            summary = self.normalize_summary(record.summary)
            if not summary:
                continue
            # 为每条记忆追加时间前缀，便于模型感知信息时序
            prefix_ts = record.updated_time or record.created_time
            prefix = self._format_memory_time_prefix(prefix_ts)
            if prefix:
                chunks.append(f"{prefix} {summary}")
            else:
                chunks.append(summary)
        merged = "\n".join(chunks).strip()
        if not merged:
            return ""
        # 统一加上长期记忆标签，后面保持纯文本列表格式
        return t("memory.block_prefix") + "\n" + merged

    @staticmethod
    def _format_memory_time_prefix(ts: float) -> str:
        """格式化长期记忆时间前缀（年月日时分）。"""
        if not isinstance(ts, (int, float)) or ts <= 0:
            return ""
        dt = datetime.fromtimestamp(float(ts))
        return t(
            "memory.time_prefix",
            year=dt.year,
            month=dt.month,
            day=dt.day,
            hour=dt.hour,
            minute=dt.minute,
        )

    async def is_enabled(self, user_id: str) -> bool:
        """读取用户长期记忆开关状态。"""
        value = await asyncio.to_thread(self._storage.get_memory_enabled, user_id)
        return bool(value)

    async def set_enabled(self, user_id: str, enabled: bool) -> None:
        """更新用户长期记忆开关状态。"""
        await asyncio.to_thread(self._storage.set_memory_enabled, user_id, enabled)

    async def list_settings(self) -> Dict[str, Dict[str, Any]]:
        """批量读取所有用户的记忆开关配置。"""
        return await asyncio.to_thread(self._storage.load_memory_settings)

    async def list_record_stats(self) -> Dict[str, Dict[str, Any]]:
        """读取所有用户的记忆记录统计信息。"""
        return await asyncio.to_thread(self._storage.get_memory_record_stats)

    async def list_records(
        self,
        user_id: str,
        *,
        limit: Optional[int] = None,
        order_desc: bool = True,
    ) -> List[MemoryRecord]:
        """读取指定用户的记忆记录列表。"""
        safe_limit = self._max_records if limit is None else max(1, int(limit))
        records = await asyncio.to_thread(
            self._storage.load_memory_records,
            user_id,
            safe_limit,
            order_desc=order_desc,
        )
        output: List[MemoryRecord] = []
        for record in records:
            output.append(
                MemoryRecord(
                    session_id=str(record.get("session_id", "")),
                    summary=str(record.get("summary", "")),
                    created_time=float(record.get("created_time", 0)),
                    updated_time=float(record.get("updated_time", 0)),
                )
            )
        return output

    @staticmethod
    def _format_task_log(item: Dict[str, Any]) -> Dict[str, Any]:
        """格式化长期记忆任务日志的时间字段，便于接口输出。"""
        queued_ts = float(item.get("queued_time") or 0)
        started_ts = float(item.get("started_time") or 0)
        finished_ts = float(item.get("finished_time") or 0)
        status = str(item.get("status") or "").strip()
        status_map = {
            "排队中": "queued",
            "queued": "queued",
            "正在处理": "running",
            "processing": "running",
            "已完成": "done",
            "completed": "done",
            "失败": "failed",
            "failed": "failed",
        }
        normalized = status_map.get(status.lower(), status_map.get(status, ""))
        if normalized == "queued":
            status = t("memory.status.queued")
        elif normalized == "running":
            status = t("memory.status.running")
        elif normalized == "done":
            status = t("memory.status.done")
        elif normalized == "failed":
            status = t("memory.status.failed")
        return {
            "task_id": str(item.get("task_id") or ""),
            "user_id": str(item.get("user_id") or ""),
            "session_id": str(item.get("session_id") or ""),
            "status": status,
            "queued_time": _format_ts(queued_ts),
            "queued_time_ts": queued_ts,
            "started_time": _format_ts(started_ts),
            "started_time_ts": started_ts,
            "finished_time": _format_ts(finished_ts),
            "finished_time_ts": finished_ts,
            "elapsed_s": float(item.get("elapsed_s") or 0),
        }

    @staticmethod
    def _parse_task_request(payload_text: str) -> Dict[str, Any]:
        """解析任务请求负载 JSON 文本，保持为空时返回空对象。"""
        if not payload_text:
            return {}
        try:
            data = json.loads(payload_text)
        except json.JSONDecodeError:
            return {}
        if isinstance(data, dict):
            return data
        return {}

    async def list_task_logs(self, limit: Optional[int] = None) -> List[Dict[str, Any]]:
        """读取长期记忆任务日志列表，用于历史队列展示。"""
        rows = await asyncio.to_thread(self._storage.load_memory_task_logs, limit)
        return [self._format_task_log(row) for row in rows]

    async def get_task_log(self, task_id: str) -> Optional[Dict[str, Any]]:
        """读取指定任务日志详情，用于调试弹窗。"""
        row = await asyncio.to_thread(self._storage.load_memory_task_log_by_task_id, task_id)
        if not row:
            return None
        detail = self._format_task_log(row)
        detail["request"] = self._parse_task_request(row.get("request_payload") or "")
        detail["result"] = str(row.get("result") or "")
        detail["error"] = str(row.get("error") or "")
        return detail

    async def upsert_task_log(
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
        now_ts: Optional[float] = None,
    ) -> None:
        """写入或覆盖长期记忆任务日志。"""
        await asyncio.to_thread(
            self._storage.upsert_memory_task_log,
            user_id,
            session_id,
            task_id,
            status,
            queued_time,
            started_time,
            finished_time,
            elapsed_s,
            request_payload,
            result,
            error,
            updated_time=now_ts,
        )

    async def upsert_record(
        self,
        user_id: str,
        session_id: str,
        summary: str,
        *,
        now_ts: Optional[float] = None,
    ) -> bool:
        """写入/覆盖用户的会话记忆记录。"""
        normalized = self.normalize_summary(summary)
        if not normalized:
            return False
        now = float(now_ts if now_ts is not None else time.time())
        await asyncio.to_thread(
            self._storage.upsert_memory_record,
            user_id,
            session_id,
            normalized,
            self._max_records,
            now,
        )
        return True

    async def update_record(
        self,
        user_id: str,
        session_id: str,
        summary: str,
        *,
        now_ts: Optional[float] = None,
    ) -> bool:
        """更新指定会话的记忆记录内容。"""
        return await self.upsert_record(
            user_id, session_id, summary, now_ts=now_ts
        )

    async def delete_record(self, user_id: str, session_id: str) -> int:
        """删除指定会话的记忆记录，并同步清理任务日志。"""
        deleted = await asyncio.to_thread(
            self._storage.delete_memory_record, user_id, session_id
        )
        await asyncio.to_thread(
            self._storage.delete_memory_task_log, user_id, session_id
        )
        return deleted

    async def clear_records(self, user_id: str) -> int:
        """清空用户所有记忆记录，并同步清理任务日志。"""
        deleted = await asyncio.to_thread(
            self._storage.delete_memory_records_by_user, user_id
        )
        await asyncio.to_thread(
            self._storage.delete_memory_task_logs_by_user, user_id
        )
        return deleted

