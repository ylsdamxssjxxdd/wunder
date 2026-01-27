from __future__ import annotations

import time
from typing import Annotated, Any, Sequence

from mcp.server.fastmcp import FastMCP
from pydantic import Field

from ...common.async_utils import run_in_thread
from .config import build_db_description_hint, get_db_config
from .db import execute_sql_sync, fetch_schema_sync


def _error_response(exc: Exception) -> dict[str, Any]:
    return {"ok": False, "error": str(exc)}


def _normalize_optional_text(value: str) -> str | None:
    text = value.strip()
    return text or None


def _normalize_tables(tables: Sequence[str]) -> list[str] | None:
    cleaned = []
    for item in tables:
        text = str(item).strip()
        if text:
            cleaned.append(text)
    return cleaned or None


def _normalize_params(params: Sequence[Any]) -> list[Any] | None:
    if not params:
        return None
    return list(params)


def _normalize_max_rows(value: int) -> int:
    if value <= 0:
        return 200
    return min(value, 5000)


def _compose_description(base: str) -> str:
    hint = build_db_description_hint()
    if not hint:
        return base
    return f"{base}{hint}"


def register_tools(mcp: FastMCP) -> None:
    @mcp.tool(
        name="db_get_schema",
        title="查询数据库结构",
        description=_compose_description("查询数据库的表结构与字段信息。"),
        annotations={
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def db_get_schema(
        database: Annotated[
            str,
            Field(
                description="数据库名，留空则使用默认配置。",
                title="数据库名",
            ),
        ] = "",
        tables: Annotated[
            Sequence[str],
            Field(
                description="可选表名列表，留空返回全部表。",
                title="表名列表",
            ),
        ] = (),
    ) -> dict[str, Any]:
        """查询数据库结构信息。"""
        try:
            cfg = get_db_config(
                _normalize_optional_text(database),
                None,
            )
            table_list = _normalize_tables(tables)
            return await run_in_thread(fetch_schema_sync, cfg, table_list)
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="db_query",
        title="执行 SQL 查询",
        description=_compose_description("执行只读 SQL 查询并返回结果。"),
        annotations={
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": False,
            "openWorldHint": False,
        },
    )
    async def db_query(
        sql: Annotated[
            str,
            Field(
                description="SQL 查询语句（仅允许只读语句）。",
                title="SQL 语句",
            ),
        ],
        database: Annotated[
            str,
            Field(
                description="数据库名，留空则使用默认配置。",
                title="数据库名",
            ),
        ] = "",
        params: Annotated[
            Sequence[Any],
            Field(
                description="SQL 参数列表，可留空。",
                title="SQL 参数",
            ),
        ] = (),
        max_rows: Annotated[
            int,
            Field(
                description="最多返回行数，默认 200。",
                title="返回行数上限",
            ),
        ] = 200,
    ) -> dict[str, Any]:
        """执行 SQL 查询并返回结果。"""
        start = time.perf_counter()
        try:
            sql_text = sql.strip()
            if not sql_text:
                return {"ok": False, "error": "SQL 语句不能为空。"}
            cfg = get_db_config(
                _normalize_optional_text(database),
                None,
            )
            result = await run_in_thread(
                execute_sql_sync,
                cfg,
                sql_text,
                _normalize_params(params),
                _normalize_max_rows(max_rows),
                False,
            )
            result["elapsed_ms"] = round((time.perf_counter() - start) * 1000, 2)
            return result
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)
