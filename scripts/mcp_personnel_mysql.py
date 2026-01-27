#!/usr/bin/env python3
"""
Sample FastMCP server that exposes a personnel MySQL database.

Features:
- Return database schema (tables + columns)
- Execute SQL and return results

Environment variables (choose either PERSONNEL_DB_* or MYSQL_*):
  PERSONNEL_DB_HOST / MYSQL_HOST (default: 127.0.0.1)
  PERSONNEL_DB_PORT / MYSQL_PORT (default: 3306)
  PERSONNEL_DB_USER / MYSQL_USER (default: root)
  PERSONNEL_DB_PASSWORD / MYSQL_PASSWORD (default: "")
  PERSONNEL_DB_NAME / MYSQL_DATABASE / MYSQL_DB (required if not provided in tool input)

Optional MCP runtime:
  MCP_TRANSPORT (stdio | sse | streamable-http, default: stdio)
  MCP_HOST (default: 127.0.0.1)
  MCP_PORT (default: 8000)

Dependencies:
  pip install fastmcp pymysql
"""

from __future__ import annotations

import asyncio
import base64
import os
import time
from dataclasses import dataclass
from datetime import date, datetime, time as time_type, timedelta
from decimal import Decimal
from typing import Any, Iterable, Literal, Sequence

from mcp.server.fastmcp import FastMCP
from pydantic import BaseModel, ConfigDict, Field

READ_ONLY_PREFIXES = ("select", "show", "describe", "explain", "with")
MAX_TABLE_FILTERS = 50


@dataclass(frozen=True)
class DbConfig:
    host: str
    port: int
    user: str
    password: str
    database: str
    connect_timeout: int


class SchemaInput(BaseModel):
    """Input for personnel_get_schema."""

    model_config = ConfigDict(extra="forbid", str_strip_whitespace=True)

    database: str | None = Field(
        default=None,
        description="Database name. If omitted, uses PERSONNEL_DB_NAME/MYSQL_DATABASE.",
        min_length=1,
        max_length=128,
    )
    tables: list[str] | None = Field(
        default=None,
        description="Optional table whitelist to reduce schema size.",
        max_items=MAX_TABLE_FILTERS,
    )


class QueryInput(BaseModel):
    """Input for personnel_query."""

    model_config = ConfigDict(extra="forbid", str_strip_whitespace=True)

    sql: str = Field(
        ...,
        description="SQL statement to execute.",
        min_length=1,
        max_length=10000,
    )
    params: list[str | int | float | bool | None] | None = Field(
        default=None,
        description="Optional parameters for the SQL statement.",
    )
    database: str | None = Field(
        default=None,
        description="Database name. If omitted, uses PERSONNEL_DB_NAME/MYSQL_DATABASE.",
        min_length=1,
        max_length=128,
    )
    max_rows: int = Field(
        default=200,
        description="Max rows to return for result sets.",
        ge=1,
        le=5000,
    )
    allow_write: bool = Field(
        default=False,
        description="Allow non read-only SQL when true.",
    )


def _env_first(*keys: str, default: str | None = None) -> str | None:
    for key in keys:
        value = os.getenv(key)
        if value:
            return value
    return default


def _parse_int(value: str | None, fallback: int) -> int:
    if not value:
        return fallback
    try:
        return int(value)
    except ValueError:
        return fallback


