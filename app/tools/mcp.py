from typing import Any, Dict, List, Optional
from urllib.parse import urlparse

from fastmcp.client.client import Client

from app.core.auth import API_KEY_HEADER, AUTHORIZATION_HEADER, normalize_api_key
from app.core.config import WunderConfig
from app.core.i18n import t


class MCPClient:
    """基于 fastmcp 的 MCP 客户端封装。"""

    def __init__(
        self, config: WunderConfig, *, servers: Optional[List[Any]] = None
    ) -> None:
        """支持传入自定义服务列表，避免强依赖全局配置。"""
        target = servers if servers is not None else config.mcp.servers
        self._servers = {server.name: server for server in target}
        # MCP 调用超时设置，避免服务无响应导致线程长时间阻塞。
        self._timeout_s = self._normalize_timeout(getattr(config.mcp, "timeout_s", None))
        # 缓存 API Key，供本地 MCP 调用自动注入。
        self._api_key = normalize_api_key(getattr(config.security, "api_key", ""))

    @staticmethod
    def _normalize_timeout(timeout_s: Optional[int]) -> Optional[int]:
        """统一处理 MCP 超时，小于等于 0 视为不设置。"""
        if timeout_s is None:
            return None
        try:
            value = int(timeout_s)
        except (TypeError, ValueError):
            return None
        if value <= 0:
            return None
        return value

    def _should_attach_api_key(self, server) -> bool:
        """判断是否需要为指定 MCP 服务注入 API Key。"""
        if not self._api_key:
            return False
        endpoint = str(getattr(server, "endpoint", "") or "")
        if not endpoint:
            return False
        parsed = urlparse(endpoint)
        path = (parsed.path or "").rstrip("/")
        # 优先根据路径判断为本地自托管 MCP 服务，避免误发给外部服务。
        if path.endswith("/wunder/mcp"):
            return True
        server_name = str(getattr(server, "name", "") or "").strip().lower()
        return server_name == "wunder"

    def _build_transport_config(self, server) -> Dict[str, Any]:
        """根据服务配置构建 fastmcp 传输配置。"""
        transport = server.transport or getattr(server, "type", None)
        headers = dict(server.headers or {})
        if self._should_attach_api_key(server):
            # 已显式配置认证头时不覆盖，避免意外替换外部认证策略。
            existing_keys = {key.lower() for key in headers}
            if API_KEY_HEADER.lower() not in existing_keys and AUTHORIZATION_HEADER.lower() not in existing_keys:
                headers[API_KEY_HEADER] = self._api_key
        server_config: Dict[str, Any] = {
            "url": server.endpoint,
            "headers": headers,
        }
        if transport:
            server_config["transport"] = transport
        if server.description:
            server_config["description"] = server.description
        if server.auth:
            server_config["auth"] = server.auth
        return {"mcpServers": {server.name: server_config}}

    def _get_server(self, server_name: str):
        if server_name not in self._servers:
            raise ValueError(t("error.mcp_server_not_found", name=server_name))
        server = self._servers[server_name]
        if not getattr(server, "enabled", True):
            raise ValueError(t("error.mcp_server_disabled", name=server_name))
        return server

    @staticmethod
    def _serialize_content_blocks(blocks: List[Any]) -> List[Dict[str, Any]]:
        """将 MCP 内容块转换为可序列化结构。"""
        output: List[Dict[str, Any]] = []
        for block in blocks:
            if hasattr(block, "model_dump"):
                output.append(block.model_dump())
            else:
                output.append({"type": "unknown", "value": str(block)})
        return output

    async def call_tool(self, server_name: str, tool_name: str, args: Dict[str, Any]) -> Dict[str, Any]:
        """调用指定 MCP Server 的工具。"""
        server = self._get_server(server_name)
        if server.allow_tools and tool_name not in server.allow_tools:
            raise ValueError(t("error.mcp_tool_not_allowed"))

        transport_config = self._build_transport_config(server)
        async with Client(transport_config, timeout=self._timeout_s) as client:
            result = await client.call_tool(tool_name, arguments=args, raise_on_error=False)
        return {
            "content": self._serialize_content_blocks(result.content),
            "structured_content": result.structured_content,
            "meta": result.meta,
            "data": result.data,
            "is_error": result.is_error,
        }

    async def list_tools(self, server_name: str) -> List[Dict[str, Any]]:
        """列出指定 MCP Server 的工具清单。"""
        server = self._get_server(server_name)
        transport_config = self._build_transport_config(server)
        async with Client(transport_config, timeout=self._timeout_s) as client:
            tools = await client.list_tools()

        tool_list = []
        for tool in tools:
            if hasattr(tool, "model_dump"):
                tool_data = tool.model_dump()
            else:
                tool_data = {"name": getattr(tool, "name", ""), "description": str(tool)}
            if server.allow_tools and tool_data.get("name") not in server.allow_tools:
                continue
            tool_list.append(tool_data)
        return tool_list
