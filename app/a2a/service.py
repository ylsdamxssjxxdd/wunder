"""A2A 协议核心服务：负责请求解析、任务映射与流式事件生成。"""

from __future__ import annotations

import asyncio
import base64
import logging
import uuid
from dataclasses import dataclass
from typing import Any, AsyncGenerator, Dict, Iterable, List, Optional, Tuple

from app.a2a.constants import A2A_ERROR_CODES, A2A_PROTOCOL_VERSION, JSONRPC_ERROR_CODES
from app.a2a.utils import (
    build_artifact,
    build_data_part,
    build_message,
    build_status,
    build_task,
    build_task_artifact_update_event,
    build_task_status_update_event,
    build_text_part,
    format_timestamp,
    parse_task_name,
    safe_json_loads,
    utc_now,
)
from app.core.errors import ErrorCodes
from app.core.i18n import get_language, t
from app.knowledge.service import build_knowledge_tool_specs
from app.monitor.registry import monitor
from app.schemas.wunder import WunderRequest
from app.storage import get_storage
from app.tools.availability import (
    build_a2a_tool_specs,
    build_enabled_builtin_specs,
    build_mcp_tool_specs,
)
from app.tools.registry import ToolSpec


class A2AError(Exception):
    """A2A 业务异常，用于统一生成 JSON-RPC 错误响应。"""

    def __init__(self, code: int, message: str, data: Optional[Dict[str, Any]] = None) -> None:
        super().__init__(message)
        self.code = int(code)
        self.message = message
        self.data = data or {}


@dataclass
class A2AStreamState:
    """流式会话上下文，用于在 SSE 流中保存 Task 元数据。"""

    session_id: str
    context_id: str
    user_id: str
    final_sent: bool = False


