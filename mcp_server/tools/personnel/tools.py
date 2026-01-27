from __future__ import annotations

import time
from typing import Any

from mcp.server.fastmcp import FastMCP

from ...common.async_utils import run_in_thread
from .config import get_db_config, summarize_db_targets
from .db import (
    count_rows_sync,
    describe_table_sync,
    execute_sql_sync,
    fetch_schema_sync,
    list_tables_sync,
    ping_db_sync,
    preview_rows_sync,
)
from .models import (
    DatabaseListInput,
    HealthInput,
    QueryInput,
    SchemaInput,
    TableCountInput,
    TableDescribeInput,
    TableListInput,
    TablePreviewInput,
)


def _error_response(exc: Exception) -> dict[str, Any]:
    return {"ok": False, "error": str(exc)}


def register_tools(mcp: FastMCP) -> None:
    @mcp.tool(
        name="personnel_list_databases",
        annotations={
            "title": "List Personnel Database Targets",
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def personnel_list_databases(params: DatabaseListInput) -> dict[str, Any]:
        """List configured database targets for this MCP service."""
        try:
            return summarize_db_targets(params.db_key)
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="personnel_ping",
        annotations={
            "title": "Ping Personnel Database",
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def personnel_ping(params: HealthInput) -> dict[str, Any]:
        """Check database connectivity and return latency."""
        start = time.perf_counter()
        try:
            cfg = get_db_config(params.database, params.db_key)
            await run_in_thread(ping_db_sync, cfg)
            return {
                "ok": True,
                "database": cfg.database,
                "db_key": params.db_key,
                "elapsed_ms": round((time.perf_counter() - start) * 1000, 2),
            }
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="personnel_list_tables",
        annotations={
            "title": "List Personnel Tables",
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def personnel_list_tables(params: TableListInput) -> dict[str, Any]:
        """List tables in the personnel database."""
        try:
            cfg = get_db_config(params.database, params.db_key)
            return await run_in_thread(
                list_tables_sync, cfg, params.pattern, params.limit
            )
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="personnel_get_schema",
        annotations={
            "title": "Get Personnel DB Schema",
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def personnel_get_schema(params: SchemaInput) -> dict[str, Any]:
        """Return tables and columns from the personnel database schema."""
        try:
            cfg = get_db_config(params.database, params.db_key)
            return await run_in_thread(fetch_schema_sync, cfg, params.tables)
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="personnel_describe_table",
        annotations={
            "title": "Describe Personnel Table",
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def personnel_describe_table(params: TableDescribeInput) -> dict[str, Any]:
        """Return column details for a specific table."""
        try:
            cfg = get_db_config(params.database, params.db_key)
            return await run_in_thread(describe_table_sync, cfg, params.table)
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="personnel_preview_rows",
        annotations={
            "title": "Preview Personnel Table Rows",
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def personnel_preview_rows(params: TablePreviewInput) -> dict[str, Any]:
        """Preview rows from a personnel table."""
        try:
            cfg = get_db_config(params.database, params.db_key)
            return await run_in_thread(
                preview_rows_sync,
                cfg,
                params.table,
                params.columns,
                params.limit,
                params.order_by,
                params.order_desc,
            )
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="personnel_count_rows",
        annotations={
            "title": "Count Personnel Table Rows",
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
            "openWorldHint": False,
        },
    )
    async def personnel_count_rows(params: TableCountInput) -> dict[str, Any]:
        """Return row count for a personnel table."""
        try:
            cfg = get_db_config(params.database, params.db_key)
            return await run_in_thread(count_rows_sync, cfg, params.table)
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)

    @mcp.tool(
        name="personnel_query",
        annotations={
            "title": "Query Personnel Database",
            "readOnlyHint": False,
            "destructiveHint": True,
            "idempotentHint": False,
            "openWorldHint": False,
        },
    )
    async def personnel_query(params: QueryInput) -> dict[str, Any]:
        """Execute SQL against the personnel database and return results."""
        start = time.perf_counter()
        try:
            cfg = get_db_config(params.database, params.db_key)
            result = await run_in_thread(
                execute_sql_sync,
                cfg,
                params.sql,
                params.params,
                params.max_rows,
                params.allow_write,
            )
            result["elapsed_ms"] = round((time.perf_counter() - start) * 1000, 2)
            return result
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)
