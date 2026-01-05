from datetime import datetime
import time
import asyncio
from pathlib import Path
from typing import Optional

import shutil
import tempfile
import zipfile
from fastapi import APIRouter, File, Form, HTTPException, Query, UploadFile

from app.api.deps import get_config_path, get_orchestrator, get_orchestrator_if_ready
from app.api.responses import json_response
from app.core.config import KnowledgeBaseConfig, WunderConfig, load_config
from app.core.i18n import get_language, t
from app.core.config_store import (
    update_a2a_services,
    update_builtin_tools,
    update_knowledge_config,
    update_llm_config,
    update_mcp_servers,
    update_skills,
)
from app.knowledge.converter import (
    convert_to_markdown,
    get_supported_extensions,
    resolve_unique_markdown_path,
    sanitize_filename_stem,
)
from app.knowledge.service import refresh_knowledge_cache
from app.llm.openai_compatible import probe_openai_context_window
from app.monitor.registry import monitor
from app.schemas.wunder import (
    A2aCardRequest,
    A2aCardResponse,
    A2aListResponse,
    A2aServiceItem,
    A2aUpdateRequest,
    BuiltinToolsResponse,
    BuiltinToolsUpdateRequest,
    KnowledgeActionResponse,
    KnowledgeConfigResponse,
    KnowledgeConfigUpdateRequest,
    KnowledgeFileResponse,
    KnowledgeFileUpdateRequest,
    KnowledgeFilesResponse,
    KnowledgeUploadResponse,
    LlmConfigItem,
    LlmConfigResponse,
    LlmConfigSet,
    LlmConfigUpdateRequest,
    LlmContextProbeRequest,
    LlmContextProbeResponse,
    McpListResponse,
    McpServerItem,
    McpToolsRequest,
    McpToolsResponse,
    McpUpdateRequest,
    MonitorCancelResponse,
    MonitorDeleteResponse,
    MonitorDetailResponse,
    MonitorListResponse,
    MonitorToolUsageResponse,
    MemoryActionResponse,
    MemoryEnabledUpdateRequest,
    MemoryQueueDetailResponse,
    MemoryQueueStatusResponse,
    MemoryRecordUpdateRequest,
    MemoryRecordsResponse,
    MemoryStatusResponse,
    MemoryUsersResponse,
    SkillContentResponse,
    SkillsDeleteResponse,
    SkillsListResponse,
    SkillsUploadResponse,
    SkillsUpdateRequest,
    UserDeleteResponse,
    UserSessionsResponse,
    UserStatsResponse,
)
from app.services.config_service import apply_config_update
from app.services.a2a_service import fetch_a2a_agent_card
from app.services.mcp_service import fetch_mcp_tools
from app.skills.loader import load_skills
from app.tools.catalog import build_builtin_tool_aliases, resolve_builtin_tool_name
from app.tools.constants import BUILTIN_TOOL_NAMES
from app.tools.specs import build_eva_tool_specs


router = APIRouter()


def _get_knowledge_base(config: WunderConfig, base_name: str) -> KnowledgeBaseConfig:
    """根据名称定位知识库配置，找不到时抛出异常。"""
    cleaned = base_name.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400,
            detail={"message": t("error.knowledge_base_name_required")},
        )
    for base in config.knowledge.bases:
        if base.name == cleaned:
            return base
    raise HTTPException(
        status_code=404, detail={"message": t("error.knowledge_base_not_found")}
    )


def _resolve_knowledge_root(base: KnowledgeBaseConfig, *, create: bool = False) -> Path:
    """解析知识库根目录，必要时创建。"""
    root = Path(base.root).resolve()
    if not root.exists():
        if create:
            root.mkdir(parents=True, exist_ok=True)
        else:
            raise HTTPException(
                status_code=404,
                detail={"message": t("error.knowledge_root_not_found")},
            )
    if not root.is_dir():
        raise HTTPException(
            status_code=400,
            detail={"message": t("error.knowledge_root_not_dir")},
        )
    return root


def _build_builtin_tools_payload(config: WunderConfig) -> tuple[list[str], list[dict]]:
    """按当前语言构建内置工具展示清单与启用列表。"""
    enabled = list(config.tools.builtin.enabled or [])
    enabled_set = set(enabled)
    specs = build_eva_tool_specs()
    aliases_by_name = build_builtin_tool_aliases()
    language = get_language()
    prefer_alias = language.startswith("en")
    tools = []
    for name in BUILTIN_TOOL_NAMES:
        spec = specs.get(name)
        if not spec:
            continue
        display_name = name
        if prefer_alias:
            aliases = aliases_by_name.get(name, ())
            if aliases:
                display_name = aliases[0]
        canonical_name = resolve_builtin_tool_name(display_name) or name
        tools.append(
            {
                "name": display_name,
                "description": spec.description,
                "input_schema": spec.args_schema,
                "enabled": canonical_name in enabled_set,
            }
        )
    enabled_display = [tool["name"] for tool in tools if tool.get("enabled")]
    return enabled_display, tools


def _build_builtin_tool_display_map() -> dict[str, str]:
    """按当前语言输出内置工具的展示名称映射。"""
    aliases_by_name = build_builtin_tool_aliases()
    prefer_alias = get_language().startswith("en")
    display_map: dict[str, str] = {}
    for name in BUILTIN_TOOL_NAMES:
        display_name = name
        if prefer_alias:
            aliases = aliases_by_name.get(name, ())
            if aliases:
                display_name = aliases[0]
        display_map[name] = display_name
    return display_map