class A2AService:
    """A2A 协议服务入口，提供 JSON-RPC 方法的实现。"""

    _DEFAULT_SKILL_NAME = "wunder-general"

    def __init__(self, orchestrator) -> None:
        self._orchestrator = orchestrator
        self._storage = get_storage(orchestrator.config.storage)
        self._logger = logging.getLogger("wunder.a2a")

    def _resolve_agent_description(self) -> str:
        """根据当前语言返回 AgentCard 描述文本。"""
        language = get_language().lower()
        if language.startswith("en"):
            return "Wunder agent router"
        return "Wunder 智能体路由器"

    def _tool_spec_to_payload(self, spec: ToolSpec, *, kind: str) -> Dict[str, Any]:
        """将 ToolSpec 转换为 AgentCard 可序列化结构。"""
        payload: Dict[str, Any] = {
            "name": spec.name,
            "description": spec.description,
        }
        if kind == "mcp":
            if "@" in spec.name:
                server, tool = spec.name.split("@", 1)
                payload["server"] = server
                payload["tool"] = tool
            else:
                payload["tool"] = spec.name
        return payload

    def _build_tooling_specs(self) -> Dict[str, Any]:
        """汇总内置/MCP/知识库工具清单，用于 AgentCard 扩展展示。"""
        config = self._orchestrator.config
        skill_names = {spec.name for spec in self._orchestrator.skills.list_specs()}
        builtin_specs = build_enabled_builtin_specs(config)
        mcp_specs = build_mcp_tool_specs(config)
        a2a_specs = build_a2a_tool_specs(config)
        knowledge_specs = build_knowledge_tool_specs(config, blocked_names=skill_names)
        return {
            "builtin": [self._tool_spec_to_payload(spec, kind="builtin") for spec in builtin_specs],
            "mcp": [self._tool_spec_to_payload(spec, kind="mcp") for spec in mcp_specs],
            "a2a": [self._tool_spec_to_payload(spec, kind="a2a") for spec in a2a_specs],
            "knowledge": [
                self._tool_spec_to_payload(spec, kind="knowledge") for spec in knowledge_specs
            ],
        }

    def build_agent_card(self, base_url: str, *, extended: bool = False) -> Dict[str, Any]:
        """构造 AgentCard，提供 A2A 服务发现与能力声明。"""
        config = self._orchestrator.config
        base = base_url.rstrip("/")
        description = self._resolve_agent_description()
        card: Dict[str, Any] = {
            "protocolVersion": A2A_PROTOCOL_VERSION,
            "name": "Wunder",
            "description": description,
            "supportedInterfaces": [
                {"url": f"{base}/a2a", "protocolBinding": "JSONRPC"}
            ],
            "provider": {
                "organization": "Wunder",
                "url": base,
            },
            "version": "0.1.0",
            "capabilities": {
                "streaming": True,
                "pushNotifications": False,
                "stateTransitionHistory": False,
            },
            "defaultInputModes": ["text/plain"],
            "defaultOutputModes": ["text/plain", "application/json"],
            "supportsExtendedAgentCard": True,
        }

        # 生成技能清单，优先使用已启用技能；无技能时提供默认能力描述。
        card["skills"] = self._build_skill_specs()
        # 扩展补充工具清单，方便 A2A 客户端理解可用工具范围。
        card["tooling"] = self._build_tooling_specs()

        api_key = str(config.security.api_key or "").strip()
        if api_key:
            card["securitySchemes"] = {
                "apiKey": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-API-Key",
                    "description": "Wunder API Key",
                }
            }
            card["security"] = [{"apiKey": []}]

        if extended:
            card["documentationUrl"] = f"{base}/wunder/web"
        return card

    def _build_skill_specs(self) -> List[Dict[str, Any]]:
        """从技能注册表生成 AgentCard.skills 列表。"""
        skills: List[Dict[str, Any]] = []
        specs = self._orchestrator.skills.list_specs()
        for spec in specs:
            skills.append(
                {
                    "id": spec.name,
                    "name": spec.name,
                    "description": spec.description,
                    "tags": [spec.name],
                    "examples": [],
                    "inputModes": ["text/plain"],
                    "outputModes": ["text/plain"],
                }
            )
        if not skills:
            skills.append(
                {
                    "id": self._DEFAULT_SKILL_NAME,
                    "name": "通用对话",
                    "description": "支持工具调用与知识检索的通用智能体能力。",
                    "tags": ["general", "tools"],
                    "examples": ["请帮我总结当前工作区内容"],
                    "inputModes": ["text/plain"],
                    "outputModes": ["text/plain"],
                }
            )
        return skills

    async def send_message(self, params: Dict[str, Any]) -> Dict[str, Any]:
        """处理 SendMessage 请求，返回 Task 或 Message 结果。"""
        message = params.get("message") or params.get("request")
        if not isinstance(message, dict):
            raise self._invalid_params("message")

        user_id = self._resolve_user_id(params)
        session_id, context_id = self._resolve_session_ids(message)
        question = self._extract_question(message)
        configuration = params.get("configuration") or {}
        blocking = bool(configuration.get("blocking", False))
        history_length = configuration.get("historyLength")
        tool_names = self._normalize_list(params.get("toolNames"))
        model_name = self._normalize_text(params.get("modelName"))

        request = WunderRequest(
            user_id=user_id,
            question=question,
            session_id=session_id,
            stream=False,
            tool_names=tool_names,
            model_name=model_name or None,
        )

        if blocking:
            result = await self._orchestrator.run(request)
            task = await self._build_task_from_result(
                result.session_id,
                user_id,
                result.answer,
                result.usage or {},
                history_length=history_length,
                context_id=context_id or result.session_id,
            )
            return {"task": task}

        # 非阻塞模式下将任务放到后台执行，及时返回 Task 信息。
        asyncio.create_task(self._run_background(request))
        status = build_status(
            state="working",
            message_text=t("monitor.summary.received"),
            context_id=context_id,
            task_id=session_id,
        )
        task = build_task(
            task_id=session_id,
            context_id=context_id,
            status=status,
            artifacts=[],
            history=None,
            metadata={"queued": True},
        )
        return {"task": task}

    async def send_streaming_message(
        self, params: Dict[str, Any]
    ) -> AsyncGenerator[Dict[str, Any], None]:
        """处理 SendStreamingMessage，请求转为 Wunder SSE 并映射为 A2A 流式事件。"""
        message = params.get("message") or params.get("request")
        if not isinstance(message, dict):
            raise self._invalid_params("message")

        user_id = self._resolve_user_id(params)
        session_id, context_id = self._resolve_session_ids(message)
        question = self._extract_question(message)
        tool_names = self._normalize_list(params.get("toolNames"))
        model_name = self._normalize_text(params.get("modelName"))

        request = WunderRequest(
            user_id=user_id,
            question=question,
            session_id=session_id,
            stream=True,
            tool_names=tool_names,
            model_name=model_name or None,
        )
        state = A2AStreamState(
            session_id=session_id,
            context_id=context_id,
            user_id=user_id,
        )

        # 首包事件发送 Task 对象，符合 A2A Streaming 规范要求。
        initial_status = build_status(
            state="submitted",
            message_text=t("monitor.summary.received"),
            context_id=context_id,
            task_id=session_id,
        )
        initial_task = build_task(
            task_id=session_id,
            context_id=context_id,
            status=initial_status,
            artifacts=[],
            history=None,
        )
        yield {"task": initial_task}

        async for event in self._iter_wunder_events(request):
            for payload, final in self._map_wunder_event(state, event):
                yield payload
                if final:
                    state.final_sent = True
            if state.final_sent:
                break

    async def subscribe_to_task(
        self, params: Dict[str, Any]
    ) -> AsyncGenerator[Dict[str, Any], None]:
        """订阅已存在任务，轮询监控事件并输出 A2A Streaming 更新。"""
        name = params.get("name") or ""
        session_id = parse_task_name(str(name))
        if not session_id:
            raise self._invalid_params("name")

        record = await self._get_monitor_record(session_id)
        if not record:
            raise self._task_not_found(session_id)

        user_id = str(record.get("user_id", "") or "").strip() or "a2a"
        context_id = session_id
        state = A2AStreamState(session_id=session_id, context_id=context_id, user_id=user_id)
        status = self._build_status_from_record(record, session_id)
        task = build_task(
            task_id=session_id,
            context_id=context_id,
            status=status,
            artifacts=[],
            history=None,
        )
        yield {"task": task}

        # 已完成任务直接结束订阅。
        if status.get("state") in {"completed", "failed", "cancelled", "rejected"}:
            return

        last_event_index = 0
        while True:
            detail = monitor.get_detail(session_id)
            events = []
            if detail:
                events = detail.get("events") or []
            if last_event_index < len(events):
                new_events = events[last_event_index:]
                last_event_index = len(events)
                for item in new_events:
                    payloads, final = self._map_monitor_event(state, item)
                    for payload in payloads:
                        yield payload
                    if final:
                        return
            await asyncio.sleep(0.5)

    async def get_task(self, params: Dict[str, Any]) -> Dict[str, Any]:
        """查询 Task 状态，返回完整 Task 对象。"""
        name = params.get("name") or ""
        session_id = parse_task_name(str(name))
        if not session_id:
            raise self._invalid_params("name")

        history_length = params.get("historyLength")
        record = await self._get_monitor_record(session_id)
        if not record:
            raise self._task_not_found(session_id)

        user_id = str(record.get("user_id", "") or "").strip() or "a2a"
        task = await self._build_task_from_record(
            record,
            user_id=user_id,
            include_artifacts=True,
            history_length=history_length,
        )
        return task

    async def list_tasks(self, params: Dict[str, Any]) -> Dict[str, Any]:
        """返回任务列表，支持过滤与分页。"""
        context_id = self._normalize_text(params.get("contextId"))
        status_filter = self._normalize_text(params.get("status"))
        page_size = self._normalize_page_size(params.get("pageSize"))
        page_token = self._normalize_text(params.get("pageToken"))
        history_length = params.get("historyLength")
        include_artifacts = bool(params.get("includeArtifacts", False))

        records = self._storage.load_monitor_records()
        filtered = []
        for record in records:
            if not isinstance(record, dict):
                continue
            if context_id and record.get("session_id") != context_id:
                continue
            if status_filter:
                mapped = self._map_task_state(record.get("status"))
                if mapped != status_filter:
                    continue
            filtered.append(record)

        # 按更新时间降序排列，符合 A2A 的列表要求。
        filtered.sort(key=lambda item: float(item.get("updated_time", 0) or 0), reverse=True)

        offset = self._decode_page_token(page_token)
        total_size = len(filtered)
        page_records = filtered[offset : offset + page_size]
        next_offset = offset + len(page_records)
        next_token = self._encode_page_token(next_offset) if next_offset < total_size else ""

        tasks: List[Dict[str, Any]] = []
        for record in page_records:
            user_id = str(record.get("user_id", "") or "").strip() or "a2a"
            task = await self._build_task_from_record(
                record,
                user_id=user_id,
                include_artifacts=include_artifacts,
                history_length=history_length,
            )
            if not include_artifacts and "artifacts" in task:
                task.pop("artifacts", None)
            tasks.append(task)

        return {
            "tasks": tasks,
            "nextPageToken": next_token,
            "pageSize": page_size,
            "totalSize": total_size,
        }

    async def cancel_task(self, params: Dict[str, Any]) -> Dict[str, Any]:
        """取消任务并返回最新 Task 状态。"""
        name = params.get("name") or ""
        session_id = parse_task_name(str(name))
        if not session_id:
            raise self._invalid_params("name")

        record = await self._get_monitor_record(session_id)
        if not record:
            raise self._task_not_found(session_id)

        if not monitor.cancel(session_id):
            raise self._task_not_cancelable(session_id)

        updated = await self._get_monitor_record(session_id) or record
        user_id = str(updated.get("user_id", "") or "").strip() or "a2a"
        task = await self._build_task_from_record(
            updated,
            user_id=user_id,
            include_artifacts=True,
            history_length=None,
        )
        return task

    async def get_extended_agent_card(self, base_url: str) -> Dict[str, Any]:
        """返回扩展 AgentCard，当前与公开版本一致。"""
        return self.build_agent_card(base_url, extended=True)

    async def _run_background(self, request: WunderRequest) -> None:
        """后台执行请求，吞掉异常避免任务日志污染。"""
        try:
            await self._orchestrator.run(request)
        except Exception as exc:  # noqa: BLE001
            self._logger.warning("A2A 后台任务失败: %s", exc)

    async def _build_task_from_result(
        self,
        session_id: str,
        user_id: str,
        answer: str,
        usage: Dict[str, Any],
        *,
        history_length: Optional[int],
        context_id: Optional[str],
    ) -> Dict[str, Any]:
        """根据同步执行结果构造 Task 对象。"""
        status = build_status(
            state="completed",
            message_text=t("monitor.summary.finished"),
            context_id=context_id,
            task_id=session_id,
        )
        artifacts = []
        if answer:
            artifacts.append(
                build_artifact(
                    artifact_id=uuid.uuid4().hex,
                    name="final-answer",
                    parts=[build_text_part(answer)],
                )
            )
        metadata = {"tokenUsage": usage} if usage else {}
        history = await self._load_history(user_id, session_id, history_length)
        return build_task(
            task_id=session_id,
            context_id=context_id or session_id,
            status=status,
            artifacts=artifacts,
            history=history,
            metadata=metadata or None,
        )

    async def _build_task_from_record(
        self,
        record: Dict[str, Any],
        *,
        user_id: str,
        include_artifacts: bool,
        history_length: Optional[int],
    ) -> Dict[str, Any]:
        """将监控记录转换为 Task 对象。"""
        session_id = str(record.get("session_id", "") or "").strip()
        context_id = session_id
        status = self._build_status_from_record(record, session_id)
        metadata = self._build_task_metadata(record)
        history = await self._load_history(user_id, session_id, history_length)
        artifacts: Optional[List[Dict[str, Any]]] = None
        if include_artifacts:
            artifacts = await self._load_artifacts(user_id, session_id, history)
        return build_task(
            task_id=session_id,
            context_id=context_id,
            status=status,
            artifacts=artifacts,
            history=history,
            metadata=metadata or None,
        )

    def _build_status_from_record(self, record: Dict[str, Any], session_id: str) -> Dict[str, Any]:
        """根据监控记录生成 TaskStatus。"""
        summary = str(record.get("summary") or record.get("stage") or "").strip()
        state = self._map_task_state(record.get("status"))
        raw_timestamp = record.get("updated_time")
        if isinstance(raw_timestamp, str) and raw_timestamp.strip():
            timestamp = raw_timestamp.strip()
        else:
            timestamp = format_timestamp(raw_timestamp) or utc_now()
        return build_status(
            state=state,
            message_text=summary or t("monitor.summary.received"),
            context_id=session_id,
            task_id=session_id,
            timestamp=timestamp,
        )

    def _build_task_metadata(self, record: Dict[str, Any]) -> Dict[str, Any]:
        """提取监控记录中的可用元信息。"""
        metadata: Dict[str, Any] = {}
        stage = record.get("stage")
        if stage:
            metadata["stage"] = stage
        token_usage = record.get("token_usage")
        if token_usage is not None:
            metadata["tokenUsage"] = token_usage
        if record.get("cancel_requested"):
            metadata["cancelRequested"] = True
        return metadata

    async def _load_history(
        self, user_id: str, session_id: str, history_length: Optional[int]
    ) -> Optional[List[Dict[str, Any]]]:
        """读取历史消息并转换为 A2A Message 列表。"""
        if history_length is None:
            return None
        limit: Optional[int]
        try:
            limit = int(history_length)
        except (TypeError, ValueError):
            limit = None
        if limit is None or limit <= 0:
            return None
        records = await self._orchestrator.workspace_manager.load_history(
            user_id, session_id, limit
        )
        messages: List[Dict[str, Any]] = []
        for item in records:
            if not isinstance(item, dict):
                continue
            role = str(item.get("role") or "")
            if role not in {"user", "assistant"}:
                continue
            content = str(item.get("content") or "")
            messages.append(
                build_message(
                    role="user" if role == "user" else "agent",
                    parts=[build_text_part(content)],
                    context_id=session_id,
                    task_id=session_id,
                )
            )
        return messages

    async def _load_artifacts(
        self,
        user_id: str,
        session_id: str,
        history: Optional[List[Dict[str, Any]]],
    ) -> List[Dict[str, Any]]:
        """从产物日志与最终回复中汇总 A2A Artifact。"""
        artifacts: List[Dict[str, Any]] = []
        logs = await self._orchestrator.workspace_manager.load_artifact_logs(
            user_id, session_id, limit=50
        )
        for item in logs:
            if not isinstance(item, dict):
                continue
            artifact_id = f"log-{item.get('artifact_id') or uuid.uuid4().hex}"
            artifacts.append(
                build_artifact(
                    artifact_id=artifact_id,
                    name=str(item.get("name") or ""),
                    parts=[build_data_part(item)],
                )
            )

        final_answer = self._extract_final_answer(history)
        if final_answer:
            artifacts.append(
                build_artifact(
                    artifact_id=f"final-{uuid.uuid4().hex}",
                    name="final-answer",
                    parts=[build_text_part(final_answer)],
                )
            )
        return artifacts

    @staticmethod
    def _extract_final_answer(history: Optional[List[Dict[str, Any]]]) -> str:
        """从历史消息中提取最后一条 agent 输出作为最终回答。"""
        if not history:
            return ""
        for item in reversed(history):
            if item.get("role") == "agent":
                parts = item.get("parts") or []
                for part in parts:
                    if isinstance(part, dict) and "text" in part:
                        return str(part.get("text") or "").strip()
        return ""

    async def _get_monitor_record(self, session_id: str) -> Optional[Dict[str, Any]]:
        """读取指定 session_id 的监控记录，优先内存，其次持久化。"""
        detail = monitor.get_detail(session_id)
        if detail and isinstance(detail.get("session"), dict):
            session = detail.get("session")
            if session and session.get("session_id") == session_id:
                return session
        return self._storage.get_monitor_record(session_id)

    def _resolve_user_id(self, params: Dict[str, Any]) -> str:
        """从 A2A 参数中提取 user_id，缺省时回退到 a2a。"""
        for key in ("userId", "user_id", "tenant"):
            value = params.get(key)
            if isinstance(value, str) and value.strip():
                return value.strip()
        return "a2a"

    def _resolve_session_ids(self, message: Dict[str, Any]) -> Tuple[str, str]:
        """从 A2A Message 中解析 taskId/contextId，缺省时生成新 ID。"""
        task_id = str(message.get("taskId") or "").strip()
        context_id = str(message.get("contextId") or "").strip()
        if task_id and context_id and task_id != context_id:
            raise self._invalid_params("contextId")
        if not task_id and context_id:
            task_id = context_id
        if not task_id:
            task_id = uuid.uuid4().hex
        if not context_id:
            context_id = task_id
        return task_id, context_id

    def _extract_question(self, message: Dict[str, Any]) -> str:
        """从 A2A Message.parts 中抽取文本内容，当前仅支持 text/plain。"""
        parts = message.get("parts")
        if not isinstance(parts, list) or not parts:
            raise self._invalid_params("parts")
        texts: List[str] = []
        for part in parts:
            if not isinstance(part, dict):
                continue
            if "text" in part:
                texts.append(str(part.get("text") or ""))
                continue
            # 暂不支持文件/结构化输入，返回协议规定的错误码。
            raise self._content_type_not_supported()
        question = "\n".join(texts).strip()
        if not question:
            raise self._invalid_params("text")
        return question

    @staticmethod
    def _normalize_text(value: Any) -> str:
        """统一文本字段的清洗逻辑。"""
        return str(value or "").strip()

    @staticmethod
    def _normalize_list(value: Any) -> Optional[List[str]]:
        """统一数组字段清洗，仅保留非空字符串项。"""
        if not isinstance(value, list):
            return None
        cleaned = [str(item).strip() for item in value if str(item).strip()]
        return cleaned or None

    @staticmethod
    def _normalize_page_size(value: Any) -> int:
        """解析分页大小，默认 50，最大 100。"""
        try:
            size = int(value)
        except (TypeError, ValueError):
            size = 50
        size = max(1, size)
        return min(100, size)

    @staticmethod
    def _encode_page_token(offset: int) -> str:
        """将偏移量编码为 pageToken，避免直接暴露整数。"""
        raw = str(max(0, int(offset))).encode("utf-8")
        return base64.urlsafe_b64encode(raw).decode("ascii")

    @staticmethod
    def _decode_page_token(token: str) -> int:
        """解析 pageToken 为偏移量，异常时回退为 0。"""
        if not token:
            return 0
        try:
            raw = base64.urlsafe_b64decode(token.encode("ascii")).decode("utf-8")
            return max(0, int(raw))
        except (ValueError, UnicodeError):
            return 0

    def _map_task_state(self, status: Any) -> str:
        """将 Wunder 监控状态映射为 A2A TaskState。"""
        value = str(status or "").lower()
        if value in {"finished", "final"}:
            return "completed"
        if value in {"error", "failed"}:
            return "failed"
        if value in {"cancelled", "canceled"}:
            return "cancelled"
        if value in {"rejected"}:
            return "rejected"
        if value in {"input_required"}:
            return "input-required"
        return "working"

    async def _iter_wunder_events(
        self, request: WunderRequest
    ) -> AsyncGenerator[Dict[str, Any], None]:
        """消费 Wunder SSE 流，解析为结构化事件对象。"""
        async for chunk in self._orchestrator.sse_stream(request):
            for event in self._parse_sse_chunk(chunk):
                yield event

    @staticmethod
    def _parse_sse_chunk(chunk: str) -> Iterable[Dict[str, Any]]:
        """解析单段 SSE 文本，输出 event/data 结构。"""
        current: Dict[str, Any] = {}
        for line in str(chunk or "").splitlines():
            if not line.strip():
                if current:
                    yield current
                    current = {}
                continue
            if line.startswith("event:"):
                current["event"] = line[len("event:") :].strip()
                continue
            if line.startswith("data:"):
                raw = line[len("data:") :].strip()
                payload = safe_json_loads(raw)
                current["data"] = payload if payload is not None else raw
        if current:
            yield current

    def _map_wunder_event(
        self, state: A2AStreamState, event: Dict[str, Any]
    ) -> Iterable[Tuple[Dict[str, Any], bool]]:
        """将 Wunder SSE 事件映射为 A2A StreamResponse。"""
        event_type = str(event.get("event") or "").strip()
        payload = event.get("data")
        if not isinstance(payload, dict):
            return []
        data = payload.get("data") if isinstance(payload.get("data"), dict) else {}
        timestamp = payload.get("timestamp")
        return self._map_stream_payload(state, event_type, data, timestamp)

    def _map_monitor_event(
        self, state: A2AStreamState, item: Dict[str, Any]
    ) -> Tuple[List[Dict[str, Any]], bool]:
        """将监控事件映射为 A2A StreamResponse，返回 payload 列表与是否结束标记。"""
        event_type = str(item.get("type") or "").strip()
        data = item.get("data") if isinstance(item.get("data"), dict) else {}
        timestamp = item.get("timestamp")
        mapped = list(self._map_stream_payload(state, event_type, data, timestamp))
        if not mapped:
            return [], False
        payloads = [payload for payload, _ in mapped]
        final = any(flag for _, flag in mapped)
        return payloads, final

    def _map_stream_payload(
        self,
        state: A2AStreamState,
        event_type: str,
        data: Dict[str, Any],
        timestamp: Optional[str],
    ) -> Iterable[Tuple[Dict[str, Any], bool]]:
        """统一处理事件映射逻辑，避免 SSE 与监控重复代码。"""
        if not event_type:
            return []

        if event_type in {"progress", "received", "round_start"}:
            summary = str(data.get("summary") or "").strip()
            message_text = summary or t("monitor.summary.received")
            status = build_status(
                state="working",
                message_text=message_text,
                context_id=state.context_id,
                task_id=state.session_id,
                timestamp=timestamp,
            )
            metadata = {"stage": str(data.get("stage") or "")} if data.get("stage") else None
            payload = build_task_status_update_event(
                task_id=state.session_id,
                context_id=state.context_id,
                status=status,
                final=False,
                metadata=metadata,
            )
            return [(payload, False)]

        if event_type == "a2ui":
            artifact_payload = {
                "uid": data.get("uid"),
                "messages": data.get("messages"),
                "content": data.get("content"),
            }
            artifact = build_artifact(
                artifact_id=f"a2ui-{uuid.uuid4().hex}",
                name="a2ui",
                parts=[build_data_part(artifact_payload)],
            )
            payload = build_task_artifact_update_event(
                task_id=state.session_id,
                context_id=state.context_id,
                artifact=artifact,
            )
            return [(payload, False)]

        if event_type == "final":
            answer = str(data.get("answer") or "")
            usage = data.get("usage") if isinstance(data.get("usage"), dict) else {}
            events: List[Tuple[Dict[str, Any], bool]] = []
            if answer:
                artifact = build_artifact(
                    artifact_id=f"final-{uuid.uuid4().hex}",
                    name="final-answer",
                    parts=[build_text_part(answer)],
                )
                events.append(
                    (
                        build_task_artifact_update_event(
                            task_id=state.session_id,
                            context_id=state.context_id,
                            artifact=artifact,
                        ),
                        False,
                    )
                )
            status = build_status(
                state="completed",
                message_text=t("monitor.summary.finished"),
                context_id=state.context_id,
                task_id=state.session_id,
                timestamp=timestamp,
            )
            metadata = {"tokenUsage": usage} if usage else None
            events.append(
                (
                    build_task_status_update_event(
                        task_id=state.session_id,
                        context_id=state.context_id,
                        status=status,
                        final=True,
                        metadata=metadata,
                    ),
                    True,
                )
            )
            return events

        if event_type == "error":
            message_text = str(data.get("message") or t("monitor.summary.exception"))
            error_code = str(data.get("code") or "").upper()
            mapped_state = "cancelled" if error_code == ErrorCodes.CANCELLED else "failed"
            status = build_status(
                state=mapped_state,
                message_text=message_text,
                context_id=state.context_id,
                task_id=state.session_id,
                timestamp=timestamp,
            )
            metadata = {"error": data} if data else None
            payload = build_task_status_update_event(
                task_id=state.session_id,
                context_id=state.context_id,
                status=status,
                final=True,
                metadata=metadata,
            )
            return [(payload, True)]

        if event_type == "cancel":
            message_text = str(data.get("summary") or t("monitor.summary.cancel_requested"))
            status = build_status(
                state="working",
                message_text=message_text,
                context_id=state.context_id,
                task_id=state.session_id,
                timestamp=timestamp,
            )
            payload = build_task_status_update_event(
                task_id=state.session_id,
                context_id=state.context_id,
                status=status,
                final=False,
                metadata={"cancelRequested": True},
            )
            return [(payload, False)]

        if event_type == "cancelled":
            message_text = str(data.get("summary") or t("monitor.summary.cancelled"))
            status = build_status(
                state="cancelled",
                message_text=message_text,
                context_id=state.context_id,
                task_id=state.session_id,
                timestamp=timestamp,
            )
            payload = build_task_status_update_event(
                task_id=state.session_id,
                context_id=state.context_id,
                status=status,
                final=True,
            )
            return [(payload, True)]

        if event_type == "finished":
            message_text = str(data.get("summary") or t("monitor.summary.finished"))
            status = build_status(
                state="completed",
                message_text=message_text,
                context_id=state.context_id,
                task_id=state.session_id,
                timestamp=timestamp,
            )
            payload = build_task_status_update_event(
                task_id=state.session_id,
                context_id=state.context_id,
                status=status,
                final=True,
            )
            return [(payload, True)]

        return []

    def _invalid_params(self, detail: str) -> A2AError:
        """构造 JSON-RPC 参数错误，统一消息格式。"""
        message = t("error.param_required")
        message = f"{message}: {detail}" if detail else message
        data = {"parameter": detail} if detail else {}
        return A2AError(JSONRPC_ERROR_CODES["InvalidParams"], message, data)

    def _task_not_found(self, session_id: str) -> A2AError:
        """构造任务不存在错误。"""
        return A2AError(
            A2A_ERROR_CODES["TaskNotFoundError"],
            t("error.task_not_found"),
            {"taskId": session_id},
        )

    def _task_not_cancelable(self, session_id: str) -> A2AError:
        """构造任务不可取消错误。"""
        return A2AError(
            A2A_ERROR_CODES["TaskNotCancelableError"],
            "任务不可取消",
            {"taskId": session_id},
        )

    def _content_type_not_supported(self) -> A2AError:
        """构造不支持的内容类型错误。"""
        return A2AError(
            A2A_ERROR_CODES["ContentTypeNotSupportedError"],
            "不支持的内容类型",
        )
