from datetime import datetime
from pathlib import Path
from typing import List, Optional

import shutil
import tempfile
import zipfile
from fastapi import APIRouter, BackgroundTasks, File, Form, HTTPException, Query, UploadFile
from fastapi.responses import FileResponse

from app.api.deps import get_orchestrator
from app.api.responses import json_response
from app.core.i18n import t
from app.schemas.wunder import (
    WorkspaceActionResponse,
    WorkspaceBatchRequest,
    WorkspaceBatchResponse,
    WorkspaceContentResponse,
    WorkspaceCopyRequest,
    WorkspaceDirRequest,
    WorkspaceFileUpdateRequest,
    WorkspaceListResponse,
    WorkspaceMoveRequest,
    WorkspaceSearchResponse,
)


router = APIRouter()


def _cleanup_archive(path: Path) -> None:
    """清理临时生成的工作区压缩包文件。"""
    try:
        path.unlink(missing_ok=True)
    except OSError:
        pass


def _write_archive_entries(zipf: zipfile.ZipFile, target: Path, base_root: Path) -> None:
    """将目标路径内容写入压缩包，保持相对路径结构。"""
    if target.is_file():
        rel_path = target.relative_to(base_root).as_posix()
        zipf.write(target, rel_path)
        return
    file_count = 0
    for file_path in target.rglob("*"):
        if not file_path.is_file():
            continue
        rel_path = file_path.relative_to(base_root).as_posix()
        zipf.write(file_path, rel_path)
        file_count += 1
    if file_count == 0:
        # 空目录也写入占位，避免解压后目录丢失
        dir_rel = target.relative_to(base_root).as_posix().rstrip("/") + "/"
        if dir_rel and dir_rel not in {".", "./"}:
            zipf.writestr(dir_rel, "")


@router.get("/wunder/workspace", response_model=WorkspaceListResponse)
async def wunder_workspace_list(
    user_id: str = Query(..., description="用户唯一标识"),
    path: str = Query(default="", description="相对路径"),
    refresh_tree: bool = Query(default=False, description="是否刷新工作区树缓存"),
    keyword: str = Query(default="", description="名称关键字过滤"),
    offset: int = Query(default=0, description="分页偏移量"),
    limit: int = Query(default=0, description="分页大小，0 表示不分页"),
    sort_by: str = Query(default="name", description="排序字段：name/size/updated_time"),
    order: str = Query(default="asc", description="排序方向：asc/desc"),
):
    """列出工作区目录内容。"""
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(user_id)
    normalized_path = path.replace("\\", "/").strip()
    target_path = normalized_path or "."
    if refresh_tree:
        manager.refresh_workspace_tree(user_id)
    try:
        entries, tree_version, current_path, parent, total = manager.list_workspace_entries(
            user_id,
            target_path,
            keyword=keyword,
            offset=offset,
            limit=limit,
            sort_by=sort_by,
            order=order,
        )
    except ValueError as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    response = WorkspaceListResponse(
        user_id=user_id,
        path=current_path,
        parent=parent,
        entries=entries,
        tree_version=tree_version,
        total=total,
        offset=max(int(offset or 0), 0),
        limit=max(int(limit or 0), 0),
    )
    return json_response(response)


