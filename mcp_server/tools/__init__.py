from __future__ import annotations

from mcp.server.fastmcp import FastMCP

from .database import register_tools as register_database_tools


def register_all(mcp: FastMCP) -> None:
    register_database_tools(mcp)
