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
    normalized = re.sub(r"\W+", "_", value.strip().lower(), flags=re.UNICODE)
    normalized = re.sub(r"_+", "_", normalized).strip("_")
    if not normalized:
        normalized = "target"
    if normalized[0].isdigit():
        normalized = f"t_{normalized}"
    return normalized


def _build_tool_names(prefix: str, targets: Sequence[DbQueryTarget]) -> list[str]:
    names: list[str] = []
    seen: dict[str, int] = {}
    for target in targets:
        suffix = _sanitize_tool_suffix(target.name or target.table or target.key)
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
    parts = ["字段：" + ", ".join(column_parts) + tail]
    if example_parts:
        parts.append("已知取值/示例：" + "; ".join(example_parts[:6]))
    if note_parts:
        parts.append("备注：" + "; ".join(note_parts[:4]))
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
            "标识符提示：该表名包含中文或特殊字符；"
            f"在 MySQL 中请使用反引号，例如 FROM `{table}`。"
        )
    if cfg.engine == "postgres":
        return (
            "标识符提示：该表名包含中文或特殊字符；"
            f"在 PostgreSQL 中请使用双引号，例如 FROM \"{table}\"。"
        )
    return ""


def _build_query_description(
    target: DbQueryTarget,
    schema_hint: str,
    include_db_key: bool,
    identifier_quote_hint: str,
) -> str:
    parts = [
        f"对表 {target.table} 执行只读 SQL，并返回紧凑结果。",
        "强约束：查询只能访问这个绑定表。",
        "数据量较大时，优先使用 LIMIT/OFFSET 分页，并尽量收窄筛选条件。",
        "分页查询必须包含稳定的 ORDER BY。",
        "成功调用后会返回 query_handle，可直接传给配套导出工具。",
    ]
    if target.description:
        parts.append(f"用途：{target.description}。")
    if include_db_key and target.db_key:
        parts.append(f"数据库目标：{target.db_key}。")
    if identifier_quote_hint:
        parts.append(identifier_quote_hint)
    if schema_hint:
        parts.append(schema_hint + "。")
    return "".join(parts).strip()


def _build_export_description(
    target: DbQueryTarget,
    *,
    query_tool_name: str,
    include_db_key: bool,
) -> str:
    parts = [
        f"将表 {target.table} 的只读 SQL 结果直接导出为 xlsx 或 csv 文件。",
        "强约束：查询只能访问这个绑定表。",
        f"建议先用 {query_tool_name} 校验计数或小样本，再优先复用返回的 query_handle；正式全量导出时，只应复用不带 LIMIT/OFFSET 的完整明细 query_handle。",
        "适合生成 Excel/CSV 交付物，不要把大量分页结果塞进模型上下文。",
        "如果希望文件直接落到当前工作区，请把 path 写成 `/workspaces/{user_id}/exports/...`；返回结果会包含规范化后的 path 和 workspace_relative_path，便于后续工具继续处理。",
        "默认拒绝仍包含 LIMIT/OFFSET 的 SQL 或 query_handle；只有在明确要导出局部结果时，才将 allow_limited_export 设为 true。",
    ]
    if target.description:
        parts.append(f"用途：{target.description}。")
    if include_db_key and target.db_key:
        parts.append(f"数据库目标：{target.db_key}。")
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
        "执行只读 SQL，并返回紧凑结果。"
        "数据量较大时，优先使用 LIMIT/OFFSET 分页，并尽量收窄筛选条件。"
        "分页查询必须包含稳定的 ORDER BY。"
        "成功调用后会返回 query_handle，可直接传给 db_export。"
    )


def _build_generic_export_description() -> str:
    return (
        "将只读 SQL 结果直接导出为 xlsx 或 csv 文件。"
        "建议先用 db_query 校验计数或小样本，再优先复用返回的 query_handle；正式全量导出时，只应复用不带 LIMIT/OFFSET 的完整明细 query_handle。"
        "适合生成 Excel/CSV 交付物，不要把大量分页结果塞进模型上下文。"
        "如果希望文件直接落到当前工作区，请把 path 写成 `/workspaces/{user_id}/exports/...`；返回结果会包含规范化后的 path 和 workspace_relative_path，便于后续工具继续处理。"
        "默认拒绝仍包含 LIMIT/OFFSET 的 SQL 或 query_handle；只有在明确要导出局部结果时，才将 allow_limited_export 设为 true。"
    )


