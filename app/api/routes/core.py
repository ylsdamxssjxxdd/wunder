import asyncio
import tempfile
import time
from pathlib import Path
from typing import Optional

from fastapi import APIRouter, File, HTTPException, Query, UploadFile
from fastapi.responses import StreamingResponse

from app.api.deps import get_config_path, get_orchestrator
from app.api.responses import json_response
from app.core.config import load_config
from app.core.errors import ErrorCodes, WunderError
from app.knowledge.converter import (
    convert_to_markdown,
    get_supported_extensions,
    sanitize_filename_stem,
)
from app.schemas.wunder import (
    AvailableToolsResponse,
    AttachmentConvertResponse,
    WunderPromptRequest,
    WunderPromptResponse,
    WunderRequest,
    WunderResponse,
)
from app.services.tool_service import build_available_tools


router = APIRouter()


async def _save_upload_file(upload_file: UploadFile, target: Path) -> None:
    """将上传文件落盘，避免一次性读入内存导致占用过高。"""
    target.parent.mkdir(parents=True, exist_ok=True)
    chunk_size = 1024 * 1024
    with target.open("wb") as handle:
        while True:
            chunk = await upload_file.read(chunk_size)
            if not chunk:
                break
            handle.write(chunk)


@router.post("/wunder", response_model=WunderResponse)
async def wunder_endpoint(request: WunderRequest):
    """wunder 统一入口，支持流式与非流式响应。"""
    orchestrator = get_orchestrator()
    if request.stream:
        return StreamingResponse(
            orchestrator.sse_stream(request),
            media_type="text/event-stream; charset=utf-8",
        )
    try:
        result = await orchestrator.run(request)
    except WunderError as exc:
        status_code = 429 if exc.code == ErrorCodes.USER_BUSY else 400
        raise HTTPException(status_code=status_code, detail=exc.to_dict()) from exc
    return json_response(result)


@router.post("/wunder/attachments/convert", response_model=AttachmentConvertResponse)
async def wunder_attachment_convert(
    file: UploadFile = File(..., description="上传文件"),
):
    """上传附件并解析为 Markdown，供调试面板附加请求使用。"""
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
    try:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp_root = Path(temp_dir)
            input_path = temp_root / f"{stem}{extension}"
            output_path = temp_root / f"{stem}.md"
            await _save_upload_file(file, input_path)
            result = await asyncio.to_thread(
                convert_to_markdown, input_path, output_path, extension
            )
            content = output_path.read_text(encoding="utf-8", errors="ignore")
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    finally:
        await file.close()
    if not content.strip():
        raise HTTPException(status_code=400, detail={"message": "解析结果为空"})
    response = AttachmentConvertResponse(
        ok=True,
        name=filename,
        content=content,
        converter=result.converter,
        warnings=result.warnings,
    )
    return json_response(response)


@router.post("/wunder/system_prompt", response_model=WunderPromptResponse)
async def wunder_system_prompt(request: WunderPromptRequest):
    """获取系统提示词，用于前端调试展示。"""
    orchestrator = get_orchestrator()
    # 统计系统提示词构建耗时，便于前端展示性能数据
    start_time = time.perf_counter()
    try:
        prompt = await orchestrator.get_system_prompt(
            request.user_id, request.config_overrides, request.tool_names
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    build_time_ms = (time.perf_counter() - start_time) * 1000
    return json_response(
        WunderPromptResponse(prompt=prompt, build_time_ms=round(build_time_ms, 2))
    )


@router.get("/wunder/tools", response_model=AvailableToolsResponse)
async def wunder_tools_list(
    user_id: Optional[str] = Query(default=None, description="用户唯一标识"),
):
    """列出对开发者可见的工具清单。"""
    config = load_config(get_config_path())
    orchestrator = get_orchestrator()
    response = build_available_tools(config, orchestrator, user_id=user_id)
    return json_response(response)
