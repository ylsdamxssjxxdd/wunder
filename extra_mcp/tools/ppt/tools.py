from __future__ import annotations

import time
from typing import Annotated, Any, Literal

from mcp.server.fastmcp import FastMCP
from pydantic import Field

from ...common.async_utils import run_in_thread
from .config import get_ppt_config
from .model import (
    PresentationManifest,
    SlideSpec,
    make_presentation_id,
    merge_refine_slide,
    normalize_slide_ids,
    parse_slides_payload,
)
from .render import render_manifest, summarize_pptx
from .storage import (
    create_presentation_dir,
    load_manifest,
    output_metadata_to_manifest,
    resolve_readable_pptx_path,
    resolve_output_path,
    save_manifest,
    sanitize_presentation_id,
)
from .templates import (
    builtin_template_summary,
    is_builtin_template,
    list_builtin_templates,
    normalize_template_id,
)


def _error_response(exc: Exception) -> dict[str, Any]:
    return {"ok": False, "error": str(exc)}


def _slide_summary(slide: SlideSpec, index: int) -> dict[str, Any]:
    return {
        "slide_id": slide.slide_id,
        "index": index,
        "type": slide.slide_type,
        "title": slide.title,
    }


def _manifest_response(manifest: PresentationManifest, elapsed_ms: float, warnings: list[str] | None = None) -> dict[str, Any]:
    return {
        "ok": True,
        "presentation_id": manifest.presentation_id,
        "presentation_name": manifest.presentation_name,
        "template_id": manifest.theme.get("template_id") or "",
        "path": manifest.public_path or manifest.output_path,
        "output_path": manifest.output_path,
        "workspace_relative_path": manifest.workspace_relative_path,
        "slide_count": len(manifest.slides),
        "slides": [_slide_summary(slide, idx) for idx, slide in enumerate(manifest.slides, start=1)],
        "warnings": warnings or [],
        "elapsed_ms": elapsed_ms,
    }


def _write_sync(
    *,
    presentation_id: str,
    presentation_name: str,
    insert_before: str,
    content: str,
    lang: str,
    template_id: str,
    output_path: str,
    overwrite: bool,
) -> dict[str, Any]:
    start = time.perf_counter()
    config = get_ppt_config()
    new_slides = parse_slides_payload(content, lang)
    if presentation_id.strip():
        manifest = load_manifest(config, presentation_id)
        if presentation_name.strip():
            manifest.presentation_name = presentation_name.strip()
        if template_id.strip():
            manifest.theme["template_id"] = normalize_template_id(template_id)
        existing = list(manifest.slides)
        if insert_before.strip():
            target_id = insert_before.strip()
            try:
                insert_at = next(idx for idx, slide in enumerate(existing) if slide.slide_id == target_id)
            except StopIteration as exc:
                raise ValueError(f"insert_before slide_id not found: {target_id}") from exc
            existing[insert_at:insert_at] = new_slides
        else:
            existing.extend(new_slides)
        manifest.slides = normalize_slide_ids(existing)
    else:
        pid = make_presentation_id()
        create_presentation_dir(config, pid)
        manifest = PresentationManifest(
            presentation_id=pid,
            presentation_name=presentation_name.strip() or "presentation",
            slides=normalize_slide_ids(new_slides),
            theme={"template_id": normalize_template_id(template_id)},
        )

    destination, metadata = resolve_output_path(
        config,
        output_path,
        manifest.presentation_id,
        manifest.presentation_name,
        overwrite,
    )
    render_manifest(manifest, destination)
    output_metadata_to_manifest(manifest, destination, metadata)
    save_manifest(config, manifest)
    elapsed_ms = round((time.perf_counter() - start) * 1000, 2)
    return _manifest_response(manifest, elapsed_ms)


