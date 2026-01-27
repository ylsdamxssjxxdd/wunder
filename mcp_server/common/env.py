from __future__ import annotations

import os


def env_first(*keys: str, default: str | None = None) -> str | None:
    for key in keys:
        value = os.getenv(key)
        if value:
            return value
    return default


def parse_int(value: str | None, fallback: int) -> int:
    if not value:
        return fallback
    try:
        return int(value)
    except ValueError:
        return fallback
