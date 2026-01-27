from __future__ import annotations

import base64
from datetime import date, datetime, time as time_type, timedelta
from decimal import Decimal
from typing import Any, Sequence

from .config import DbConfig

READ_ONLY_PREFIXES = ("select", "show", "describe", "explain", "with")


def open_connection(cfg: DbConfig):
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


def _quote_identifier(name: str) -> str:
    return f"`{name}`"


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


def fetch_schema_sync(cfg: DbConfig, tables: list[str] | None) -> dict[str, Any]:
    connection = open_connection(cfg)
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


def list_tables_sync(cfg: DbConfig, pattern: str | None, limit: int) -> dict[str, Any]:
    connection = open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            query = (
                "SELECT TABLE_NAME, TABLE_COMMENT, ENGINE, TABLE_ROWS "
                "FROM information_schema.tables "
                "WHERE table_schema = %s"
            )
            params: list[Any] = [cfg.database]
            if pattern:
                query += " AND TABLE_NAME LIKE %s"
                params.append(pattern)
            query += " ORDER BY TABLE_NAME LIMIT %s"
            params.append(limit)
            cursor.execute(query, params)
            rows = cursor.fetchall()

        tables = [
            {
                "name": row[0],
                "comment": row[1],
                "engine": row[2],
                "rows_estimate": _normalize_value(row[3]),
            }
            for row in rows
        ]
        return {
            "ok": True,
            "database": cfg.database,
            "count": len(tables),
            "tables": tables,
        }
    finally:
        connection.close()


def describe_table_sync(cfg: DbConfig, table: str) -> dict[str, Any]:
    connection = open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            query = (
                "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE, COLUMN_KEY, "
                "COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT "
                "FROM information_schema.columns "
                "WHERE table_schema = %s AND table_name = %s "
                "ORDER BY ORDINAL_POSITION"
            )
            cursor.execute(query, (cfg.database, table))
            rows = cursor.fetchall()

        columns = [
            {
                "name": row[0],
                "type": row[1],
                "nullable": row[2] == "YES",
                "key": row[3],
                "default": _normalize_value(row[4]),
                "extra": row[5],
                "comment": row[6],
            }
            for row in rows
        ]
        if not columns:
            return {
                "ok": False,
                "error": f"Table '{table}' not found.",
                "database": cfg.database,
                "table": table,
            }
        return {
            "ok": True,
            "database": cfg.database,
            "table": table,
            "columns": columns,
        }
    finally:
        connection.close()


def preview_rows_sync(
    cfg: DbConfig,
    table: str,
    columns: list[str] | None,
    limit: int,
    order_by: str | None,
    order_desc: bool,
) -> dict[str, Any]:
    connection = open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            column_sql = "*"
            if columns:
                column_sql = ", ".join(_quote_identifier(col) for col in columns)
            sql = f"SELECT {column_sql} FROM {_quote_identifier(table)}"
            if order_by:
                direction = "DESC" if order_desc else "ASC"
                sql += f" ORDER BY {_quote_identifier(order_by)} {direction}"
            sql += " LIMIT %s"
            cursor.execute(sql, (limit,))
            rows = cursor.fetchall()
            columns_out = [col[0] for col in cursor.description] if cursor.description else []

        result_rows = [_normalize_row(row, columns_out) for row in rows]
        return {
            "ok": True,
            "database": cfg.database,
            "table": table,
            "columns": columns_out,
            "rows": result_rows,
            "row_count": len(result_rows),
        }
    finally:
        connection.close()


def count_rows_sync(cfg: DbConfig, table: str) -> dict[str, Any]:
    connection = open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            sql = f"SELECT COUNT(*) AS total FROM {_quote_identifier(table)}"
            cursor.execute(sql)
            row = cursor.fetchone()
        count_value = row[0] if row else 0
        return {
            "ok": True,
            "database": cfg.database,
            "table": table,
            "count": _normalize_value(count_value),
        }
    finally:
        connection.close()


def ping_db_sync(cfg: DbConfig) -> None:
    connection = open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            cursor.execute("SELECT 1")
            cursor.fetchone()
    finally:
        connection.close()


def execute_sql_sync(
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

    connection = open_connection(cfg)
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
