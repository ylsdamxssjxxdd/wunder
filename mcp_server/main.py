#!/usr/bin/env python3
"""
FastMCP server entrypoint for the wunder MCP service.

Database configuration:
  MCP_CONFIG_PATH (optional JSON config file, default: mcp_server/mcp_config.json)
  Database settings are read from mcp_config.json (single DB or database.targets).

Optional MCP runtime:
  MCP_TRANSPORT (stdio | sse | streamable-http, default: stdio)
  MCP_HOST (default: 127.0.0.1)
  MCP_PORT (default: 8000)

Run:
  python3 -m mcp_server.main
"""

from __future__ import annotations

from mcp.server.fastmcp import FastMCP

from .runtime import get_mcp_runtime_config
from .tools import register_all


def build_server() -> FastMCP:
    runtime = get_mcp_runtime_config()
    mcp = FastMCP("wunder_mcp", host=runtime.host, port=runtime.port)
    register_all(mcp)
    return mcp


def main() -> None:
    runtime = get_mcp_runtime_config()
    mcp = FastMCP("wunder_mcp", host=runtime.host, port=runtime.port)
    register_all(mcp)
    mcp.run(transport=runtime.transport)


if __name__ == "__main__":
    main()
