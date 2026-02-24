from __future__ import annotations

import re
from typing import Annotated, Any

from mcp.server.fastmcp import FastMCP
from pydantic import Field

from ...common.async_utils import run_in_thread
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
        return f"Search configured knowledge target: {cfg.description}."
    return f"Search configured knowledge target: {cfg.key}."


def _register_bound_kb_query_tool(
    mcp: FastMCP,
    tool_name: str,
    description: str,
    cfg: KnowledgeTargetConfig,
) -> None:
    @mcp.tool(
        name=tool_name,
        title=f"KB Query ({cfg.key})",
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
                description="Question or keywords to search.",
                title="Query",
            ),
        ],
        limit: Annotated[
            int,
            Field(
                description="Maximum result chunks, default 20.",
                title="Limit",
            ),
        ] = 20,
    ) -> dict[str, Any]:
        """Search knowledge base and return compact retrieval results."""
        try:
            query_text = query.strip()
            if not query_text:
                return {"ok": False, "error": "Query cannot be empty."}
            return await run_in_thread(query_kb_sync, cfg, query_text, limit)
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)


def _register_generic_kb_query_tool(mcp: FastMCP) -> None:
    @mcp.tool(
        name="kb_query",
        title="KB Query",
        description="Search knowledge base content chunks.",
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
                description="Question or keywords to search.",
                title="Query",
            ),
        ],
        limit: Annotated[
            int,
            Field(
                description="Maximum result chunks, default 20.",
                title="Limit",
            ),
        ] = 20,
    ) -> dict[str, Any]:
        """Search knowledge base and return compact retrieval results."""
        try:
            query_text = query.strip()
            if not query_text:
                return {"ok": False, "error": "Query cannot be empty."}
            cfg = get_kb_config(None)
            return await run_in_thread(query_kb_sync, cfg, query_text, limit)
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)


def register_tools(mcp: FastMCP) -> None:
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
