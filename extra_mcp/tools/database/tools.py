from __future__ import annotations

import logging
import re
import time
from typing import Annotated, Any, Literal, Sequence

from mcp.server.fastmcp import FastMCP
from pydantic import Field

from ...common.async_utils import run_in_thread
from .config import DbQueryTarget, get_db_config, load_db_query_targets
from .db import execute_sql_sync, get_table_schema_compact_sync, validate_sql_against_target_table
from .exporter import build_query_handle, export_sql_to_file_sync, resolve_query_request

logger = logging.getLogger(__name__)
SAFE_ASCII_IDENTIFIER_PATTERN = re.compile(r"^[A-Za-z_][A-Za-z0-9_$]*$")
SCHEMA_HINT_RETRY_DELAYS_S = (0.0, 0.2, 0.5)


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


def _build_tool_names(prefix: str, targets: Sequence[DbQueryTarget]) -> list[str]:
    if len(targets) <= 1:
        return [prefix]
    names: list[str] = []
    seen: dict[str, int] = {}
    for target in targets:
        suffix = _sanitize_tool_suffix(target.key)
        count = seen.get(suffix, 0) + 1
        seen[suffix] = count
        if count > 1:
            suffix = f"{suffix}_{count}"
        names.append(f"{prefix}_{suffix}")
    return names


def _build_schema_hint(columns: Sequence[dict[str, Any]]) -> str:
    if not columns:
        return ""
    column_parts: list[str] = []
    example_parts: list[str] = []
    note_parts: list[str] = []
    for column in columns:
        name = str(column.get("name") or "").strip()
        column_type = str(column.get("full_type") or column.get("type") or "").strip()
        if not name or not column_type:
            continue
        column_parts.append(f"{name}:{column_type}")
        examples = column.get("examples")
        if isinstance(examples, list):
            cleaned_examples = [
                repr(str(item).strip())
                for item in examples
                if str(item).strip()
            ]
            if cleaned_examples:
                example_parts.append(f"{name}={{{', '.join(cleaned_examples[:3])}}}")
        comment = str(column.get("comment") or "").strip()
        if comment:
            note_parts.append(f"{name}: {comment}")
    if not column_parts:
        return ""
    truncated = len(column_parts) > 24
    if truncated:
        column_parts = column_parts[:24]
    tail = ", ..." if truncated else ""
    parts = ["Columns: " + ", ".join(column_parts) + tail]
    if example_parts:
        parts.append("Known values/examples: " + "; ".join(example_parts[:6]))
    if note_parts:
        parts.append("Notes: " + "; ".join(note_parts[:4]))
    return ". ".join(parts)


def _resolve_schema_hint(target: DbQueryTarget) -> str:
    last_error: Exception | None = None
    for delay_s in SCHEMA_HINT_RETRY_DELAYS_S:
        if delay_s > 0:
            time.sleep(delay_s)
        try:
            cfg = get_db_config(None, target.db_key)
            schema = get_table_schema_compact_sync(cfg, target.table)
        except Exception as exc:  # pragma: no cover - depends on runtime DB availability
            last_error = exc
            continue
        if not schema.get("ok"):
            message = str(schema.get("error") or "schema query failed")
            last_error = RuntimeError(message)
            continue
        columns = schema.get("columns")
        if not isinstance(columns, list):
            last_error = RuntimeError("schema columns payload is invalid")
            continue
        compact_columns: list[dict[str, Any]] = []
        for column in columns:
            if not isinstance(column, dict):
                continue
            name = str(column.get("name") or "").strip()
            column_type = str(column.get("type") or "").strip()
            if not name or not column_type:
                continue
            entry: dict[str, Any] = {"name": name, "type": column_type}
            full_type = str(column.get("full_type") or "").strip()
            if full_type:
                entry["full_type"] = full_type
            examples = column.get("examples")
            if isinstance(examples, list) and examples:
                entry["examples"] = examples[:3]
            comment = str(column.get("comment") or "").strip()
            if comment:
                entry["comment"] = comment
            compact_columns.append(entry)
        return _build_schema_hint(compact_columns)
    if last_error is not None:
        logger.warning(
            "Failed to resolve schema hint for table '%s': %s",
            target.table,
            last_error,
        )
    return ""


