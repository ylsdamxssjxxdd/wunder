from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Literal

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
    transport = validate_transport(os.getenv("MCP_TRANSPORT", "stdio").lower())
    host = os.getenv("MCP_HOST", "127.0.0.1")
    port = parse_int(os.getenv("MCP_PORT"), 8000)
    return McpRuntimeConfig(transport=transport, host=host, port=port)
