import asyncio
import tempfile
from pathlib import Path
from typing import List

import shutil
import zipfile
from fastapi import APIRouter, File, Form, HTTPException, Query, UploadFile

from app.api.deps import get_config_path, get_orchestrator
from app.api.responses import json_response
from app.core.config import KnowledgeBaseConfig, load_config
from app.knowledge.converter import (
    convert_to_markdown,
    get_supported_extensions,
    resolve_unique_markdown_path,
    sanitize_filename_stem,
)
from app.knowledge.service import refresh_knowledge_cache
from app.schemas.wunder import (
    KnowledgeActionResponse,
    KnowledgeFileResponse,
    KnowledgeFilesResponse,
    KnowledgeUploadResponse,
    McpToolsRequest,
    McpToolsResponse,
    SkillContentResponse,
    SkillsUploadResponse,
    UserExtraPromptResponse,
    UserExtraPromptUpdateRequest,
    UserKnowledgeConfigResponse,
    UserKnowledgeConfigUpdateRequest,
    UserKnowledgeFileUpdateRequest,
    UserMcpListResponse,
    UserMcpServerItem,
    UserMcpUpdateRequest,
    UserSkillsResponse,
    UserSkillsUpdateRequest,
)
from app.services.mcp_service import fetch_mcp_tools
from app.skills.loader import load_skills


router = APIRouter()


def _resolve_knowledge_path(root: Path, relative_path: str) -> Path:
    """解析知识库文件路径，阻止目录穿越。"""
    rel = Path(relative_path)
    if rel.is_absolute():
        raise HTTPException(status_code=400, detail={"message": "不允许使用绝对路径"})
    target = (root / rel).resolve()
    if target != root and root not in target.parents:
        raise HTTPException(status_code=400, detail={"message": "路径越界访问被禁止"})
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


