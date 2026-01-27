from __future__ import annotations

import json
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, unquote, urlparse

from ...common.env import env_first, parse_int

DEFAULT_DB_KEY = "default"


@dataclass(frozen=True)
class DbConfig:
    host: str
    port: int
    user: str
    password: str
    database: str
    connect_timeout: int


def _parse_mysql_dsn(dsn: str, key: str) -> DbConfig:
    parsed = urlparse(dsn)
    if parsed.scheme not in ("mysql", "mariadb"):
        raise ValueError(f"Invalid DSN scheme for '{key}': {parsed.scheme}")
    host = parsed.hostname or "127.0.0.1"
    port = parsed.port or 3306
    user = unquote(parsed.username or "")
    password = unquote(parsed.password or "")
    database = parsed.path.lstrip("/")
    if not database:
        raise ValueError(f"Missing database name in DSN for '{key}'")
    params = parse_qs(parsed.query)
    connect_timeout = parse_int(
        params.get("connect_timeout", [None])[0],
        5,
    )
    return DbConfig(
        host=host,
        port=port,
        user=user or "root",
        password=password,
        database=database,
        connect_timeout=connect_timeout,
    )


def _parse_target_config(key: str, raw: Any) -> DbConfig:
    if isinstance(raw, str):
        return _parse_mysql_dsn(raw, key)
    if not isinstance(raw, dict):
        raise ValueError(f"Invalid config for '{key}': expected object or DSN string")
    if "dsn" in raw:
        return _parse_mysql_dsn(str(raw["dsn"]), key)
    host = raw.get("host") or "127.0.0.1"
    port = parse_int(str(raw.get("port")) if raw.get("port") is not None else None, 3306)
    user = raw.get("user") or "root"
    password = raw.get("password") or ""
    database = raw.get("database") or ""
    if not database:
        raise ValueError(f"Missing database name for '{key}'")
    connect_timeout = parse_int(
        str(raw.get("connect_timeout")) if raw.get("connect_timeout") is not None else None,
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


def _load_db_targets_raw() -> dict[str, Any] | None:
    raw = env_first("PERSONNEL_DB_TARGETS")
    if raw:
        parsed = json.loads(raw)
        if not isinstance(parsed, dict):
            raise ValueError("PERSONNEL_DB_TARGETS must be a JSON object")
        return parsed
    path = env_first("PERSONNEL_DB_TARGETS_PATH")
    if path:
        content = Path(path).read_text(encoding="utf-8")
        parsed = json.loads(content)
        if not isinstance(parsed, dict):
            raise ValueError("PERSONNEL_DB_TARGETS_PATH must point to a JSON object")
        return parsed
    return None


def load_db_targets() -> dict[str, DbConfig] | None:
    raw_targets = _load_db_targets_raw()
    if raw_targets is None:
        return None
    return {key: _parse_target_config(key, value) for key, value in raw_targets.items()}


def _single_db_config(database_override: str | None) -> DbConfig:
    host = env_first("PERSONNEL_DB_HOST", "MYSQL_HOST", default="127.0.0.1")
    port = parse_int(env_first("PERSONNEL_DB_PORT", "MYSQL_PORT"), 3306)
    user = env_first("PERSONNEL_DB_USER", "MYSQL_USER", default="root")
    password = env_first("PERSONNEL_DB_PASSWORD", "MYSQL_PASSWORD", default="")
    database = database_override or env_first(
        "PERSONNEL_DB_NAME",
        "MYSQL_DATABASE",
        "MYSQL_DB",
        default="",
    )
    if not database:
        raise ValueError(
            "Database name is required. Set PERSONNEL_DB_NAME or pass database in tool input."
        )
    connect_timeout = parse_int(
        env_first("PERSONNEL_DB_CONNECT_TIMEOUT", "MYSQL_CONNECT_TIMEOUT"),
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


def get_default_db_key() -> str:
    return env_first("PERSONNEL_DB_DEFAULT", default=DEFAULT_DB_KEY) or DEFAULT_DB_KEY


def get_db_config(database_override: str | None, db_key: str | None) -> DbConfig:
    targets = load_db_targets()
    if targets:
        target_key = db_key or get_default_db_key()
        if target_key not in targets:
            available = ", ".join(sorted(targets))
            raise ValueError(
                f"Unknown db_key '{target_key}'. Available keys: {available}"
            )
        selected = targets[target_key]
        if database_override:
            return replace(selected, database=database_override)
        return selected
    if db_key:
        raise ValueError("db_key provided but PERSONNEL_DB_TARGETS is not configured.")
    return _single_db_config(database_override)


def summarize_db_targets(db_key: str | None) -> dict[str, Any]:
    targets = load_db_targets()
    default_key = get_default_db_key()
    if targets:
        keys = sorted(targets)
        if db_key:
            if db_key not in targets:
                available = ", ".join(keys)
                raise ValueError(
                    f"Unknown db_key '{db_key}'. Available keys: {available}"
                )
            keys = [db_key]
        summaries = [
            {
                "key": key,
                "host": targets[key].host,
                "port": targets[key].port,
                "user": targets[key].user,
                "database": targets[key].database,
                "password_set": bool(targets[key].password),
            }
            for key in keys
        ]
        return {
            "ok": True,
            "default_key": default_key,
            "count": len(summaries),
            "targets": summaries,
        }

    cfg = _single_db_config(None)
    if db_key and db_key != default_key:
        raise ValueError(
            f"Only '{default_key}' is available without PERSONNEL_DB_TARGETS."
        )
    summaries = [
        {
            "key": default_key,
            "host": cfg.host,
            "port": cfg.port,
            "user": cfg.user,
            "database": cfg.database,
            "password_set": bool(cfg.password),
        }
    ]
    return {
        "ok": True,
        "default_key": default_key,
        "count": 1,
        "targets": summaries,
    }
