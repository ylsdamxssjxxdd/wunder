from __future__ import annotations

import base64
import re
from datetime import date, datetime, time as time_type, timedelta
from decimal import Decimal
from typing import Any, Sequence

from .config import DbConfig

READ_ONLY_PREFIXES = ("select", "show", "describe", "explain", "with")


def open_connection(cfg: DbConfig):
    if cfg.engine == "postgres":
        try:
            import psycopg
        except ImportError as exc:  # pragma: no cover
            raise RuntimeError("Missing dependency: pip install psycopg") from exc
        return psycopg.connect(
            host=cfg.host,
            port=cfg.port,
            user=cfg.user,
            password=cfg.password,
            dbname=cfg.database,
            connect_timeout=cfg.connect_timeout,
            autocommit=True,
        )

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


def _quote_identifier(cfg: DbConfig, name: str) -> str:
    if cfg.engine == "postgres":
        return f"\"{name}\""
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


TABLE_REFERENCE_PATTERN = re.compile(
    r'\b(?:from|join)\s+((?:`[^`]+`|"[^"]+"|[A-Za-z_][\w$]*)(?:\s*\.\s*(?:`[^`]+`|"[^"]+"|[A-Za-z_][\w$]*))?)',
    re.IGNORECASE,
)
DISALLOWED_KEYWORDS_PATTERN = re.compile(
    r"\b(?:insert|update|delete|replace|alter|drop|truncate|create|grant|revoke|call|set)\b",
    re.IGNORECASE,
)
SYSTEM_SCHEMA_PATTERN = re.compile(
    r"\b(?:information_schema|pg_catalog|mysql\.|performance_schema)\b",
    re.IGNORECASE,
)


def _strip_single_quoted_literals(sql: str) -> str:
    output: list[str] = []
    in_single_quote = False
    idx = 0
    while idx < len(sql):
        char = sql[idx]
        if in_single_quote:
            if char == "'":
                if idx + 1 < len(sql) and sql[idx + 1] == "'":
                    idx += 2
                    continue
                in_single_quote = False
            idx += 1
            continue
        if char == "'":
            in_single_quote = True
            idx += 1
            continue
        output.append(char)
        idx += 1
    return "".join(output)


def _normalize_identifier_token(token: str) -> tuple[str | None, str] | None:
    text = token.strip().rstrip(',').strip()
    if not text:
        return None

    parts = [part.strip() for part in re.split(r"\s*\.\s*", text) if part.strip()]
    if not parts:
        return None

    normalized_parts: list[str] = []
    for part in parts:
        if (
            len(part) >= 2
            and ((part.startswith('`') and part.endswith('`')) or (part.startswith('"') and part.endswith('"')))
        ):
            part = part[1:-1]
        normalized_parts.append(part.strip())

    if not normalized_parts:
        return None
    if len(normalized_parts) == 1:
        return (None, normalized_parts[0].lower())

    return (normalized_parts[-2].lower(), normalized_parts[-1].lower())


def _extract_table_references(sql: str) -> list[tuple[str | None, str]]:
    text = _strip_single_quoted_literals(_strip_sql_comments(sql))
    references: list[tuple[str | None, str]] = []
    for match in TABLE_REFERENCE_PATTERN.finditer(text):
        normalized = _normalize_identifier_token(match.group(1))
        if normalized is None:
            continue
        references.append(normalized)
    return references


def validate_sql_against_target_table(sql: str, cfg: DbConfig, table: str) -> str | None:
    cleaned = _strip_single_quoted_literals(_strip_sql_comments(sql)).lower()
    if DISALLOWED_KEYWORDS_PATTERN.search(cleaned):
        return "Only SELECT/EXPLAIN/WITH read-only SQL is allowed."
    if SYSTEM_SCHEMA_PATTERN.search(cleaned):
        return "System schema queries are blocked for table-bound db_query tools."

    references = _extract_table_references(sql)
    if not references:
        return f"SQL must include FROM/JOIN on bound table '{table}'."

    expected_table = table.lower()
    expected_database = cfg.database.lower()
    for schema, actual_table in references:
        if actual_table != expected_table:
            return f"This tool can only query table '{table}'."
        if schema and cfg.engine == "mysql" and schema != expected_database:
            return (
                "Cross-database access is blocked for this tool. "
                f"Use table '{table}' in database '{cfg.database}'."
            )
    return None


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