@router.get("/wunder/workspace/content", response_model=WorkspaceContentResponse)
async def wunder_workspace_content(
    user_id: str = Query(..., description="用户唯一标识"),
    path: str = Query(default="", description="相对路径"),
    include_content: bool = Query(default=True, description="是否返回内容"),
    max_bytes: int = Query(default=512 * 1024, description="文件内容最大字节数"),
    depth: int = Query(default=1, description="目录内容展开深度"),
    keyword: str = Query(default="", description="名称关键字过滤"),
    offset: int = Query(default=0, description="分页偏移量"),
    limit: int = Query(default=0, description="分页大小，0 表示不分页"),
    sort_by: str = Query(default="name", description="排序字段：name/size/updated_time"),
    order: str = Query(default="asc", description="排序方向：asc/desc"),
):
    """获取工作区条目内容或目录列表。"""
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(user_id)
    normalized_path = path.replace("\\", "/").strip()
    target_path = normalized_path or "."
    try:
        target = manager.resolve_path(user_id, target_path)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    if not target.exists():
        raise HTTPException(
            status_code=404, detail={"message": t("workspace.error.path_not_found")}
        )

    stat = target.stat()
    updated_time = datetime.fromtimestamp(stat.st_mtime).isoformat()
    safe_depth = max(int(depth or 1), 1)
    safe_offset = max(int(offset or 0), 0)
    safe_limit = max(int(limit or 0), 0)

    if target.is_dir():
        entries, _, current_path, _, total = manager.list_workspace_entries(
            user_id,
            target_path,
            keyword=keyword,
            offset=safe_offset,
            limit=safe_limit,
            sort_by=sort_by,
            order=order,
        )

        def attach_children(items: List[dict], remaining_depth: int) -> None:
            if remaining_depth <= 1:
                return
            for item in items:
                if item.get("type") != "dir":
                    continue
                children, _, _, _, _ = manager.list_workspace_entries(
                    user_id,
                    item.get("path", ""),
                    sort_by=sort_by,
                    order=order,
                )
                item["children"] = children
                attach_children(children, remaining_depth - 1)

        if include_content and safe_depth > 1:
            attach_children(entries, safe_depth)

        response = WorkspaceContentResponse(
            user_id=user_id,
            path=current_path,
            type="dir",
            size=0,
            updated_time=updated_time,
            entries=entries if include_content else [],
            total=total,
            offset=safe_offset,
            limit=safe_limit,
            format="dir",
        )
        return json_response(response)

    content: Optional[str] = None
    truncated = False
    format_value = "text"
    if include_content:
        safe_max_bytes = max(int(max_bytes or 0), 0)
        with target.open("rb") as file:
            payload = file.read(safe_max_bytes + 1 if safe_max_bytes else -1)
        if safe_max_bytes and len(payload) > safe_max_bytes:
            truncated = True
            payload = payload[:safe_max_bytes]
        content = payload.decode("utf-8", errors="ignore")
    response = WorkspaceContentResponse(
        user_id=user_id,
        path=normalized_path,
        type="file",
        size=int(stat.st_size),
        updated_time=updated_time,
        content=content,
        truncated=truncated,
        format=format_value,
    )
    return json_response(response)


@router.get("/wunder/workspace/search", response_model=WorkspaceSearchResponse)
async def wunder_workspace_search(
    user_id: str = Query(..., description="用户唯一标识"),
    keyword: str = Query(..., description="搜索关键字"),
    offset: int = Query(default=0, description="分页偏移量"),
    limit: int = Query(default=100, description="分页大小"),
    include_files: bool = Query(default=True, description="是否包含文件"),
    include_dirs: bool = Query(default=True, description="是否包含目录"),
):
    """按名称搜索工作区条目。"""
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(user_id)
    results, total = manager.search_workspace_entries(
        user_id,
        keyword=keyword,
        offset=offset,
        limit=limit,
        include_files=include_files,
        include_dirs=include_dirs,
    )
    response = WorkspaceSearchResponse(
        user_id=user_id,
        keyword=keyword,
        entries=results,
        total=total,
        offset=max(int(offset or 0), 0),
        limit=max(int(limit or 0), 0),
    )
    return json_response(response)


@router.post("/wunder/workspace/upload", response_model=WorkspaceActionResponse)
async def wunder_workspace_upload(
    user_id: str = Form(..., description="用户唯一标识"),
    path: str = Form(default="", description="相对路径"),
    files: List[UploadFile] = File(...),
    relative_paths: List[str] = Form(default=[]),
):
    """上传文件到工作区。"""
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(user_id)
    normalized_path = path.replace("\\", "/").strip()
    target_path = normalized_path or "."
    try:
        target_dir = manager.resolve_path(user_id, target_path)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    if target_dir.exists() and not target_dir.is_dir():
        raise HTTPException(
            status_code=400, detail={"message": t("workspace.error.target_not_dir")}
        )
    target_dir.mkdir(parents=True, exist_ok=True)

    uploaded: List[str] = []
    for index, upload in enumerate(files):
        raw_path = ""
        if index < len(relative_paths):
            raw_path = str(relative_paths[index] or "").strip()
        if not raw_path:
            raw_path = upload.filename or ""
        normalized_relative = raw_path.replace("\\", "/").lstrip("/")
        if not normalized_relative:
            await upload.close()
            continue
        try:
            dest = manager.resolve_path(
                user_id, str(Path(target_path) / normalized_relative).replace("\\", "/")
            )
        except ValueError as exc:
            await upload.close()
            raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
        dest.parent.mkdir(parents=True, exist_ok=True)
        with dest.open("wb") as dst:
            shutil.copyfileobj(upload.file, dst)
        uploaded.append(dest.relative_to(manager.workspace_files_root(user_id)).as_posix())
        await upload.close()

    manager.refresh_workspace_tree(user_id)
    response = WorkspaceActionResponse(
        ok=True,
        message=t("message.upload_success"),
        tree_version=manager.get_tree_version(user_id),
        files=uploaded,
    )
    return json_response(response)