def _normalize_tool_stats(tool_stats: list[dict]) -> list[dict]:
    """合并内置工具别名统计并按当前语言返回展示名称。"""
    display_map = _build_builtin_tool_display_map()
    merged: dict[str, int] = {}
    for item in tool_stats or []:
        raw_name = str(item.get("tool") or item.get("name") or "").strip()
        if not raw_name:
            continue
        try:
            calls = int(item.get("calls") or item.get("count") or item.get("tool_calls") or 0)
        except (TypeError, ValueError):
            calls = 0
        canonical = resolve_builtin_tool_name(raw_name)
        key = canonical if canonical in BUILTIN_TOOL_NAMES else raw_name
        merged[key] = merged.get(key, 0) + max(0, calls)
    if not merged:
        return []
    return [
        {"tool": display_map.get(name, name), "calls": count}
        for name, count in sorted(merged.items(), key=lambda item: item[1], reverse=True)
    ]


def _merge_tool_session_usage(records: list[dict]) -> list[dict]:
    """合并同一会话的工具调用统计，避免别名拆分。"""
    merged: dict[tuple[str, str], dict] = {}
    for record in records or []:
        session_id = str(record.get("session_id", "")).strip()
        if not session_id:
            continue
        user_id = str(record.get("user_id", "")).strip()
        key = (session_id, user_id)
        entry = merged.get(key)
        if not entry:
            entry = {
                "session_id": session_id,
                "user_id": user_id,
                "tool_calls": 0,
                "last_time": 0,
            }
            merged[key] = entry
        if not entry.get("user_id") and user_id:
            entry["user_id"] = user_id
        try:
            calls = int(record.get("tool_calls", 0) or 0)
        except (TypeError, ValueError):
            calls = 0
        entry["tool_calls"] = int(entry.get("tool_calls", 0) or 0) + max(0, calls)
        last_time = record.get("last_time") or 0
        if isinstance(last_time, (int, float)) and last_time > float(entry.get("last_time", 0) or 0):
            entry["last_time"] = float(last_time)
    return list(merged.values())


def _resolve_knowledge_path(root: Path, relative_path: str) -> Path:
    """解析知识库文件路径，阻止目录穿越。"""
    rel = Path(relative_path)
    if rel.is_absolute():
        raise HTTPException(
            status_code=400,
            detail={"message": t("error.absolute_path_forbidden")},
        )
    target = (root / rel).resolve()
    if target != root and root not in target.parents:
        raise HTTPException(
            status_code=400,
            detail={"message": t("error.path_out_of_bounds")},
        )
    return target


async def _save_upload_file(upload_file: UploadFile, target: Path) -> None:
    """将上传文件落盘，避免一次性读入内存。"""
    target.parent.mkdir(parents=True, exist_ok=True)
    chunk_size = 1024 * 1024
    with target.open("wb") as handle:
        while True:
            chunk = await upload_file.read(chunk_size)
            if not chunk:
                break
            handle.write(chunk)


@router.get("/wunder/admin/llm", response_model=LlmConfigResponse)
async def wunder_llm_get():
    """获取 LLM 配置。"""
    config = load_config(get_config_path())
    models = {
        name: LlmConfigItem(**item.model_dump())
        for name, item in (config.llm.models or {}).items()
    }
    llm = LlmConfigSet(default=config.llm.default, models=models)
    return json_response(LlmConfigResponse(llm=llm))