def _build_identifier_quote_hint(target: DbQueryTarget) -> str:
    table = target.table.strip()
    if not table or SAFE_ASCII_IDENTIFIER_PATTERN.fullmatch(table):
        return ""
    try:
        cfg = get_db_config(None, target.db_key)
    except Exception:  # pragma: no cover - depends on runtime config
        return ""
    if cfg.engine == "mysql":
        return (
            "Identifier hint: this table name contains non-ASCII/special characters; "
            f"in MySQL use backticks, for example FROM `{table}`. "
        )
    if cfg.engine == "postgres":
        return (
            "Identifier hint: this table name contains non-ASCII/special characters; "
            f"in PostgreSQL use double quotes, for example FROM \"{table}\". "
        )
    return ""


def _build_query_description(
    target: DbQueryTarget,
    schema_hint: str,
    include_db_key: bool,
    identifier_quote_hint: str,
) -> str:
    parts = [
        f"Run read-only SQL and return compact rows for table {target.table}. ",
        "Strong constraint: queries can only access this bound table. ",
        "For large datasets, prefer LIMIT/OFFSET pagination and narrower filters. ",
        "Pagination queries must include a stable ORDER BY. ",
        "Successful calls return a query_handle that can be passed to the paired export tool. ",
    ]
    if target.description:
        parts.append(f"Purpose: {target.description}. ")
    if include_db_key and target.db_key:
        parts.append(f"Database target: {target.db_key}. ")
    if identifier_quote_hint:
        parts.append(identifier_quote_hint)
    if schema_hint:
        parts.append(schema_hint + ".")
    return "".join(parts).strip()


def _build_export_description(
    target: DbQueryTarget,
    *,
    query_tool_name: str,
    include_db_key: bool,
) -> str:
    parts = [
        f"Export read-only SQL results from table {target.table} directly to xlsx or csv files. ",
        "Strong constraint: queries can only access this bound table. ",
        f"Prefer using query_handle returned by {query_tool_name} after validating counts or samples, but only reuse a full-detail query_handle without LIMIT/OFFSET for formal exports. ",
        "Use this for deliverables such as Excel exports instead of paging rows through model context. ",
        "Use `/workspaces/{user_id}/exports/...` in path to save into the current workspace; the result returns canonical `path` plus `workspace_relative_path` for follow-up tools and final links. ",
        "SQL/query_handle that still contains LIMIT/OFFSET is rejected by default; set allow_limited_export=true only when a partial export is intentional. ",
    ]
    if target.description:
        parts.append(f"Purpose: {target.description}. ")
    if include_db_key and target.db_key:
        parts.append(f"Database target: {target.db_key}. ")
    return "".join(parts).strip()


def _annotate_query_result(
    result: dict[str, Any],
    *,
    sql_text: str,
    params: list[Any] | None,
    target: DbQueryTarget | None,
) -> dict[str, Any]:
    if not result.get("ok"):
        return result
    result["query_handle"] = build_query_handle(sql_text, params, target)
    return result


def _build_generic_query_description() -> str:
    return (
        "Run read-only SQL and return compact rows. "
        "For large datasets, prefer LIMIT/OFFSET pagination and narrower filters. "
        "Pagination queries must include a stable ORDER BY. "
        "Successful calls return a query_handle that can be passed to db_export."
    )


def _build_generic_export_description() -> str:
    return (
        "Export read-only SQL results directly to xlsx or csv files. "
        "Prefer using query_handle returned by db_query after validating counts or samples, but only reuse a full-detail query_handle without LIMIT/OFFSET for formal exports. "
        "Use this for deliverables such as Excel exports instead of paging rows through model context. "
        "Use `/workspaces/{user_id}/exports/...` in path to save into the current workspace; the result returns canonical `path` plus `workspace_relative_path` for follow-up tools and final links. "
        "SQL/query_handle that still contains LIMIT/OFFSET is rejected by default; set allow_limited_export=true only when a partial export is intentional."
    )


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
                description="Maximum returned rows, default 200; if truncated=true, continue with LIMIT/OFFSET or narrower filters.",
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
            params_list = _normalize_params(params)
            result = await run_in_thread(
                execute_sql_sync,
                cfg,
                sql_text,
                params_list,
                _normalize_max_rows(max_rows),
                False,
            )
            _annotate_query_result(
                result,
                sql_text=sql_text,
                params=params_list,
                target=target,
            )
            result["elapsed_ms"] = round((time.perf_counter() - start) * 1000, 2)
            return result
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)


