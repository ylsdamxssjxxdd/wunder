from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, List, Optional

from app.core.config import WunderConfig
from app.core.i18n import t
from app.tools.availability import (
    build_enabled_builtin_specs,
    build_knowledge_tool_specs_filtered,
    build_mcp_tool_specs,
    normalize_mcp_input_schema,
    resolve_mcp_description,
)
from app.orchestrator.engine import WunderOrchestrator
from app.schemas.wunder import AvailableToolsResponse
from app.skills.loader import load_skills


def build_available_tools(
    config: WunderConfig,
    orchestrator: WunderOrchestrator,
    user_id: Optional[str] = None,
) -> AvailableToolsResponse:
    """组装对外可见的工具清单，统一供 /wunder/tools 使用。"""
    # ???????????????????????????
    builtin_tools = [
        {
            "name": spec.name,
            "description": spec.description,
            "input_schema": spec.args_schema,
        }
        for spec in build_enabled_builtin_specs(config)
    ]

    # MCP ????????????????????????
    mcp_tools = [
        {
            "name": spec.name,
            "description": spec.description,
            "input_schema": spec.args_schema,
        }
        for spec in build_mcp_tool_specs(config)
    ]

    eva_skills = Path("EVA_SKILLS")
    scan_paths = list(config.skills.paths)
    if eva_skills.exists() and str(eva_skills) not in scan_paths:
        scan_paths.append(str(eva_skills))

    scan_config = config.model_copy(deep=True)
    scan_config.skills.paths = scan_paths
    registry = load_skills(scan_config, load_entrypoints=False, only_enabled=True)
    skills = []
    for spec in registry.list_specs():
        skills.append(
            {
                "name": spec.name,
                "description": spec.description,
                "input_schema": spec.input_schema or {},
            }
        )

    blocked_names = {tool["name"] for tool in builtin_tools + mcp_tools + skills}
    # ????????????????????????
    knowledge_tools = [
        {
            "name": spec.name,
            "description": spec.description,
            "input_schema": spec.args_schema,
        }
        for spec in build_knowledge_tool_specs_filtered(config, blocked_names=blocked_names)
    ]

    user_tools: List[Dict[str, Any]] = []
    shared_tools: List[Dict[str, Any]] = []
    extra_prompt = ""
    cleaned_user_id = str(user_id or "").strip()
    if cleaned_user_id:
        store = orchestrator.user_tool_store
        user_payload = store.load_user_tools(cleaned_user_id)
        extra_prompt = user_payload.extra_prompt or ""
        used_names = {
            tool["name"] for tool in builtin_tools + mcp_tools + skills + knowledge_tools
        }
        knowledge_schema = {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": t("knowledge.tool.query.description"),
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": t("knowledge.tool.limit.description"),
                },
            },
            "required": ["query"],
        }

        def _append_tool(
            bucket: List[Dict[str, Any]],
            owner_id: str,
            tool_name: str,
            description: str,
            input_schema: Dict[str, Any],
            include_owner: bool = False,
        ) -> None:
            alias = store.build_alias_name(owner_id, tool_name)
            if alias in used_names:
                return
            item: Dict[str, Any] = {
                "name": alias,
                "description": description,
                "input_schema": input_schema,
            }
            if include_owner:
                item["owner_id"] = owner_id
            used_names.add(alias)
            bucket.append(item)

        def _collect_mcp_tools(
            bucket: List[Dict[str, Any]],
            owner_id: str,
            servers: List[Any],
            shared_only: bool = False,
            include_owner: bool = False,
        ) -> None:
            for server in servers:
                server_name = str(getattr(server, "name", "") or "").strip()
                if not server_name or "@" in server_name:
                    continue
                if not getattr(server, "enabled", True):
                    continue
                cached_tools = getattr(server, "tool_specs", None)
                if not isinstance(cached_tools, list) or not cached_tools:
                    continue
                allow_tools = (
                    list(getattr(server, "allow_tools", []) or []) if server.allow_tools else []
                )
                shared_list = list(getattr(server, "shared_tools", []) or [])
                tool_pool = [
                    str(tool.get("name", "")).strip()
                    for tool in cached_tools
                    if str(tool.get("name", "")).strip()
                ]
                enabled_names = set(allow_tools or tool_pool)
                if shared_only:
                    enabled_names &= set(shared_list)
                for tool in cached_tools:
                    tool_name = str(tool.get("name", "")).strip()
                    if tool_name not in enabled_names:
                        continue
                    input_schema = normalize_mcp_input_schema(tool)
                    description = resolve_mcp_description(server, tool)
                    _append_tool(
                        bucket,
                        owner_id,
                        f"{server_name}@{tool_name}",
                        description,
                        input_schema,
                        include_owner=include_owner,
                    )

        def _collect_skill_tools(
            bucket: List[Dict[str, Any]],
            owner_id: str,
            enabled: set[str],
            shared: set[str],
            shared_only: bool = False,
            include_owner: bool = False,
        ) -> None:
            skill_root = store.get_skill_root(owner_id)
            if not skill_root.exists():
                return
            scan_config = config.model_copy(deep=True)
            scan_config.skills.paths = [str(skill_root)]
            scan_config.skills.enabled = []
            registry = load_skills(scan_config, load_entrypoints=False, only_enabled=False)
            for spec in registry.list_specs():
                if shared_only:
                    if spec.name not in shared:
                        continue
                else:
                    if spec.name not in enabled:
                        continue
                _append_tool(
                    bucket,
                    owner_id,
                    spec.name,
                    spec.description,
                    spec.input_schema or {},
                    include_owner=include_owner,
                )

        def _collect_knowledge_tools(
            bucket: List[Dict[str, Any]],
            owner_id: str,
            bases: List[Any],
            shared_only: bool = False,
            include_owner: bool = False,
        ) -> None:
            for base in bases:
                base_name = str(getattr(base, "name", "") or "").strip()
                if not base_name:
                    continue
                if shared_only and not getattr(base, "shared", False):
                    continue
                description = str(getattr(base, "description", "") or "").strip()
                if not description:
                    description = t("knowledge.tool.description", name=base_name)
                _append_tool(
                    bucket,
                    owner_id,
                    base_name,
                    description,
                    knowledge_schema,
                    include_owner=include_owner,
                )

        owner_id = user_payload.user_id or cleaned_user_id
        _collect_mcp_tools(user_tools, owner_id, user_payload.mcp_servers)
        _collect_skill_tools(
            user_tools,
            owner_id,
            set(user_payload.skills.enabled),
            set(user_payload.skills.shared),
        )
        _collect_knowledge_tools(user_tools, owner_id, user_payload.knowledge_bases)

        for shared_payload in store.list_shared_payloads(cleaned_user_id):
            shared_owner = shared_payload.user_id or cleaned_user_id
            _collect_mcp_tools(
                shared_tools,
                shared_owner,
                shared_payload.mcp_servers,
                shared_only=True,
                include_owner=True,
            )
            _collect_skill_tools(
                shared_tools,
                shared_owner,
                set(shared_payload.skills.enabled),
                set(shared_payload.skills.shared),
                shared_only=True,
                include_owner=True,
            )
            _collect_knowledge_tools(
                shared_tools,
                shared_owner,
                shared_payload.knowledge_bases,
                shared_only=True,
                include_owner=True,
            )

    return AvailableToolsResponse(
        builtin_tools=builtin_tools,
        mcp_tools=mcp_tools,
        skills=skills,
        knowledge_tools=knowledge_tools,
        user_tools=user_tools,
        shared_tools=shared_tools,
        extra_prompt=extra_prompt,
    )
