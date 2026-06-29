from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from ...common.config import get_config_section, get_section_value


@dataclass(frozen=True)
class PptConfig:
    root: Path
    workspace_root: Path
    workspace_public_root: str
    workspace_single_root: bool


def _as_bool(value: Any, default: bool) -> bool:
    if value is None:
        return default
    if isinstance(value, bool):
        return value
    normalized = str(value).strip().lower()
    if normalized in {"1", "true", "yes", "on"}:
        return True
    if normalized in {"0", "false", "no", "off"}:
        return False
    return default


def _default_root(section: dict[str, Any]) -> Path:
    raw = (
        os.getenv("EXTRA_MCP_PPT_ROOT")
        or get_section_value(section, "root", "output_root")
        or ""
    )
    if raw:
        return Path(str(raw)).expanduser()
    workspace_root = Path(os.getenv("WUNDER_WORKSPACE_ROOT") or "/workspaces")
    return workspace_root / ".extra_mcp" / "ppt"


def get_ppt_config() -> PptConfig:
    section = get_config_section("ppt")
    root = _default_root(section).resolve()
    workspace_root = Path(
        os.getenv("WUNDER_WORKSPACE_ROOT")
        or str(get_section_value(section, "workspace_root") or "/workspaces")
    ).expanduser().resolve()
    workspace_public_root = str(
        get_section_value(section, "workspace_public_root") or "/workspaces"
    ).rstrip("/") or "/workspaces"
    workspace_single_root = _as_bool(
        get_section_value(section, "workspace_single_root"),
        False,
    )
    return PptConfig(
        root=root,
        workspace_root=workspace_root,
        workspace_public_root=workspace_public_root,
        workspace_single_root=workspace_single_root,
    )
