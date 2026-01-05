from __future__ import annotations

import asyncio
import copy
import json
import random
import re
import time
import uuid
from collections import deque
from contextlib import suppress
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, AsyncGenerator, Dict, List, Optional, Sequence, Set, Tuple

from app.core.config import LLMConfig, WunderConfig, load_config, resolve_llm_config
from app.core.errors import ErrorCodes, WunderError
from app.core.i18n import get_language, reset_language, set_language, t
from app.core.token_utils import (
    approx_token_count,
    estimate_message_tokens,
    estimate_messages_tokens,
    trim_text_to_tokens,
    trim_messages_to_budget,
)
from app.tools.availability import collect_available_tool_names
from app.tools.catalog import resolve_builtin_tool_name
from app.llm.base import LLMResponse, LLMStreamChunk, LLMUnavailableError
from app.llm.factory import build_llm_client as _build_llm_client
from app.memory.workspace import WorkspaceContext, WorkspaceManager
from app.memory.longterm import MemoryStore
from app.monitor.registry import monitor
from app.orchestrator.constants import (
    COMPACTION_KEEP_RECENT_TOKENS,
    COMPACTION_HISTORY_RATIO,
    COMPACTION_META_TYPE,
    COMPACTION_MIN_OBSERVATION_TOKENS,
    COMPACTION_SUMMARY_MAX_OUTPUT,
    COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS,
    OBSERVATION_PREFIX,
    SESSION_LOCK_HEARTBEAT_S,
    SESSION_LOCK_POLL_INTERVAL_S,
    SESSION_LOCK_TTL_S,
    STREAM_EVENT_CLEANUP_INTERVAL_S,
    STREAM_EVENT_FETCH_LIMIT,
    STREAM_EVENT_POLL_INTERVAL_S,
    STREAM_EVENT_QUEUE_SIZE,
    STREAM_EVENT_TTL_S,
    TOOL_CALL_PATTERN,
    TOOL_CALL_CLOSE_PATTERN,
    TOOL_CALL_OPEN_PATTERN,
)
from app.orchestrator.context import RequestContext, UserToolBindings
from app.orchestrator.history import HistoryManager
from app.orchestrator.limiter import RequestLimiter
from app.orchestrator.prompting import PromptComposer
from app.orchestrator.prompt_builder import read_prompt_template
from app.orchestrator.tool_executor import ToolCallParser, ToolExecutor
from app.orchestrator.user_tools import UserToolManager
from app.orchestrator.stream_events import StreamEventStore
from app.sandbox.client import SandboxClient
from app.schemas.wunder import StreamEvent, WunderAttachment, WunderRequest, WunderResponse
from app.skills.loader import load_skills
from app.skills.registry import SkillRegistry
from app.tools.constants import SANDBOX_TOOL_NAMES
from app.tools.factory import build_tool_registry
from app.tools.mcp import MCPClient
from app.tools.registry import ToolRegistry
from app.tools.types import ToolResult
from app.user_tools.store import UserToolStore
from app.storage import get_storage


def build_llm_client(config: LLMConfig):
    """对外暴露 LLM 构建函数，方便测试注入假实现。"""
    return _build_llm_client(config)


_SSE_QUEUE_DONE = object()
_MEMORY_SUMMARY_PROMPT_PATH = Path(__file__).resolve().parent.parent / "prompts" / "memory_summary.txt"


@dataclass(frozen=True)
class PreparedRequest:
    """规范化后的请求参数，避免重复清洗。"""

    user_id: str
    question: str
    session_id: str
    tool_names: Optional[List[str]]
    model_name: Optional[str]
    config_overrides: Optional[Dict[str, Any]]
    stream: bool
    attachments: Optional[List[WunderAttachment]]
    language: str


@dataclass
class MemorySummaryTask:
    """长期记忆总结任务描述，用于队列排队执行。"""

    task_id: str
    user_id: str
    session_id: str
    queued_time: float
    config_overrides: Optional[Dict[str, Any]]
    model_name: Optional[str]
    attachments: Optional[List[WunderAttachment]]
    request_messages: Optional[List[Dict[str, Any]]]
    language: str = "zh-CN"
    status: str = ""
    start_time: float = 0.0
    end_time: float = 0.0
    request_payload: Optional[Dict[str, Any]] = None
    final_answer: str = ""
    summary_result: str = ""
    error: str = ""


class _EventEmitter:
    """统一的事件发射器：写入监控并按需推送 SSE。"""

    def __init__(
        self,
        session_id: str,
        user_id: str,
        queue: Optional[asyncio.Queue] = None,
        event_store: Optional[StreamEventStore] = None,
        done_sentinel: object = _SSE_QUEUE_DONE,
    ) -> None:
        self._session_id = session_id
        self._user_id = user_id
        self._queue = queue
        self._event_store = event_store
        self._done_sentinel = done_sentinel
        self._closed = False
        self._next_event_id = 1

    def close(self) -> None:
        """标记流式通道关闭，避免阻塞后台任务。"""
        self._closed = True

    def emit(self, event_type: str, data: Dict[str, Any]) -> StreamEvent:
        """创建并发送事件，自动写入监控记录。"""
        event = StreamEvent(type=event_type, session_id=self._session_id, data=data)
        self._assign_event_id(event)
        monitor.record_event(self._session_id, event_type, data)
        self._enqueue(event)
        return event

    def emit_event(self, event: StreamEvent) -> None:
        """发送已构建的事件对象。"""
        self._assign_event_id(event)
        monitor.record_event(event.session_id, event.type, event.data)
        self._enqueue(event)

    def finish(self) -> None:
        """发送结束信号，通知 SSE 停止读取。"""
        if not self._queue or self._closed:
            return
        try:
            self._queue.put_nowait(self._done_sentinel)
        except asyncio.QueueFull:
            pass

    def _assign_event_id(self, event: StreamEvent) -> None:
        """为事件补充递增序号，便于溢出回放去重。"""
        if getattr(event, "event_id", None) is not None:
            return
        event.event_id = self._next_event_id
        self._next_event_id += 1

    def _enqueue(self, event: StreamEvent) -> None:
        """非阻塞写入事件队列，避免 SSE 客户端断开导致任务卡死。"""
        if not self._queue or self._closed:
            return
        try:
            self._queue.put_nowait(event)
        except asyncio.QueueFull:
            # 队列满时将事件落库，供后续回放补齐
            if self._event_store and self._user_id:
                try:
                    self._event_store.record_overflow_event(self._user_id, event)
                except Exception:
                    return


