from __future__ import annotations

from mcp.server.fastmcp import FastMCP

from .personnel import register_tools as register_personnel_tools


def register_all(mcp: FastMCP) -> None:
    register_personnel_tools(mcp)
