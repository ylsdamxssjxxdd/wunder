from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List, Optional

from app.core.config import LLMConfig
from app.core.i18n import get_known_prefixes, t
from app.orchestrator.constants import (
    ARTIFACT_INDEX_MAX_ITEMS,
    COMPACTION_META_TYPE,
    COMPACTION_OUTPUT_RESERVE,
    COMPACTION_RATIO,
    COMPACTION_SAFETY_MARGIN,
    OBSERVATION_PREFIX,
)
from app.orchestrator.prompt_builder import read_prompt_template
from app.orchestrator.context import RequestContext


class HistoryManager:
    """对话历史加载与压缩辅助。"""

    @staticmethod
    def format_compaction_summary(summary: str) -> str:
        """统一摘要格式，便于后续识别与注入上下文。"""
        cleaned = summary.strip()
        if not cleaned:
            cleaned = t("memory.empty_summary")
        prefixes = get_known_prefixes("history.compaction_prefix")
        if not any(cleaned.startswith(prefix) for prefix in prefixes):
            cleaned = f"{t('history.compaction_prefix')}\n{cleaned}"
        return cleaned

    @staticmethod
    def format_artifact_index(content: str) -> str:
        """统一产物索引格式，避免提示词识别失败。"""
        cleaned = str(content or "").strip()
        if not cleaned:
            return ""
        prefixes = get_known_prefixes("history.artifact_prefix")
        if not any(cleaned.startswith(prefix) for prefix in prefixes):
            cleaned = f"{t('history.artifact_prefix')}\n{cleaned}"
        return cleaned

    @staticmethod
    def is_compaction_summary_item(item: Dict[str, Any]) -> bool:
        """判断历史记录是否为压缩摘要条目。"""
        meta = item.get("meta")
        if isinstance(meta, dict) and meta.get("type") == COMPACTION_META_TYPE:
            return True
        content = item.get("content")
        if isinstance(content, str):
            prefixes = get_known_prefixes("history.compaction_prefix")
            if any(content.startswith(prefix) for prefix in prefixes):
                return True
        return False

    @staticmethod
    def load_compaction_prompt() -> str:
        """读取上下文压缩提示词，保证可维护性。"""
        prompt_path = Path(__file__).resolve().parent.parent / "prompts" / "compact_prompt.txt"
        try:
            return read_prompt_template(prompt_path).strip()
        except OSError:
            return t("history.compaction_prompt_fallback")

    @staticmethod
    def _unique_in_order(items: List[str]) -> List[str]:
        """保留顺序去重，避免产物索引出现重复条目。"""
        seen: set[str] = set()
        output: List[str] = []
        for item in items:
            if not item or item in seen:
                continue
            seen.add(item)
            output.append(item)
        return output

    @staticmethod
    def _format_index_items(items: List[str], limit: int) -> str:
        """按数量截断产物索引条目并追加统计信息。"""
        if not items:
            return ""
        total = len(items)
        display = items[:limit]
        suffix = t("history.items_suffix", total=total) if total > limit else ""
        return ", ".join(display) + suffix

    def _build_artifact_index_text(self, artifacts: List[Dict[str, Any]]) -> str:
        """聚合产物索引日志，生成结构化索引文本。"""
        if not artifacts:
            return ""
        file_reads: List[str] = []
        file_changes: Dict[str, List[str]] = {}
        commands: List[str] = []
        scripts: List[str] = []
        failures: List[str] = []
        action_labels = {
            "read": t("history.action.read"),
            "write": t("history.action.write"),
            "replace": t("history.action.replace"),
            "edit": t("history.action.edit"),
            "execute": t("history.action.execute"),
            "run": t("history.action.run"),
        }
        for entry in artifacts:
            kind = str(entry.get("kind", "")).strip()
            action = str(entry.get("action", "")).strip()
            name = str(entry.get("name", "")).strip()
            ok = entry.get("ok", True)
            error = str(entry.get("error", "") or "").strip()
            meta = entry.get("meta") if isinstance(entry.get("meta"), dict) else {}
            if error or ok is False:
                label = (
                    name
                    or str(entry.get("tool", "") or "").strip()
                    or t("history.failure.unknown_item")
                )
                failure_text = error or t("history.failure.execution")
                failures.append(f"{label}: {failure_text}")
            if not name:
                continue
            if kind == "file":
                if action == "read":
                    file_reads.append(name)
                else:
                    actions = file_changes.setdefault(name, [])
                    action_label = action_labels.get(action, action or t("history.action.unknown"))
                    if action_label not in actions:
                        actions.append(action_label)
            elif kind == "command":
                returncode = meta.get("returncode")
                rc_text = f"rc={returncode}" if returncode is not None else ""
                commands.append(f"{name}{'(' + rc_text + ')' if rc_text else ''}")
            elif kind == "script":
                returncode = meta.get("returncode")
                rc_text = f"rc={returncode}" if returncode is not None else ""
                scripts.append(f"{name}{'(' + rc_text + ')' if rc_text else ''}")
        file_reads = self._unique_in_order(file_reads)
        commands = self._unique_in_order(commands)
        scripts = self._unique_in_order(scripts)
        failures = self._unique_in_order(failures)
        file_change_items = []
        for path, actions in file_changes.items():
            action_text = "/".join(actions) if actions else t("history.action.unknown")
            file_change_items.append(f"{path}({action_text})")
        file_change_items = self._unique_in_order(file_change_items)
        list_limit = 12
        lines: List[str] = [t("history.artifact_prefix")]
        if file_reads:
            lines.append(
                t(
                    "history.summary.file_reads",
                    count=len(file_reads),
                    items=self._format_index_items(file_reads, list_limit),
                )
            )
        if file_change_items:
            lines.append(
                t(
                    "history.summary.file_changes",
                    count=len(file_change_items),
                    items=self._format_index_items(file_change_items, list_limit),
                )
            )
        if commands:
            lines.append(
                t(
                    "history.summary.command_runs",
                    count=len(commands),
                    items=self._format_index_items(commands, list_limit),
                )
            )
        if scripts:
            lines.append(
                t(
                    "history.summary.script_runs",
                    count=len(scripts),
                    items=self._format_index_items(scripts, list_limit),
                )
            )
        if failures:
            lines.append(
                t(
                    "history.summary.failures",
                    count=len(failures),
                    items=self._format_index_items(failures, list_limit),
                )
            )
        return "\n".join(lines)

    async def load_artifact_index_message(
        self, ctx: RequestContext, user_id: str, session_id: str
    ) -> str:
        """读取并生成产物索引消息，供上下文注入使用。"""
        artifacts = await ctx.workspace_manager.load_artifact_logs(
            user_id, session_id, ARTIFACT_INDEX_MAX_ITEMS
        )
        text = self._build_artifact_index_text(artifacts)
        return self.format_artifact_index(text)

    @staticmethod
    def _parse_timestamp(value: Any) -> Optional[float]:
        """解析时间戳，兼容 ISO 字符串与数值时间戳。"""
        if isinstance(value, (int, float)) and value > 0:
            return float(value)
        if not isinstance(value, str):
            return None
        text = value.strip()
        if not text:
            return None
        if text.endswith("Z"):
            text = f"{text[:-1]}+00:00"
        try:
            return datetime.fromisoformat(text).timestamp()
        except ValueError:
            return None

    @classmethod
    def _extract_compacted_until_ts(cls, item: Optional[Dict[str, Any]]) -> Optional[float]:
        """从摘要条目中提取压缩覆盖的时间边界。"""
        if not item:
            return None
        meta = item.get("meta")
        if not isinstance(meta, dict):
            return None
        raw = meta.get("compacted_until_ts") or meta.get("compacted_until")
        return cls._parse_timestamp(raw)

    @classmethod
    def get_item_timestamp(cls, item: Dict[str, Any]) -> Optional[float]:
        """读取历史条目的时间戳，供压缩边界计算使用。"""
        return cls._parse_timestamp(item.get("timestamp"))

    @staticmethod
    def get_auto_compact_limit(llm_config: LLMConfig) -> Optional[int]:
        """根据最大上下文估算自动压缩阈值，并预留输出与安全冗余。"""
        max_context = llm_config.max_context
        if not max_context or max_context <= 0:
            return None
        ratio_limit = int(max_context * COMPACTION_RATIO)
        reserve_output = (
            int(llm_config.max_output)
            if isinstance(llm_config.max_output, int) and llm_config.max_output > 0
            else COMPACTION_OUTPUT_RESERVE
        )
        reserve_output = max(0, reserve_output)
        hard_limit = max_context - reserve_output - COMPACTION_SAFETY_MARGIN
        if hard_limit <= 0:
            return max(1, min(max_context, ratio_limit))
        return max(1, min(ratio_limit, hard_limit))

    @classmethod
    def _find_latest_summary(
        cls, history: List[Dict[str, Any]]
    ) -> tuple[int, Optional[Dict[str, Any]]]:
        """定位最新的压缩摘要条目，返回索引与条目对象。"""
        summary_index = -1
        summary_item: Optional[Dict[str, Any]] = None
        for index, item in enumerate(history):
            if cls.is_compaction_summary_item(item):
                summary_index = index
                summary_item = item
        return summary_index, summary_item

    @classmethod
    def _filter_history_items(
        cls, history: List[Dict[str, Any]]
    ) -> tuple[List[Dict[str, Any]], Optional[Dict[str, Any]], Optional[float], int]:
        """过滤被摘要覆盖的历史条目，返回剩余条目与摘要信息。"""
        summary_index, summary_item = cls._find_latest_summary(history)
        compacted_until_ts = cls._extract_compacted_until_ts(summary_item)
        filtered: List[Dict[str, Any]] = []
        for index, item in enumerate(history):
            if cls.is_compaction_summary_item(item):
                continue
            role = item.get("role")
            if role == "system":
                continue
            if compacted_until_ts is not None:
                item_ts = cls._parse_timestamp(item.get("timestamp"))
                # 无时间戳时回退索引判断，避免历史被完全保留
                if item_ts is None and summary_index >= 0 and index <= summary_index:
                    continue
                if item_ts is not None and item_ts <= compacted_until_ts:
                    continue
            elif summary_index >= 0 and index <= summary_index:
                continue
            filtered.append(item)
        return filtered, summary_item, compacted_until_ts, summary_index

    @staticmethod
    def _build_message_from_item(
        item: Dict[str, Any], *, include_reasoning: bool = True
    ) -> Optional[Dict[str, Any]]:
        """将历史条目转换为上下文消息结构。"""
        role = item.get("role")
        content = item.get("content")
        if not role or content is None:
            return None
        if role == "tool":
            return {"role": "user", "content": OBSERVATION_PREFIX + str(content)}
        message = {"role": role, "content": content}
        if include_reasoning and role == "assistant":
            reasoning = item.get("reasoning_content") or item.get("reasoning") or ""
            if reasoning:
                message["reasoning_content"] = reasoning
        return message

    async def load_history_messages(
        self, ctx: RequestContext, user_id: str, session_id: str
    ) -> List[Dict[str, Any]]:
        """加载历史消息用于构建上下文。"""
        history = await ctx.workspace_manager.load_history(
            user_id, session_id, ctx.config.workspace.max_history_items
        )
        filtered_items, summary_item, _, _ = self._filter_history_items(history)
        messages: List[Dict[str, Any]] = []
        if summary_item:
            summary_content = self.format_compaction_summary(
                str(summary_item.get("content", ""))
            )
            messages.append({"role": "system", "content": summary_content})

        artifact_content = await self.load_artifact_index_message(
            ctx, user_id, session_id
        )
        if artifact_content:
            messages.append({"role": "system", "content": artifact_content})

        for item in filtered_items:
            message = self._build_message_from_item(item, include_reasoning=True)
            if message:
                messages.append(message)
        return messages

    @classmethod
    def build_compaction_candidates(
        cls, history: List[Dict[str, Any]]
    ) -> tuple[List[Dict[str, Any]], List[Dict[str, Any]]]:
        """整理可参与压缩的历史条目及其消息表示。"""
        filtered_items, _, _, _ = cls._filter_history_items(history)
        items: List[Dict[str, Any]] = []
        messages: List[Dict[str, Any]] = []
        for item in filtered_items:
            message = cls._build_message_from_item(item, include_reasoning=True)
            if not message:
                continue
            items.append(item)
            messages.append(message)
        return items, messages