def _get_db_config(database_override: str | None) -> DbConfig:
    host = _env_first("PERSONNEL_DB_HOST", "MYSQL_HOST", default="127.0.0.1")
    port = _parse_int(_env_first("PERSONNEL_DB_PORT", "MYSQL_PORT"), 3306)
    user = _env_first("PERSONNEL_DB_USER", "MYSQL_USER", default="root")
    password = _env_first("PERSONNEL_DB_PASSWORD", "MYSQL_PASSWORD", default="")
    database = database_override or _env_first(
        "PERSONNEL_DB_NAME",
        "MYSQL_DATABASE",
        "MYSQL_DB",
        default="",
    )
    if not database:
        raise ValueError(
            "Database name is required. Set PERSONNEL_DB_NAME or pass database in tool input."
        )
    connect_timeout = _parse_int(
        _env_first("PERSONNEL_DB_CONNECT_TIMEOUT", "MYSQL_CONNECT_TIMEOUT"),
        5,
    )
    return DbConfig(
        host=host,
        port=port,
        user=user,
        password=password,
        database=database,
        connect_timeout=connect_timeout,
    )


def _open_connection(cfg: DbConfig):
    try:
        import pymysql
    except ImportError as exc:  # pragma: no cover
        raise RuntimeError("Missing dependency: pip install pymysql") from exc

    return pymysql.connect(
        host=cfg.host,
        port=cfg.port,
        user=cfg.user,
        password=cfg.password,
        database=cfg.database,
        charset="utf8mb4",
        autocommit=True,
        connect_timeout=cfg.connect_timeout,
    )


def _strip_sql_comments(sql: str) -> str:
    text = sql.lstrip()
    while True:
        if text.startswith("--"):
            newline_idx = text.find("\n")
            text = "" if newline_idx == -1 else text[newline_idx + 1 :].lstrip()
            continue
        if text.startswith("/*"):
            end_idx = text.find("*/")
            text = "" if end_idx == -1 else text[end_idx + 2 :].lstrip()
            continue
        return text


def _has_multiple_statements(sql: str) -> bool:
    trimmed = sql.strip()
    if ";" not in trimmed:
        return False
    without_trailing = trimmed.rstrip(";")
    return ";" in without_trailing


def _is_read_only_sql(sql: str) -> bool:
    stripped = _strip_sql_comments(sql)
    if not stripped:
        return False
    return stripped.lower().startswith(READ_ONLY_PREFIXES)


def _normalize_value(value: Any) -> Any:
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    if isinstance(value, Decimal):
        return str(value)
    if isinstance(value, (datetime, date, time_type)):
        return value.isoformat()
    if isinstance(value, timedelta):
        return str(value)
    if isinstance(value, (bytes, bytearray, memoryview)):
        raw = bytes(value)
        try:
            return raw.decode("utf-8")
        except UnicodeDecodeError:
            return "base64:" + base64.b64encode(raw).decode("ascii")
    return str(value)


def _normalize_row(row: Sequence[Any], columns: Sequence[str]) -> dict[str, Any]:
    return {col: _normalize_value(val) for col, val in zip(columns, row)}


def _fetch_schema_sync(cfg: DbConfig, tables: list[str] | None) -> dict[str, Any]:
    connection = _open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            filtered_tables = [t for t in (tables or []) if t]
            table_query = (
                "SELECT TABLE_NAME, TABLE_COMMENT "
                "FROM information_schema.tables "
                "WHERE table_schema = %s"
            )
            params: list[Any] = [cfg.database]
            if filtered_tables:
                placeholders = ", ".join(["%s"] * len(filtered_tables))
                table_query += f" AND TABLE_NAME IN ({placeholders})"
                params.extend(filtered_tables)
            table_query += " ORDER BY TABLE_NAME"
            cursor.execute(table_query, params)
            table_rows = cursor.fetchall()

            column_query = (
                "SELECT TABLE_NAME, COLUMN_NAME, DATA_TYPE, IS_NULLABLE, "
                "COLUMN_KEY, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT "
                "FROM information_schema.columns "
                "WHERE table_schema = %s"
            )
            column_params: list[Any] = [cfg.database]
            if filtered_tables:
                placeholders = ", ".join(["%s"] * len(filtered_tables))
                column_query += f" AND TABLE_NAME IN ({placeholders})"
                column_params.extend(filtered_tables)
            column_query += " ORDER BY TABLE_NAME, ORDINAL_POSITION"
            cursor.execute(column_query, column_params)
            column_rows = cursor.fetchall()

        tables_map: dict[str, dict[str, Any]] = {}
        for table_name, table_comment in table_rows:
            tables_map[table_name] = {
                "name": table_name,
                "comment": table_comment,
                "columns": [],
            }
        for (
            table_name,
            column_name,
            data_type,
            is_nullable,
            column_key,
            column_default,
            extra,
            column_comment,
        ) in column_rows:
            table_entry = tables_map.get(table_name)
            if table_entry is None:
                table_entry = {"name": table_name, "comment": None, "columns": []}
                tables_map[table_name] = table_entry
            table_entry["columns"].append(
                {
                    "name": column_name,
                    "type": data_type,
                    "nullable": is_nullable == "YES",
                    "key": column_key,
                    "default": _normalize_value(column_default),
                    "extra": extra,
                    "comment": column_comment,
                }
            )

        return {
            "ok": True,
            "database": cfg.database,
            "table_count": len(tables_map),
            "tables": list(tables_map.values()),
        }
    finally:
        connection.close()


