import asyncio
import json
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple
from urllib.parse import urlparse

from app.core.config import KnowledgeBaseConfig
from app.core.i18n import t
from app.knowledge.service import KnowledgeQueryError, query_knowledge_documents
from app.orchestrator.constants import TOOL_CALL_OPEN_PATTERN, TOOL_CALL_PATTERN
from app.orchestrator.context import RequestContext, UserToolBindings
from app.orchestrator.user_tools import UserToolManager
from app.sandbox.client import SandboxClient, SandboxClientError
from app.schemas.wunder import StreamEvent
from app.tools.catalog import is_workspace_mutation_tool, resolve_builtin_tool_name
from app.tools.constants import SANDBOX_TOOL_NAMES
from app.tools.availability import extract_a2a_service_name, is_a2a_tool_name
from app.tools import builtin
from app.tools.mcp import MCPClient
from app.tools.types import ToolContext, ToolResult


class ToolCallParser:
    """模型工具调用解析器。"""

    @staticmethod
    def parse(text: str) -> List[Dict[str, Any]]:
        """从模型输出中解析工具调用。"""
        content = str(text or "")
        if not content:
            return []
        calls = ToolCallParser._parse_closed_tags(content)
        if calls:
            return calls
        calls = ToolCallParser._parse_open_tags(content)
        if calls:
            return calls
        return ToolCallParser._parse_payload(content)

    @staticmethod
    def _parse_closed_tags(text: str) -> List[Dict[str, Any]]:
        """解析成对标签包裹的工具调用。"""
        calls: List[Dict[str, Any]] = []
        for match in TOOL_CALL_PATTERN.finditer(text):
            payload = match.group("payload").strip()
            calls.extend(ToolCallParser._parse_payload(payload))
        return calls

    @staticmethod
    def _parse_open_tags(text: str) -> List[Dict[str, Any]]:
        """解析未闭合标签的工具调用。"""
        matches = list(TOOL_CALL_OPEN_PATTERN.finditer(text))
        if not matches:
            return []
        calls: List[Dict[str, Any]] = []
        for index, match in enumerate(matches):
            start_index = match.end()
            end_index = (
                matches[index + 1].start() if index + 1 < len(matches) else len(text)
            )
            payload = text[start_index:end_index].strip()
            if not payload:
                continue
            calls.extend(ToolCallParser._parse_payload(payload))
        return calls

    @staticmethod
    def _parse_payload(payload: str) -> List[Dict[str, Any]]:
        """从文本中提取并规整工具调用 JSON。"""
        if not payload:
            return []
        parsed = ToolCallParser._load_json(payload)
        if parsed is None:
            parsed = ToolCallParser._extract_json(payload)
        return ToolCallParser._normalize_calls(parsed)

    @staticmethod
    def _load_json(payload: str) -> Any | None:
        """尝试直接解析 JSON。"""
        try:
            return json.loads(payload)
        except json.JSONDecodeError:
            return None

    @staticmethod
    def _extract_json(payload: str) -> Any | None:
        """从文本中提取第一个完整 JSON 对象或数组。"""
        for index, char in enumerate(payload):
            if char not in "{[":
                continue
            end_index = ToolCallParser._find_json_end(payload, index)
            if end_index is None:
                continue
            candidate = payload[index:end_index]
            try:
                return json.loads(candidate)
            except json.JSONDecodeError:
                continue
        return None

    @staticmethod
    def _find_json_end(text: str, start: int) -> int | None:
        """从起点定位 JSON 结束位置。"""
        stack: List[str] = []
        in_string = False
        escape = False
        for index in range(start, len(text)):
            char = text[index]
            if in_string:
                if escape:
                    escape = False
                    continue
                if char == "\\":
                    escape = True
                    continue
                if char == "\"":
                    in_string = False
                continue
            if char == "\"":
                in_string = True
                continue
            if char in "{[":
                stack.append(char)
                continue
            if char in "}]":
                if not stack:
                    return None
                opening = stack.pop()
                if opening == "{" and char != "}":
                    return None
                if opening == "[" and char != "]":
                    return None
                if not stack:
                    return index + 1
        return None

    @staticmethod
    def _normalize_calls(payload: Any | None) -> List[Dict[str, Any]]:
        """规整工具调用结构并返回合格记录。"""
        if payload is None:
            return []
        calls: List[Dict[str, Any]] = []
        if isinstance(payload, dict):
            call = ToolCallParser._normalize_call(payload)
            if call:
                calls.append(call)
            return calls
        if isinstance(payload, list):
            for item in payload:
                if not isinstance(item, dict):
                    continue
                call = ToolCallParser._normalize_call(item)
                if call:
                    calls.append(call)
            return calls
        return []

    @staticmethod
    def _normalize_call(call: Dict[str, Any]) -> Dict[str, Any] | None:
        """校验工具调用并解析 arguments 字段。"""
        if "name" not in call or "arguments" not in call:
            return None
        if isinstance(call["arguments"], str):
            try:
                call["arguments"] = json.loads(call["arguments"])
            except json.JSONDecodeError:
                call["arguments"] = {"raw": call["arguments"]}
        return call


