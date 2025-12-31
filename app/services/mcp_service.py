from typing import Dict, List

from app.core.config import MCPConfig, MCPServerConfig, get_config
from app.schemas.wunder import McpToolsRequest, McpToolsResponse
from app.tools.mcp import MCPClient


async def fetch_mcp_tools(request: McpToolsRequest) -> McpToolsResponse:
    """连接 MCP 服务并返回工具清单。"""
    server = MCPServerConfig(
        name=request.name,
        endpoint=request.endpoint,
        allow_tools=[],
        enabled=True,
        transport=request.transport,
        headers=request.headers,
        auth=request.auth,
    )
    # 复用全局配置，确保携带 API Key 等安全参数。
    base_config = get_config()
    config = base_config.model_copy(
        update={"mcp": MCPConfig(servers=[server], timeout_s=base_config.mcp.timeout_s)}
    )
    client = MCPClient(config)
    tools = await client.list_tools(request.name)

    items: List[Dict[str, object]] = []
    for tool in tools:
        name = str(tool.get("name", "")).strip()
        if not name:
            continue
        items.append(
            {
                "name": name,
                "description": str(tool.get("description", "")).strip(),
                "input_schema": tool.get("inputSchema") or tool.get("input_schema") or {},
            }
        )
    return McpToolsResponse(tools=items)
