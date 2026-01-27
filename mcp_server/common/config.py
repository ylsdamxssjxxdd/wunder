from __future__ import annotations

import json
import os
from functools import lru_cache
from pathlib import Path
from typing import Any

DEFAULT_CONFIG_FILENAME = "mcp_config.json"


def _resolve_config_path() -> Path | None:
    path_text = os.getenv("MCP_CONFIG_PATH", "").strip()
    if path_text:
        path = Path(path_text)
        if path.is_absolute():
            return path
        cwd_path = (Path.cwd() / path).resolve()
        if cwd_path.exists():
            return cwd_path
        repo_root = Path(__file__).resolve().parents[2]
        return (repo_root / path).resolve()

    default_path = Path(__file__).resolve().parents[1] / DEFAULT_CONFIG_FILENAME
    if default_path.exists():
        return default_path
    return None


@lru_cache(maxsize=1)
def load_mcp_config() -> dict[str, Any]:
    path = _resolve_config_path()
    if path is None or not path.exists():
        return {}
    raw = path.read_text(encoding="utf-8").strip()
    if not raw:
        return {}
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ValueError(f"MCP config JSON parse failed: {path}") from exc
    if not isinstance(data, dict):
        raise ValueError("MCP config root must be a JSON object.")
    return data


def get_config_section(name: str) -> dict[str, Any]:
    config = load_mcp_config()
    section = config.get(name)
    if section is None:
        return {}
    if not isinstance(section, dict):
        raise ValueError(f"MCP config section '{name}' must be a JSON object.")
    return section


def get_section_value(section: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        if key in section and section[key] not in (None, ""):
            return section[key]
    return None