class ToolExecutor:
    """工具执行封装，兼容自建/共享别名与 MCP 工具。"""

    def __init__(self, user_tool_manager: UserToolManager) -> None:
        self._user_tool_manager = user_tool_manager

    async def execute(
        self,
        name: str,
        args: Dict[str, Any],
        ctx: RequestContext,
        workspace,
        user_tool_bindings: UserToolBindings | None = None,
        *,
        emit_event: Optional[Callable[[StreamEvent], None]] = None,
    ) -> Tuple[ToolResult, List[StreamEvent]]:
        """执行工具调用，返回工具结果与调试事件列表。"""
        debug_events: List[StreamEvent] = []
        alias_entry = (
            user_tool_bindings.alias_map.get(name) if user_tool_bindings else None
        )

        def _emit_debug_event(event_type: str, data: Dict[str, Any]) -> None:
            # 统一构建工具调试事件，优先实时抛给 SSE，否则暂存等待统一回放
            event = StreamEvent(type=event_type, session_id=workspace.session_id, data=data)
            if emit_event:
                emit_event(event)
                return
            debug_events.append(event)

        def _finish(result: ToolResult, mutated: bool = False) -> Tuple[ToolResult, List[StreamEvent]]:
            """在返回前统一处理工作区变更标记，避免遗漏目录树刷新。"""
            if mutated:
                ctx.workspace_manager.mark_tree_dirty(workspace.user_id)
            return result, debug_events

        # 合并 allow_paths 与技能路径，供沙盒工具执行校验
        allow_paths: List[str] = []
        seen_paths: set[str] = set()

        def _normalize_allow_path(raw_path: Any) -> str:
            """规范化允许路径，返回绝对路径或原始值。"""
            path_value = str(raw_path).strip()
            if not path_value:
                return ""
            try:
                return str(Path(path_value).expanduser().resolve())
            except Exception:
                # 路径解析失败时保留原始值
                return path_value

        for raw_path in ctx.config.security.allow_paths:
            normalized = _normalize_allow_path(raw_path)
            if not normalized or normalized in seen_paths:
                continue
            allow_paths.append(normalized)
            seen_paths.add(normalized)
        for raw_path in ctx.config.skills.paths:
            normalized = _normalize_allow_path(raw_path)
            if not normalized or normalized in seen_paths:
                continue
            allow_paths.append(normalized)
            seen_paths.add(normalized)
        if user_tool_bindings:
            # 追加用户技能根目录，确保读取 SKILL.md 等文件
            for source in user_tool_bindings.skill_sources.values():
                normalized = _normalize_allow_path(source.root)
                if not normalized or normalized in seen_paths:
                    continue
                allow_paths.append(normalized)
                seen_paths.add(normalized)

        tool_context = ToolContext(
            workspace=workspace,
            config={
                "allow_commands": ctx.config.security.allow_commands,
                "allow_paths": allow_paths,
                "deny_globs": ctx.config.security.deny_globs,
                "storage_backend": ctx.config.storage.backend,
                "storage_db_path": ctx.config.storage.db_path,
                "storage_postgres_dsn": ctx.config.storage.postgres.dsn,
                "api_key": ctx.config.security.api_key,
                "server_port": ctx.config.server.port,
                # 透传 A2A 服务配置，便于工具侧复用鉴权与默认超时
                "a2a_services": ctx.config.a2a.services,
                "a2a_timeout_s": getattr(ctx.config.a2a, "timeout_s", 120),
            },
            emit_event=_emit_debug_event,
        )

        if alias_entry:
            if alias_entry.kind == "skill":
                registry = self._user_tool_manager.get_user_skill_registry(
                    ctx, user_tool_bindings, alias_entry.owner_id
                )
                if not registry:
                    return _finish(
                        ToolResult(
                            ok=False,
                            data={},
                            error=t("tool.invoke.user_skill_not_loaded"),
                        )
                    )
                try:
                    skill = registry.get(alias_entry.target)
                except KeyError:
                    return _finish(
                        ToolResult(
                            ok=False,
                            data={},
                            error=t("tool.invoke.user_skill_not_found"),
                        )
                    )
                payload = args.get("payload") if isinstance(args, dict) else None
                if isinstance(payload, dict):
                    payload_data = payload
                else:
                    payload_data = args if isinstance(args, dict) else {}
                try:
                    result = await skill(payload_data)
                    return _finish(
                        ToolResult(
                            ok=True,
                            data={"name": alias_entry.target, "result": result},
                        ),
                        mutated=True,
                    )
                except Exception as exc:
                    return _finish(
                        ToolResult(
                            ok=False,
                            data={},
                            error=t("tool.invoke.user_skill_failed", detail=str(exc)),
                        ),
                        mutated=True,
                    )
            if alias_entry.kind == "knowledge":
                query = str(args.get("query", "")).strip()
                if not query:
                    return _finish(
                        ToolResult(
                            ok=False, data={}, error=t("error.knowledge_query_required")
                        )
                    )
                try:
                    root = self._user_tool_manager.store.resolve_knowledge_base_root(
                        alias_entry.owner_id, alias_entry.target
                    )
                except Exception as exc:
                    return _finish(ToolResult(ok=False, data={}, error=str(exc)))
                base_config = KnowledgeBaseConfig(
                    name=name,
                    description="",
                    root=str(root),
                )
                limit = args.get("limit")

                def _log_request(payload: Dict[str, Any]) -> None:
                    if tool_context.emit_event:
                        tool_context.emit_event("knowledge_request", payload)

                try:
                    docs = await query_knowledge_documents(
                        query=query,
                        base=base_config,
                        llm_config=ctx.llm_config,
                        limit=limit,
                        request_logger=_log_request,
                    )
                except KnowledgeQueryError as exc:
                    return _finish(ToolResult(ok=False, data={}, error=str(exc)))
                return _finish(
                    ToolResult(
                        ok=True,
                        data={
                            "knowledge_base": base_config.name,
                            "documents": [doc.to_dict() for doc in docs],
                        },
                    )
                )
            if alias_entry.kind == "mcp":
                if "@" not in alias_entry.target:
                    return _finish(
                        ToolResult(
                            ok=False,
                            data={},
                            error=t("tool.invoke.mcp_name_invalid"),
                        )
                    )
                server_name, tool_name = alias_entry.target.split("@", 1)
                server_map = user_tool_bindings.mcp_servers.get(alias_entry.owner_id, {})
                server_config = server_map.get(server_name)
                if not server_config:
                    return _finish(
                        ToolResult(
                            ok=False,
                            data={},
                            error=t("tool.invoke.mcp_server_unavailable"),
                        )
                    )
                try:
                    client = MCPClient(ctx.config, servers=[server_config])
                    result = await client.call_tool(server_name, tool_name, args)
                    ok = not bool(result.get("is_error"))
                    return _finish(
                        ToolResult(
                            ok=ok,
                            data={
                                "server": server_name,
                                "tool": tool_name,
                                "result": result,
                            },
                            error="" if ok else t("tool.invoke.mcp_result_error"),
                        )
                    )
                except Exception as exc:
                    return _finish(
                        ToolResult(
                            ok=False,
                            data={},
                            error=t("tool.invoke.mcp_call_failed", detail=str(exc)),
                        )
                    )
            return _finish(
                ToolResult(ok=False, data={}, error=t("tool.invoke.user_tool_unknown"))
            )

        try:
            skill = ctx.skills.get(name)
        except KeyError:
            skill = None
        if skill:
            payload = args.get("payload") if isinstance(args, dict) else None
            if isinstance(payload, dict):
                payload_data = payload
            else:
                payload_data = args if isinstance(args, dict) else {}
            try:
                result = await skill(payload_data)
                return _finish(
                    ToolResult(ok=True, data={"name": name, "result": result}),
                    mutated=True,
                )
            except Exception as exc:
                return _finish(
                    ToolResult(
                        ok=False,
                        data={},
                        error=t("tool.invoke.skill_failed", detail=str(exc)),
                    ),
                    mutated=True,
                )

        if name == "wunder@run":
            # 直连内置 wunder@run，避免本地 MCP 自调用导致回传卡住
            try:
                from app.mcp.server import run_wunder_task

                task_value = ""
                if isinstance(args, dict):
                    task_value = args.get("task") or args.get("raw") or ""
                else:
                    task_value = args
                result_payload = await run_wunder_task(task=str(task_value or ""))
                server_name, tool_name = name.split("@", 1)
                return _finish(
                    ToolResult(
                        ok=True,
                        data={
                            "server": server_name,
                            "tool": tool_name,
                            "result": {
                                "content": [],
                                "structured_content": result_payload,
                                "meta": None,
                                "is_error": False,
                            },
                        },
                    )
                )
            except Exception as exc:
                return _finish(
                    ToolResult(
                        ok=False,
                        data={},
                        error=t("tool.invoke.wunder_run_failed", detail=str(exc)),
                    )
                )

        if is_a2a_tool_name(name):
            service_name = extract_a2a_service_name(name)
            service = next(
                (
                    item
                    for item in ctx.config.a2a.services
                    if str(getattr(item, "name", "")).strip() == service_name
                ),
                None,
            )
            if not service or not getattr(service, "enabled", True):
                return _finish(
                    ToolResult(
                        ok=False,
                        data={},
                        error=t("tool.a2a.service_unavailable", name=service_name or name),
                    )
                )

            def _normalize_endpoint(raw_endpoint: Any) -> str:
                """规范化端点地址，便于本地判断。"""
                value = str(raw_endpoint or "").strip()
                if value and not value.startswith(("http://", "https://")):
                    value = f"http://{value}"
                return value

            def _should_attach_api_key(raw_endpoint: Any, target_name: str) -> bool:
                """判断是否需要附加全局 API Key。"""
                api_key = str(ctx.config.security.api_key or "").strip()
                if not api_key:
                    return False
                if str(target_name or "").strip().lower() == "wunder":
                    return True
                normalized = _normalize_endpoint(raw_endpoint)
                if not normalized:
                    return False
                parsed = urlparse(normalized)
                host = (parsed.hostname or "").lower()
                if host not in {"127.0.0.1", "localhost"}:
                    return False
                port = parsed.port or (443 if parsed.scheme == "https" else 80)
                return port == int(ctx.config.server.port)

            a2a_args: Dict[str, Any] = dict(args) if isinstance(args, dict) else {"content": args}
            a2a_args["endpoint"] = getattr(service, "endpoint", "")
            a2a_args.setdefault("service_name", service_name)
            service_type = str(getattr(service, "service_type", "") or "").strip().lower()
            service_user_id = str(getattr(service, "user_id", "") or "").strip()
            if service_type == "internal" and service_user_id:
                if "user_id" not in a2a_args and "userId" not in a2a_args:
                    a2a_args["user_id"] = service_user_id
            raw_method = str(a2a_args.get("method") or "").strip()
            stream_flag = a2a_args.pop("stream", None)
            if not raw_method:
                if stream_flag:
                    raw_method = "SendStreamingMessage"
                else:
                    raw_method = str(getattr(service, "default_method", "") or "SendMessage")
            if raw_method:
                a2a_args["method"] = raw_method

            if getattr(service, "allow_self", False):
                a2a_args.setdefault("allow_self", True)
            max_depth = getattr(service, "max_depth", 0)
            if max_depth:
                a2a_args.setdefault("max_depth", max_depth)

            service_headers = dict(getattr(service, "headers", {}) or {})
            call_headers = a2a_args.get("headers")
            if isinstance(call_headers, dict):
                service_headers.update(call_headers)
            if service_headers:
                a2a_args["headers"] = service_headers

            if (
                getattr(service, "auth", None)
                and not any(key.lower() == "authorization" for key in service_headers)
                and not a2a_args.get("authorization")
                and not a2a_args.get("auth")
            ):
                a2a_args["auth"] = service.auth

            if _should_attach_api_key(getattr(service, "endpoint", ""), service_name):
                a2a_args.setdefault("api_key", ctx.config.security.api_key)

            if "timeout_s" not in a2a_args:
                a2a_args["timeout_s"] = getattr(ctx.config.a2a, "timeout_s", 120)

            result = await asyncio.to_thread(builtin.a2a_delegate, tool_context, a2a_args)
            return _finish(result)

        if "@" in name:
            server, tool = name.split("@", 1)
            try:
                result = await ctx.mcp_client.call_tool(server, tool, args)
                ok = not bool(result.get("is_error"))
                return _finish(
                    ToolResult(
                        ok=ok,
                        data={"server": server, "tool": tool, "result": result},
                        error="" if ok else t("tool.invoke.mcp_result_error"),
                    )
                )
            except Exception as exc:
                return _finish(
                    ToolResult(
                        ok=False,
                        data={},
                        error=t("tool.invoke.mcp_call_failed", detail=str(exc)),
                    )
                )

        # 内置工具支持英文别名，执行前统一转为标准名称。
        resolved_name = resolve_builtin_tool_name(name)
        if resolved_name:
            name = resolved_name

        if (
            str(ctx.config.sandbox.mode).lower() == "sandbox"
            and name in SANDBOX_TOOL_NAMES
        ):
            sandbox_client = SandboxClient(ctx.config.sandbox)
            try:
                result, sandbox_events = await sandbox_client.execute_tool(
                    tool_name=name,
                    args=args,
                    workspace=workspace,
                    allow_paths=allow_paths,
                    deny_globs=ctx.config.security.deny_globs,
                    allow_commands=ctx.config.security.allow_commands,
                )
            except SandboxClientError as exc:
                return _finish(ToolResult(ok=False, data={}, error=str(exc)))

            for event in sandbox_events:
                event_type = str(event.get("type", "")).strip()
                if not event_type:
                    continue
                data = event.get("data")
                payload = data if isinstance(data, dict) else {"detail": data}
                debug_events.append(
                    StreamEvent(
                        type=event_type,
                        session_id=workspace.session_id,
                        data=payload,
                    )
                )
            return _finish(result, mutated=is_workspace_mutation_tool(name))

        try:
            result = await ctx.tools.execute(name, tool_context, args)
            return _finish(result, mutated=is_workspace_mutation_tool(name))
        except Exception as exc:
            return _finish(
                ToolResult(ok=False, data={}, error=str(exc)),
                mutated=is_workspace_mutation_tool(name),
            )