def _execute_sql_sync(
    cfg: DbConfig,
    sql: str,
    params: list[str | int | float | bool | None] | None,
    max_rows: int,
    allow_write: bool,
) -> dict[str, Any]:
    if _has_multiple_statements(sql):
        return {"ok": False, "error": "Multiple statements are not allowed."}
    if not allow_write and not _is_read_only_sql(sql):
        return {
            "ok": False,
            "error": "Only read-only SQL is allowed. Set allow_write=true to override.",
        }

    connection = _open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            cursor.execute(sql, params or ())
            if cursor.description:
                columns = [col[0] for col in cursor.description]
                rows = cursor.fetchmany(max_rows + 1)
                truncated = len(rows) > max_rows
                if truncated:
                    rows = rows[:max_rows]
                result_rows = [_normalize_row(row, columns) for row in rows]
                return {
                    "ok": True,
                    "columns": columns,
                    "rows": result_rows,
                    "row_count": len(result_rows),
                    "rows_truncated": truncated,
                }

            return {
                "ok": True,
                "columns": [],
                "rows": [],
                "row_count": cursor.rowcount,
                "rows_truncated": False,
                "lastrowid": cursor.lastrowid,
            }
    finally:
        connection.close()


async def _run_in_thread(func, *args, **kwargs):
    return await asyncio.to_thread(func, *args, **kwargs)


MCP_TRANSPORT = os.getenv("MCP_TRANSPORT", "stdio").lower()
MCP_HOST = os.getenv("MCP_HOST", "127.0.0.1")
MCP_PORT = _parse_int(os.getenv("MCP_PORT"), 8000)

mcp = FastMCP("personnel_mcp", host=MCP_HOST, port=MCP_PORT)


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
        cfg = _get_db_config(params.database)
        return await _run_in_thread(_fetch_schema_sync, cfg, params.tables)
    except Exception as exc:  # pragma: no cover
        return {"ok": False, "error": str(exc)}


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
        cfg = _get_db_config(params.database)
        result = await _run_in_thread(
            _execute_sql_sync,
            cfg,
            params.sql,
            params.params,
            params.max_rows,
            params.allow_write,
        )
        result["elapsed_ms"] = round((time.perf_counter() - start) * 1000, 2)
        return result
    except Exception as exc:  # pragma: no cover
        return {"ok": False, "error": str(exc)}


def _validate_transport(value: str) -> Literal["stdio", "sse", "streamable-http"]:
    if value not in ("stdio", "sse", "streamable-http"):
        raise ValueError("MCP_TRANSPORT must be stdio, sse, or streamable-http.")
    return value  # type: ignore[return-value]


if __name__ == "__main__":
    transport = _validate_transport(MCP_TRANSPORT)
    mcp.run(transport=transport)