@router.post("/wunder/workspace/dir", response_model=WorkspaceActionResponse)
async def wunder_workspace_dir(request: WorkspaceDirRequest):
    """新建工作区目录。"""
    normalized_path = request.path.replace("\\", "/").strip()
    if not normalized_path or normalized_path in {".", "/"}:
        raise HTTPException(
            status_code=400, detail={"message": t("workspace.error.dir_path_required")}
        )
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(request.user_id)
    try:
        target_dir = manager.resolve_path(request.user_id, normalized_path)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    # 已存在文件时禁止创建目录，避免覆盖
    if target_dir.exists() and not target_dir.is_dir():
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.target_exists_not_dir")},
        )
    target_dir.mkdir(parents=True, exist_ok=True)

    manager.refresh_workspace_tree(request.user_id)
    response = WorkspaceActionResponse(
        ok=True,
        message=t("workspace.message.dir_created"),
        tree_version=manager.get_tree_version(request.user_id),
        files=[normalized_path],
    )
    return json_response(response)


@router.post("/wunder/workspace/move", response_model=WorkspaceActionResponse)
async def wunder_workspace_move(request: WorkspaceMoveRequest):
    """移动或重命名工作区条目。"""
    source = request.source.replace("\\", "/").strip()
    destination = request.destination.replace("\\", "/").strip()
    if not source or source in {".", "/"}:
        raise HTTPException(
            status_code=400, detail={"message": t("workspace.error.source_path_required")}
        )
    if not destination or destination in {".", "/"}:
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.destination_path_required")},
        )
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(request.user_id)
    if source == destination:
        response = WorkspaceActionResponse(
            ok=True,
            message=t("workspace.message.path_unchanged"),
            tree_version=manager.get_tree_version(request.user_id),
            files=[destination],
        )
        return json_response(response)
    try:
        source_path = manager.resolve_path(request.user_id, source)
        destination_path = manager.resolve_path(request.user_id, destination)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    if not source_path.exists():
        raise HTTPException(
            status_code=404, detail={"message": t("workspace.error.source_not_found")}
        )
    if destination_path.exists():
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.destination_exists")},
        )
    destination_parent = destination_path.parent
    if not destination_parent.exists() or not destination_parent.is_dir():
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.destination_parent_missing")},
        )
    # 禁止目录移动到自身或子目录，避免递归错误
    if source_path.is_dir():
        try:
            destination_path.relative_to(source_path)
        except ValueError:
            pass
        else:
            raise HTTPException(
                status_code=400,
                detail={"message": t("workspace.error.move_to_self_or_child")},
            )

    shutil.move(str(source_path), str(destination_path))
    manager.refresh_workspace_tree(request.user_id)
    response = WorkspaceActionResponse(
        ok=True,
        message=t("workspace.message.moved"),
        tree_version=manager.get_tree_version(request.user_id),
        files=[destination],
    )
    return json_response(response)


@router.post("/wunder/workspace/copy", response_model=WorkspaceActionResponse)
async def wunder_workspace_copy(request: WorkspaceCopyRequest):
    """复制工作区文件或目录。"""
    source = request.source.replace("\\", "/").strip()
    destination = request.destination.replace("\\", "/").strip()
    if not source or source in {".", "/"}:
        raise HTTPException(
            status_code=400, detail={"message": t("workspace.error.source_path_required")}
        )
    if not destination or destination in {".", "/"}:
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.destination_path_required")},
        )
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(request.user_id)
    if source == destination:
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.source_destination_same")},
        )
    try:
        source_path = manager.resolve_path(request.user_id, source)
        destination_path = manager.resolve_path(request.user_id, destination)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    if not source_path.exists():
        raise HTTPException(
            status_code=404, detail={"message": t("workspace.error.source_not_found")}
        )
    if destination_path.exists():
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.destination_exists")},
        )
    destination_parent = destination_path.parent
    if not destination_parent.exists() or not destination_parent.is_dir():
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.destination_parent_missing")},
        )
    if source_path.is_dir():
        try:
            destination_path.relative_to(source_path)
        except ValueError:
            pass
        else:
            raise HTTPException(
                status_code=400,
                detail={"message": t("workspace.error.copy_to_self_or_child")},
            )
        shutil.copytree(source_path, destination_path)
    else:
        shutil.copy2(source_path, destination_path)

    manager.refresh_workspace_tree(request.user_id)
    response = WorkspaceActionResponse(
        ok=True,
        message=t("workspace.message.copied"),
        tree_version=manager.get_tree_version(request.user_id),
        files=[destination],
    )
    return json_response(response)


