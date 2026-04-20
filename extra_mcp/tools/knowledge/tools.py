from __future__ import annotations

import re
from typing import Annotated, Any

from mcp.server.fastmcp import FastMCP
from pydantic import Field

from ...common.async_utils import run_in_thread
from ...common.config import get_config_section, get_section_value
from .client import query_kb_sync
from .config import KnowledgeTargetConfig, get_kb_config, load_kb_targets


def _error_response(exc: Exception) -> dict[str, Any]:
    return {"ok": False, "error": str(exc)}


def _sanitize_tool_suffix(value: str) -> str:
    normalized = re.sub(r"[^0-9a-zA-Z_]+", "_", value.strip().lower())
    normalized = re.sub(r"_+", "_", normalized).strip("_")
    if not normalized:
        normalized = "target"
    if normalized[0].isdigit():
        normalized = f"t_{normalized}"
    return normalized


def _build_tool_names(keys: list[str]) -> list[str]:
    if len(keys) <= 1:
        return ["kb_query"]
    names: list[str] = []
    seen: dict[str, int] = {}
    for key in keys:
        suffix = _sanitize_tool_suffix(key)
        count = seen.get(suffix, 0) + 1
        seen[suffix] = count
        if count > 1:
            suffix = f"{suffix}_{count}"
        names.append(f"kb_query_{suffix}")
    return names


def _build_description(cfg: KnowledgeTargetConfig) -> str:
    if cfg.description:
        return (
            "在指定的 RAGFlow 知识库目标中检索内容，返回紧凑的召回片段。"
            f"当前目标用途：{cfg.description}。"
        )
    return f"在指定的 RAGFlow 知识库目标中检索内容，返回紧凑的召回片段。当前目标：{cfg.key}。"


def _register_bound_kb_query_tool(
    mcp: FastMCP,
    tool_name: str,
    description: str,
    cfg: KnowledgeTargetConfig,
) -> None:
    @mcp.tool(
        name=tool_name,
        title=f"知识库检索（{cfg.key}）",
        description=description,
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
                description="要检索的问题、关键词或短语。",
                title="检索内容",
            ),
        ],
        limit: Annotated[
            int,
            Field(
                description="最多返回的结果片段数，默认 20。",
                title="返回数量",
            ),
        ] = 20,
    ) -> dict[str, Any]:
        """检索知识库并返回紧凑结果。"""
        try:
            query_text = query.strip()
            if not query_text:
                return {"ok": False, "error": "检索内容不能为空。"}
            return await run_in_thread(query_kb_sync, cfg, query_text, limit)
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)


def _register_generic_kb_query_tool(mcp: FastMCP) -> None:
    @mcp.tool(
        name="kb_query",
        title="知识库检索",
        description="检索已配置的 RAGFlow 知识库内容片段，返回紧凑的召回结果，适合输入问题、关键词、主题词或文档线索。",
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
                description="要检索的问题、关键词或短语。",
                title="检索内容",
            ),
        ],
        limit: Annotated[
            int,
            Field(
                description="最多返回的结果片段数，默认 20。",
                title="返回数量",
            ),
        ] = 20,
    ) -> dict[str, Any]:
        """检索知识库并返回紧凑结果。"""
        try:
            query_text = query.strip()
            if not query_text:
                return {"ok": False, "error": "检索内容不能为空。"}
            cfg = get_kb_config(None)
            return await run_in_thread(query_kb_sync, cfg, query_text, limit)
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)


def register_tools(mcp: FastMCP) -> None:
    # Allow explicit disable to keep the MCP tool surface minimal for scenario-based demos.
    section = get_config_section("knowledge") or get_config_section("kb")
    if section and get_section_value(section, "enabled") is False:
        return

    targets = load_kb_targets()
    if not targets:
        _register_generic_kb_query_tool(mcp)
        return

    keys = list(targets.keys())
    tool_names = _build_tool_names(keys)
    for key, tool_name in zip(keys, tool_names):
        cfg = targets[key]
        _register_bound_kb_query_tool(
            mcp=mcp,
            tool_name=tool_name,
            description=_build_description(cfg),
            cfg=cfg,
        )