def _refine_sync(
    *,
    presentation_id: str,
    content: str,
    lang: str,
    template_id: str,
    output_path: str,
    overwrite: bool,
) -> dict[str, Any]:
    start = time.perf_counter()
    config = get_ppt_config()
    manifest = load_manifest(config, presentation_id)
    if template_id.strip():
        manifest.theme["template_id"] = normalize_template_id(template_id)
    updates = parse_slides_payload(content, lang)
    by_id = {slide.slide_id: slide for slide in manifest.slides}
    changed: list[str] = []
    for update in updates:
        if not update.slide_id or update.slide_id not in by_id:
            raise ValueError(f"slide_id not found: {update.slide_id}")
        by_id[update.slide_id] = merge_refine_slide(by_id[update.slide_id], update)
        changed.append(update.slide_id)
    manifest.slides = [by_id[slide.slide_id] for slide in manifest.slides]
    destination, metadata = resolve_output_path(
        config,
        output_path,
        manifest.presentation_id,
        manifest.presentation_name,
        overwrite,
    )
    render_manifest(manifest, destination)
    output_metadata_to_manifest(manifest, destination, metadata)
    save_manifest(config, manifest)
    elapsed_ms = round((time.perf_counter() - start) * 1000, 2)
    response = _manifest_response(manifest, elapsed_ms)
    response["changed_slide_ids"] = changed
    return response


def _delete_sync(*, presentation_id: str, slide_ids: list[str], output_path: str, overwrite: bool) -> dict[str, Any]:
    start = time.perf_counter()
    if not slide_ids:
        raise ValueError("slide_ids is required.")
    config = get_ppt_config()
    manifest = load_manifest(config, presentation_id)
    targets = {str(item).strip() for item in slide_ids if str(item).strip()}
    if not targets:
        raise ValueError("slide_ids cannot be empty.")
    known = {slide.slide_id for slide in manifest.slides}
    missing = sorted(targets - known)
    if missing:
        raise ValueError(f"slide_id not found: {', '.join(missing)}")
    remaining = [slide for slide in manifest.slides if slide.slide_id not in targets]
    if not remaining:
        raise ValueError("Cannot delete all slides.")
    manifest.slides = remaining
    destination, metadata = resolve_output_path(
        config,
        output_path,
        manifest.presentation_id,
        manifest.presentation_name,
        overwrite,
    )
    render_manifest(manifest, destination)
    output_metadata_to_manifest(manifest, destination, metadata)
    save_manifest(config, manifest)
    elapsed_ms = round((time.perf_counter() - start) * 1000, 2)
    response = _manifest_response(manifest, elapsed_ms)
    response["deleted_slide_ids"] = sorted(targets)
    return response


def _read_sync(*, presentation_id: str, path: str, slide_ids: list[str], max_slides: int) -> dict[str, Any]:
    config = get_ppt_config()
    if presentation_id.strip():
        manifest = load_manifest(config, presentation_id)
        requested = {item.strip() for item in slide_ids if item.strip()}
        slides = [
            {
                **_slide_summary(slide, idx),
                "prompt": slide.prompt[:800],
                "body": slide.body[:800],
                "bullets": slide.bullets[:8],
            }
            for idx, slide in enumerate(manifest.slides, start=1)
            if not requested or slide.slide_id in requested
        ]
        return {
            "ok": True,
            "presentation_id": manifest.presentation_id,
            "presentation_name": manifest.presentation_name,
            "template_id": manifest.theme.get("template_id") or "",
            "path": manifest.public_path or manifest.output_path,
            "slide_count": len(manifest.slides),
            "slides": slides,
        }
    if not path.strip():
        raise ValueError("presentation_id or path is required.")
    return summarize_pptx(resolve_readable_pptx_path(config, path), max_slides=max_slides)


def _template_read_sync(*, template_id: str, path: str, max_slides: int) -> dict[str, Any]:
    target = path.strip() or template_id.strip()
    if not target:
        return {
            "ok": True,
            "type": "builtin_template_list",
            "templates": list_builtin_templates(),
        }
    if is_builtin_template(target) and not path.strip():
        return {
            "ok": True,
            "type": "builtin_template",
            "template": builtin_template_summary(target),
        }
    config = get_ppt_config()
    summary = summarize_pptx(resolve_readable_pptx_path(config, target), max_slides=max_slides)
    summary["template_id"] = template_id.strip() or target
    return summary