@router.post("/wunder/admin/llm", response_model=LlmConfigResponse)
async def wunder_llm_update(request: LlmConfigUpdateRequest):
    """更新 LLM 配置。"""
    try:
        updated = apply_config_update(
            get_orchestrator(),
            get_config_path(),
            update_llm_config,
            request.llm.model_dump(),
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc

    models = {
        name: LlmConfigItem(**item.model_dump())
        for name, item in (updated.llm.models or {}).items()
    }
    llm = LlmConfigSet(default=updated.llm.default, models=models)
    return json_response(LlmConfigResponse(llm=llm))


@router.post("/wunder/admin/llm/context_window", response_model=LlmContextProbeResponse)
async def wunder_llm_context_window(request: LlmContextProbeRequest):
    """探测模型最大上下文长度。"""
    base_url = request.base_url.strip()
    model = request.model.strip()
    if not base_url or not model:
        raise HTTPException(
            status_code=400,
            detail={"message": t("error.base_url_or_model_required")},
        )

    if request.provider != "openai_compatible":
        return json_response(
            LlmContextProbeResponse(
                max_context=None, message=t("probe.provider_unsupported")
            )
        )

    try:
        max_context = await probe_openai_context_window(
            base_url=base_url,
            api_key=request.api_key,
            model=model,
            timeout_s=request.timeout_s or 15,
        )
    except Exception as exc:
        return json_response(
            LlmContextProbeResponse(
                max_context=None, message=t("probe.failed", detail=str(exc))
            )
        )

    if not max_context:
        return json_response(
            LlmContextProbeResponse(
                max_context=None, message=t("probe.no_context")
            )
        )

    return json_response(
        LlmContextProbeResponse(max_context=max_context, message=t("probe.success"))
    )


@router.get("/wunder/admin/mcp", response_model=McpListResponse)
async def wunder_mcp_list():
    """获取 MCP 服务配置。"""
    config = load_config(get_config_path())
    servers = [
        McpServerItem(
            name=server.name,
            endpoint=server.endpoint,
            allow_tools=server.allow_tools,
            enabled=server.enabled,
            transport=server.transport,
            description=server.description,
            display_name=server.display_name,
            headers=server.headers,
            auth=server.auth,
            tool_specs=server.tool_specs,
        )
        for server in config.mcp.servers
    ]
    return json_response(McpListResponse(servers=servers))


@router.post("/wunder/admin/mcp", response_model=McpListResponse)
async def wunder_mcp_update(request: McpUpdateRequest):
    """更新 MCP 服务配置。"""
    try:
        updated = apply_config_update(
            get_orchestrator(),
            get_config_path(),
            update_mcp_servers,
            [server.model_dump() for server in request.servers],
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc

    servers = [
        McpServerItem(
            name=server.name,
            endpoint=server.endpoint,
            allow_tools=server.allow_tools,
            enabled=server.enabled,
            transport=server.transport,
            description=server.description,
            display_name=server.display_name,
            headers=server.headers,
            auth=server.auth,
            tool_specs=server.tool_specs,
        )
        for server in updated.mcp.servers
    ]
    return json_response(McpListResponse(servers=servers))


@router.post("/wunder/admin/mcp/tools", response_model=McpToolsResponse)
async def wunder_mcp_tools(request: McpToolsRequest):
    """连接 MCP 服务并列出工具清单。"""
    try:
        response = await fetch_mcp_tools(request)
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    return json_response(response)


@router.get("/wunder/admin/a2a", response_model=A2aListResponse)
async def wunder_a2a_list():
    """获取 A2A 服务配置。"""
    config = load_config(get_config_path())
    services = [
        A2aServiceItem(
            name=service.name,
            endpoint=service.endpoint,
            service_type=getattr(service, "service_type", "external"),
            user_id=getattr(service, "user_id", ""),
            enabled=service.enabled,
            description=service.description,
            display_name=service.display_name,
            headers=service.headers,
            auth=service.auth,
            agent_card=service.agent_card,
            allow_self=service.allow_self,
            max_depth=service.max_depth,
            default_method=service.default_method,
        )
        for service in config.a2a.services
    ]
    return json_response(A2aListResponse(services=services))


@router.post("/wunder/admin/a2a", response_model=A2aListResponse)
async def wunder_a2a_update(request: A2aUpdateRequest):
    """更新 A2A 服务配置。"""
    try:
        updated = apply_config_update(
            get_orchestrator(),
            get_config_path(),
            update_a2a_services,
            [service.model_dump() for service in request.services],
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc

    services = [
        A2aServiceItem(
            name=service.name,
            endpoint=service.endpoint,
            service_type=getattr(service, "service_type", "external"),
            user_id=getattr(service, "user_id", ""),
            enabled=service.enabled,
            description=service.description,
            display_name=service.display_name,
            headers=service.headers,
            auth=service.auth,
            agent_card=service.agent_card,
            allow_self=service.allow_self,
            max_depth=service.max_depth,
            default_method=service.default_method,
        )
        for service in updated.a2a.services
    ]
    return json_response(A2aListResponse(services=services))


@router.post("/wunder/admin/a2a/card", response_model=A2aCardResponse)
async def wunder_a2a_card(request: A2aCardRequest):
    """获取指定 A2A 服务的 AgentCard。"""
    config = load_config(get_config_path())
    timeout_s = int(getattr(config.a2a, "timeout_s", 120) or 120)
    try:
        card = await fetch_a2a_agent_card(
            request.endpoint,
            request.headers,
            request.auth,
            timeout_s,
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    return json_response(A2aCardResponse(agent_card=card))


@router.get("/wunder/admin/skills", response_model=SkillsListResponse)
async def wunder_skills_list():
    """获取技能清单与启用状态。"""
    config = load_config(get_config_path())
    eva_skills = Path("EVA_SKILLS")
    scan_paths = list(config.skills.paths)
    if eva_skills.exists() and str(eva_skills) not in scan_paths:
        scan_paths.append(str(eva_skills))

    scan_config = config.model_copy(deep=True)
    scan_config.skills.paths = scan_paths
    scan_config.skills.enabled = []
    registry = load_skills(scan_config, load_entrypoints=False, only_enabled=False)
    enabled = set(config.skills.enabled)

    skills = []
    for spec in registry.list_specs():
        skills.append(
            {
                "name": spec.name,
                "description": spec.description,
                "path": spec.path,
                "input_schema": spec.input_schema or {},
                "enabled": spec.name in enabled,
            }
        )

    response = SkillsListResponse(paths=scan_paths, enabled=list(enabled), skills=skills)
    return json_response(response)


@router.get("/wunder/admin/skills/content", response_model=SkillContentResponse)
async def wunder_skills_content(name: str = Query(..., description="技能名称")):
    """读取指定技能的 SKILL.md 内容。"""
    cleaned = name.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400, detail={"message": t("error.skill_name_required")}
        )
    config = load_config(get_config_path())
    eva_skills = Path("EVA_SKILLS")
    scan_paths = list(config.skills.paths)
    if eva_skills.exists() and str(eva_skills) not in scan_paths:
        scan_paths.append(str(eva_skills))

    scan_config = config.model_copy(deep=True)
    scan_config.skills.paths = scan_paths
    scan_config.skills.enabled = []
    registry = load_skills(scan_config, load_entrypoints=False, only_enabled=False)

    skill_spec = None
    for spec in registry.list_specs():
        if spec.name == cleaned:
            skill_spec = spec
            break
    if not skill_spec:
        raise HTTPException(status_code=404, detail={"message": t("error.skill_not_found")})
    skill_path = Path(skill_spec.path)
    if not skill_path.exists() or not skill_path.is_file():
        raise HTTPException(
            status_code=404, detail={"message": t("error.skill_file_not_found")}
        )
    try:
        content = skill_path.read_text(encoding="utf-8")
    except Exception as exc:
        raise HTTPException(
            status_code=400,
            detail={"message": t("error.skill_file_read_failed", detail=str(exc))},
        ) from exc
    response = SkillContentResponse(
        name=skill_spec.name,
        path=str(skill_path),
        content=content,
    )
    return json_response(response)


@router.post("/wunder/admin/skills", response_model=SkillsListResponse)
async def wunder_skills_update(request: SkillsUpdateRequest):
    """更新技能启用列表。"""
    try:
        updated = apply_config_update(
            get_orchestrator(),
            get_config_path(),
            update_skills,
            request.enabled,
            request.paths,
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc

    eva_skills = Path("EVA_SKILLS")
    scan_paths = list(updated.skills.paths)
    if eva_skills.exists() and str(eva_skills) not in scan_paths:
        scan_paths.append(str(eva_skills))

    scan_config = updated.model_copy(deep=True)
    scan_config.skills.paths = scan_paths
    scan_config.skills.enabled = []
    registry = load_skills(scan_config, load_entrypoints=False, only_enabled=False)
    enabled = set(updated.skills.enabled)

    skills = []
    for spec in registry.list_specs():
        skills.append(
            {
                "name": spec.name,
                "description": spec.description,
                "path": spec.path,
                "input_schema": spec.input_schema or {},
                "enabled": spec.name in enabled,
            }
        )
    response = SkillsListResponse(paths=scan_paths, enabled=list(enabled), skills=skills)
    return json_response(response)


@router.delete("/wunder/admin/skills", response_model=SkillsDeleteResponse)
async def wunder_skills_delete(name: str = Query(..., description="技能名称")):
    """删除 EVA_SKILLS 目录下的技能。"""
    cleaned = name.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400, detail={"message": t("error.skill_name_required")}
        )

    config = load_config(get_config_path())
    eva_skills = Path("EVA_SKILLS")
    eva_root = eva_skills.resolve()
    scan_paths = list(config.skills.paths)
    if eva_skills.exists() and str(eva_skills) not in scan_paths:
        scan_paths.append(str(eva_skills))

    scan_config = config.model_copy(deep=True)
    scan_config.skills.paths = scan_paths
    scan_config.skills.enabled = []
    registry = load_skills(scan_config, load_entrypoints=False, only_enabled=False)

    skill_spec = None
    for spec in registry.list_specs():
        if spec.name == cleaned:
            skill_spec = spec
            break
    if not skill_spec:
        raise HTTPException(status_code=404, detail={"message": t("error.skill_not_found")})

    skill_path = Path(skill_spec.path).resolve()
    if not skill_path.exists() or not skill_path.is_file():
        raise HTTPException(
            status_code=404, detail={"message": t("error.skill_file_not_found")}
        )

    skill_dir = skill_path.parent.resolve()
    if not eva_root.exists():
        raise HTTPException(
            status_code=400, detail={"message": t("error.skills_dir_missing")}
        )
    if skill_dir != eva_root and eva_root not in skill_dir.parents:
        raise HTTPException(
            status_code=400,
            detail={"message": t("error.skill_delete_restricted")},
        )

    try:
        shutil.rmtree(skill_dir)
    except OSError as exc:
        raise HTTPException(
            status_code=400,
            detail={"message": t("error.skill_delete_failed", detail=str(exc))},
        ) from exc

    enabled = [name for name in (config.skills.enabled or []) if name != cleaned]
    if enabled != list(config.skills.enabled or []):
        try:
            apply_config_update(
                get_orchestrator(),
                get_config_path(),
                update_skills,
                enabled,
                None,
            )
        except Exception as exc:
            raise HTTPException(
                status_code=400,
                detail={"message": t("error.skill_delete_update_failed", detail=str(exc))},
            ) from exc

    response = SkillsDeleteResponse(
        ok=True, name=cleaned, message=t("message.skill_deleted")
    )
    return json_response(response)


@router.post("/wunder/admin/skills/upload", response_model=SkillsUploadResponse)
async def wunder_skills_upload(file: UploadFile = File(...)):
    """上传技能压缩包并解压到 EVA_SKILLS。"""
    filename = file.filename or ""
    if not filename.lower().endswith(".zip"):
        raise HTTPException(
            status_code=400, detail={"message": t("error.skill_upload_zip_only")}
        )

    target_root = Path("EVA_SKILLS").resolve()
    target_root.mkdir(parents=True, exist_ok=True)
    extracted = 0

    try:
        file.file.seek(0)
        with zipfile.ZipFile(file.file) as zip_file:
            for info in zip_file.infolist():
                if info.is_dir():
                    continue
                raw_name = info.filename.replace("\\", "/")
                if not raw_name or raw_name.startswith("/") or raw_name.startswith("\\"):
                    raise HTTPException(
                        status_code=400, detail={"message": t("error.zip_path_invalid")}
                    )
                path = Path(raw_name)
                if any(part == ".." for part in path.parts):
                    raise HTTPException(
                        status_code=400, detail={"message": t("error.zip_path_illegal")}
                    )
                dest = (target_root / path).resolve()
                if dest != target_root and target_root not in dest.parents:
                    raise HTTPException(
                        status_code=400, detail={"message": t("error.zip_path_out_of_bounds")}
                    )
                dest.parent.mkdir(parents=True, exist_ok=True)
                with zip_file.open(info) as src, dest.open("wb") as dst:
                    shutil.copyfileobj(src, dst)
                extracted += 1
    except zipfile.BadZipFile as exc:
        raise HTTPException(
            status_code=400, detail={"message": t("error.zip_invalid")}
        ) from exc
    finally:
        await file.close()

    return json_response(
        SkillsUploadResponse(
            ok=True, extracted=extracted, message=t("message.upload_success")
        )
    )


@router.get("/wunder/admin/tools", response_model=BuiltinToolsResponse)
async def wunder_builtin_tools():
    """获取内置工具启用列表。"""
    config = load_config(get_config_path())
    enabled, tools = _build_builtin_tools_payload(config)
    return json_response(BuiltinToolsResponse(enabled=enabled, tools=tools))


@router.post("/wunder/admin/tools", response_model=BuiltinToolsResponse)
async def wunder_builtin_tools_update(request: BuiltinToolsUpdateRequest):
    """更新内置工具启用列表。"""
    try:
        updated = apply_config_update(
            get_orchestrator(),
            get_config_path(),
            update_builtin_tools,
            request.enabled,
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    enabled, tools = _build_builtin_tools_payload(updated)
    return json_response(BuiltinToolsResponse(enabled=enabled, tools=tools))


@router.get("/wunder/admin/knowledge", response_model=KnowledgeConfigResponse)
async def wunder_knowledge_get():
    """获取知识库配置。"""
    config = load_config(get_config_path())
    bases = [
        {
            "name": base.name,
            "description": base.description,
            "root": base.root,
            "enabled": base.enabled,
        }
        for base in config.knowledge.bases
    ]
    response = KnowledgeConfigResponse(knowledge={"bases": bases})
    return json_response(response)


@router.post("/wunder/admin/knowledge", response_model=KnowledgeConfigResponse)
async def wunder_knowledge_update(request: KnowledgeConfigUpdateRequest):
    """更新知识库配置。"""
    try:
        updated = apply_config_update(
            get_orchestrator(),
            get_config_path(),
            update_knowledge_config,
            request.knowledge.model_dump(),
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc

    bases = [
        {
            "name": base.name,
            "description": base.description,
            "root": base.root,
            "enabled": base.enabled,
        }
        for base in updated.knowledge.bases
    ]
    response = KnowledgeConfigResponse(knowledge={"bases": bases})
    return json_response(response)


@router.post("/wunder/admin/knowledge/upload", response_model=KnowledgeUploadResponse)
async def wunder_knowledge_upload(
    base: str = Form(..., description="知识库名称"),
    file: UploadFile = File(..., description="上传文件"),
):
    """上传文件并转换为 Markdown 保存到知识库。"""
    config = load_config(get_config_path())
    target = _get_knowledge_base(config, base)
    root = _resolve_knowledge_root(target, create=True)
    filename = file.filename or "upload"
    extension = Path(filename).suffix.lower()
    supported = set(get_supported_extensions())
    if not extension:
        raise HTTPException(
            status_code=400, detail={"message": t("error.file_extension_missing")}
        )
    if extension not in supported:
        raise HTTPException(
            status_code=400,
            detail={"message": t("error.unsupported_file_type", extension=extension)},
        )
    stem = sanitize_filename_stem(Path(filename).stem) or "document"
    output_path = resolve_unique_markdown_path(root, stem)
    try:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_path = Path(temp_dir) / f"{stem}{extension}"
            await _save_upload_file(file, temp_path)
            result = await asyncio.to_thread(
                convert_to_markdown, temp_path, output_path, extension
            )
    except Exception as exc:
        if output_path.exists():
            try:
                output_path.unlink()
            except OSError:
                pass
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    finally:
        await file.close()
    await refresh_knowledge_cache(
        KnowledgeBaseConfig(name=base, description=target.description, root=str(root))
    )
    relative_path = output_path.relative_to(root).as_posix()
    message = t("message.upload_converted")
    response = KnowledgeUploadResponse(
        ok=True,
        message=message,
        path=relative_path,
        converter=result.converter,
        warnings=result.warnings,
    )
    return json_response(response)


@router.get("/wunder/admin/knowledge/files", response_model=KnowledgeFilesResponse)
async def wunder_knowledge_files(
    base: str = Query(..., description="知识库名称"),
):
    """列出知识库目录下的 Markdown 文件。"""
    config = load_config(get_config_path())
    target = _get_knowledge_base(config, base)
    root = _resolve_knowledge_root(target, create=True)
    files = sorted(
        [
            path.relative_to(root).as_posix()
            for path in root.rglob("*.md")
            if path.is_file()
        ]
    )
    response = KnowledgeFilesResponse(base=base, files=files)
    return json_response(response)


@router.get("/wunder/admin/knowledge/file", response_model=KnowledgeFileResponse)
async def wunder_knowledge_file(
    base: str = Query(..., description="知识库名称"),
    path: str = Query(..., description="相对知识库根目录的路径"),
):
    """读取知识库文件内容。"""
    config = load_config(get_config_path())
    target = _get_knowledge_base(config, base)
    root = _resolve_knowledge_root(target, create=True)
    file_path = _resolve_knowledge_path(root, path)
    if file_path.suffix.lower() != ".md":
        raise HTTPException(
            status_code=400, detail={"message": t("error.markdown_only")}
        )
    if not file_path.exists() or not file_path.is_file():
        raise HTTPException(
            status_code=404, detail={"message": t("error.file_not_found")}
        )
    content = file_path.read_text(encoding="utf-8", errors="ignore")
    response = KnowledgeFileResponse(base=base, path=path, content=content)
    return json_response(response)


@router.put("/wunder/admin/knowledge/file", response_model=KnowledgeActionResponse)
async def wunder_knowledge_file_update(request: KnowledgeFileUpdateRequest):
    """保存知识库文件内容。"""
    config = load_config(get_config_path())
    target = _get_knowledge_base(config, request.base)
    root = _resolve_knowledge_root(target, create=True)
    file_path = _resolve_knowledge_path(root, request.path)
    if file_path.suffix.lower() != ".md":
        raise HTTPException(
            status_code=400, detail={"message": t("error.markdown_only")}
        )
    file_path.parent.mkdir(parents=True, exist_ok=True)
    file_path.write_text(request.content or "", encoding="utf-8")
    await refresh_knowledge_cache(
        KnowledgeBaseConfig(name=request.base, description="", root=str(root))
    )
    response = KnowledgeActionResponse(
        ok=True, message=t("message.saved_and_reindexed")
    )
    return json_response(response)


@router.delete("/wunder/admin/knowledge/file", response_model=KnowledgeActionResponse)
async def wunder_knowledge_file_delete(
    base: str = Query(..., description="知识库名称"),
    path: str = Query(..., description="相对知识库根目录的路径"),
):
    """删除知识库文件。"""
    config = load_config(get_config_path())
    target = _get_knowledge_base(config, base)
    root = _resolve_knowledge_root(target, create=True)
    file_path = _resolve_knowledge_path(root, path)
    if file_path.suffix.lower() != ".md":
        raise HTTPException(
            status_code=400, detail={"message": t("error.markdown_only")}
        )
    if file_path.exists() and file_path.is_file():
        file_path.unlink()
        await refresh_knowledge_cache(
            KnowledgeBaseConfig(name=base, description="", root=str(root))
        )
    response = KnowledgeActionResponse(ok=True, message=t("message.deleted"))
    return json_response(response)


@router.post("/wunder/admin/knowledge/refresh", response_model=KnowledgeActionResponse)
async def wunder_knowledge_refresh(
    base: str = Form(..., description="知识库名称"),
):
    """主动刷新知识库索引。"""
    config = load_config(get_config_path())
    target = _get_knowledge_base(config, base)
    root = _resolve_knowledge_root(target, create=True)
    await refresh_knowledge_cache(
        KnowledgeBaseConfig(name=base, description=target.description, root=str(root))
    )
    response = KnowledgeActionResponse(
        ok=True, message=t("message.index_refreshed")
    )
    return json_response(response)


@router.get("/wunder/admin/monitor", response_model=MonitorListResponse)
async def wunder_monitor(
    active_only: bool = Query(default=True),
    tool_hours: Optional[float] = Query(default=None),
    start_time: Optional[float] = Query(default=None, description="筛选开始时间戳（秒）"),
    end_time: Optional[float] = Query(default=None, description="筛选结束时间戳（秒）"),
):
    """获取系统监控信息与活动会话列表。"""
    # 触发监控历史后台加载，避免首次进入页面阻塞
    monitor.warm_history(background=True)
    system = monitor.get_system_metrics()
    sessions = monitor.list_sessions(active_only=active_only)
    # 工具调用热力图按时间窗口统计，便于在内部状态面板展示最新趋势
    since_time = None
    until_time = None
    recent_window_s = None
    service_now = None

    def _normalize_ts(value: Optional[float]) -> Optional[float]:
        """规范化时间戳输入，确保传入有效秒数。"""
        if not isinstance(value, (int, float)):
            return None
        parsed = float(value)
        return parsed if parsed > 0 else None

    start_ts = _normalize_ts(start_time)
    end_ts = _normalize_ts(end_time)
    if start_ts is not None and end_ts is not None and end_ts < start_ts:
        start_ts, end_ts = end_ts, start_ts
    if start_ts is not None or end_ts is not None:
        since_time = start_ts
        until_time = end_ts
        service_now = end_ts if end_ts is not None else time.time()
        if start_ts is not None:
            recent_window_s = max(0.0, service_now - start_ts)
    elif isinstance(tool_hours, (int, float)) and tool_hours > 0:
        recent_window_s = float(tool_hours) * 3600
        since_time = time.time() - recent_window_s
    service = monitor.get_service_metrics(recent_window_s, service_now)
    orchestrator = get_orchestrator_if_ready()
    tool_stats = []
    if orchestrator is not None:
        tool_stats = _normalize_tool_stats(
            orchestrator.workspace_manager.get_tool_usage_stats(since_time, until_time)
        )
    return json_response(
        MonitorListResponse(
            system=system,
            service=service,
            sandbox=monitor.get_sandbox_metrics(since_time, until_time),
            sessions=sessions,
            tool_stats=tool_stats,
        )
    )


@router.get("/wunder/admin/monitor/tool_usage", response_model=MonitorToolUsageResponse)
async def wunder_monitor_tool_usage(
    tool: str = Query(..., description="工具名称"),
    tool_hours: Optional[float] = Query(default=None),
    start_time: Optional[float] = Query(default=None, description="筛选开始时间戳（秒）"),
    end_time: Optional[float] = Query(default=None, description="筛选结束时间戳（秒）"),
):
    """获取指定工具的调用会话列表。"""
    cleaned = tool.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400, detail={"message": t("error.tool_name_required")}
        )

    def _normalize_ts(value: Optional[float]) -> Optional[float]:
        """规范化时间戳输入，避免无效值影响筛选。"""
        if not isinstance(value, (int, float)):
            return None
        parsed = float(value)
        return parsed if parsed > 0 else None

    start_ts = _normalize_ts(start_time)
    end_ts = _normalize_ts(end_time)
    if start_ts is not None and end_ts is not None and end_ts < start_ts:
        start_ts, end_ts = end_ts, start_ts

    since_time = None
    until_time = None
    if start_ts is not None or end_ts is not None:
        since_time = start_ts
        until_time = end_ts
    elif isinstance(tool_hours, (int, float)) and tool_hours > 0:
        since_time = time.time() - float(tool_hours) * 3600

    orchestrator = get_orchestrator_if_ready()
    if orchestrator is None:
        return json_response(MonitorToolUsageResponse(tool=cleaned, sessions=[]))
    canonical_name = resolve_builtin_tool_name(cleaned)
    # tool_name 保留为真实工具名，供前端定位工具事件
    tool_name = cleaned
    display_map = _build_builtin_tool_display_map()
    if canonical_name in BUILTIN_TOOL_NAMES:
        aliases = build_builtin_tool_aliases().get(canonical_name, ())
        names = [canonical_name]
        for alias in aliases:
            if alias and alias not in names:
                names.append(alias)
        usage_records: list[dict] = []
        for name in names:
            usage_records.extend(
                orchestrator.workspace_manager.get_tool_session_usage(
                    name, since_time, until_time
                )
            )
        usage_records = _merge_tool_session_usage(usage_records)
        display_name = display_map.get(canonical_name, canonical_name)
        tool_name = canonical_name
    else:
        usage_records = orchestrator.workspace_manager.get_tool_session_usage(
            cleaned, since_time, until_time
        )
        display_name = cleaned
    session_map = {
        session.get("session_id", ""): session
        for session in monitor.list_sessions(active_only=False)
    }
    sessions = []
    for record in usage_records:
        session_id = str(record.get("session_id", "")).strip()
        if not session_id:
            continue
        last_time = record.get("last_time") or 0
        last_time_text = ""
        if isinstance(last_time, (int, float)) and last_time > 0:
            last_time_text = datetime.utcfromtimestamp(float(last_time)).isoformat() + "Z"
        session_info = session_map.get(session_id) or {}
        # 用工具日志补齐会话元信息，保证热力图弹窗展示完整信息
        user_id = str(record.get("user_id") or session_info.get("user_id") or "").strip()
        sessions.append(
            {
                "session_id": session_id,
                "user_id": user_id,
                "question": session_info.get("question", ""),
                "status": session_info.get("status", "unknown"),
                "stage": session_info.get("stage", ""),
                "start_time": session_info.get("start_time", ""),
                "updated_time": session_info.get("updated_time", "") or last_time_text,
                "elapsed_s": float(session_info.get("elapsed_s", 0) or 0),
                "token_usage": int(session_info.get("token_usage", 0) or 0),
                "tool_calls": int(record.get("tool_calls", 0) or 0),
                "last_time": last_time_text,
            }
        )

    return json_response(
        MonitorToolUsageResponse(tool=display_name, tool_name=tool_name, sessions=sessions)
    )


@router.get("/wunder/admin/monitor/{session_id}", response_model=MonitorDetailResponse)
async def wunder_monitor_detail(session_id: str):
    """获取指定会话的运行详情。"""
    detail = monitor.get_detail(session_id)
    if not detail:
        raise HTTPException(
            status_code=404, detail={"message": t("error.session_not_found")}
        )
    return json_response(MonitorDetailResponse(**detail))


@router.post("/wunder/admin/monitor/{session_id}/cancel", response_model=MonitorCancelResponse)
async def wunder_monitor_cancel(session_id: str):
    """请求终止指定会话。"""
    ok = monitor.cancel(session_id)
    if not ok:
        return json_response(
            MonitorCancelResponse(
                ok=False, message=t("error.session_not_found_or_finished")
            )
        )
    return json_response(
        MonitorCancelResponse(ok=True, message=t("message.cancel_requested"))
    )


@router.delete("/wunder/admin/monitor/{session_id}", response_model=MonitorDeleteResponse)
async def wunder_monitor_delete(session_id: str):
    """删除指定历史会话。"""
    ok = monitor.delete_session(session_id)
    if not ok:
        return json_response(
            MonitorDeleteResponse(
                ok=False, message=t("error.session_not_found_or_running")
            )
        )
    return json_response(MonitorDeleteResponse(ok=True, message=t("message.deleted")))


@router.get("/wunder/admin/users", response_model=UserStatsResponse)
async def wunder_admin_users():
    """汇总用户历史对话与工具调用统计信息。"""
    # 触发监控历史后台加载，提升首次访问的响应速度
    monitor.warm_history(background=True)
    sessions = monitor.list_sessions(active_only=False)
    orchestrator = get_orchestrator_if_ready()
    usage_stats = {}
    if orchestrator is not None:
        usage_stats = orchestrator.workspace_manager.get_user_usage_stats()
    stats_map: dict[str, dict[str, int | str]] = {}
    active_statuses = {monitor.STATUS_RUNNING, monitor.STATUS_CANCELLING}

    def _ensure_entry(target_user_id: str) -> dict[str, int | str]:
        """初始化用户统计结构，避免字段遗漏。"""
        if target_user_id not in stats_map:
            stats_map[target_user_id] = {
                "user_id": target_user_id,
                "active_sessions": 0,
                "history_sessions": 0,
                "total_sessions": 0,
                "chat_records": 0,
                "tool_calls": 0,
                "token_usage": 0,
            }
        return stats_map[target_user_id]

    for session in sessions:
        user_id = str(session.get("user_id", "")).strip()
        if not user_id:
            continue
        entry = _ensure_entry(user_id)
        entry["total_sessions"] = int(entry["total_sessions"]) + 1
        # 汇总会话当前 token_usage，作为用户占用 Token 统计
        entry["token_usage"] = int(entry["token_usage"]) + int(session.get("token_usage", 0) or 0)
        if session.get("status") in active_statuses:
            entry["active_sessions"] = int(entry["active_sessions"]) + 1
        else:
            entry["history_sessions"] = int(entry["history_sessions"]) + 1

    for user_id, stats in usage_stats.items():
        entry = _ensure_entry(user_id)
        entry["chat_records"] = int(stats.get("chat_records", 0))
        entry["tool_calls"] = int(stats.get("tool_records", 0))

    users = sorted(
        stats_map.values(),
        key=lambda item: (
            -int(item.get("active_sessions", 0)),
            -int(item.get("total_sessions", 0)),
            str(item.get("user_id", "")),
        ),
    )
    return json_response(UserStatsResponse(users=users))


@router.get("/wunder/admin/memory/users", response_model=MemoryUsersResponse)
async def wunder_admin_memory_users():
    """汇总用户长期记忆开关与记录统计。"""
    monitor.warm_history(background=True)
    sessions = monitor.list_sessions(active_only=False)
    user_ids = {str(session.get("user_id", "")).strip() for session in sessions}
    user_ids.discard("")

    orchestrator = get_orchestrator()
    memory_store = orchestrator.memory_store
    settings = await memory_store.list_settings()
    record_stats = await memory_store.list_record_stats()

    # 合并所有来源的用户集合，确保新老用户都可被管理
    user_ids.update(settings.keys())
    user_ids.update(record_stats.keys())

    users = []
    for user_id in sorted(user_ids):
        setting = settings.get(user_id, {})
        stats = record_stats.get(user_id, {})
        last_ts = float(stats.get("last_time", 0) or 0)
        last_time = (
            datetime.utcfromtimestamp(last_ts).isoformat() + "Z" if last_ts > 0 else ""
        )
        users.append(
            {
                "user_id": user_id,
                "enabled": bool(setting.get("enabled", False)),
                "record_count": int(stats.get("record_count", 0) or 0),
                "last_updated_time": last_time,
                "last_updated_time_ts": last_ts,
            }
        )
    return json_response(MemoryUsersResponse(users=users))


@router.get("/wunder/admin/memory/status", response_model=MemoryQueueStatusResponse)
async def wunder_admin_memory_status():
    """获取长期记忆队列运行状态。"""
    orchestrator = get_orchestrator()
    status = await orchestrator.get_memory_queue_status()
    return json_response(MemoryQueueStatusResponse(**status))


@router.get(
    "/wunder/admin/memory/status/{task_id}",
    response_model=MemoryQueueDetailResponse,
)
async def wunder_admin_memory_status_detail(task_id: str):
    """获取指定长期记忆任务的详情。"""
    cleaned = task_id.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400, detail={"message": t("error.task_id_required")}
        )
    orchestrator = get_orchestrator()
    detail = await orchestrator.get_memory_queue_detail(cleaned)
    if not detail:
        raise HTTPException(
            status_code=404, detail={"message": t("error.task_not_found")}
        )
    return json_response(MemoryQueueDetailResponse(**detail))


@router.get("/wunder/admin/memory/{user_id}", response_model=MemoryRecordsResponse)
async def wunder_admin_memory_records(user_id: str):
    """获取指定用户的长期记忆记录列表。"""
    cleaned = user_id.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400, detail={"message": t("error.user_id_required")}
        )
    orchestrator = get_orchestrator()
    memory_store = orchestrator.memory_store
    enabled = await memory_store.is_enabled(cleaned)
    records = await memory_store.list_records(cleaned, order_desc=True)
    return json_response(
        MemoryRecordsResponse(
            user_id=cleaned,
            enabled=enabled,
            records=[record.to_dict() for record in records],
        )
    )


@router.put(
    "/wunder/admin/memory/{user_id}/{session_id}",
    response_model=MemoryActionResponse,
)
async def wunder_admin_memory_record_update(
    user_id: str, session_id: str, request: MemoryRecordUpdateRequest
):
    """更新指定会话的长期记忆内容。"""
    cleaned = user_id.strip()
    cleaned_session = session_id.strip()
    summary = str(request.summary or "").strip()
    if not cleaned or not cleaned_session:
        raise HTTPException(
            status_code=400, detail={"message": t("error.param_required")}
        )
    if not summary:
        raise HTTPException(
            status_code=400, detail={"message": t("error.content_required")}
        )
    orchestrator = get_orchestrator()
    ok = await orchestrator.memory_store.update_record(
        cleaned, cleaned_session, summary, now_ts=time.time()
    )
    if not ok:
        raise HTTPException(
            status_code=400, detail={"message": t("error.content_required")}
        )
    return json_response(
        MemoryActionResponse(ok=True, message=t("message.updated"), deleted=0)
    )


@router.post(
    "/wunder/admin/memory/{user_id}/enabled",
    response_model=MemoryStatusResponse,
)
async def wunder_admin_memory_enabled(
    user_id: str, request: MemoryEnabledUpdateRequest
):
    """更新指定用户的长期记忆开关。"""
    cleaned = user_id.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400, detail={"message": t("error.user_id_required")}
        )
    orchestrator = get_orchestrator()
    await orchestrator.memory_store.set_enabled(cleaned, request.enabled)
    return json_response(
        MemoryStatusResponse(user_id=cleaned, enabled=request.enabled)
    )


@router.delete(
    "/wunder/admin/memory/{user_id}/{session_id}",
    response_model=MemoryActionResponse,
)
async def wunder_admin_memory_record_delete(user_id: str, session_id: str):
    """删除指定会话的长期记忆记录。"""
    cleaned = user_id.strip()
    cleaned_session = session_id.strip()
    if not cleaned or not cleaned_session:
        raise HTTPException(
            status_code=400, detail={"message": t("error.param_required")}
        )
    orchestrator = get_orchestrator()
    deleted = await orchestrator.memory_store.delete_record(cleaned, cleaned_session)
    return json_response(
        MemoryActionResponse(ok=True, message=t("message.deleted"), deleted=deleted)
    )


@router.delete(
    "/wunder/admin/memory/{user_id}",
    response_model=MemoryActionResponse,
)
async def wunder_admin_memory_clear(user_id: str):
    """清空指定用户的长期记忆记录。"""
    cleaned = user_id.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400, detail={"message": t("error.user_id_required")}
        )
    orchestrator = get_orchestrator()
    deleted = await orchestrator.memory_store.clear_records(cleaned)
    return json_response(
        MemoryActionResponse(ok=True, message=t("message.cleared"), deleted=deleted)
    )


