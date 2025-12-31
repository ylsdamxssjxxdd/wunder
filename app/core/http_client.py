from __future__ import annotations

import asyncio
from typing import Optional

import httpx


_client: Optional[httpx.AsyncClient] = None
_client_lock = asyncio.Lock()


async def get_async_client() -> httpx.AsyncClient:
    """获取全局复用的 HTTP 客户端，减少重复建连开销。"""
    global _client
    if _client is not None:
        return _client
    async with _client_lock:
        if _client is None:
            _client = httpx.AsyncClient(
                limits=httpx.Limits(max_connections=200, max_keepalive_connections=50),
                follow_redirects=True,
            )
    return _client


async def close_async_client() -> None:
    """关闭全局 HTTP 客户端，释放连接池资源。"""
    global _client
    if _client is None:
        return
    await _client.aclose()
    _client = None