@router.get("/wunder/user_tools/mcp", response_model=UserMcpListResponse)
async def wunder_user_mcp_list(user_id: str = Query(..., description="用户唯一标识")):
    """获取用户 MCP 服务配置。"""
    cleaned = str(user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    payload = get_orchestrator().user_tool_store.load_user_tools(cleaned)
    servers = [
        UserMcpServerItem(
            name=server.name,
            endpoint=server.endpoint,
            allow_tools=server.allow_tools,
            shared_tools=server.shared_tools,
            enabled=server.enabled,
            transport=server.transport or None,
            description=server.description,
            display_name=server.display_name,
            headers=server.headers,
            auth=server.auth or None,
            tool_specs=server.tool_specs,
        )
        for server in payload.mcp_servers
    ]
    return json_response(UserMcpListResponse(servers=servers))


@router.post("/wunder/user_tools/mcp", response_model=UserMcpListResponse)
async def wunder_user_mcp_update(request: UserMcpUpdateRequest):
    """更新用户 MCP 服务配置。"""
    cleaned = str(request.user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    raw_servers = [server.model_dump() for server in request.servers or []]
    payload = get_orchestrator().user_tool_store.update_mcp_servers(cleaned, raw_servers)
    servers = [
        UserMcpServerItem(
            name=server.name,
            endpoint=server.endpoint,
            allow_tools=server.allow_tools,
            shared_tools=server.shared_tools,
            enabled=server.enabled,
            transport=server.transport or None,
            description=server.description,
            display_name=server.display_name,
            headers=server.headers,
            auth=server.auth or None,
            tool_specs=server.tool_specs,
        )
        for server in payload.mcp_servers
    ]
    return json_response(UserMcpListResponse(servers=servers))


@router.post("/wunder/user_tools/mcp/tools", response_model=McpToolsResponse)
async def wunder_user_mcp_tools(request: McpToolsRequest):
    """连接用户 MCP 服务并列出工具清单。"""
    try:
        response = await fetch_mcp_tools(request)
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    return json_response(response)


@router.get("/wunder/user_tools/skills", response_model=UserSkillsResponse)
async def wunder_user_skills_list(user_id: str = Query(..., description="用户唯一标识")):
    """获取用户技能清单与启用状态。"""
    cleaned = str(user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    orchestrator = get_orchestrator()
    payload = orchestrator.user_tool_store.load_user_tools(cleaned)
    skill_root = orchestrator.user_tool_store.get_skill_root(cleaned)
    scan_config = load_config(get_config_path())
    scan_config = scan_config.model_copy(deep=True)
    scan_config.skills.paths = [str(skill_root)]
    scan_config.skills.enabled = []
    registry = load_skills(scan_config, load_entrypoints=False, only_enabled=False)
    enabled_set = set(payload.skills.enabled)
    shared_set = set(payload.skills.shared)

    skills = []
    for spec in registry.list_specs():
        skills.append(
            {
                "name": spec.name,
                "description": spec.description,
                "path": spec.path,
                "input_schema": spec.input_schema or {},
                "enabled": spec.name in enabled_set,
                "shared": spec.name in shared_set,
            }
        )
    response = UserSkillsResponse(
        enabled=list(enabled_set),
        shared=list(shared_set),
        skills=skills,
    )
    return json_response(response)


@router.get("/wunder/user_tools/skills/content", response_model=SkillContentResponse)
async def wunder_user_skills_content(
    user_id: str = Query(..., description="用户唯一标识"),
    name: str = Query(..., description="技能名称"),
):
    """读取用户技能的 SKILL.md 内容。"""
    cleaned = str(user_id or "").strip()
    name = name.strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    if not name:
        raise HTTPException(status_code=400, detail={"message": "技能名称不能为空"})
    orchestrator = get_orchestrator()
    skill_root = orchestrator.user_tool_store.get_skill_root(cleaned)
    scan_config = load_config(get_config_path())
    scan_config = scan_config.model_copy(deep=True)
    scan_config.skills.paths = [str(skill_root)]
    scan_config.skills.enabled = []
    registry = load_skills(scan_config, load_entrypoints=False, only_enabled=False)

    skill_spec = None
    for spec in registry.list_specs():
        if spec.name == name:
            skill_spec = spec
            break
    if not skill_spec:
        raise HTTPException(status_code=404, detail={"message": "技能不存在"})
    skill_path = Path(skill_spec.path)
    if not skill_path.exists() or not skill_path.is_file():
        raise HTTPException(status_code=404, detail={"message": "技能文件不存在"})
    try:
        content = skill_path.read_text(encoding="utf-8")
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": f"读取技能文件失败: {exc}"}) from exc

    response = SkillContentResponse(
        name=skill_spec.name,
        path=str(skill_path),
        content=content,
    )
    return json_response(response)


@router.post("/wunder/user_tools/skills", response_model=UserSkillsResponse)
async def wunder_user_skills_update(request: UserSkillsUpdateRequest):
    """更新用户技能启用状态。"""
    cleaned = str(request.user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    orchestrator = get_orchestrator()
    payload = orchestrator.user_tool_store.update_skills(
        cleaned, request.enabled, request.shared
    )
    # 技能配置变更后清理缓存，确保提示词与技能清单即时刷新
    orchestrator.user_tool_manager.clear_skill_cache(cleaned)
    skill_root = orchestrator.user_tool_store.get_skill_root(cleaned)
    scan_config = load_config(get_config_path())
    scan_config = scan_config.model_copy(deep=True)
    scan_config.skills.paths = [str(skill_root)]
    scan_config.skills.enabled = []
    registry = load_skills(scan_config, load_entrypoints=False, only_enabled=False)
    enabled_set = set(payload.skills.enabled)
    shared_set = set(payload.skills.shared)

    skills = []
    for spec in registry.list_specs():
        skills.append(
            {
                "name": spec.name,
                "description": spec.description,
                "path": spec.path,
                "input_schema": spec.input_schema or {},
                "enabled": spec.name in enabled_set,
                "shared": spec.name in shared_set,
            }
        )
    response = UserSkillsResponse(
        enabled=list(enabled_set),
        shared=list(shared_set),
        skills=skills,
    )
    return json_response(response)


@router.post("/wunder/user_tools/skills/upload", response_model=SkillsUploadResponse)
async def wunder_user_skills_upload(
    user_id: str = Form(..., description="用户唯一标识"),
    file: UploadFile = File(...),
):
    """上传技能压缩包并解压到用户技能目录。"""
    cleaned = str(user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    filename = file.filename or ""
    if not filename.lower().endswith(".zip"):
        raise HTTPException(status_code=400, detail={"message": "仅支持上传 .zip 压缩包"})

    target_root = get_orchestrator().user_tool_store.get_skill_root(cleaned).resolve()
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
                    raise HTTPException(status_code=400, detail={"message": "压缩包路径非法"})
                path = Path(raw_name)
                if any(part == ".." for part in path.parts):
                    raise HTTPException(status_code=400, detail={"message": "压缩包包含非法路径"})
                dest = (target_root / path).resolve()
                if dest != target_root and target_root not in dest.parents:
                    raise HTTPException(status_code=400, detail={"message": "压缩包包含越界路径"})
                dest.parent.mkdir(parents=True, exist_ok=True)
                with zip_file.open(info) as src, dest.open("wb") as dst:
                    shutil.copyfileobj(src, dst)
                extracted += 1
    except zipfile.BadZipFile as exc:
        raise HTTPException(status_code=400, detail={"message": "压缩包格式错误"}) from exc
    finally:
        await file.close()

    # 上传技能包会改变技能目录内容，清理缓存避免读取旧的技能信息
    get_orchestrator().user_tool_manager.clear_skill_cache(cleaned)
    return json_response(
        SkillsUploadResponse(ok=True, extracted=extracted, message="上传成功")
    )


@router.get("/wunder/user_tools/knowledge", response_model=UserKnowledgeConfigResponse)
async def wunder_user_knowledge_get(user_id: str = Query(..., description="用户唯一标识")):
    """获取用户知识库配置。"""
    cleaned = str(user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    payload = get_orchestrator().user_tool_store.load_user_tools(cleaned)
    bases = []
    for base in payload.knowledge_bases:
        root = ""
        if base.name:
            try:
                root = str(
                    get_orchestrator().user_tool_store.resolve_knowledge_base_root(
                        cleaned, base.name
                    )
                )
            except Exception:
                root = ""
        bases.append(
            {
                "name": base.name,
                "description": base.description,
                "root": root,
                "enabled": base.enabled,
                "shared": bool(base.shared),
            }
        )
    response = UserKnowledgeConfigResponse(knowledge={"bases": bases})
    return json_response(response)


@router.post("/wunder/user_tools/knowledge", response_model=UserKnowledgeConfigResponse)
async def wunder_user_knowledge_update(request: UserKnowledgeConfigUpdateRequest):
    """更新用户知识库配置。"""
    cleaned = str(request.user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    raw_bases = [base.model_dump() for base in request.knowledge.bases or []]
    payload = get_orchestrator().user_tool_store.update_knowledge_bases(cleaned, raw_bases)
    bases = []
    for base in payload.knowledge_bases:
        root = ""
        if base.name:
            try:
                root = str(
                    get_orchestrator().user_tool_store.resolve_knowledge_base_root(
                        cleaned, base.name, create=True
                    )
                )
            except Exception:
                root = ""
        bases.append(
            {
                "name": base.name,
                "description": base.description,
                "root": root,
                "enabled": base.enabled,
                "shared": bool(base.shared),
            }
        )
    response = UserKnowledgeConfigResponse(knowledge={"bases": bases})
    return json_response(response)


@router.post("/wunder/user_tools/knowledge/upload", response_model=KnowledgeUploadResponse)
async def wunder_user_knowledge_upload(
    user_id: str = Form(..., description="用户唯一标识"),
    base: str = Form(..., description="知识库名称"),
    file: UploadFile = File(..., description="上传文件"),
):
    """上传文件并转换为 Markdown 保存到用户知识库。"""
    cleaned = str(user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    base_name = str(base or "").strip()
    if not base_name:
        raise HTTPException(status_code=400, detail={"message": "知识库名称不能为空"})
    try:
        root = get_orchestrator().user_tool_store.resolve_knowledge_base_root(
            cleaned, base_name, create=True
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    filename = file.filename or "upload"
    extension = Path(filename).suffix.lower()
    supported = set(get_supported_extensions())
    if not extension:
        raise HTTPException(status_code=400, detail={"message": "文件缺少扩展名"})
    if extension not in supported:
        raise HTTPException(
            status_code=400,
            detail={"message": f"不支持的文件类型: {extension}"},
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
        KnowledgeBaseConfig(name=base_name, description="", root=str(root))
    )
    relative_path = output_path.relative_to(root).as_posix()
    response = KnowledgeUploadResponse(
        ok=True,
        message="上传并转换完成",
        path=relative_path,
        converter=result.converter,
        warnings=result.warnings,
    )
    return json_response(response)


@router.get("/wunder/user_tools/knowledge/files", response_model=KnowledgeFilesResponse)
async def wunder_user_knowledge_files(
    user_id: str = Query(..., description="用户唯一标识"),
    base: str = Query(..., description="知识库名称"),
):
    """列出用户知识库目录下的 Markdown 文件。"""
    cleaned = str(user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    try:
        root = get_orchestrator().user_tool_store.resolve_knowledge_base_root(cleaned, base)
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    if not root.exists() or not root.is_dir():
        response = KnowledgeFilesResponse(base=base, files=[])
        return json_response(response)
    files = sorted(
        [
            path.relative_to(root).as_posix()
            for path in root.rglob("*.md")
            if path.is_file()
        ]
    )
    response = KnowledgeFilesResponse(base=base, files=files)
    return json_response(response)


@router.get("/wunder/user_tools/knowledge/file", response_model=KnowledgeFileResponse)
async def wunder_user_knowledge_file(
    user_id: str = Query(..., description="用户唯一标识"),
    base: str = Query(..., description="知识库名称"),
    path: str = Query(..., description="相对知识库根目录的路径"),
):
    """读取用户知识库文件内容。"""
    cleaned = str(user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    try:
        root = get_orchestrator().user_tool_store.resolve_knowledge_base_root(cleaned, base)
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    target = _resolve_knowledge_path(root, path)
    if target.suffix.lower() != ".md":
        raise HTTPException(status_code=400, detail={"message": "仅支持 Markdown 文件"})
    if not target.exists() or not target.is_file():
        raise HTTPException(status_code=404, detail={"message": "文件不存在"})
    content = target.read_text(encoding="utf-8", errors="ignore")
    response = KnowledgeFileResponse(base=base, path=path, content=content)
    return json_response(response)


@router.put("/wunder/user_tools/knowledge/file", response_model=KnowledgeActionResponse)
async def wunder_user_knowledge_file_update(request: UserKnowledgeFileUpdateRequest):
    """保存用户知识库文件内容。"""
    cleaned = str(request.user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    try:
        root = get_orchestrator().user_tool_store.resolve_knowledge_base_root(
            cleaned, request.base, create=True
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    target = _resolve_knowledge_path(root, request.path)
    if target.suffix.lower() != ".md":
        raise HTTPException(status_code=400, detail={"message": "仅支持 Markdown 文件"})
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(request.content or "", encoding="utf-8")
    await refresh_knowledge_cache(
        KnowledgeBaseConfig(name=request.base, description="", root=str(root))
    )
    response = KnowledgeActionResponse(ok=True, message="已保存并刷新索引")
    return json_response(response)


@router.delete("/wunder/user_tools/knowledge/file", response_model=KnowledgeActionResponse)
async def wunder_user_knowledge_file_delete(
    user_id: str = Query(..., description="用户唯一标识"),
    base: str = Query(..., description="知识库名称"),
    path: str = Query(..., description="相对知识库根目录的路径"),
):
    """删除用户知识库文件。"""
    cleaned = str(user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    try:
        root = get_orchestrator().user_tool_store.resolve_knowledge_base_root(cleaned, base)
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    target = _resolve_knowledge_path(root, path)
    if target.suffix.lower() != ".md":
        raise HTTPException(status_code=400, detail={"message": "仅支持 Markdown 文件"})
    if target.exists() and target.is_file():
        target.unlink()
        await refresh_knowledge_cache(
            KnowledgeBaseConfig(name=base, description="", root=str(root))
        )
    response = KnowledgeActionResponse(ok=True, message="已删除")
    return json_response(response)


@router.post("/wunder/user_tools/extra_prompt", response_model=UserExtraPromptResponse)
async def wunder_user_extra_prompt_update(request: UserExtraPromptUpdateRequest):
    """更新用户附加提示词。"""
    cleaned = str(request.user_id or "").strip()
    if not cleaned:
        raise HTTPException(status_code=400, detail={"message": "用户不能为空"})
    payload = get_orchestrator().user_tool_store.update_extra_prompt(
        cleaned, str(request.extra_prompt or "")
    )
    response = UserExtraPromptResponse(
        user_id=cleaned,
        extra_prompt=payload.extra_prompt or "",
    )
    return json_response(response)