@router.get("/wunder/admin/users/{user_id}/sessions", response_model=UserSessionsResponse)
async def wunder_admin_user_sessions(
    user_id: str,
    active_only: bool = Query(default=False, description="是否仅返回活动线程"),
):
    """列出指定用户的历史会话。"""
    cleaned = user_id.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400, detail={"message": t("error.user_id_required")}
        )
    sessions = monitor.list_sessions(active_only=active_only)
    filtered = [session for session in sessions if session.get("user_id") == cleaned]
    return json_response(UserSessionsResponse(user_id=cleaned, sessions=filtered))


@router.delete("/wunder/admin/users/{user_id}", response_model=UserDeleteResponse)
async def wunder_admin_user_delete(user_id: str):
    """删除指定用户的活动线程、历史记录与工作区数据。"""
    cleaned = user_id.strip()
    if not cleaned:
        raise HTTPException(
            status_code=400, detail={"message": t("error.user_id_required")}
        )
    monitor_result = monitor.purge_user_sessions(cleaned)
    purge_result = get_orchestrator().workspace_manager.purge_user_data(cleaned)
    response = UserDeleteResponse(
        ok=True,
        message=t("message.user_deleted"),
        cancelled_sessions=monitor_result.get("cancelled", 0),
        deleted_sessions=monitor_result.get("deleted", 0),
        deleted_chat_records=purge_result.get("chat_records", 0),
        deleted_tool_records=purge_result.get("tool_records", 0),
        workspace_deleted=purge_result.get("workspace_deleted", False),
        legacy_history_deleted=purge_result.get("legacy_history_deleted", False),
    )
    return json_response(response)
