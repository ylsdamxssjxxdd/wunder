from __future__ import annotations

from mcp.server.fastmcp import FastMCP

from .database import register_tools as register_database_tools
from .knowledge import register_tools as register_knowledge_tools


def register_all(mcp: FastMCP) -> None:
    register_database_tools(mcp)
    register_knowledge_tools(mcp)