class WunderOrchestrator:
    """核心调度器：串联提示词、LLM、工具与历史记录。"""

    def __init__(self, config_path: str) -> None:
        self._config_path = Path(config_path)
        self._config = load_config(self._config_path)
        self._config_version = int(time.time())
        # 提示词缓存由版本号/工具选择等条件驱动失效，适当延长 TTL 降低重复构建开销
        self._prompt_composer = PromptComposer(cache_ttl_s=60)
        self._prompt_composer.set_config_version(self._config_version)
        self._storage = get_storage(self._config.storage)
        self._memory_store = MemoryStore(self._storage)
        self._event_store = StreamEventStore(
            self._storage,
            ttl_s=STREAM_EVENT_TTL_S,
            cleanup_interval_s=STREAM_EVENT_CLEANUP_INTERVAL_S,
        )
        self._workspace_manager = WorkspaceManager(self._config)
        # 长期记忆总结队列：按线程完成时间排队单线程处理
        self._memory_queue: Optional[asyncio.PriorityQueue] = None
        self._memory_queue_task: Optional[asyncio.Task] = None
        self._memory_queue_guard: Optional[asyncio.Lock] = None
        self._memory_queue_seq = 0
        self._memory_active_task: Optional[MemorySummaryTask] = None
        # 记忆总结历史队列：用于展示最近完成的任务
        self._memory_task_history: deque[MemorySummaryTask] = deque(maxlen=100)
        # 全局并发限制器：超过上限时排队等待，默认 30。
        self._request_limiter = RequestLimiter(
            self._storage,
            self._config.server.max_active_sessions,
            poll_interval_s=SESSION_LOCK_POLL_INTERVAL_S,
            lock_ttl_s=SESSION_LOCK_TTL_S,
        )
        self._user_tool_store = UserToolStore(self._config)
        self._user_tool_manager = UserToolManager(self._user_tool_store)
        self._history_manager = HistoryManager()
        self._tool_executor = ToolExecutor(self._user_tool_manager)
        self._skills = self._load_skill_registry(self._config)

    @property
    def workspace_manager(self) -> WorkspaceManager:
        """对外暴露工作区管理器。"""
        return self._workspace_manager

    @property
    def config(self) -> WunderConfig:
        """对外暴露当前生效配置。"""
        return self._config

    @property
    def skills(self) -> SkillRegistry:
        """对外暴露技能注册表。"""
        return self._skills

    @property
    def user_tool_store(self) -> UserToolStore:
        """对外暴露用户工具配置存储。"""
        return self._user_tool_store

    @property
    def user_tool_manager(self) -> UserToolManager:
        """对外暴露用户工具管理器，便于执行阶段复用缓存。"""
        return self._user_tool_manager

    @property
    def memory_store(self) -> MemoryStore:
        """对外暴露长期记忆存储管理器。"""
        return self._memory_store

    async def get_memory_queue_status(self) -> Dict[str, Any]:
        """读取长期记忆总结队列状态，供管理端轮询展示。"""
        now_ts = time.time()
        active_task = self._memory_active_task
        queued_items: List[Tuple[float, int, MemorySummaryTask]] = []
        if self._memory_queue is not None:
            try:
                raw_queue = list(getattr(self._memory_queue, "_queue", []))
            except Exception:
                raw_queue = []
            for item in raw_queue:
                if not isinstance(item, tuple) or len(item) < 3:
                    continue
                queued_time, seq, task = item
                if not isinstance(task, MemorySummaryTask):
                    continue
                queued_items.append((float(queued_time or 0), int(seq or 0), task))
        queued_items.sort(key=lambda entry: (entry[0], entry[1]))
        active: List[Dict[str, Any]] = []
        if active_task is not None:
            active.append(self._format_memory_task(active_task, now_ts))
        active.extend(
            self._format_memory_task(task, now_ts) for _, _, task in queued_items
        )
        # 历史队列从存储中读取，保证重启后仍可追踪
        try:
            history = await self._memory_store.list_task_logs()
        except Exception:
            history = [
                self._format_memory_task(task, now_ts)
                for task in list(self._memory_task_history)
            ]
        return {
            "active": active,
            "history": history,
        }

    async def get_memory_queue_detail(self, task_id: str) -> Optional[Dict[str, Any]]:
        """读取指定长期记忆任务详情，包含完整请求载荷。"""
        cleaned = str(task_id or "").strip()
        if not cleaned:
            return None
        task = self._find_memory_task(cleaned)
        if task is not None:
            detail = self._format_memory_task(task, time.time())
            if task.request_payload is None:
                try:
                    task.request_payload = (
                        await self._build_memory_summary_request_payload(task)
                    )
                except Exception as exc:  # noqa: BLE001
                    detail["error"] = str(exc)
            detail["request"] = task.request_payload or {}
            detail["result"] = task.summary_result or ""
            if task.error:
                detail["error"] = task.error
            return detail
        return await self._memory_store.get_task_log(cleaned)

    def _find_memory_task(self, task_id: str) -> Optional[MemorySummaryTask]:
        """在活动、排队与历史任务中查找指定任务。"""
        active_task = self._memory_active_task
        if active_task and active_task.task_id == task_id:
            return active_task
        if self._memory_queue is not None:
            try:
                raw_queue = list(getattr(self._memory_queue, "_queue", []))
            except Exception:
                raw_queue = []
            for item in raw_queue:
                if not isinstance(item, tuple) or len(item) < 3:
                    continue
                task = item[2]
                if isinstance(task, MemorySummaryTask) and task.task_id == task_id:
                    return task
        for task in self._memory_task_history:
            if task.task_id == task_id:
                return task
        return None

    async def _build_memory_summary_request_payload(
        self, task: MemorySummaryTask
    ) -> Dict[str, Any]:
        """按当前配置重建记忆总结请求载荷，供管理端调试。"""
        token = set_language(task.language)
        try:
            ctx, llm_name, summary_llm_config = self._build_memory_summary_context(task)
            messages = await self._build_memory_summary_messages(
                ctx, task, summary_llm_config
            )
            payload_messages = self._sanitize_messages_for_log(
                copy.deepcopy(messages), task.attachments
            )
            return self._build_memory_summary_payload(task, llm_name, payload_messages)
        finally:
            reset_language(token)

    def apply_config(self, config: WunderConfig) -> None:
        """更新运行时配置并刷新关联缓存。"""
        self._config = config
        self._config_version += 1
        self._prompt_composer.set_config_version(self._config_version)
        self._prompt_composer.clear_cache()
        # 配置更新后需重建依赖配置的模块实例
        self._storage = get_storage(self._config.storage)
        self._memory_store = MemoryStore(self._storage)
        self._event_store = StreamEventStore(
            self._storage,
            ttl_s=STREAM_EVENT_TTL_S,
            cleanup_interval_s=STREAM_EVENT_CLEANUP_INTERVAL_S,
        )
        self._workspace_manager = WorkspaceManager(self._config)
        self._request_limiter = RequestLimiter(
            self._storage,
            self._config.server.max_active_sessions,
            poll_interval_s=SESSION_LOCK_POLL_INTERVAL_S,
            lock_ttl_s=SESSION_LOCK_TTL_S,
        )
        self._user_tool_store = UserToolStore(self._config)
        self._user_tool_manager = UserToolManager(self._user_tool_store)
        self._tool_executor = ToolExecutor(self._user_tool_manager)
        self._skills = self._load_skill_registry(self._config)

    def _load_skill_registry(self, config: WunderConfig) -> SkillRegistry:
        """加载技能注册表，补齐 EVA_SKILLS 目录。"""
        scan_paths = list(config.skills.paths)
        eva_skills = Path("EVA_SKILLS")
        if eva_skills.exists() and str(eva_skills) not in scan_paths:
            scan_paths.append(str(eva_skills))
        scan_config = config.model_copy(deep=True)
        scan_config.skills.paths = scan_paths
        return load_skills(scan_config, load_entrypoints=True, only_enabled=True)

    def _resolve_config(self, overrides: Optional[Dict[str, Any]]) -> WunderConfig:
        """根据覆盖配置构建当前请求配置。"""
        if overrides:
            return load_config(self._config_path, overrides)
        return self._config

    async def get_system_prompt(
        self,
        user_id: str,
        config_overrides: Optional[Dict[str, Any]] = None,
        tool_names: Optional[List[str]] = None,
    ) -> str:
        """构建指定用户的系统提示词。"""
        cleaned_user_id = str(user_id or "").strip()
        if not cleaned_user_id:
            raise ValueError(t("error.user_id_required"))
        config = self._resolve_config(config_overrides)
        llm_name, llm_config = resolve_llm_config(config)
        skills = self._skills if config is self._config else self._load_skill_registry(config)
        # 系统提示词预览不依赖实际工具执行器，使用轻量注册表避免初始化成本
        tools = ToolRegistry()
        ctx = RequestContext(
            config=config,
            llm_config=llm_config,
            llm_name=llm_name,
            tools=tools,
            skills=skills,
            mcp_client=MCPClient(config),
            workspace_manager=self._workspace_manager,
        )
        user_tool_bindings = self._user_tool_manager.build_bindings(ctx, cleaned_user_id)
        allowed_tool_names = self._resolve_allowed_tool_names(
            ctx, tool_names, user_tool_bindings
        )
        workdir = self._workspace_manager.ensure_workspace(cleaned_user_id)
        prompt = await self._prompt_composer.build_system_prompt_cached(
            ctx,
            workdir,
            cleaned_user_id,
            config_overrides,
            allowed_tool_names,
            user_tool_bindings,
        )
        prompt = await self._append_memory_prompt(cleaned_user_id, prompt)
        return prompt

    async def run(self, request: WunderRequest) -> WunderResponse:
        """执行一次非流式请求。"""
        prepared = self._prepare_request(request)
        emitter = _EventEmitter(prepared.session_id, prepared.user_id)
        return await self._execute_request(prepared, emitter)

    async def sse_stream(self, request: WunderRequest) -> AsyncGenerator[str, None]:
        """执行流式请求并返回 SSE 数据流。"""
        prepared = self._prepare_request(request)
        queue: asyncio.Queue[StreamEvent | object] = asyncio.Queue(
            maxsize=STREAM_EVENT_QUEUE_SIZE
        )
        emitter = _EventEmitter(
            prepared.session_id,
            prepared.user_id,
            queue=queue,
            event_store=self._event_store,
            done_sentinel=_SSE_QUEUE_DONE,
        )

        async def _runner() -> None:
            try:
                await self._execute_request(prepared, emitter)
            except Exception:
                # 异常已在执行流程中转换为事件，这里仅避免任务未捕获报错
                return

        def _silence_task(task: asyncio.Task) -> None:
            try:
                task.result()
            except Exception:
                return

        task = asyncio.create_task(_runner())
        task.add_done_callback(_silence_task)
        last_event_id = 0
        closed = False
        poll_interval = STREAM_EVENT_POLL_INTERVAL_S
        fetch_limit = STREAM_EVENT_FETCH_LIMIT

        async def _drain_overflow_until(target_event_id: int) -> List[StreamEvent]:
            if not self._event_store or target_event_id <= last_event_id:
                return []
            drained: List[StreamEvent] = []
            current = last_event_id
            while current < target_event_id:
                overflow_events = await self._event_store.load_overflow_events(
                    prepared.session_id, current, fetch_limit
                )
                if not overflow_events:
                    break
                progressed = False
                for overflow_event in overflow_events:
                    if overflow_event.event_id is None:
                        continue
                    if overflow_event.event_id <= current:
                        continue
                    drained.append(overflow_event)
                    current = overflow_event.event_id
                    progressed = True
                    if current >= target_event_id:
                        break
                if not progressed:
                    break
            return drained
        try:
            while True:
                event: StreamEvent | object | None = None
                if not closed:
                    try:
                        event = await asyncio.wait_for(
                            queue.get(), timeout=poll_interval
                        )
                    except asyncio.TimeoutError:
                        event = None
                if event is _SSE_QUEUE_DONE:
                    closed = True
                    continue
                if isinstance(event, StreamEvent):
                    if event.event_id is not None and event.event_id > last_event_id + 1:
                        drained_events = await _drain_overflow_until(
                            event.event_id - 1
                        )
                        for drained_event in drained_events:
                            yield self._format_sse(drained_event)
                            if drained_event.event_id is not None:
                                last_event_id = drained_event.event_id
                    if event.event_id is not None and event.event_id <= last_event_id:
                        continue
                    yield self._format_sse(event)
                    if event.event_id is not None:
                        last_event_id = event.event_id
                    continue
                if self._event_store:
                    overflow_events = await self._event_store.load_overflow_events(
                        prepared.session_id, last_event_id, fetch_limit
                    )
                    if overflow_events:
                        for overflow_event in overflow_events:
                            yield self._format_sse(overflow_event)
                            if overflow_event.event_id is not None:
                                last_event_id = overflow_event.event_id
                        continue
                if closed:
                    if task.done():
                        break
                    await asyncio.sleep(poll_interval)
                if task.done() and queue.empty():
                    break
        finally:
            # 客户端断开后关闭队列写入，后台任务继续执行
            emitter.close()

    def _prepare_request(self, request: WunderRequest) -> PreparedRequest:
        """清洗并标准化请求参数。"""
        user_id = str(request.user_id or "").strip()
        question = str(request.question or "").strip()
        if not user_id:
            raise WunderError(ErrorCodes.INVALID_REQUEST, t("error.user_id_required"))
        if not question:
            raise WunderError(ErrorCodes.INVALID_REQUEST, t("error.question_required"))
        session_id = str(request.session_id or "").strip() or self._generate_session_id()
        tool_names: Optional[List[str]] = None
        if request.tool_names is not None:
            cleaned: List[str] = []
            seen: Set[str] = set()
            for raw in request.tool_names:
                name = str(raw or "").strip()
                if not name or name in seen:
                    continue
                cleaned.append(name)
                seen.add(name)
            tool_names = cleaned
        model_name = str(request.model_name or "").strip() or None
        overrides = (
            request.config_overrides
            if isinstance(request.config_overrides, dict)
            else None
        )
        attachments: Optional[List[WunderAttachment]] = None
        if isinstance(request.attachments, list):
            # 仅保留有内容的附件，避免空数据污染上下文
            cleaned_attachments: List[WunderAttachment] = []
            for item in request.attachments:
                if not item or not str(item.content or "").strip():
                    continue
                cleaned_attachments.append(item)
            if cleaned_attachments:
                attachments = cleaned_attachments
        stream = bool(request.stream)
        return PreparedRequest(
            user_id=user_id,
            question=question,
            session_id=session_id,
            tool_names=tool_names,
            model_name=model_name,
            config_overrides=overrides,
            stream=stream,
            attachments=attachments,
            language=get_language(),
        )

    async def _execute_request(
        self, prepared: PreparedRequest, emitter: _EventEmitter
    ) -> WunderResponse:
        """执行完整的调度流程并返回结果。"""
        user_id = prepared.user_id
        session_id = prepared.session_id
        question = prepared.question
        limiter = self._request_limiter
        acquired = False
        heartbeat_task: Optional[asyncio.Task] = None
        try:
            # 使用跨进程会话锁控制用户互斥与全局并发。
            acquired = await limiter.acquire(session_id=session_id, user_id=user_id)
            if not acquired:
                raise WunderError(ErrorCodes.USER_BUSY, t("error.user_session_busy"))
            # 启动心跳续租，避免长任务被误判过期。
            heartbeat_task = asyncio.create_task(
                self._keep_session_lock(limiter, session_id)
            )
            monitor.register(session_id, user_id, question)
            emitter.emit(
                "progress",
                {"stage": "received", "summary": t("monitor.summary.received")},
            )

            config = self._resolve_config(prepared.config_overrides)
            llm_name, llm_config = resolve_llm_config(config, prepared.model_name)
            skills = self._skills if config is self._config else self._load_skill_registry(config)
            tools = build_tool_registry(config, llm_config=llm_config)
            ctx = RequestContext(
                config=config,
                llm_config=llm_config,
                llm_name=llm_name,
                tools=tools,
                skills=skills,
                mcp_client=MCPClient(config),
                workspace_manager=self._workspace_manager,
            )
            user_tool_bindings = self._user_tool_manager.build_bindings(ctx, user_id)
            allowed_tool_names = self._resolve_allowed_tool_names(
                ctx, prepared.tool_names, user_tool_bindings
            )

            workspace_root = self._workspace_manager.ensure_workspace(user_id)
            workspace = WorkspaceContext(
                user_id=user_id, session_id=session_id, root=workspace_root
            )
            skill_path_index = self._user_tool_manager.build_skill_path_index(
                ctx, user_tool_bindings
            )

            system_prompt = await self._prompt_composer.build_system_prompt_cached(
                ctx,
                workspace_root,
                user_id,
                prepared.config_overrides,
                allowed_tool_names,
                user_tool_bindings,
            )
            system_prompt = await self._resolve_session_prompt(
                ctx,
                user_id,
                session_id,
                system_prompt,
                prepared.tool_names,
                prepared.config_overrides,
                prepared.language,
            )
            system_prompt = await self._append_memory_prompt(user_id, system_prompt)
            history_messages = await self._history_manager.load_history_messages(
                ctx, user_id, session_id
            )
            messages: List[Dict[str, Any]] = [
                {"role": "system", "content": system_prompt}
            ]
            messages.extend(history_messages)
            messages.append(self._build_user_message(question, prepared.attachments))
            await self._append_chat(user_id, session_id, "user", question)

            max_rounds = max(1, int(ctx.llm_config.max_rounds or 1))
            # 仅保留最新一次模型调用的用量，避免跨轮次累加
            last_usage: Optional[Dict[str, int]] = None
            answer = ""
            a2ui_uid: Optional[str] = None
            a2ui_messages: Optional[List[Dict[str, Any]]] = None
            last_response: Optional[LLMResponse] = None
            # 记录最后一次模型调用的请求消息，供长期记忆总结复用
            last_request_messages: Optional[List[Dict[str, Any]]] = None

            for round_index in range(1, max_rounds + 1):
                # 每轮调用前检查取消标记，避免继续进入模型推理
                self._ensure_not_cancelled(session_id)
                messages, _ = await self._maybe_compact_messages(
                    ctx, user_id, session_id, messages, emitter
                )
                # 压缩流程可能触发耗时 LLM 调用，结束后再次确认取消标记
                self._ensure_not_cancelled(session_id)
                # 保存本轮调用的请求消息快照，避免后续变更影响总结输入
                last_request_messages = copy.deepcopy(
                    self._sanitize_messages_for_log(messages, prepared.attachments)
                )
                emitter.emit(
                    "progress",
                    {
                        "stage": "llm_call",
                        "summary": t("monitor.summary.model_call"),
                        "round": round_index,
                    },
                )
                llm_response, usage = await self._call_llm(
                    ctx,
                    messages,
                    emitter,
                    session_id,
                    prepared.stream,
                    round_index,
                    attachments=prepared.attachments,
                )
                last_response = llm_response
                if usage:
                    last_usage = usage
                    # 同步最新用量，供后续压缩判断使用
                    try:
                        total_tokens = int(usage.get("total_tokens", 0))
                        await ctx.workspace_manager.save_session_token_usage(
                            user_id, session_id, max(0, total_tokens)
                        )
                    except Exception:
                        pass

                content = llm_response.content
                reasoning = llm_response.reasoning
                tool_calls = ToolCallParser.parse(content or "")
                if not tool_calls:
                    answer = self._resolve_final_answer(content, reasoning)
                    if answer:
                        await self._append_chat(
                            user_id, session_id, "assistant", answer, reasoning
                        )
                    else:
                        await self._append_chat(
                            user_id, session_id, "assistant", content, reasoning
                        )
                        answer = str(content or "").strip()
                    break

                cleaned_content = self._strip_tool_calls(content)
                if cleaned_content.strip():
                    messages.append(
                        {
                            "role": "assistant",
                            "content": cleaned_content,
                            "reasoning_content": reasoning,
                        }
                    )
                    await self._append_chat(
                        user_id,
                        session_id,
                        "assistant",
                        cleaned_content,
                        reasoning,
                    )

                for call in tool_calls:
                    name = str(call.get("name", "")).strip()
                    args = call.get("arguments", {})
                    if not name:
                        continue
                    # 兼容内置工具英文别名，确保允许名单与执行入口一致。
                    name = resolve_builtin_tool_name(name)
                    # 工具调用前检查取消状态，避免继续执行耗时步骤
                    self._ensure_not_cancelled(session_id)
                    if name == "a2ui":
                        uid, messages, content = self._resolve_a2ui_tool_payload(
                            args, user_id, session_id
                        )
                        if messages:
                            emitter.emit(
                                "a2ui",
                                {
                                    "uid": uid,
                                    "messages": messages,
                                    "content": content,
                                },
                            )
                        a2ui_uid = uid or None
                        a2ui_messages = messages or None
                        answer = content or t("response.a2ui_fallback")
                        await self._log_a2ui_tool_call(
                            user_id,
                            session_id,
                            name,
                            args,
                            uid,
                            len(messages),
                            content,
                        )
                        if answer:
                            await self._append_chat(
                                user_id, session_id, "assistant", answer
                            )
                        break
                    if name == "最终回复":
                        answer = self._resolve_final_answer_from_tool(args)
                        await self._log_final_tool_call(
                            user_id, session_id, name, args
                        )
                        if answer:
                            await self._append_chat(
                                user_id, session_id, "assistant", answer
                            )
                        break
                    if name not in allowed_tool_names:
                        safe_args = args if isinstance(args, dict) else {"raw": args}
                        emitter.emit("tool_call", {"tool": name, "args": safe_args})
                        result = self._build_tool_error_result(
                            name, t("error.tool_disabled_or_unavailable")
                        )
                        payload = {"tool": name, "ok": result.ok, "data": result.data}
                        if result.error:
                            payload["error"] = result.error
                        if self._is_sandbox_tool(ctx, name):
                            payload["sandbox"] = True
                        emitter.emit("tool_result", payload)
                    else:
                        result = await self._execute_tool_call(
                            ctx,
                            workspace,
                            name,
                            args,
                            emitter,
                            user_tool_bindings,
                        )
                    observation = self._build_tool_observation(name, result)
                    messages.append(
                        {
                            "role": "user",
                            "content": OBSERVATION_PREFIX + observation,
                        }
                    )
                    await self._append_chat(user_id, session_id, "tool", observation)
                    await self._append_tool_log(
                        user_id,
                        session_id,
                        name,
                        args,
                        result,
                        sandbox=self._is_sandbox_tool(ctx, name),
                    )
                    await self._append_artifact_logs(
                        ctx, user_id, session_id, name, args, result
                    )
                    if name == "读取文件":
                        await self._append_skill_usage_logs(
                            user_id,
                            session_id,
                            args,
                            workspace.root,
                            skill_path_index,
                        )
                    await self._release_sandbox_if_needed(ctx, user_id, session_id, name)
                    # 工具执行后再次检查取消状态，尽早终止后续流程
                    self._ensure_not_cancelled(session_id)
                    if answer:
                        break
                if answer:
                    break

            if not answer and last_response:
                answer = self._resolve_final_answer(
                    last_response.content, last_response.reasoning
                )
            if not answer:
                answer = t("error.max_rounds_no_final_answer")

            # 仅在正常结束时排队长期记忆总结任务
            await self._enqueue_memory_summary(
                prepared, last_request_messages, answer
            )

            usage_payload = (
                last_usage if last_usage and last_usage.get("total_tokens", 0) > 0 else None
            )
            response = WunderResponse(
                session_id=session_id,
                answer=answer,
                usage=usage_payload,
                uid=a2ui_uid,
                a2ui=a2ui_messages,
            )
            emitter.emit("final", {"answer": answer, "usage": usage_payload or {}})
            monitor.mark_finished(session_id)
            return response
        except WunderError as exc:
            emitter.emit("error", exc.to_dict())
            if exc.code == ErrorCodes.CANCELLED:
                monitor.mark_cancelled(session_id)
            elif exc.code != ErrorCodes.USER_BUSY:
                monitor.mark_error(session_id, exc.message)
            raise
        except Exception as exc:  # noqa: BLE001
            payload = {
                "code": ErrorCodes.INTERNAL_ERROR,
                "message": t("error.internal_error"),
                "detail": {"error": str(exc)},
            }
            emitter.emit("error", payload)
            monitor.mark_error(session_id, str(exc))
            raise WunderError(
                ErrorCodes.INTERNAL_ERROR,
                t("error.internal_error"),
                {"error": str(exc)},
            ) from exc
        finally:
            if heartbeat_task:
                heartbeat_task.cancel()
                with suppress(asyncio.CancelledError):
                    await heartbeat_task
            if acquired:
                await limiter.release(session_id=session_id)
            emitter.finish()

    def _resolve_allowed_tool_names(
        self,
        ctx: RequestContext,
        tool_names: Optional[List[str]],
        user_tool_bindings: Optional[UserToolBindings],
    ) -> Set[str]:
        """解析可用工具名称集合，支持空值代表全量工具。"""
        if tool_names is None:
            allowed = self._collect_available_tool_names(ctx, user_tool_bindings)
        else:
            allowed = self._prompt_composer.resolve_allowed_tool_names(
                ctx, tool_names, user_tool_bindings
            )
        return self._apply_a2ui_tool_policy(allowed, tool_names)

    @staticmethod
    def _apply_a2ui_tool_policy(
        allowed_tool_names: Set[str],
        tool_names: Optional[List[str]],
    ) -> Set[str]:
        """应用 a2ui 与最终回复的互斥策略，避免默认流程被强制切换。"""
        normalized = set(allowed_tool_names)
        # a2ui 仅在显式勾选时开放，避免默认请求被 UI 输出打断。
        if tool_names is None:
            normalized.discard("a2ui")
        if "a2ui" in normalized:
            # 启用 a2ui 时移除最终回复工具，避免模型双重收尾。
            normalized.discard("最终回复")
            normalized.discard("final_response")
        return normalized

    @staticmethod
    def _collect_available_tool_names(
        ctx: RequestContext,
        user_tool_bindings: Optional[UserToolBindings],
    ) -> Set[str]:
        """收集可用工具名称集合，便于后续注入系统提示词。"""
        # 统一收集工具名称，减少重复计算开销
        return collect_available_tool_names(
            ctx.config, ctx.skills.list_specs(), user_tool_bindings
        )


    async def _resolve_session_prompt(
        self,
        ctx: RequestContext,
        user_id: str,
        session_id: str,
        prompt: str,
        tool_names: Optional[List[str]],
        overrides: Optional[Dict[str, Any]],
        language: Optional[str] = None,
    ) -> str:
        """会话级系统提示词优先复用历史记录。"""
        stored = await ctx.workspace_manager.load_session_system_prompt(
            user_id, session_id, language=language
        )
        if stored and tool_names is None and not overrides:
            return stored
        if not stored:
            await ctx.workspace_manager.save_session_system_prompt(
                user_id, session_id, prompt, language=language
            )
        return prompt

    def _load_memory_summary_prompt(self) -> str:
        """读取长期记忆总结指令模板。"""
        try:
            return read_prompt_template(_MEMORY_SUMMARY_PROMPT_PATH).strip()
        except OSError:
            return t("memory.summary_prompt_fallback")

    @staticmethod
    def _format_memory_task(
        task: Optional[MemorySummaryTask], now_ts: Optional[float] = None
    ) -> Dict[str, Any]:
        """格式化长期记忆任务信息，便于管理端展示。"""
        if task is None:
            return {}
        now_value = float(now_ts) if now_ts is not None else time.time()
        queued_ts = float(task.queued_time or 0)
        start_ts = float(task.start_time or 0)
        end_ts = float(task.end_time or 0)
        status = str(task.status or "").strip()
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
        if not status:
            if end_ts > 0:
                status = t("memory.status.done")
            elif start_ts > 0:
                status = t("memory.status.running")
            else:
                status = t("memory.status.queued")
        else:
            normalized = status_map.get(status.lower(), status_map.get(status))
            if normalized == "queued":
                status = t("memory.status.queued")
            elif normalized == "running":
                status = t("memory.status.running")
            elif normalized == "done":
                status = t("memory.status.done")
            elif normalized == "failed":
                status = t("memory.status.failed")

        def _format_ts(value: float) -> str:
            if value <= 0:
                return ""
            return datetime.utcfromtimestamp(value).isoformat() + "Z"

        elapsed_s = 0.0
        if end_ts > 0:
            base_ts = start_ts or queued_ts
            elapsed_s = max(0.0, end_ts - base_ts) if base_ts > 0 else 0.0
        elif start_ts > 0:
            elapsed_s = max(0.0, now_value - start_ts)
        elif queued_ts > 0:
            elapsed_s = max(0.0, now_value - queued_ts)

        return {
            "task_id": task.task_id,
            "user_id": task.user_id,
            "session_id": task.session_id,
            "status": status,
            "queued_time": _format_ts(queued_ts),
            "queued_time_ts": queued_ts,
            "started_time": _format_ts(start_ts),
            "started_time_ts": start_ts,
            "finished_time": _format_ts(end_ts),
            "finished_time_ts": end_ts,
            "elapsed_s": elapsed_s,
        }

    async def _append_memory_prompt(self, user_id: str, prompt: str) -> str:
        """按用户长期记忆开关追加系统提示词内容。"""
        if not prompt:
            return prompt
        try:
            enabled = await self._memory_store.is_enabled(user_id)
        except Exception:
            return prompt
        if not enabled:
            return prompt
        try:
            records = await self._memory_store.list_records(
                user_id, order_desc=False
            )
        except Exception:
            return prompt
        memory_block = self._memory_store.build_prompt_block(records)
        if not memory_block:
            return prompt
        return prompt.rstrip() + "\n\n" + memory_block

    async def _ensure_memory_worker(self) -> None:
        """启动长期记忆队列处理器，确保只启动一个消费者。"""
        if self._memory_queue_guard is None:
            self._memory_queue_guard = asyncio.Lock()
        async with self._memory_queue_guard:
            if self._memory_queue is None:
                self._memory_queue = asyncio.PriorityQueue()
            if self._memory_queue_task is None or self._memory_queue_task.done():
                self._memory_queue_task = asyncio.create_task(
                    self._memory_worker_loop()
                )

    async def _enqueue_memory_summary(
        self,
        prepared: PreparedRequest,
        request_messages: Optional[List[Dict[str, Any]]] = None,
        final_answer: Optional[str] = None,
    ) -> None:
        """将长期记忆总结任务加入队列。"""
        if not await self._memory_store.is_enabled(prepared.user_id):
            return
        await self._ensure_memory_worker()
        if not self._memory_queue:
            return
        self._memory_queue_seq += 1
        task = MemorySummaryTask(
            task_id=uuid.uuid4().hex,
            user_id=prepared.user_id,
            session_id=prepared.session_id,
            queued_time=time.time(),
            config_overrides=prepared.config_overrides,
            model_name=prepared.model_name,
            attachments=prepared.attachments,
            request_messages=request_messages,
            final_answer=str(final_answer or "").strip(),
            language=prepared.language,
            status=t("memory.status.queued"),
        )
        await self._memory_queue.put(
            (task.queued_time, self._memory_queue_seq, task)
        )

    async def _memory_worker_loop(self) -> None:
        """按线程完成时间顺序消费长期记忆总结任务。"""
        if not self._memory_queue:
            return
        while True:
            try:
                _, _, task = await self._memory_queue.get()
            except asyncio.CancelledError:
                self._memory_active_task = None
                return
            stored = False
            token = set_language(task.language)
            try:
                task.start_time = time.time()
                task.status = t("memory.status.running")
                self._memory_active_task = task
                stored = await self._run_memory_summary_task(task)
                task.status = t("memory.status.done")
            except Exception as exc:  # noqa: BLE001
                task.status = t("memory.status.failed")
                task.error = str(exc)
                self._log_memory_summary_error(task, exc)
            finally:
                task.end_time = time.time()
                self._memory_active_task = None
                if stored:
                    base_ts = task.start_time or task.queued_time
                    elapsed_s = max(0.0, task.end_time - base_ts) if base_ts > 0 else 0.0
                    try:
                        await self._memory_store.upsert_task_log(
                            task.user_id,
                            task.session_id,
                            task.task_id,
                            task.status,
                            task.queued_time,
                            task.start_time,
                            task.end_time,
                            elapsed_s,
                            task.request_payload or {},
                            task.summary_result,
                            task.error,
                            now_ts=task.end_time,
                        )
                    except Exception:
                        pass
                self._memory_task_history.appendleft(task)
                self._memory_queue.task_done()
                reset_language(token)

    def _log_memory_summary_error(
        self, task: MemorySummaryTask, error: Exception
    ) -> None:
        """写入长期记忆总结失败日志，避免影响主流程。"""
        try:
            created_at = datetime.utcnow().isoformat() + "Z"
            self._storage.write_system_log(
                created_at=created_at,
                level="ERROR",
                logger="memory_summary",
                message=str(error),
                payload={
                    "user_id": task.user_id,
                    "session_id": task.session_id,
                },
            )
        except Exception:
            return

    def _build_memory_summary_context(
        self, task: MemorySummaryTask
    ) -> Tuple[RequestContext, str, LLMConfig]:
        """构建长期记忆总结专用上下文与模型配置。"""
        config = self._resolve_config(task.config_overrides)
        llm_name, llm_config = resolve_llm_config(config, task.model_name)
        summary_llm_config = llm_config.model_copy(deep=True)
        if (
            summary_llm_config.max_output is None
            or summary_llm_config.max_output > COMPACTION_SUMMARY_MAX_OUTPUT
        ):
            summary_llm_config.max_output = COMPACTION_SUMMARY_MAX_OUTPUT
        summary_llm_config.max_rounds = 1

        skills = (
            self._skills
            if config is self._config
            else self._load_skill_registry(config)
        )
        tools = build_tool_registry(config, llm_config=llm_config)
        ctx = RequestContext(
            config=config,
            llm_config=llm_config,
            llm_name=llm_name,
            tools=tools,
            skills=skills,
            mcp_client=MCPClient(config),
            workspace_manager=self._workspace_manager,
        )
        return ctx, llm_name, summary_llm_config

    async def _build_memory_summary_messages(
        self,
        ctx: RequestContext,
        task: MemorySummaryTask,
        summary_llm_config: LLMConfig,
    ) -> List[Dict[str, Any]]:
        """构建长期记忆总结请求消息，复用现有对话上下文。"""
        summary_instruction = self._load_memory_summary_prompt()
        # 总结任务使用独立系统提示词，用户内容由历史消息融合而成
        source_messages = (
            copy.deepcopy(task.request_messages)
            if task.request_messages
            else await self._history_manager.load_history_messages(
                ctx, task.user_id, task.session_id
            )
        )
        user_content = self._build_memory_summary_user_content(
            source_messages, task.final_answer
        )
        messages = [
            {"role": "system", "content": summary_instruction},
            {"role": "user", "content": user_content},
        ]

        # 控制单条消息长度并裁剪上下文，避免总结请求溢出
        messages = self._prepare_summary_messages(
            messages, COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS
        )
        limit = self._history_manager.get_auto_compact_limit(summary_llm_config)
        if limit:
            system_tokens = estimate_message_tokens(messages[0])
            if estimate_messages_tokens(messages) > limit and len(messages) > 1:
                remaining = max(1, limit - system_tokens)
                trimmed_tail = trim_messages_to_budget(messages[1:], remaining)
                messages = [messages[0]] + trimmed_tail
        return messages

    def _build_memory_summary_user_content(
        self, messages: List[Dict[str, Any]], final_answer: str
    ) -> str:
        """将历史消息融合为单条总结输入内容，避免工具输出干扰。"""
        separator = t("memory.summary.role.separator")
        user_label = t("memory.summary.role.user")
        assistant_label = t("memory.summary.role.assistant")
        lines: List[str] = []
        last_assistant = ""
        for message in messages:
            if not isinstance(message, dict):
                continue
            role = str(message.get("role") or "").strip()
            if not role or role == "system":
                continue
            if self._is_observation_message(message):
                continue
            content = self._extract_memory_summary_text(message.get("content"))
            if not content:
                continue
            label = user_label if role == "user" else assistant_label if role == "assistant" else role
            lines.append(f"{label}{separator}{content}")
            if role == "assistant":
                last_assistant = content

        final_text = str(final_answer or "").strip()
        if final_text and final_text != last_assistant:
            lines.append(f"{assistant_label}{separator}{final_text}")
        return "\n".join(lines).strip()

    def _extract_memory_summary_text(self, content: Any) -> str:
        """提取消息中的可读文本，忽略多模态图片本体。"""
        if content is None:
            return ""
        if isinstance(content, str):
            return self._strip_tool_calls(content).strip()
        if isinstance(content, list):
            parts: List[str] = []
            for part in content:
                if not isinstance(part, dict):
                    continue
                part_type = str(part.get("type") or "").strip().lower()
                if part_type == "text":
                    text = str(part.get("text") or "")
                    cleaned = self._strip_tool_calls(text).strip()
                    if cleaned:
                        parts.append(cleaned)
                elif part_type == "image_url":
                    parts.append(t("memory.summary.image_placeholder"))
            return "\n".join([item for item in parts if item]).strip()
        return self._strip_tool_calls(str(content)).strip()

    @staticmethod
    def _build_memory_summary_payload(
        task: MemorySummaryTask,
        llm_name: str,
        messages: List[Dict[str, Any]],
    ) -> Dict[str, Any]:
        """组装长期记忆总结请求载荷，用于调试展示。"""
        payload: Dict[str, Any] = {
            "user_id": task.user_id,
            "session_id": task.session_id,
            "model_name": llm_name,
            "tool_names": [],
            "messages": messages,
        }
        if task.config_overrides:
            payload["config_overrides"] = task.config_overrides
        return payload

    async def _run_memory_summary_task(self, task: MemorySummaryTask) -> bool:
        """执行单条长期记忆总结任务。"""
        token = set_language(task.language)
        try:
            if not await self._memory_store.is_enabled(task.user_id):
                return False
            ctx, llm_name, summary_llm_config = self._build_memory_summary_context(task)
            messages = await self._build_memory_summary_messages(
                ctx, task, summary_llm_config
            )
            # 预先保存可展示的总结请求载荷，便于管理端调试
            payload_messages = self._sanitize_messages_for_log(
                copy.deepcopy(messages), task.attachments
            )
            task.request_payload = self._build_memory_summary_payload(
                task, llm_name, payload_messages
            )

            emitter = _EventEmitter(task.session_id, task.user_id)
            response, _ = await self._call_llm(
                ctx,
                messages,
                emitter,
                task.session_id,
                stream=False,
                round_index=1,
                emit_events=False,
                attachments=task.attachments,
                llm_config_override=summary_llm_config,
            )
            summary_text = self._resolve_final_answer(
                response.content, response.reasoning
            )
            normalized_summary = self._memory_store.normalize_summary(summary_text)
            task.summary_result = normalized_summary
            stored = await self._memory_store.upsert_record(
                task.user_id,
                task.session_id,
                normalized_summary,
                now_ts=task.queued_time,
            )
            return stored
        finally:
            reset_language(token)

    @staticmethod
    def _is_observation_message(message: Dict[str, Any]) -> bool:
        """判断是否为工具观察消息，便于后续裁剪与统计。"""
        if not isinstance(message, dict):
            return False
        content = message.get("content")
        return (
            message.get("role") == "user"
            and isinstance(content, str)
            and content.startswith(OBSERVATION_PREFIX)
        )

    def _prepare_summary_messages(
        self, messages: List[Dict[str, Any]], max_message_tokens: int
    ) -> List[Dict[str, Any]]:
        """准备摘要输入消息：移除推理内容并限制单条消息长度。"""
        prepared: List[Dict[str, Any]] = []
        for message in messages:
            new_message = dict(message)
            # 摘要时不需要传入思考链路，避免占用上下文
            new_message.pop("reasoning_content", None)
            new_message.pop("reasoning", None)
            content = new_message.get("content")
            if isinstance(content, str):
                if approx_token_count(content) > max_message_tokens:
                    new_message["content"] = trim_text_to_tokens(
                        content, max_message_tokens
                    )
            prepared.append(new_message)
        return prepared

    @staticmethod
    def _locate_tail_block_start(messages: List[Dict[str, Any]]) -> int:
        """定位需要整体保留的尾部消息块起点。"""
        if not messages:
            return 0
        last_user_index = None
        for index in range(len(messages) - 1, -1, -1):
            message = messages[index]
            if isinstance(message, dict) and message.get("role") == "user":
                last_user_index = index
                break
        if last_user_index is None:
            return max(0, len(messages) - 1)
        assistant_index = None
        for index in range(last_user_index - 1, -1, -1):
            message = messages[index]
            if isinstance(message, dict) and message.get("role") == "assistant":
                assistant_index = index
                break
        if assistant_index is None:
            return last_user_index
        for index in range(assistant_index - 1, -1, -1):
            message = messages[index]
            if isinstance(message, dict) and message.get("role") == "user":
                return index
        return assistant_index

    def _trim_messages_keep_tail(
        self, messages: List[Dict[str, Any]], max_tokens: int
    ) -> List[Dict[str, Any]]:
        """按预算保留尾部消息块，避免截断最近一轮交互。"""
        if not messages:
            return []
        if max_tokens <= 0:
            return [messages[-1]]
        tail_start = self._locate_tail_block_start(messages)
        if tail_start < 0 or tail_start >= len(messages):
            tail_start = max(0, len(messages) - 1)
        tail_messages = messages[tail_start:]
        tail_tokens = estimate_messages_tokens(tail_messages)
        if tail_tokens >= max_tokens:
            return tail_messages
        remaining = max_tokens - tail_tokens
        head_messages = trim_messages_to_budget(messages[:tail_start], remaining)
        return head_messages + tail_messages

    def _shrink_messages_to_limit(
        self, messages: List[Dict[str, Any]], limit: int
    ) -> List[Dict[str, Any]]:
        """在极端超限时裁剪工具结果，尽量让上下文落回安全范围。"""
        total_tokens = estimate_messages_tokens(messages)
        if total_tokens <= limit:
            return messages
        overflow = total_tokens - limit
        trimmed = [dict(message) for message in messages]
        for index, message in enumerate(trimmed):
            if overflow <= 0:
                break
            if not self._is_observation_message(message):
                continue
            content = str(message.get("content") or "")
            current_tokens = approx_token_count(content)
            if current_tokens <= COMPACTION_MIN_OBSERVATION_TOKENS:
                continue
            target_tokens = max(
                COMPACTION_MIN_OBSERVATION_TOKENS, current_tokens - overflow
            )
            new_content = trim_text_to_tokens(content, target_tokens)
            if new_content == content:
                continue
            trimmed[index]["content"] = new_content
            overflow = max(0, estimate_messages_tokens(trimmed) - limit)
        return trimmed

    async def _maybe_compact_messages(
        self,
        ctx: RequestContext,
        user_id: str,
        session_id: str,
        messages: List[Dict[str, Any]],
        emitter: _EventEmitter,
    ) -> Tuple[List[Dict[str, Any]], str]:
        """必要时压缩上下文，保留近期对话并写入摘要。"""
        limit = self._history_manager.get_auto_compact_limit(ctx.llm_config)
        if not limit:
            return messages, ""
        history_usage = 0
        try:
            history_usage = await ctx.workspace_manager.load_session_token_usage(
                user_id, session_id
            )
        except Exception:
            history_usage = 0
        max_context = ctx.llm_config.max_context
        ratio = ctx.llm_config.history_compaction_ratio
        if not isinstance(ratio, (int, float)):
            ratio = COMPACTION_HISTORY_RATIO
        ratio = float(ratio)
        if ratio <= 0:
            ratio = COMPACTION_HISTORY_RATIO
        elif ratio > 1:
            # 支持 0-1 比例，也兼容 1-100 的百分比输入
            ratio = ratio / 100 if ratio <= 100 else 1.0
        history_threshold = (
            int(max_context * ratio)
            if isinstance(max_context, int) and max_context > 0
            else None
        )
        should_compact_by_history = (
            bool(history_threshold) and history_usage >= int(history_threshold)
        )
        total_tokens = estimate_messages_tokens(messages)
        if not should_compact_by_history and total_tokens <= limit:
            return messages, ""

        reset_mode = ""
        if should_compact_by_history:
            reset_mode = str(ctx.llm_config.history_compaction_reset or "").strip().lower()
            if reset_mode not in {"zero", "current", "keep"}:
                reset_mode = "zero"

        compaction_payload = {
            "reason": "history" if should_compact_by_history else "overflow",
            "history_usage": int(history_usage),
            "history_threshold": int(history_threshold) if history_threshold else None,
            "history_ratio": ratio,
            "max_context": int(max_context) if isinstance(max_context, int) else None,
            "limit": int(limit),
            "total_tokens": int(total_tokens),
            "reset_mode": reset_mode or None,
        }

        summary_text = (
            t("compaction.reason.history_threshold")
            if should_compact_by_history
            else t("compaction.reason.context_too_long")
        )
        emitter.emit("progress", {"stage": "compacting", "summary": summary_text})

        async def _apply_history_reset(target_tokens: Optional[int] = None) -> bool:
            """根据策略重置会话 token 统计，避免重复触发压缩。"""
            if not should_compact_by_history:
                return False
            if reset_mode == "keep":
                return False
            if reset_mode == "current":
                current_tokens = (
                    int(target_tokens)
                    if isinstance(target_tokens, int)
                    else int(total_tokens)
                )
                await ctx.workspace_manager.save_session_token_usage(
                    user_id, session_id, max(0, current_tokens)
                )
                return True
            await ctx.workspace_manager.save_session_token_usage(user_id, session_id, 0)
            return True

        system_message = (
            messages[0] if messages and messages[0].get("role") == "system" else None
        )
        other_messages = messages[1:] if system_message else list(messages)
        # 过滤掉系统消息，避免将摘要内容再次摘要
        candidate_messages = [
            message for message in other_messages if message.get("role") != "system"
        ]
        if not candidate_messages:
            reset_applied = await _apply_history_reset()
            compaction_payload.update(
                {"status": "skipped", "skip_reason": "no_candidates"}
            )
            emitter.emit("compaction", compaction_payload)
            return messages, reset_mode if reset_applied else ""

        force_history_compaction = should_compact_by_history and len(candidate_messages) > 1
        keep_recent_tokens = min(COMPACTION_KEEP_RECENT_TOKENS, max(1, limit // 2))
        recent_messages = self._trim_messages_keep_tail(
            candidate_messages, keep_recent_tokens
        )
        recent_messages_tokens = estimate_messages_tokens(recent_messages)
        if (
            len(recent_messages) >= len(candidate_messages)
            and recent_messages_tokens <= keep_recent_tokens
        ):
            if not force_history_compaction:
                reset_applied = await _apply_history_reset()
                compaction_payload.update(
                    {"status": "skipped", "skip_reason": "keep_recent"}
                )
                emitter.emit("compaction", compaction_payload)
                return messages, reset_mode if reset_applied else ""
            # 历史阈值触发时强制压缩，至少保留最后一条消息
            recent_messages = candidate_messages[-1:]
            keep_recent_tokens = max(1, estimate_messages_tokens(recent_messages))
            compaction_payload.update(
                {"forced": True, "force_reason": "keep_recent"}
            )

        older_count = len(candidate_messages) - len(recent_messages)
        compaction_prompt = self._history_manager.load_compaction_prompt()
        summary_input = copy.deepcopy(messages)
        last_user_index = None
        for index in range(len(summary_input) - 1, -1, -1):
            message = summary_input[index]
            if isinstance(message, dict) and message.get("role") == "user":
                last_user_index = index
                break
        if last_user_index is None:
            summary_input.append({"role": "user", "content": compaction_prompt})
        else:
            replaced = dict(summary_input[last_user_index])
            replaced["content"] = compaction_prompt
            replaced.pop("reasoning_content", None)
            replaced.pop("reasoning", None)
            summary_input[last_user_index] = replaced

        if summary_input and summary_input[0].get("role") == "system":
            system_snapshot = summary_input[0]
            rest_messages = summary_input[1:]
            remaining = max(1, limit - estimate_messages_tokens([system_snapshot]))
            rest_messages = self._trim_messages_keep_tail(rest_messages, remaining)
            summary_input = [system_snapshot, *rest_messages]
        else:
            summary_input = self._trim_messages_keep_tail(summary_input, limit)

        summary_max_message_tokens = min(
            COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS, max(1, limit)
        )
        summary_input = self._prepare_summary_messages(
            summary_input, summary_max_message_tokens
        )

        compacted_until_ts: Optional[float] = None
        compacted_until: Optional[str] = None
        try:
            history = await ctx.workspace_manager.load_history(
                user_id, session_id, ctx.config.workspace.max_history_items
            )
            history_items, _ = self._history_manager.build_compaction_candidates(history)
            if 0 < older_count <= len(history_items):
                boundary_item = history_items[older_count - 1]
                compacted_until_ts = self._history_manager.get_item_timestamp(boundary_item)
                if isinstance(boundary_item.get("timestamp"), str):
                    compacted_until = boundary_item.get("timestamp")
        except Exception:
            compacted_until_ts = None
            compacted_until = None

        summary_max_output = COMPACTION_SUMMARY_MAX_OUTPUT
        if isinstance(ctx.llm_config.max_output, int) and ctx.llm_config.max_output > 0:
            summary_max_output = min(summary_max_output, ctx.llm_config.max_output)
        summary_config = ctx.llm_config.model_copy()
        summary_config.max_output = summary_max_output
        log_payload = self._build_llm_payload(
            self._sanitize_messages_for_log(summary_input, None),
            summary_config,
            stream=False,
        )
        # 深拷贝避免后续修改影响调试请求日志
        log_payload = copy.deepcopy(log_payload)
        emitter.emit(
            "llm_request",
            {
                "provider": summary_config.provider,
                "model": summary_config.model,
                "base_url": summary_config.base_url,
                "payload": log_payload,
                "purpose": "compaction_summary",
            },
        )
        summary_fallback = False
        summary_text = ""
        try:
            summary_response, _ = await self._call_llm(
                ctx,
                summary_input,
                emitter,
                session_id,
                stream=False,
                round_index=0,
                emit_events=False,
                llm_config_override=summary_config,
            )
            summary_text = self._extract_llm_content(summary_response)
        except WunderError:
            summary_fallback = True
            summary_text = t("compaction.summary_fallback")
        summary_text = self._history_manager.format_compaction_summary(summary_text)
        # 记录压缩总结回复，便于调试面板请求日志关联查看
        emitter.emit(
            "llm_response",
            {
                "content": summary_text,
                "reasoning": "",
                "purpose": "compaction_summary",
            },
        )
        meta: Dict[str, Any] = {"type": COMPACTION_META_TYPE}
        if compacted_until_ts is not None:
            meta["compacted_until_ts"] = compacted_until_ts
        if compacted_until:
            meta["compacted_until"] = compacted_until
        await self._append_chat(
            user_id,
            session_id,
            "system",
            summary_text,
            meta=meta,
        )

        rebuilt: List[Dict[str, Any]] = []
        if system_message:
            rebuilt.append(system_message)
        rebuilt.append({"role": "system", "content": summary_text})
        artifact_content = await self._history_manager.load_artifact_index_message(
            ctx, user_id, session_id
        )
        if artifact_content:
            rebuilt.append({"role": "system", "content": artifact_content})
        rebuilt.extend(recent_messages)
        rebuilt = self._shrink_messages_to_limit(rebuilt, limit)
        rebuilt_tokens = estimate_messages_tokens(rebuilt)
        reset_applied = await _apply_history_reset(rebuilt_tokens)
        compaction_payload.update(
            {
                "status": "fallback" if summary_fallback else "done",
                "summary_fallback": summary_fallback,
                "summary_error": "summary_failed" if summary_fallback else None,
                "summary_tokens": approx_token_count(summary_text),
                "total_tokens_after": rebuilt_tokens,
            }
        )
        emitter.emit("compaction", compaction_payload)
        return rebuilt, reset_mode if reset_applied else ""

    def _build_user_message(
        self,
        question: str,
        attachments: Optional[Sequence[WunderAttachment]],
    ) -> Dict[str, Any]:
        """构建用户消息，支持追加文件解析内容与多模态图片。"""
        if not attachments:
            return {"role": "user", "content": question}
        text_parts = [str(question or "")]
        attachment_label = t("attachment.label")
        attachment_separator = t("attachment.label.separator")
        attachment_default_name = t("attachment.default_name")
        image_parts: List[Dict[str, Any]] = []
        for attachment in attachments:
            if not attachment:
                continue
            content = str(attachment.content or "")
            if not content.strip():
                continue
            kind = str(attachment.type or "").strip().lower()
            if kind == "image":
                # 多模态图片采用 OpenAI 兼容格式，使用 data URL 传递
                image_parts.append({"type": "image_url", "image_url": {"url": content}})
                continue
            name = str(attachment.name or attachment_default_name)
            text_parts.append(f"\n\n[{attachment_label}{attachment_separator}{name}]\n{content}")
        text_content = "".join(text_parts)
        if image_parts:
            text_payload = text_content.strip() or t("attachment.image_prompt")
            return {"role": "user", "content": [{"type": "text", "text": text_payload}, *image_parts]}
        return {"role": "user", "content": text_content}

    def _sanitize_messages_for_log(
        self,
        messages: List[Dict[str, Any]],
        attachments: Optional[Sequence[WunderAttachment]],
    ) -> List[Dict[str, Any]]:
        """清理多模态图片的 base64，避免日志中出现超长 image_url。"""
        if not messages:
            return messages
        image_names = [
            str(item.name or "image")
            for item in (attachments or [])
            if item and str(item.type or "").lower() == "image"
        ]
        image_index = 0

        def _resolve_placeholder() -> str:
            nonlocal image_index
            image_index += 1
            name = (
                image_names[image_index - 1]
                if image_index - 1 < len(image_names)
                else f"image-{image_index}"
            )
            return f"attachment://{name}"

        pattern = re.compile(r"data:image/[a-zA-Z0-9+.-]+;base64,[A-Za-z0-9+/=\r\n]+")

        def _replace_data_url(text: str) -> str:
            if not isinstance(text, str) or "data:image/" not in text:
                return text
            return pattern.sub(lambda _: _resolve_placeholder(), text)

        sanitized: List[Dict[str, Any]] = []
        changed = False
        for message in messages:
            if not isinstance(message, dict):
                sanitized.append(message)
                continue
            content = message.get("content")
            if isinstance(content, str):
                replaced = _replace_data_url(content)
                if replaced != content:
                    new_message = dict(message)
                    new_message["content"] = replaced
                    sanitized.append(new_message)
                    changed = True
                else:
                    sanitized.append(message)
                continue
            if not isinstance(content, list):
                sanitized.append(message)
                continue
            new_content: List[Any] = []
            message_changed = False
            for part in content:
                new_part = part
                if isinstance(part, dict):
                    part_type = str(part.get("type", "")).lower()
                    if part_type == "image_url" or "image_url" in part:
                        image_url = part.get("image_url")
                        url = None
                        if isinstance(image_url, dict):
                            url = image_url.get("url")
                        elif isinstance(image_url, str):
                            url = image_url
                        if isinstance(url, str) and url.startswith("data:image/"):
                            placeholder = _resolve_placeholder()
                            new_image_url = dict(image_url) if isinstance(image_url, dict) else {}
                            new_image_url["url"] = placeholder
                            new_part = dict(part)
                            new_part["image_url"] = new_image_url
                            message_changed = True
                    elif part_type == "text" and isinstance(part.get("text"), str):
                        replaced_text = _replace_data_url(str(part.get("text")))
                        if replaced_text != part.get("text"):
                            new_part = dict(part)
                            new_part["text"] = replaced_text
                            message_changed = True
                new_content.append(new_part)
            if message_changed:
                new_message = dict(message)
                new_message["content"] = new_content
                sanitized.append(new_message)
                changed = True
            else:
                sanitized.append(message)
        return sanitized if changed else messages

    async def _call_llm(
        self,
        ctx: RequestContext,
        messages: List[Dict[str, Any]],
        emitter: _EventEmitter,
        session_id: str,
        stream: bool,
        round_index: int,
        *,
        emit_events: bool = True,
        attachments: Optional[Sequence[WunderAttachment]] = None,
        llm_config_override: Optional[LLMConfig] = None,
    ) -> Tuple[LLMResponse, Optional[Dict[str, int]]]:
        """调用 LLM 并返回标准化结果。"""
        # 调用模型前先检查是否已取消，避免浪费远端调用
        self._ensure_not_cancelled(session_id)
        llm_config = llm_config_override or ctx.llm_config
        payload = self._build_llm_payload(messages, llm_config, stream)
        if emit_events:
            log_payload = self._build_llm_payload(
                self._sanitize_messages_for_log(messages, attachments),
                llm_config,
                stream,
            )
            # 深拷贝请求体，避免后续消息列表被修改导致调试日志错乱
            log_payload = copy.deepcopy(log_payload)
            emitter.emit(
                "llm_request",
                {
                    "provider": llm_config.provider,
                    "model": llm_config.model,
                    "base_url": llm_config.base_url,
                    "payload": log_payload,
                },
            )
        response_usage: Optional[Dict[str, int]] = None
        try:
            client = build_llm_client(llm_config)
            if stream:
                max_attempts = max(1, int(llm_config.retry or 1))
                content = ""
                reasoning = ""
                response_usage = None
                for attempt in range(1, max_attempts + 1):
                    content_parts: List[str] = []
                    reasoning_parts: List[str] = []
                    stream_usage: Optional[Dict[str, int]] = None
                    emitted_chars = 0
                    emitted_chunks = 0
                    try:
                        async for chunk in client.stream_complete(messages):
                            # 流式接收过程中持续检查取消标记，尽快终止
                            self._ensure_not_cancelled(session_id)
                            if isinstance(chunk, LLMStreamChunk):
                                if chunk.usage:
                                    normalized = self._normalize_usage_payload(chunk.usage)
                                    if normalized:
                                        stream_usage = normalized
                                delta = chunk.content
                                reasoning_delta = chunk.reasoning
                                if not delta and not reasoning_delta:
                                    continue
                            else:
                                delta = self._extract_llm_content(chunk)
                                reasoning_delta = self._extract_llm_reasoning(chunk)
                                if not delta and not reasoning_delta:
                                    continue
                            if emit_events:
                                emitter.emit(
                                    "llm_output_delta",
                                    {
                                        "delta": delta,
                                        "reasoning_delta": reasoning_delta,
                                        "round": round_index,
                                    },
                                )
                            if delta:
                                emitted_chars += len(delta)
                                emitted_chunks += 1
                                content_parts.append(delta)
                            if reasoning_delta:
                                emitted_chars += len(reasoning_delta)
                                emitted_chunks += 1
                                reasoning_parts.append(reasoning_delta)
                        content = "".join(content_parts)
                        reasoning = "".join(reasoning_parts)
                        # 流式结束后再检查一次，避免刚好命中取消请求
                        self._ensure_not_cancelled(session_id)
                        response_usage = stream_usage
                        break
                    except LLMUnavailableError as exc:
                        retryable = self._is_stream_retryable(exc)
                        if not retryable or attempt >= max_attempts:
                            if emit_events and retryable:
                                emitter.emit(
                                    "llm_stream_retry",
                                    {
                                        "round": round_index,
                                        "attempt": attempt,
                                        "max_attempts": max_attempts,
                                        "reset_output": False,
                                        "emitted_chars": emitted_chars,
                                        "will_retry": False,
                                        "final": True,
                                        "reason": "max_attempts_reached",
                                        "detail": getattr(exc, "detail", {}),
                                    },
                                )
                            raise
                        delay = self._compute_stream_backoff(attempt)
                        if emit_events:
                            emitter.emit(
                                "llm_stream_retry",
                                {
                                    "round": round_index,
                                    "attempt": attempt,
                                    "max_attempts": max_attempts,
                                    "delay_s": round(delay, 2),
                                    "reset_output": emitted_chunks > 0,
                                    "emitted_chars": emitted_chars,
                                    "will_retry": True,
                                    "final": False,
                                    "detail": getattr(exc, "detail", {}),
                                },
                            )
                        # 流式异常重连前再次检查取消标记，避免无意义等待
                        self._ensure_not_cancelled(session_id)
                        await asyncio.sleep(delay)
                else:
                    raise LLMUnavailableError(t("error.llm_stream_retry_exhausted"))
            else:
                response = await client.complete(messages)
                # 非流式请求返回后检查取消状态，避免继续处理
                self._ensure_not_cancelled(session_id)
                content = self._extract_llm_content(response)
                reasoning = self._extract_llm_reasoning(response)
                response_usage = None
                if isinstance(response, LLMResponse):
                    response_usage = response.usage
                elif isinstance(response, dict):
                    response_usage = response.get("usage")
                response_usage = self._normalize_usage_payload(response_usage)
        except LLMUnavailableError as exc:
            raise WunderError(
                ErrorCodes.LLM_UNAVAILABLE,
                t("error.llm_unavailable", detail=str(exc)),
                getattr(exc, "detail", None),
            ) from exc
        except Exception as exc:  # noqa: BLE001
            raise WunderError(
                ErrorCodes.INTERNAL_ERROR,
                t("error.llm_call_failed", detail=str(exc)),
            ) from exc

        if emit_events:
            emitter.emit(
                "llm_output",
                {
                    "content": content,
                    "reasoning": reasoning,
                    "round": round_index,
                },
            )

        if response_usage and response_usage.get("total_tokens", 0) > 0:
            usage = response_usage
        else:
            input_tokens = estimate_messages_tokens(messages)
            output_tokens = approx_token_count(content) + approx_token_count(reasoning)
            usage = {
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens,
            }
        if emit_events:
            emitter.emit("token_usage", usage)
        return LLMResponse(content=content, reasoning=reasoning, usage=usage), usage

    @staticmethod
    def _is_stream_retryable(exc: LLMUnavailableError) -> bool:
        """判断流式异常是否可进行重连。"""
        detail = getattr(exc, "detail", None)
        if not isinstance(detail, dict):
            return False
        return bool(detail.get("stream_incomplete"))

    @staticmethod
    def _compute_stream_backoff(attempt: int) -> float:
        """计算流式重连退避时间，对齐 codex-main 的指数退避策略。"""
        base_delay_ms = 200
        exp = 2 ** max(attempt - 1, 0)
        delay_ms = base_delay_ms * exp
        jitter = random.uniform(0.9, 1.1)
        return (delay_ms * jitter) / 1000.0

    async def _await_tool_execution(
        self,
        task: asyncio.Task[Tuple[ToolResult, List[StreamEvent]]],
        session_id: str,
    ) -> Tuple[ToolResult, List[StreamEvent]]:
        """等待工具任务完成，支持在取消会话时中断阻塞调用。"""
        poll_interval = SESSION_LOCK_POLL_INTERVAL_S
        while True:
            done, _ = await asyncio.wait({task}, timeout=poll_interval)
            if done:
                try:
                    return await task
                except asyncio.CancelledError as exc:
                    raise WunderError(ErrorCodes.CANCELLED, t("error.session_cancelled")) from exc
            if monitor.is_cancelled(session_id):
                # 取消时主动打断工具执行，避免线程一直挂起
                task.cancel()
                with suppress(asyncio.CancelledError):
                    await task
                raise WunderError(ErrorCodes.CANCELLED, t("error.session_cancelled"))

    async def _execute_tool_call(
        self,
        ctx: RequestContext,
        workspace: WorkspaceContext,
        name: str,
        args: Any,
        emitter: _EventEmitter,
        user_tool_bindings: Optional[UserToolBindings],
    ) -> ToolResult:
        """执行单个工具调用并上报事件。"""
        safe_args = args if isinstance(args, dict) else {"raw": args}
        emitter.emit("tool_call", {"tool": name, "args": safe_args})
        loop = asyncio.get_running_loop()

        def _emit_tool_event(event: StreamEvent) -> None:
            # 工具事件可能来自线程池，统一通过线程安全方式投递到 SSE
            if loop.is_closed():
                return
            loop.call_soon_threadsafe(emitter.emit_event, event)

        try:
            tool_task = asyncio.create_task(
                self._tool_executor.execute(
                    name,
                    safe_args,
                    ctx,
                    workspace,
                    user_tool_bindings,
                    emit_event=_emit_tool_event,
                )
            )
            result, debug_events = await self._await_tool_execution(
                tool_task,
                workspace.session_id,
            )
        except WunderError:
            raise
        except Exception as exc:  # noqa: BLE001
            result = ToolResult(ok=False, data={}, error=str(exc))
            debug_events = []
        for event in debug_events:
            emitter.emit_event(event)

        payload = {"tool": name, "ok": result.ok, "data": result.data}
        if result.error:
            payload["error"] = result.error
        if self._is_sandbox_tool(ctx, name):
            payload["sandbox"] = True
        emitter.emit("tool_result", payload)
        return result

    async def _append_chat(
        self,
        user_id: str,
        session_id: str,
        role: str,
        content: Any,
        reasoning: str = "",
        meta: Optional[Dict[str, Any]] = None,
    ) -> None:
        """写入对话历史记录。"""
        payload: Dict[str, Any] = {
            "role": role,
            "content": "" if content is None else str(content),
            "session_id": session_id,
            "timestamp": datetime.utcnow().isoformat() + "Z",
        }
        if reasoning:
            payload["reasoning_content"] = reasoning
        if meta:
            payload["meta"] = meta
        await self._workspace_manager.append_chat(user_id, payload)

    async def _append_tool_log(
        self,
        user_id: str,
        session_id: str,
        tool_name: str,
        args: Any,
        result: ToolResult,
        *,
        sandbox: bool = False,
    ) -> None:
        """写入工具调用日志。"""
        payload: Dict[str, Any] = {
            "tool": tool_name,
            "session_id": session_id,
            "ok": result.ok,
            "error": result.error,
            "args": args if isinstance(args, dict) else {"raw": args},
            "data": result.data,
            "timestamp": datetime.utcnow().isoformat() + "Z",
        }
        if sandbox:
            payload["sandbox"] = True
        await self._workspace_manager.append_tool_log(user_id, payload)

    @staticmethod
    def _extract_file_paths(args: Any) -> List[str]:
        """从工具参数中提取文件路径列表。"""
        paths: List[str] = []
        if not isinstance(args, dict):
            return paths
        files = args.get("files")
        if isinstance(files, list):
            for item in files:
                if not isinstance(item, dict):
                    continue
                path = str(item.get("path", "")).strip()
                if path:
                    paths.append(path)
        path = str(args.get("path", "") or "").strip()
        if path:
            paths.append(path)
        # 保留顺序去重，避免重复记录
        seen: set[str] = set()
        ordered: List[str] = []
        for item in paths:
            if item in seen:
                continue
            seen.add(item)
            ordered.append(item)
        return ordered

    @staticmethod
    def _extract_command_lines(args: Any) -> List[str]:
        """从执行命令参数中提取单行命令列表。"""
        if not isinstance(args, dict):
            return []
        content = str(args.get("content", "") or "")
        commands: List[str] = []
        for line in content.splitlines():
            command = line.strip()
            if command:
                commands.append(command)
        return commands

    def _build_artifact_entries(
        self, tool_name: str, args: Any, result: ToolResult
    ) -> List[Dict[str, Any]]:
        """根据工具调用生成产物索引条目。"""
        entries: List[Dict[str, Any]] = []
        file_actions = {
            "读取文件": "read",
            "写入文件": "write",
            "替换文本": "replace",
            "编辑文件": "edit",
        }
        if tool_name in file_actions:
            action = file_actions[tool_name]
            paths = self._extract_file_paths(args)
            for path in paths:
                meta: Dict[str, Any] = {}
                if isinstance(result.data, dict):
                    if tool_name == "替换文本":
                        meta["replaced"] = result.data.get("replaced")
                    elif tool_name == "写入文件":
                        meta["bytes"] = result.data.get("bytes")
                    elif tool_name == "编辑文件":
                        meta["lines"] = result.data.get("lines")
                entries.append(
                    {
                        "kind": "file",
                        "action": action,
                        "name": path,
                        "meta": meta,
                    }
                )
            return entries

        if tool_name == "执行命令":
            commands = self._extract_command_lines(args)
            result_items = []
            if isinstance(result.data, dict):
                result_items = result.data.get("results") or []
            returncode_map: Dict[str, Any] = {}
            if isinstance(result_items, list):
                for item in result_items:
                    if not isinstance(item, dict):
                        continue
                    command = str(item.get("command", "")).strip()
                    if command:
                        returncode_map[command] = item.get("returncode")
            fallback_rc = None
            if isinstance(result.data, dict) and "returncode" in result.data:
                fallback_rc = result.data.get("returncode")
            for command in commands:
                returncode = returncode_map.get(command, fallback_rc)
                ok = result.ok if returncode is None else int(returncode) == 0
                entries.append(
                    {
                        "kind": "command",
                        "action": "execute",
                        "name": command,
                        "ok": ok,
                        "meta": {"returncode": returncode},
                    }
                )
            return entries

        if tool_name == "ptc":
            script_path = ""
            if isinstance(result.data, dict):
                script_path = str(result.data.get("path", "") or "").strip()
            if not script_path and isinstance(args, dict):
                script_path = str(args.get("filename", "") or "").strip()
            if script_path:
                returncode = None
                if isinstance(result.data, dict):
                    returncode = result.data.get("returncode")
                entries.append(
                    {
                        "kind": "script",
                        "action": "run",
                        "name": script_path,
                        "ok": result.ok if returncode is None else int(returncode) == 0,
                        "meta": {"returncode": returncode},
                    }
                )
            return entries

        return entries

    async def _append_artifact_logs(
        self,
        ctx: RequestContext,
        user_id: str,
        session_id: str,
        tool_name: str,
        args: Any,
        result: ToolResult,
    ) -> None:
        """写入产物索引日志，便于后续结构化摘要引用。"""
        entries = self._build_artifact_entries(tool_name, args, result)
        if not entries:
            return
        timestamp = datetime.utcnow().isoformat() + "Z"
        for entry in entries:
            payload = dict(entry)
            payload.setdefault("tool", tool_name)
            payload.setdefault("ok", result.ok)
            payload.setdefault("error", result.error)
            payload["session_id"] = session_id
            payload["timestamp"] = timestamp
            try:
                await ctx.workspace_manager.append_artifact_log(user_id, payload)
            except Exception:
                continue

    async def _append_skill_usage_logs(
        self,
        user_id: str,
        session_id: str,
        args: Any,
        workspace_root: Path,
        skill_path_index: Dict[str, List[str]],
    ) -> None:
        """记录读取 SKILL.md 的命中情况，用于工具统计。"""
        hits = self._user_tool_manager.collect_skill_hits_from_read_file(
            args, workspace_root, skill_path_index
        )
        if not hits:
            return
        for skill_name, path in hits.items():
            result = ToolResult(
                ok=True,
                data={"path": path, "source": "read_file"},
            )
            await self._append_tool_log(
                user_id,
                session_id,
                skill_name,
                {"path": path},
                result,
            )

    async def _log_final_tool_call(
        self, user_id: str, session_id: str, name: str, args: Any
    ) -> None:
        """记录最终回复工具的调用日志。"""
        content = self._resolve_final_answer_from_tool(args)
        data = {"content": content} if content else {}
        result = ToolResult(ok=True, data=data)
        await self._append_tool_log(user_id, session_id, name, args, result)

    async def _log_a2ui_tool_call(
        self,
        user_id: str,
        session_id: str,
        name: str,
        args: Any,
        uid: str,
        message_count: int,
        content: str,
    ) -> None:
        """记录 a2ui 工具调用日志，避免完整消息体写入导致膨胀。"""
        data: Dict[str, Any] = {
            "uid": uid,
            "message_count": int(message_count),
        }
        if content:
            data["content"] = content
        result = ToolResult(ok=True, data=data)
        await self._append_tool_log(user_id, session_id, name, args, result)

    async def _release_sandbox_if_needed(
        self, ctx: RequestContext, user_id: str, session_id: str, tool_name: str
    ) -> None:
        """在需要时释放沙盒资源，避免占用过久。"""
        if not self._is_sandbox_tool(ctx, tool_name):
            return
        if ctx.config.sandbox.idle_ttl_s and ctx.config.sandbox.idle_ttl_s > 0:
            return
        try:
            client = SandboxClient(ctx.config.sandbox)
            await client.release_sandbox(user_id, session_id)
        except Exception:
            return

    async def _keep_session_lock(
        self, limiter: RequestLimiter, session_id: str
    ) -> None:
        """后台心跳续租会话锁，避免长任务被误判过期。"""
        if not session_id:
            return
        while True:
            try:
                await asyncio.sleep(SESSION_LOCK_HEARTBEAT_S)
                await limiter.touch(session_id=session_id)
            except asyncio.CancelledError:
                return
            except Exception:
                # 心跳失败不影响主流程，等待下一次续租
                continue

    @staticmethod
    def _ensure_not_cancelled(session_id: str) -> None:
        """检测是否被取消，若取消则抛出异常。"""
        if monitor.is_cancelled(session_id):
            raise WunderError(ErrorCodes.CANCELLED, t("error.session_cancelled"))

    @staticmethod
    def _build_llm_payload(
        messages: List[Dict[str, Any]], llm_config: LLMConfig, stream: bool
    ) -> Dict[str, Any]:
        """构建模型请求体用于监控与调试展示。"""
        payload: Dict[str, Any] = {
            "model": llm_config.model,
            "messages": messages,
            "stream": stream,
        }
        if llm_config.temperature is not None:
            payload["temperature"] = llm_config.temperature
        if llm_config.max_output is not None and llm_config.max_output > 0:
            payload["max_tokens"] = llm_config.max_output
        stop_raw = llm_config.stop or []
        if isinstance(stop_raw, str):
            stop_raw = [stop_raw]
        stop_list = [str(item).strip() for item in stop_raw if str(item).strip()]
        if stop_list:
            payload["stop"] = stop_list
        return payload

    @staticmethod
    def _extract_llm_content(response: Any) -> str:
        """提取模型输出正文内容。"""
        if isinstance(response, LLMResponse):
            return str(response.content or "")
        if isinstance(response, LLMStreamChunk):
            return str(response.content or "")
        if isinstance(response, dict):
            return str(response.get("content", "") or "")
        return str(response or "")

    @staticmethod
    def _extract_llm_reasoning(response: Any) -> str:
        """提取模型思考内容。"""
        if isinstance(response, LLMResponse):
            return str(response.reasoning or "")
        if isinstance(response, LLMStreamChunk):
            return str(response.reasoning or "")
        if isinstance(response, dict):
            return str(
                response.get("reasoning_content")
                or response.get("reasoning")
                or ""
            )
        return ""

    @staticmethod
    def _normalize_usage_payload(raw: Any) -> Optional[Dict[str, int]]:
        """规整 usage 字段，保证 token 统计为可用的整数值。"""
        if not isinstance(raw, dict):
            return None

        def _to_int(value: Any) -> Optional[int]:
            if isinstance(value, bool):
                return None
            if isinstance(value, int):
                return value
            if isinstance(value, float):
                return int(value)
            if isinstance(value, str) and value.strip().isdigit():
                return int(value.strip())
            return None

        input_tokens = _to_int(raw.get("input_tokens"))
        if input_tokens is None:
            input_tokens = _to_int(raw.get("prompt_tokens"))
        output_tokens = _to_int(raw.get("output_tokens"))
        if output_tokens is None:
            output_tokens = _to_int(raw.get("completion_tokens"))
        total_tokens = _to_int(raw.get("total_tokens"))
        if total_tokens is None:
            if input_tokens is None and output_tokens is None:
                return None
            total_tokens = (input_tokens or 0) + (output_tokens or 0)
        return {
            "input_tokens": max(0, input_tokens or 0),
            "output_tokens": max(0, output_tokens or 0),
            "total_tokens": max(0, total_tokens),
        }

    def _resolve_final_answer(self, content: str, reasoning: str = "") -> str:
        """从模型输出中解析最终答复内容。"""
        cleaned = self._strip_tool_calls(str(content or ""))
        return cleaned.strip()

    @staticmethod
    def _resolve_final_answer_from_tool(args: Any) -> str:
        """从最终回复工具参数中提取答复。"""
        if isinstance(args, dict):
            value = args.get("content") or args.get("answer") or ""
            if isinstance(value, str):
                return value.strip()
            if value is None:
                return ""
            try:
                return json.dumps(value, ensure_ascii=False)
            except TypeError:
                return str(value)
        if isinstance(args, str):
            return args.strip()
        return ""

    def _resolve_a2ui_tool_payload(
        self, args: Any, user_id: str, session_id: str
    ) -> Tuple[str, List[Dict[str, Any]], str]:
        """解析 a2ui 工具参数，输出 uid、消息列表与文本说明。"""
        uid = ""
        content = ""
        raw_messages: Any = None
        if isinstance(args, dict):
            uid = str(args.get("uid") or "").strip()
            content = str(args.get("content") or "").strip()
            raw_messages = args.get("a2ui")
            if raw_messages is None:
                raw_messages = args.get("messages")
        else:
            raw_messages = args

        if not uid:
            # 兜底使用会话 ID，保证 UI Surface 有稳定标识。
            uid = str(session_id or user_id or "").strip()

        if isinstance(raw_messages, str):
            try:
                raw_messages = json.loads(raw_messages)
            except json.JSONDecodeError:
                raw_messages = []
        if isinstance(raw_messages, dict):
            raw_messages = [raw_messages]
        if not isinstance(raw_messages, list):
            raw_messages = []

        messages: List[Dict[str, Any]] = []
        for item in raw_messages:
            if not isinstance(item, dict):
                continue
            normalized = dict(item)
            for key in ("beginRendering", "surfaceUpdate", "dataModelUpdate", "deleteSurface"):
                payload = normalized.get(key)
                if isinstance(payload, dict):
                    if uid and not payload.get("surfaceId"):
                        payload = dict(payload)
                        payload["surfaceId"] = uid
                        normalized[key] = payload
                    break
            messages.append(normalized)

        return uid, messages, content

    @staticmethod
    def _strip_tool_calls(content: str) -> str:
        """移除模型输出中的工具调用块。"""
        if not content:
            return ""
        stripped = TOOL_CALL_PATTERN.sub("", content)
        stripped = TOOL_CALL_OPEN_PATTERN.sub("", stripped)
        stripped = TOOL_CALL_CLOSE_PATTERN.sub("", stripped)
        return stripped.strip()

    @staticmethod
    def _build_tool_observation_payload(tool_name: str, result: ToolResult) -> Dict[str, Any]:
        """构建工具观察载荷。"""
        payload: Dict[str, Any] = {
            "tool": tool_name,
            "ok": result.ok,
            "data": result.data,
            "timestamp": datetime.utcnow().isoformat() + "Z",
        }
        if result.error:
            payload["error"] = result.error
        return payload

    @staticmethod
    def _build_tool_observation(tool_name: str, result: ToolResult) -> str:
        """生成写入上下文的工具观察内容。"""
        payload = WunderOrchestrator._build_tool_observation_payload(tool_name, result)
        return json.dumps(payload, ensure_ascii=False)

    @staticmethod
    def _format_tool_result(tool_name: str, result: ToolResult) -> str:
        """统一格式化工具调用结果，用于兜底回传。"""
        payload = WunderOrchestrator._build_tool_observation_payload(tool_name, result)
        return json.dumps(payload, ensure_ascii=False)

    @staticmethod
    def _format_sse(event: StreamEvent) -> str:
        """将事件对象序列化为 SSE 文本。"""
        if hasattr(event, "model_dump"):
            payload = event.model_dump(exclude={"event_id"})
        else:
            payload = dict(event)
        data = json.dumps(payload, ensure_ascii=False)
        parts: List[str] = []
        event_id = getattr(event, "event_id", None)
        if event_id is not None:
            parts.append(f"id: {event_id}")
        parts.append(f"event: {event.type}")
        parts.append(f"data: {data}")
        return "\n".join(parts) + "\n\n"

    @staticmethod
    def _generate_session_id() -> str:
        """生成新的会话 ID。"""
        return uuid.uuid4().hex

    @staticmethod
    def _build_tool_error_result(tool_name: str, message: str) -> ToolResult:
        """构造工具错误结果，避免流程中断。"""
        return ToolResult(ok=False, data={"tool": tool_name}, error=message)

    @staticmethod
    def _is_sandbox_tool(ctx: RequestContext, tool_name: str) -> bool:
        """判断工具是否通过沙盒执行。"""
        return (
            str(ctx.config.sandbox.mode).lower() == "sandbox"
            and tool_name in SANDBOX_TOOL_NAMES
        )
