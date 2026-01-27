from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Literal

from .common.config import get_config_section, get_section_value
from .common.env import parse_int

Transport = Literal["stdio", "sse", "streamable-http"]


@dataclass(frozen=True)
class McpRuntimeConfig:
    transport: Transport
    host: str
    port: int


def validate_transport(value: str) -> Transport:
    if value not in ("stdio", "sse", "streamable-http"):
        raise ValueError("MCP_TRANSPORT must be stdio, sse, or streamable-http.")
    return value  # type: ignore[return-value]


def get_mcp_runtime_config() -> McpRuntimeConfig:
    config = get_config_section("mcp")
    transport_raw = (
        os.getenv("MCP_TRANSPORT")
        or get_section_value(config, "transport")
        or "stdio"
    )
    transport = validate_transport(str(transport_raw).lower())
    host = os.getenv("MCP_HOST") or str(
        get_section_value(config, "host") or "127.0.0.1"
    )
    port = parse_int(
        os.getenv("MCP_PORT") or str(get_section_value(config, "port") or ""),
        8000,
    )
    return McpRuntimeConfig(transport=transport, host=host, port=port)
