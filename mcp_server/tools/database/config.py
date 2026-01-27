from __future__ import annotations

import json
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, unquote, urlparse

from ...common.config import get_config_section, get_section_value
from ...common.env import env_first, parse_int

DEFAULT_DB_KEY = "default"


@dataclass(frozen=True)
class DbConfig:
    engine: str
    host: str
    port: int
    user: str
    password: str
    database: str
    connect_timeout: int
    description: str | None


def _normalize_engine(raw: str) -> str:
    value = raw.lower()
    if value in ("mysql", "mariadb"):
        return "mysql"
    if value in ("postgres", "postgresql"):
        return "postgres"
    raise ValueError(f"Unsupported database engine: {raw}")


def _get_db_section() -> dict[str, Any]:
    section = get_config_section("database")
    if section:
        return section
    return get_config_section("personnel")


def _get_target_description_map() -> dict[str, str]:
    config = _get_db_section()
    raw = get_section_value(config, "target_descriptions", "descriptions")
    mapping: dict[str, str] = {}
    if isinstance(raw, dict):
        mapping.update({str(key): str(value) for key, value in raw.items() if value})
    default_desc = get_section_value(config, "description", "desc")
    if default_desc:
        mapping.setdefault(get_default_db_key(), str(default_desc))
    return mapping


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
        engine="mysql",
        host=host,
        port=port,
        user=user or "root",
        password=password,
        database=database,
        connect_timeout=connect_timeout,
        description=None,
    )


def _parse_postgres_dsn(dsn: str, key: str) -> DbConfig:
    parsed = urlparse(dsn)
    if parsed.scheme not in ("postgres", "postgresql"):
        raise ValueError(f"Invalid DSN scheme for '{key}': {parsed.scheme}")
    host = parsed.hostname or "127.0.0.1"
    port = parsed.port or 5432
    user = unquote(parsed.username or "") or "postgres"
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
        engine="postgres",
        host=host,
        port=port,
        user=user,
        password=password,
        database=database,
        connect_timeout=connect_timeout,
        description=None,
    )


def _parse_dsn(dsn: str, key: str) -> DbConfig:
    parsed = urlparse(dsn)
    if parsed.scheme in ("mysql", "mariadb"):
        return _parse_mysql_dsn(dsn, key)
    if parsed.scheme in ("postgres", "postgresql"):
        return _parse_postgres_dsn(dsn, key)
    raise ValueError(f"Invalid DSN scheme for '{key}': {parsed.scheme}")


def _parse_target_config(key: str, raw: Any) -> DbConfig:
    if isinstance(raw, str):
        return _parse_dsn(raw, key)
    if not isinstance(raw, dict):
        raise ValueError(f"Invalid config for '{key}': expected object or DSN string")
    if "dsn" in raw:
        cfg = _parse_dsn(str(raw["dsn"]), key)
        description = raw.get("description") or raw.get("desc")
        if description:
            return replace(cfg, description=str(description))
        return cfg
    description = raw.get("description") or raw.get("desc")
    engine = _normalize_engine(raw.get("type") or raw.get("engine") or "mysql")
    host = raw.get("host") or "127.0.0.1"
    port_default = 5432 if engine == "postgres" else 3306
    port = parse_int(
        str(raw.get("port")) if raw.get("port") is not None else None, port_default
    )
    user_default = "postgres" if engine == "postgres" else "root"
    user = raw.get("user") or user_default
    password = raw.get("password") or ""
    database = raw.get("database") or ""
    if not database:
        raise ValueError(f"Missing database name for '{key}'")
    connect_timeout = parse_int(
        str(raw.get("connect_timeout")) if raw.get("connect_timeout") is not None else None,
        5,
    )
    return DbConfig(
        engine=engine,
        host=host,
        port=port,
        user=user,
        password=password,
        database=database,
        connect_timeout=connect_timeout,
        description=str(description) if description else None,
    )


def _load_db_targets_raw() -> dict[str, Any] | None:
    def merge_descriptions(raw_targets: dict[str, Any]) -> dict[str, Any]:
        config = _get_db_section()
        config_targets = get_section_value(config, "targets")
        description_map = _get_target_description_map()
        if not isinstance(config_targets, dict):
            config_targets = {}
        merged: dict[str, Any] = {}
        for key, value in raw_targets.items():
            description = None
            config_value = config_targets.get(key)
            if isinstance(config_value, dict):
                description = config_value.get("description") or config_value.get("desc")
            if not description:
                description = description_map.get(key)
            if isinstance(value, str):
                if description:
                    merged[key] = {"dsn": value, "description": description}
                else:
                    merged[key] = value
                continue
            if isinstance(value, dict):
                if description and not (value.get("description") or value.get("desc")):
                    value = {**value, "description": description}
                merged[key] = value
                continue
            merged[key] = value
        return merged

    raw = env_first("PERSONNEL_DB_TARGETS")
    if raw:
        parsed = json.loads(raw)
        if not isinstance(parsed, dict):
            raise ValueError("PERSONNEL_DB_TARGETS must be a JSON object")
        return merge_descriptions(parsed)
    path = env_first("PERSONNEL_DB_TARGETS_PATH")
    if path:
        content = Path(path).read_text(encoding="utf-8")
        parsed = json.loads(content)
        if not isinstance(parsed, dict):
            raise ValueError("PERSONNEL_DB_TARGETS_PATH must point to a JSON object")
        return merge_descriptions(parsed)
    config = _get_db_section()
    targets = get_section_value(config, "targets")
    if targets is None:
        return None
    if not isinstance(targets, dict):
        raise ValueError("database.targets must be a JSON object")
    return targets