def _compact_columns(columns: Sequence[dict[str, Any]]) -> list[dict[str, str]]:
    compacted: list[dict[str, str]] = []
    for column in columns:
        name = str(column.get("name") or "").strip()
        column_type = str(column.get("type") or "").strip()
        if not name or not column_type:
            continue
        compacted.append({"name": name, "type": column_type})
    return compacted


def get_table_schema_compact_sync(cfg: DbConfig, table: str) -> dict[str, Any]:
    details = describe_table_sync(cfg, table)
    if not details.get("ok"):
        return details
    return {
        "ok": True,
        "table": table,
        "columns": _compact_columns(details.get("columns") or []),
    }


def fetch_schema_sync(cfg: DbConfig, tables: list[str] | None) -> dict[str, Any]:
    if cfg.engine == "postgres":
        return _fetch_schema_postgres(cfg, tables)
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


def _fetch_schema_postgres(cfg: DbConfig, tables: list[str] | None) -> dict[str, Any]:
    connection = open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            filtered_tables = [t for t in (tables or []) if t]
            table_query = (
                "SELECT c.relname, obj_description(c.oid) "
                "FROM pg_class c "
                "JOIN pg_namespace n ON n.oid = c.relnamespace "
                "WHERE n.nspname = current_schema() AND c.relkind = 'r'"
            )
            params: list[Any] = []
            if filtered_tables:
                table_query += " AND c.relname = ANY(%s)"
                params.append(filtered_tables)
            table_query += " ORDER BY c.relname"
            cursor.execute(table_query, params)
            table_rows = cursor.fetchall()

            pk_query = (
                "SELECT c.relname, a.attname "
                "FROM pg_index i "
                "JOIN pg_class c ON c.oid = i.indrelid "
                "JOIN pg_namespace n ON n.oid = c.relnamespace "
                "JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) "
                "WHERE i.indisprimary AND n.nspname = current_schema()"
            )
            pk_params: list[Any] = []
            if filtered_tables:
                pk_query += " AND c.relname = ANY(%s)"
                pk_params.append(filtered_tables)
            cursor.execute(pk_query, pk_params)
            pk_rows = cursor.fetchall()
            pk_set = {(row[0], row[1]) for row in pk_rows}

            column_query = (
                "SELECT c.relname, a.attname, "
                "pg_catalog.format_type(a.atttypid, a.atttypmod), "
                "a.attnotnull, "
                "pg_get_expr(ad.adbin, ad.adrelid), "
                "col_description(a.attrelid, a.attnum), "
                "a.attidentity "
                "FROM pg_attribute a "
                "JOIN pg_class c ON c.oid = a.attrelid "
                "JOIN pg_namespace n ON n.oid = c.relnamespace "
                "LEFT JOIN pg_attrdef ad ON a.attrelid = ad.adrelid AND a.attnum = ad.adnum "
                "WHERE n.nspname = current_schema() AND c.relkind = 'r' "
                "AND a.attnum > 0 AND NOT a.attisdropped"
            )
            column_params: list[Any] = []
            if filtered_tables:
                column_query += " AND c.relname = ANY(%s)"
                column_params.append(filtered_tables)
            column_query += " ORDER BY c.relname, a.attnum"
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
            not_null,
            column_default,
            column_comment,
            identity_flag,
        ) in column_rows:
            table_entry = tables_map.get(table_name)
            if table_entry is None:
                table_entry = {"name": table_name, "comment": None, "columns": []}
                tables_map[table_name] = table_entry
            key_flag = "PRI" if (table_name, column_name) in pk_set else ""
            extra = ""
            if identity_flag and identity_flag != " ":
                extra = "identity"
            elif column_default and str(column_default).startswith("nextval("):
                extra = "auto_increment"
            table_entry["columns"].append(
                {
                    "name": column_name,
                    "type": data_type,
                    "nullable": not not_null,
                    "key": key_flag,
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
    if cfg.engine == "postgres":
        return _list_tables_postgres(cfg, pattern, limit)
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


def _list_tables_postgres(cfg: DbConfig, pattern: str | None, limit: int) -> dict[str, Any]:
    connection = open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            query = (
                "SELECT c.relname, obj_description(c.oid), c.reltuples "
                "FROM pg_class c "
                "JOIN pg_namespace n ON n.oid = c.relnamespace "
                "WHERE n.nspname = current_schema() AND c.relkind = 'r'"
            )
            params: list[Any] = []
            if pattern:
                query += " AND c.relname LIKE %s"
                params.append(pattern)
            query += " ORDER BY c.relname LIMIT %s"
            params.append(limit)
            cursor.execute(query, params)
            rows = cursor.fetchall()

        tables = [
            {
                "name": row[0],
                "comment": row[1],
                "engine": "postgres",
                "rows_estimate": _normalize_value(row[2]),
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
    if cfg.engine == "postgres":
        return _describe_table_postgres(cfg, table)
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


def _describe_table_postgres(cfg: DbConfig, table: str) -> dict[str, Any]:
    connection = open_connection(cfg)
    try:
        with connection.cursor() as cursor:
            pk_query = (
                "SELECT a.attname "
                "FROM pg_index i "
                "JOIN pg_class c ON c.oid = i.indrelid "
                "JOIN pg_namespace n ON n.oid = c.relnamespace "
                "JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) "
                "WHERE i.indisprimary AND n.nspname = current_schema() AND c.relname = %s"
            )
            cursor.execute(pk_query, (table,))
            pk_set = {row[0] for row in cursor.fetchall()}

            query = (
                "SELECT a.attname, "
                "pg_catalog.format_type(a.atttypid, a.atttypmod), "
                "a.attnotnull, "
                "pg_get_expr(ad.adbin, ad.adrelid), "
                "col_description(a.attrelid, a.attnum), "
                "a.attidentity "
                "FROM pg_attribute a "
                "JOIN pg_class c ON c.oid = a.attrelid "
                "JOIN pg_namespace n ON n.oid = c.relnamespace "
                "LEFT JOIN pg_attrdef ad ON a.attrelid = ad.adrelid AND a.attnum = ad.adnum "
                "WHERE n.nspname = current_schema() AND c.relkind = 'r' "
                "AND c.relname = %s AND a.attnum > 0 AND NOT a.attisdropped "
                "ORDER BY a.attnum"
            )
            cursor.execute(query, (table,))
            rows = cursor.fetchall()

        columns = []
        for row in rows:
            column_default = row[3]
            extra = ""
            if row[5] and row[5] != " ":
                extra = "identity"
            elif column_default and str(column_default).startswith("nextval("):
                extra = "auto_increment"
            columns.append(
                {
                    "name": row[0],
                    "type": row[1],
                    "nullable": not row[2],
                    "key": "PRI" if row[0] in pk_set else "",
                    "default": _normalize_value(column_default),
                    "extra": extra,
                    "comment": row[4],
                }
            )
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
                column_sql = ", ".join(_quote_identifier(cfg, col) for col in columns)
            sql = f"SELECT {column_sql} FROM {_quote_identifier(cfg, table)}"
            if order_by:
                direction = "DESC" if order_desc else "ASC"
                sql += f" ORDER BY {_quote_identifier(cfg, order_by)} {direction}"
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
            sql = f"SELECT COUNT(*) AS total FROM {_quote_identifier(cfg, table)}"
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
        return {"ok": False, "error": "不允许执行多条 SQL 语句。"}
    if not allow_write and not _is_read_only_sql(sql):
        return {
            "ok": False,
            "error": "仅允许只读 SQL（SELECT/SHOW/DESCRIBE/EXPLAIN/WITH）。",
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
                    "rows": result_rows,
                    "row_count": len(result_rows),
                    "truncated": truncated,
                }

            return {
                "ok": True,
                "rows": [],
                "row_count": max(cursor.rowcount, 0),
                "truncated": False,
            }
    finally:
        connection.close()