def _register_bound_db_export_tool(
    mcp: FastMCP,
    tool_name: str,
    description: str,
    target: DbQueryTarget,
) -> None:
    @mcp.tool(
        name=tool_name,
        title=f"DB Export ({target.table})",
        description=description,
        annotations={
            "readOnlyHint": False,
            "destructiveHint": False,
            "idempotentHint": False,
            "openWorldHint": False,
        },
    )
    async def table_db_export(
        query_handle: Annotated[
            str,
            Field(
                description="Opaque handle previously returned by the paired db_query tool. Preferred for exports.",
                title="Query Handle",
            ),
        ] = "",
        sql: Annotated[
            str,
            Field(
                description="Fallback SQL query (read-only statements only). Use when query_handle is unavailable.",
                title="SQL",
            ),
        ] = "",
        params: Annotated[
            Sequence[Any],
            Field(
                description="Optional SQL positional parameters used with sql when query_handle is not supplied.",
                title="Params",
            ),
        ] = (),
        path: Annotated[
            str,
            Field(
                description="Output path. Use `/workspaces/{user_id}/exports/report.xlsx` to save into the current workspace so later file tools can continue processing it; otherwise a relative path is resolved under the configured export root. If omitted, a timestamped filename is generated automatically.",
                title="Path",
            ),
        ] = "",
        format: Annotated[
            Literal["xlsx", "csv"],
            Field(
                description="Export format. Defaults to xlsx.",
                title="Format",
            ),
        ] = "xlsx",
        sheet_name: Annotated[
            str,
            Field(
                description="Optional worksheet name for xlsx exports.",
                title="Sheet Name",
            ),
        ] = "Sheet1",
        overwrite: Annotated[
            bool,
            Field(
                description="Overwrite the target file if it already exists. Defaults to false.",
                title="Overwrite",
            ),
        ] = False,
        allow_limited_export: Annotated[
            bool,
            Field(
                description="Allow exporting SQL/query_handle that still contains LIMIT/OFFSET. Keep false for formal full exports; set true only when a partial export is intentional.",
                title="Allow Limited Export",
            ),
        ] = False,
    ) -> dict[str, Any]:
        """Export SQL query results directly to a file."""
        start = time.perf_counter()
        try:
            sql_text, params_list = resolve_query_request(
                query_handle=query_handle,
                sql=sql,
                params=params,
                expected_target=target,
            )
            cfg = get_db_config(None, target.db_key)
            validation_error = validate_sql_against_target_table(sql_text, cfg, target.table)
            if validation_error:
                return {"ok": False, "error": validation_error}
            result = await run_in_thread(
                export_sql_to_file_sync,
                cfg,
                sql_text,
                params_list,
                target=target,
                path=path,
                export_format=format,
                sheet_name=sheet_name,
                overwrite=overwrite,
                allow_limited_export=allow_limited_export,
            )
            result["elapsed_ms"] = round((time.perf_counter() - start) * 1000, 2)
            return result
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)


def _register_generic_db_query_tool(mcp: FastMCP) -> None:
    @mcp.tool(
        name="db_query",
        title="DB Query",
        description=_build_generic_query_description(),
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
                description="Maximum returned rows, default 200; if truncated=true, continue with LIMIT/OFFSET or narrower filters.",
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
            params_list = _normalize_params(params)
            result = await run_in_thread(
                execute_sql_sync,
                cfg,
                sql_text,
                params_list,
                _normalize_max_rows(max_rows),
                False,
            )
            _annotate_query_result(
                result,
                sql_text=sql_text,
                params=params_list,
                target=None,
            )
            result["elapsed_ms"] = round((time.perf_counter() - start) * 1000, 2)
            return result
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)


