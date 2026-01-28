from __future__ import annotations

from typing import Annotated, Any

from mcp.server.fastmcp import FastMCP
from pydantic import Field

from ...common.async_utils import run_in_thread
from .client import query_kb_sync
from .config import build_kb_description_hint, get_kb_config


def _error_response(exc: Exception) -> dict[str, Any]:
    return {"ok": False, "error": str(exc)}


def _normalize_optional_text(value: str) -> str | None:
    text = value.strip()
    return text or None


def _compose_description(base: str) -> str:
    hint = build_kb_description_hint()
    if not hint:
        return base
    return f"{base}{hint}"


def register_tools(mcp: FastMCP) -> None:
    @mcp.tool(
        name="kb_query",
        title="查询知识库",
        description=_compose_description("检索知识库中的内容片段。"),
        annotations={
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def kb_query(
        query: Annotated[
            str,
            Field(
                description="检索问题或关键词。",
                title="查询内容",
            ),
        ],
        kb: Annotated[
            str,
            Field(
                description="知识库 key，留空则使用默认配置。",
                title="知识库",
            ),
        ] = "",
        limit: Annotated[
            int,
            Field(
                description="最多返回条数，默认 20。",
                title="返回条数",
            ),
        ] = 20,
    ) -> dict[str, Any]:
        """查询知识库并返回检索结果。"""
        try:
            query_text = query.strip()
            if not query_text:
                return {"ok": False, "error": "查询内容不能为空。"}
            cfg = get_kb_config(_normalize_optional_text(kb))
            result = await run_in_thread(query_kb_sync, cfg, query_text, limit)
            return result
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)
