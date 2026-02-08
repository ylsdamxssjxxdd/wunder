from __future__ import annotations

import re
import time
from typing import Annotated, Any, Sequence

from mcp.server.fastmcp import FastMCP
from pydantic import Field

from ...common.async_utils import run_in_thread
from .config import DbQueryTarget, get_db_config, load_db_query_targets
from .db import execute_sql_sync, get_table_schema_compact_sync, validate_sql_against_target_table


def _error_response(exc: Exception) -> dict[str, Any]:
    return {"ok": False, "error": str(exc)}


def _normalize_params(params: Sequence[Any]) -> list[Any] | None:
    if not params:
        return None
    return list(params)


def _normalize_max_rows(value: int) -> int:
    if value <= 0:
        return 200
    return min(value, 5000)


def _sanitize_tool_suffix(value: str) -> str:
    normalized = re.sub(r"[^0-9a-zA-Z_]+", "_", value.strip().lower())
    normalized = re.sub(r"_+", "_", normalized).strip("_")
    if not normalized:
        normalized = "target"
    if normalized[0].isdigit():
        normalized = f"t_{normalized}"
    return normalized


def _build_tool_names(targets: Sequence[DbQueryTarget]) -> list[str]:
    if len(targets) <= 1:
        return ["db_query"]
    names: list[str] = []
    seen: dict[str, int] = {}
    for target in targets:
        suffix = _sanitize_tool_suffix(target.key)
        count = seen.get(suffix, 0) + 1
        seen[suffix] = count
        if count > 1:
            suffix = f"{suffix}_{count}"
        names.append(f"db_query_{suffix}")
    return names


def _build_schema_hint(columns: Sequence[dict[str, str]]) -> str:
    if not columns:
        return ""
    parts = [
        f"{column['name']}:{column['type']}"
        for column in columns
        if column.get("name") and column.get("type")
    ]
    if not parts:
        return ""
    truncated = len(parts) > 24
    if truncated:
        parts = parts[:24]
    tail = ", ..." if truncated else ""
    return "Columns: " + ", ".join(parts) + tail


def _resolve_schema_hint(target: DbQueryTarget) -> str:
    try:
        cfg = get_db_config(None, target.db_key)
        schema = get_table_schema_compact_sync(cfg, target.table)
    except Exception:
        return ""
    if not schema.get("ok"):
        return ""
    columns = schema.get("columns")
    if not isinstance(columns, list):
        return ""
    compact_columns: list[dict[str, str]] = []
    for column in columns:
        if not isinstance(column, dict):
            continue
        name = str(column.get("name") or "").strip()
        column_type = str(column.get("type") or "").strip()
        if not name or not column_type:
            continue
        compact_columns.append({"name": name, "type": column_type})
    return _build_schema_hint(compact_columns)


def _build_description(target: DbQueryTarget, schema_hint: str, include_db_key: bool) -> str:
    parts = [f"Run read-only SQL and return compact rows for table {target.table}. ", "Strong constraint: queries can only access this bound table. "]
    if target.description:
        parts.append(f"Purpose: {target.description}. ")
    if include_db_key and target.db_key:
        parts.append(f"Database target: {target.db_key}. ")
    if schema_hint:
        parts.append(schema_hint + ".")
    return "".join(parts).strip()


def _register_bound_db_query_tool(
    mcp: FastMCP,
    tool_name: str,
    description: str,
    target: DbQueryTarget,
) -> None:
    @mcp.tool(
        name=tool_name,
        title=f"DB Query ({target.table})",
        description=description,
        annotations={
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": False,
            "openWorldHint": False,
        },
    )
    async def table_db_query(
        sql: Annotated[
            str,
            Field(
                description="SQL query (read-only statements only).",
                title="SQL",
            ),
        ],
        params: Annotated[
            Sequence[Any],
            Field(
                description="Optional SQL positional parameters.",
                title="Params",
            ),
        ] = (),
        max_rows: Annotated[
            int,
            Field(
                description="Maximum returned rows, default 200.",
                title="Max Rows",
            ),
        ] = 200,
    ) -> dict[str, Any]:
        """Run a SQL query and return compact results."""
        start = time.perf_counter()
        try:
            sql_text = sql.strip()
            if not sql_text:
                return {"ok": False, "error": "SQL statement cannot be empty."}
            cfg = get_db_config(None, target.db_key)
            validation_error = validate_sql_against_target_table(sql_text, cfg, target.table)
            if validation_error:
                return {"ok": False, "error": validation_error}
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


def _register_generic_db_query_tool(mcp: FastMCP) -> None:
    @mcp.tool(
        name="db_query",
        title="DB Query",
        description="Run read-only SQL and return compact rows.",
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
                description="SQL query (read-only statements only).",
                title="SQL",
            ),
        ],
        params: Annotated[
            Sequence[Any],
            Field(
                description="Optional SQL positional parameters.",
                title="Params",
            ),
        ] = (),
        max_rows: Annotated[
            int,
            Field(
                description="Maximum returned rows, default 200.",
                title="Max Rows",
            ),
        ] = 200,
    ) -> dict[str, Any]:
        """Run a SQL query and return compact results."""
        start = time.perf_counter()
        try:
            sql_text = sql.strip()
            if not sql_text:
                return {"ok": False, "error": "SQL statement cannot be empty."}
            cfg = get_db_config(None, None)
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


def register_tools(mcp: FastMCP) -> None:
    targets = load_db_query_targets()
    if not targets:
        _register_generic_db_query_tool(mcp)
        return

    tool_names = _build_tool_names(targets)
    include_db_key = len({target.db_key for target in targets if target.db_key}) > 1
    for target, tool_name in zip(targets, tool_names):
        schema_hint = _resolve_schema_hint(target)
        description = _build_description(target, schema_hint, include_db_key)
        _register_bound_db_query_tool(
            mcp=mcp,
            tool_name=tool_name,
            description=description,
            target=target,
        )
