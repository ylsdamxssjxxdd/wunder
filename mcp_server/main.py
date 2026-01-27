#!/usr/bin/env python3
"""
FastMCP server entrypoint for the wunder MCP service.

Environment variables (single DB or default target):
  MCP_CONFIG_PATH (optional JSON config file, default: mcp_server/mcp_config.json)
  PERSONNEL_DB_HOST / MYSQL_HOST (default: 127.0.0.1)
  PERSONNEL_DB_PORT / MYSQL_PORT (default: 3306)
  PERSONNEL_DB_USER / MYSQL_USER (default: root)
  PERSONNEL_DB_PASSWORD / MYSQL_PASSWORD (default: "")
  PERSONNEL_DB_NAME / MYSQL_DATABASE / MYSQL_DB (required if not provided in tool input)
  PERSONNEL_DB_TYPE (mysql | postgres, default: mysql)
  PGHOST/PGPORT/PGUSER/PGPASSWORD/PGDATABASE (PostgreSQL alternatives)
  PERSONNEL_DB_TARGETS / PERSONNEL_DB_TARGETS_PATH (JSON map for multi-db targets)
  PERSONNEL_DB_DEFAULT (default db_key when using PERSONNEL_DB_TARGETS)

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