def load_db_targets() -> dict[str, DbConfig] | None:
    raw_targets = _load_db_targets_raw()
    if raw_targets is None:
        return None
    return {key: _parse_target_config(key, value) for key, value in raw_targets.items()}


def _single_db_config(database_override: str | None) -> DbConfig:
    config = _get_db_section()
    engine_raw = env_first(
        "PERSONNEL_DB_TYPE",
        default=get_section_value(config, "db_type", "type", "engine") or "mysql",
    )
    engine = _normalize_engine(engine_raw or "mysql")
    host_keys = [
        "PERSONNEL_DB_HOST",
        "PGHOST" if engine == "postgres" else "MYSQL_HOST",
        "POSTGRES_HOST" if engine == "postgres" else None,
    ]
    host = env_first(
        *[key for key in host_keys if key],
        default=get_section_value(config, "host") or "127.0.0.1",
    )
    port_default = 5432 if engine == "postgres" else 3306
    port_keys = [
        "PERSONNEL_DB_PORT",
        "PGPORT" if engine == "postgres" else "MYSQL_PORT",
        "POSTGRES_PORT" if engine == "postgres" else None,
    ]
    port = parse_int(
        env_first(
            *[key for key in port_keys if key],
            default=str(get_section_value(config, "port") or ""),
        ),
        port_default,
    )
    user_default = "postgres" if engine == "postgres" else "root"
    user_keys = [
        "PERSONNEL_DB_USER",
        "PGUSER" if engine == "postgres" else "MYSQL_USER",
        "POSTGRES_USER" if engine == "postgres" else None,
    ]
    user = env_first(
        *[key for key in user_keys if key],
        default=get_section_value(config, "user") or user_default,
    )

    password_keys = [
        "PERSONNEL_DB_PASSWORD",
        "PGPASSWORD" if engine == "postgres" else "MYSQL_PASSWORD",
        "POSTGRES_PASSWORD" if engine == "postgres" else None,
    ]
    password = env_first(
        *[key for key in password_keys if key],
        default=get_section_value(config, "password") or "",
    )

    database_keys = [
        "PERSONNEL_DB_NAME",
        "PGDATABASE" if engine == "postgres" else "MYSQL_DATABASE",
        "POSTGRES_DB" if engine == "postgres" else "MYSQL_DB",
    ]
    database = database_override or env_first(
        *[key for key in database_keys if key],
        default=get_section_value(config, "database") or "",
    )
    if not database:
        raise ValueError(
            "Database name is required. Set PERSONNEL_DB_NAME or pass database in tool input."
        )
    timeout_keys = [
        "PERSONNEL_DB_CONNECT_TIMEOUT",
        "PGCONNECT_TIMEOUT" if engine == "postgres" else "MYSQL_CONNECT_TIMEOUT",
    ]
    connect_timeout = parse_int(
        env_first(
            *[key for key in timeout_keys if key],
            default=str(get_section_value(config, "connect_timeout") or ""),
        ),
        5,
    )
    description = get_section_value(config, "description", "desc")
    return DbConfig(
        engine=engine,
        host=host,
        port=port,
        user=user,
        password=password,
        database=database,
        connect_timeout=connect_timeout,
        description=str(description) if description else None,
    )


def get_default_db_key() -> str:
    config = _get_db_section()
    return (
        env_first(
            "PERSONNEL_DB_DEFAULT",
            default=get_section_value(config, "default_key") or DEFAULT_DB_KEY,
        )
        or DEFAULT_DB_KEY
    )


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
                "engine": targets[key].engine,
                "host": targets[key].host,
                "port": targets[key].port,
                "user": targets[key].user,
                "database": targets[key].database,
                "password_set": bool(targets[key].password),
                "description": targets[key].description,
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
            "engine": cfg.engine,
            "host": cfg.host,
            "port": cfg.port,
            "user": cfg.user,
            "database": cfg.database,
            "password_set": bool(cfg.password),
            "description": cfg.description,
        }
    ]
    return {
        "ok": True,
        "default_key": default_key,
        "count": 1,
        "targets": summaries,
    }


def build_db_description_hint() -> str:
    targets = load_db_targets()
    if targets:
        description_map = _get_target_description_map()
        items = []
        for key, cfg in targets.items():
            label = cfg.database
            description = cfg.description or description_map.get(key)
            if description:
                label = f"{label}（{description}）"
            items.append(f"{key}={label}")
        return "数据库说明：" + "；".join(items)

    config = _get_db_section()
    database = get_section_value(config, "database")
    description = get_section_value(config, "description", "desc")
    if database:
        label = str(database)
        if description:
            label = f"{label}（{description}）"
        return "数据库说明：" + label
    return ""