def register_tools(mcp: FastMCP) -> None:
    @mcp.tool(
        name="ppt_write",
        title="PPT 页面生成",
        description=(
            "创建新的 PPTX 演示文稿，或向已有 presentation_id 追加/插入页面。"
            "使用方式对齐豆包 lark-ppt：content 优先传 XML，根节点为 <slides>，每个 <slide> 至少包含 <prompt>，"
            "也可包含 <type>/<title>/<subtitle>/<body>/<bullet>/<item>/<metric>/<template_id>/<template_slide_id>。"
            "可用 template_id 选择内置模板：amber_clear、executive_green、research_blue、finance_ink、creative_coral、minimal_gray。"
            "请在 prompt 中详细写明主题、版式、文案、图表、配色、字体和素材使用要求。"
            "返回 presentation_id、pptx 路径与 slide_id，后续用 ppt_refine 精修。"
            "如需文件落到当前工作区，请把 output_path 写为 /workspaces/{user_id}/exports/report.pptx。"
        ),
        annotations={
            "readOnlyHint": False,
            "destructiveHint": False,
            "idempotentHint": False,
            "openWorldHint": False,
        },
    )
    async def ppt_write(
        content: Annotated[
            str,
            Field(
                description="新增 PPT 页面的 XML 或 JSON 内容。XML 格式：<slides><slide><prompt>...</prompt></slide></slides>。",
                title="页面内容",
            ),
        ],
        presentation_id: Annotated[
            str,
            Field(
                description="已有 PPT 的 presentation_id。首次创建新 PPT 时留空；追加或插入页面时传已有 ID。",
                title="演示文稿 ID",
            ),
        ] = "",
        presentation_name: Annotated[
            str,
            Field(
                description="PPT 标题。创建新 PPT 时建议传入明确标题。",
                title="PPT 标题",
            ),
        ] = "",
        insert_before: Annotated[
            str,
            Field(
                description="在指定 slide_id 前插入新页面；为空时默认追加到末尾。",
                title="插入位置",
            ),
        ] = "",
        lang: Annotated[
            Literal["xml", "json"],
            Field(
                description="content 的内容格式，默认 xml。",
                title="内容格式",
            ),
        ] = "xml",
        template_id: Annotated[
            str,
            Field(
                description=(
                    "内置模板 ID。可选：amber_clear、executive_green、research_blue、"
                    "finance_ink、creative_coral、minimal_gray。为空时使用 amber_clear。"
                ),
                title="模板 ID",
            ),
        ] = "",
        output_path: Annotated[
            str,
            Field(
                description="可选输出路径。支持 /workspaces/{user_id}/... 工作区路径；为空时写入 extra_mcp 的 PPT 产物目录。",
                title="输出路径",
            ),
        ] = "",
        overwrite: Annotated[
            bool,
            Field(
                description="目标文件存在时是否覆盖。默认 false，会自动生成不冲突文件名。",
                title="覆盖已有文件",
            ),
        ] = False,
    ) -> dict[str, Any]:
        try:
            return await run_in_thread(
                _write_sync,
                presentation_id=presentation_id,
                presentation_name=presentation_name,
                insert_before=insert_before,
                content=content,
                lang=lang,
                template_id=template_id,
                output_path=output_path,
                overwrite=overwrite,
            )
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="ppt_refine",
        title="PPT 页面精修",
        description=(
            "修改已有 PPT 的指定页面。content 必须包含 <slide_id> 和 <prompt>，"
            "可同时传 <title>/<body>/<bullet>/<item>/<metric> 等结构化字段。"
            "也可传 template_id 重渲染整份 PPT 的内置模板风格。"
            "当前实现对本工具生成的 PPT 使用 manifest 重渲染，能保持整份文稿风格一致。"
        ),
        annotations={
            "readOnlyHint": False,
            "destructiveHint": False,
            "idempotentHint": False,
            "openWorldHint": False,
        },
    )
    async def ppt_refine(
        presentation_id: Annotated[
            str,
            Field(description="要修改的 PPT 的 presentation_id。", title="演示文稿 ID"),
        ],
        content: Annotated[
            str,
            Field(
                description="修改页面的 XML 或 JSON 内容。XML 格式：<slides><slide><slide_id>slide_001</slide_id><prompt>...</prompt></slide></slides>。",
                title="修改内容",
            ),
        ],
        lang: Annotated[
            Literal["xml", "json"],
            Field(description="content 的内容格式，默认 xml。", title="内容格式"),
        ] = "xml",
        template_id: Annotated[
            str,
            Field(
                description=(
                    "可选内置模板 ID，用于重渲染整份 PPT 风格。可选：amber_clear、executive_green、"
                    "research_blue、finance_ink、creative_coral、minimal_gray。"
                ),
                title="模板 ID",
            ),
        ] = "",
        output_path: Annotated[
            str,
            Field(description="可选输出路径。为空时写入 extra_mcp 的 PPT 产物目录。", title="输出路径"),
        ] = "",
        overwrite: Annotated[
            bool,
            Field(description="目标文件存在时是否覆盖。", title="覆盖已有文件"),
        ] = False,
    ) -> dict[str, Any]:
        try:
            pid = sanitize_presentation_id(presentation_id)
            return await run_in_thread(
                _refine_sync,
                presentation_id=pid,
                content=content,
                lang=lang,
                template_id=template_id,
                output_path=output_path,
                overwrite=overwrite,
            )
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="ppt_read",
        title="读取 PPT 内容",
        description=(
            "读取已生成 PPT 的 manifest 内容，或读取任意本地 PPTX 的页数、尺寸、页面文字摘要。"
            "优先传 presentation_id；如果读取外部文件，则传 path。"
        ),
        annotations={
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def ppt_read(
        presentation_id: Annotated[
            str,
            Field(description="已生成 PPT 的 presentation_id。", title="演示文稿 ID"),
        ] = "",
        path: Annotated[
            str,
            Field(description="本地 PPTX 路径。仅在 presentation_id 为空时使用。", title="PPTX 路径"),
        ] = "",
        slide_ids: Annotated[
            list[str] | None,
            Field(description="可选 slide_id 列表。为空时读取全部 manifest 页面摘要。", title="页面 ID 列表"),
        ] = None,
        max_slides: Annotated[
            int,
            Field(description="读取外部 PPTX 时最多返回的页数摘要，默认 30。", title="最大页数"),
        ] = 30,
    ) -> dict[str, Any]:
        try:
            return await run_in_thread(
                _read_sync,
                presentation_id=presentation_id,
                path=path,
                slide_ids=slide_ids or [],
                max_slides=max(1, min(max_slides, 100)),
            )
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="ppt_template_read",
        title="读取 PPT 模板",
        description=(
            "读取 PPT 模板或已有 PPTX 的页面摘要、尺寸和每页文字，供模型选择 template_slide_id 或分析模板风格。"
            "template_id 和 path 都为空时返回内置模板列表；内置模板可直接传给 ppt_write/ppt_refine 的 template_id。"
            "第一版返回结构摘要；后续可扩展为截图和版式元素识别。"
        ),
        annotations={
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def ppt_template_read(
        template_id: Annotated[
            str,
            Field(description="模板 ID 或模板文件路径。为空时返回内置模板列表。", title="模板 ID"),
        ] = "",
        path: Annotated[
            str,
            Field(description="模板 PPTX 本地路径；为空时尝试把 template_id 当路径。", title="模板路径"),
        ] = "",
        max_slides: Annotated[
            int,
            Field(description="最多返回的模板页面摘要数，默认 30。", title="最大页数"),
        ] = 30,
    ) -> dict[str, Any]:
        try:
            return await run_in_thread(
                _template_read_sync,
                template_id=template_id,
                path=path,
                max_slides=max(1, min(max_slides, 100)),
            )
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="ppt_delete",
        title="删除 PPT 页面",
        description="删除本工具生成的 PPT 中指定 slide_id 的页面，并重渲染演示文稿。",
        annotations={
            "readOnlyHint": False,
            "destructiveHint": True,
            "idempotentHint": False,
            "openWorldHint": False,
        },
    )
    async def ppt_delete(
        presentation_id: Annotated[
            str,
            Field(description="目标 PPT 的 presentation_id。", title="演示文稿 ID"),
        ],
        slide_ids: Annotated[
            list[str],
            Field(description="要删除的 slide_id 列表。", title="页面 ID 列表"),
        ],
        output_path: Annotated[
            str,
            Field(description="可选输出路径。为空时写入 extra_mcp 的 PPT 产物目录。", title="输出路径"),
        ] = "",
        overwrite: Annotated[
            bool,
            Field(description="目标文件存在时是否覆盖。", title="覆盖已有文件"),
        ] = False,
    ) -> dict[str, Any]:
        try:
            pid = sanitize_presentation_id(presentation_id)
            return await run_in_thread(
                _delete_sync,
                presentation_id=pid,
                slide_ids=slide_ids,
                output_path=output_path,
                overwrite=overwrite,
            )
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)