def _register_generic_db_export_tool(mcp: FastMCP) -> None:
    @mcp.tool(
        name="db_export",
        title="DB Export",
        description=_build_generic_export_description(),
        annotations={
            "readOnlyHint": False,
            "destructiveHint": False,
            "idempotentHint": False,
            "openWorldHint": False,
        },
    )
    async def db_export(
        query_handle: Annotated[
            str,
            Field(
                description="Opaque handle previously returned by db_query. Preferred for exports.",
                title="Query Handle",
            ),
        ] = "",
        sql: Annotated[
            str,
            Field(
                description="Fallback SQL query (read-only statements only). Use when query_handle is unavailable.",
                title="SQL",
            ),
        ] = "",
        params: Annotated[
            Sequence[Any],
            Field(
                description="Optional SQL positional parameters used with sql when query_handle is not supplied.",
                title="Params",
            ),
        ] = (),
        path: Annotated[
            str,
            Field(
                description="Output path. Use `/workspaces/{user_id}/exports/report.xlsx` to save into the current workspace so later file tools can continue processing it; otherwise a relative path is resolved under the configured export root. If omitted, a timestamped filename is generated automatically.",
                title="Path",
            ),
        ] = "",
        format: Annotated[
            Literal["xlsx", "csv"],
            Field(
                description="Export format. Defaults to xlsx.",
                title="Format",
            ),
        ] = "xlsx",
        sheet_name: Annotated[
            str,
            Field(
                description="Optional worksheet name for xlsx exports.",
                title="Sheet Name",
            ),
        ] = "Sheet1",
        overwrite: Annotated[
            bool,
            Field(
                description="Overwrite the target file if it already exists. Defaults to false.",
                title="Overwrite",
            ),
        ] = False,
        allow_limited_export: Annotated[
            bool,
            Field(
                description="Allow exporting SQL/query_handle that still contains LIMIT/OFFSET. Keep false for formal full exports; set true only when a partial export is intentional.",
                title="Allow Limited Export",
            ),
        ] = False,
    ) -> dict[str, Any]:
        """Export SQL query results directly to a file."""
        start = time.perf_counter()
        try:
            sql_text, params_list = resolve_query_request(
                query_handle=query_handle,
                sql=sql,
                params=params,
                expected_target=None,
            )
            cfg = get_db_config(None, None)
            result = await run_in_thread(
                export_sql_to_file_sync,
                cfg,
                sql_text,
                params_list,
                target=None,
                path=path,
                export_format=format,
                sheet_name=sheet_name,
                overwrite=overwrite,
                allow_limited_export=allow_limited_export,
            )
            result["elapsed_ms"] = round((time.perf_counter() - start) * 1000, 2)
            return result
        except Exception as exc:  # pragma: no cover
            return _error_response(exc)


def register_tools(mcp: FastMCP) -> None:
    targets = load_db_query_targets()
    if not targets:
        _register_generic_db_query_tool(mcp)
        _register_generic_db_export_tool(mcp)
        return

    query_tool_names = _build_tool_names("db_query", targets)
    export_tool_names = _build_tool_names("db_export", targets)
    include_db_key = len({target.db_key for target in targets if target.db_key}) > 1
    for target, query_tool_name, export_tool_name in zip(targets, query_tool_names, export_tool_names):
        schema_hint = _resolve_schema_hint(target)
        identifier_quote_hint = _build_identifier_quote_hint(target)
        query_description = _build_query_description(
            target,
            schema_hint,
            include_db_key,
            identifier_quote_hint,
        )
        export_description = _build_export_description(
            target,
            query_tool_name=query_tool_name,
            include_db_key=include_db_key,
        )
        _register_bound_db_query_tool(
            mcp=mcp,
            tool_name=query_tool_name,
            description=query_description,
            target=target,
        )
        _register_bound_db_export_tool(
            mcp=mcp,
            tool_name=export_tool_name,
            description=export_description,
            target=target,
        )