def _register_bound_db_query_tool(
    mcp: FastMCP,
    tool_name: str,
    description: str,
    target: DbQueryTarget,
) -> None:
    @mcp.tool(
        name=tool_name,
        title=f"数据库查询（{target.table}）",
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
                description="SQL 查询语句，仅允许只读语句。",
                title="SQL 语句",
            ),
        ],
        params: Annotated[
            Sequence[Any],
            Field(
                description="可选的 SQL 位置参数。",
                title="参数",
            ),
        ] = (),
        max_rows: Annotated[
            int,
            Field(
                description="最多返回的行数，默认 200；若结果被截断（truncated=true），请结合 LIMIT/OFFSET 或更窄的筛选条件继续查询。",
                title="最大返回行数",
            ),
        ] = 200,
    ) -> dict[str, Any]:
        """执行 SQL 查询并返回紧凑结果。"""
        start = time.perf_counter()
        try:
            sql_text = sql.strip()
            if not sql_text:
                return {"ok": False, "error": "SQL 语句不能为空。"}
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
        title=f"数据库导出（{target.table}）",
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
                description="由配套 db_query 工具返回的不透明 query_handle。导出时优先使用它。",
                title="查询句柄",
            ),
        ] = "",
        sql: Annotated[
            str,
            Field(
                description="兜底 SQL 查询语句，仅允许只读语句。当 query_handle 不可用时再使用。",
                title="SQL 语句",
            ),
        ] = "",
        params: Annotated[
            Sequence[Any],
            Field(
                description="在未提供 query_handle、改用 sql 时使用的可选 SQL 位置参数。",
                title="参数",
            ),
        ] = (),
        path: Annotated[
            str,
            Field(
                description="导出文件路径。若希望文件直接落到当前工作区，请使用 `/workspaces/{user_id}/exports/report.xlsx` 这类路径，便于后续文件工具继续处理；否则相对路径会解析到配置的导出根目录下。不填时会自动生成带时间戳的文件名。",
                title="输出路径",
            ),
        ] = "",
        format: Annotated[
            Literal["xlsx", "csv"],
            Field(
                description="导出格式，默认 xlsx。",
                title="导出格式",
            ),
        ] = "xlsx",
        sheet_name: Annotated[
            str,
            Field(
                description="xlsx 导出时可选的工作表名称。",
                title="工作表名称",
            ),
        ] = "Sheet1",
        overwrite: Annotated[
            bool,
            Field(
                description="如果目标文件已存在，是否覆盖。默认 false。",
                title="覆盖已有文件",
            ),
        ] = False,
        allow_limited_export: Annotated[
            bool,
            Field(
                description="是否允许导出仍包含 LIMIT/OFFSET 的 SQL 或 query_handle。正式全量导出应保持 false；只有明确要导出局部结果时才设为 true。",
                title="允许局部导出",
            ),
        ] = False,
    ) -> dict[str, Any]:
        """将 SQL 查询结果直接导出到文件。"""
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
        title="数据库查询",
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
                description="SQL 查询语句，仅允许只读语句。",
                title="SQL 语句",
            ),
        ],
        params: Annotated[
            Sequence[Any],
            Field(
                description="可选的 SQL 位置参数。",
                title="参数",
            ),
        ] = (),
        max_rows: Annotated[
            int,
            Field(
                description="最多返回的行数，默认 200；若结果被截断（truncated=true），请结合 LIMIT/OFFSET 或更窄的筛选条件继续查询。",
                title="最大返回行数",
            ),
        ] = 200,
    ) -> dict[str, Any]:
        """执行 SQL 查询并返回紧凑结果。"""
        start = time.perf_counter()
        try:
            sql_text = sql.strip()
            if not sql_text:
                return {"ok": False, "error": "SQL 语句不能为空。"}
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
        title="数据库导出",
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
                description="由 db_query 返回的不透明 query_handle。导出时优先使用它。",
                title="查询句柄",
            ),
        ] = "",
        sql: Annotated[
            str,
            Field(
                description="兜底 SQL 查询语句，仅允许只读语句。当 query_handle 不可用时再使用。",
                title="SQL 语句",
            ),
        ] = "",
        params: Annotated[
            Sequence[Any],
            Field(
                description="在未提供 query_handle、改用 sql 时使用的可选 SQL 位置参数。",
                title="参数",
            ),
        ] = (),
        path: Annotated[
            str,
            Field(
                description="导出文件路径。若希望文件直接落到当前工作区，请使用 `/workspaces/{user_id}/exports/report.xlsx` 这类路径，便于后续文件工具继续处理；否则相对路径会解析到配置的导出根目录下。不填时会自动生成带时间戳的文件名。",
                title="输出路径",
            ),
        ] = "",
        format: Annotated[
            Literal["xlsx", "csv"],
            Field(
                description="导出格式，默认 xlsx。",
                title="导出格式",
            ),
        ] = "xlsx",
        sheet_name: Annotated[
            str,
            Field(
                description="xlsx 导出时可选的工作表名称。",
                title="工作表名称",
            ),
        ] = "Sheet1",
        overwrite: Annotated[
            bool,
            Field(
                description="如果目标文件已存在，是否覆盖。默认 false。",
                title="覆盖已有文件",
            ),
        ] = False,
        allow_limited_export: Annotated[
            bool,
            Field(
                description="是否允许导出仍包含 LIMIT/OFFSET 的 SQL 或 query_handle。正式全量导出应保持 false；只有明确要导出局部结果时才设为 true。",
                title="允许局部导出",
            ),
        ] = False,
    ) -> dict[str, Any]:
        """将 SQL 查询结果直接导出到文件。"""
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