@router.post("/wunder/workspace/batch", response_model=WorkspaceBatchResponse)
async def wunder_workspace_batch(request: WorkspaceBatchRequest):
    """批量处理工作区条目。"""
    action = request.action
    raw_paths = request.paths or []
    if not raw_paths:
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.batch_paths_missing")},
        )
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(request.user_id)

    destination_dir: Optional[str] = None
    destination_path: Optional[Path] = None
    if action in {"move", "copy"}:
        destination_dir = (request.destination or "").replace("\\", "/").strip()
        if destination_dir in {None, ""}:
            destination_dir = ""
        try:
            destination_path = manager.resolve_path(request.user_id, destination_dir or ".")
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
        if not destination_path.exists() or not destination_path.is_dir():
            raise HTTPException(
                status_code=400,
                detail={"message": t("workspace.error.destination_dir_missing")},
            )

    succeeded: List[str] = []
    failed: List[dict] = []

    for raw_path in raw_paths:
        normalized = str(raw_path or "").replace("\\", "/").strip()
        if not normalized or normalized in {".", "/"}:
            failed.append(
                {"path": normalized or "/", "message": t("workspace.error.path_required")}
            )
            continue
        try:
            source_path = manager.resolve_path(request.user_id, normalized)
        except ValueError as exc:
            failed.append({"path": normalized, "message": str(exc)})
            continue
        if not source_path.exists():
            failed.append(
                {"path": normalized, "message": t("workspace.error.path_not_found")}
            )
            continue

        if action == "delete":
            try:
                if source_path.is_dir():
                    shutil.rmtree(source_path)
                else:
                    source_path.unlink(missing_ok=True)
                succeeded.append(normalized)
            except OSError as exc:
                failed.append({"path": normalized, "message": str(exc)})
            continue

        if action not in {"move", "copy"}:
            failed.append(
                {
                    "path": normalized,
                    "message": t("workspace.error.batch_action_unsupported"),
                }
            )
            continue
        if not destination_path:
            failed.append(
                {"path": normalized, "message": t("workspace.error.destination_unready")}
            )
            continue
        entry_name = source_path.name
        target_path = destination_path / entry_name
        if target_path.exists():
            failed.append(
                {
                    "path": normalized,
                    "message": t("workspace.error.destination_exists"),
                }
            )
            continue
        if source_path.is_dir():
            try:
                target_path.relative_to(source_path)
            except ValueError:
                pass
            else:
                failed.append(
                    {
                        "path": normalized,
                        "message": t("workspace.error.move_to_self_or_child"),
                    }
                )
                continue
        try:
            if action == "move":
                shutil.move(str(source_path), str(target_path))
            else:
                if source_path.is_dir():
                    shutil.copytree(source_path, target_path)
                else:
                    shutil.copy2(source_path, target_path)
            succeeded.append(target_path.relative_to(manager.workspace_files_root(request.user_id)).as_posix())
        except OSError as exc:
            failed.append({"path": normalized, "message": str(exc)})

    manager.refresh_workspace_tree(request.user_id)
    ok = len(failed) == 0
    message = (
        t("workspace.message.batch_success")
        if ok
        else t("workspace.message.batch_partial")
    )
    response = WorkspaceBatchResponse(
        ok=ok,
        message=message,
        tree_version=manager.get_tree_version(request.user_id),
        succeeded=succeeded,
        failed=failed,
    )
    return json_response(response)


@router.post("/wunder/workspace/file", response_model=WorkspaceActionResponse)
async def wunder_workspace_file_update(request: WorkspaceFileUpdateRequest):
    """保存工作区文件内容。"""
    normalized_path = request.path.replace("\\", "/").strip()
    if not normalized_path:
        raise HTTPException(
            status_code=400, detail={"message": t("workspace.error.file_path_required")}
        )
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(request.user_id)
    try:
        target = manager.resolve_path(request.user_id, normalized_path)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    if target.exists():
        if not target.is_file():
            raise HTTPException(
                status_code=400,
                detail={"message": t("workspace.error.target_not_file")},
            )
    else:
        if not request.create_if_missing:
            raise HTTPException(
                status_code=404, detail={"message": t("error.file_not_found")}
            )
        # 仅允许在目标父目录存在时创建文件，避免隐式创建目录结构
        if not target.parent.exists() or not target.parent.is_dir():
            raise HTTPException(
                status_code=400,
                detail={"message": t("workspace.error.destination_parent_missing")},
            )
    target.write_text(request.content or "", encoding="utf-8")

    manager.refresh_workspace_tree(request.user_id)
    response = WorkspaceActionResponse(
        ok=True,
        message=t("workspace.message.file_saved"),
        tree_version=manager.get_tree_version(request.user_id),
        files=[normalized_path],
    )
    return json_response(response)


@router.get("/wunder/workspace/archive")
async def wunder_workspace_archive(
    background_tasks: BackgroundTasks,
    user_id: str = Query(..., description="用户唯一标识"),
    path: str = Query(default="", description="相对路径（可选，默认全量）"),
):
    """下载工作区全量或指定目录的压缩包。"""
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(user_id)
    root = manager.workspace_files_root(user_id)
    if not root.exists() or not root.is_dir():
        raise HTTPException(
            status_code=404, detail={"message": t("workspace.error.workspace_not_found")}
        )

    normalized_path = path.replace("\\", "/").strip()
    if not normalized_path or normalized_path in {".", "/"}:
        target = root
        base_root = root
        filename_prefix = f"workspace_{user_id}"
    else:
        try:
            target = manager.resolve_path(user_id, normalized_path)
        except ValueError as exc:
            raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
        if not target.exists():
            raise HTTPException(
                status_code=404,
                detail={"message": t("workspace.error.path_not_found")},
            )
        base_root = target.parent
        filename_prefix = target.name or f"workspace_{user_id}"

    # 生成临时压缩包文件，避免占用内存与工作区目录
    temp_file = tempfile.NamedTemporaryFile(prefix="wunder_workspace_", suffix=".zip", delete=False)
    archive_path = Path(temp_file.name)
    temp_file.close()

    try:
        # 逐个打包目标内容，保持相对路径结构
        with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as zipf:
            _write_archive_entries(zipf, target, base_root)
    except Exception as exc:
        _cleanup_archive(archive_path)
        raise HTTPException(status_code=500, detail={"message": str(exc)}) from exc

    filename = (
        filename_prefix if filename_prefix.lower().endswith(".zip") else f"{filename_prefix}.zip"
    )
    background_tasks.add_task(_cleanup_archive, archive_path)
    return FileResponse(archive_path, filename=filename, background=background_tasks)


@router.get("/wunder/workspace/download")
async def wunder_workspace_download(
    user_id: str = Query(..., description="用户唯一标识"),
    path: str = Query(..., description="相对路径"),
):
    """下载工作区文件。"""
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(user_id)
    normalized_path = path.replace("\\", "/").strip()
    if not normalized_path:
        raise HTTPException(
            status_code=400, detail={"message": t("workspace.error.path_required")}
        )
    try:
        target = manager.resolve_path(user_id, normalized_path)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    if not target.exists() or not target.is_file():
        raise HTTPException(
            status_code=404, detail={"message": t("error.file_not_found")}
        )
    return FileResponse(target, filename=target.name)


@router.delete("/wunder/workspace", response_model=WorkspaceActionResponse)
async def wunder_workspace_delete(
    user_id: str = Query(..., description="用户唯一标识"),
    path: str = Query(..., description="相对路径"),
):
    """删除工作区文件或目录。"""
    normalized_path = path.replace("\\", "/").strip()
    if not normalized_path or normalized_path in {".", "/"}:
        raise HTTPException(
            status_code=400,
            detail={"message": t("workspace.error.delete_root_forbidden")},
        )
    manager = get_orchestrator().workspace_manager
    manager.ensure_workspace(user_id)
    try:
        target = manager.resolve_path(user_id, normalized_path)
    except ValueError as exc:
        raise HTTPException(status_code=400, detail={"message": str(exc)}) from exc
    if not target.exists():
        raise HTTPException(
            status_code=404, detail={"message": t("workspace.error.path_not_found")}
        )
    if target.is_dir():
        shutil.rmtree(target)
    else:
        target.unlink(missing_ok=True)

    manager.refresh_workspace_tree(user_id)
    response = WorkspaceActionResponse(
        ok=True,
        message=t("message.deleted"),
        tree_version=manager.get_tree_version(user_id),
    )
    return json_response(response)
