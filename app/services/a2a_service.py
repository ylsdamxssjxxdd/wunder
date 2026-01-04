from __future__ import annotations

from typing import Any, Dict, List, Optional
from urllib.parse import urlparse, urlunparse

import httpx

from app.core.http_client import get_async_client


def _normalize_endpoint(endpoint: str) -> str:
    """规范化 A2A 端点地址，补齐协议头。"""
    value = str(endpoint or "").strip()
    if not value:
        return ""
    if not value.startswith(("http://", "https://")):
        value = f"http://{value}"
    return value


def _build_agent_card_urls(endpoint: str) -> List[str]:
    """基于端点地址构造 AgentCard 可能的访问路径。"""
    normalized = _normalize_endpoint(endpoint)
    if not normalized:
        return []
    parsed = urlparse(normalized)
    base = urlunparse(parsed._replace(path="", params="", query="", fragment="")).rstrip("/")
    urls: List[str] = []

    def _append(url: str) -> None:
        if url and url not in urls:
            urls.append(url)

    _append(f"{base}/.well-known/agent-card.json")
    path = (parsed.path or "").rstrip("/")
    if path and path != "/":
        base_path = path
        if base_path.endswith("/a2a"):
            base_path = base_path[:-4]
        base_path = base_path.rstrip("/")
        if base_path:
            _append(f"{base}{base_path}/.well-known/agent-card.json")
    return urls


def _merge_headers(headers: Dict[str, str], auth: Optional[str]) -> Dict[str, str]:
    """合并自定义 Headers 与授权信息。"""
    merged: Dict[str, str] = {}
    for key, value in (headers or {}).items():
        header_key = str(key or "").strip()
        header_value = str(value or "").strip()
        if header_key and header_value:
            merged[header_key] = header_value
    if auth and not any(key.lower() == "authorization" for key in merged):
        auth_value = str(auth).strip()
        if auth_value:
            merged["Authorization"] = (
                auth_value
                if auth_value.lower().startswith("bearer ")
                else f"Bearer {auth_value}"
            )
    return merged


async def fetch_a2a_agent_card(
    endpoint: str,
    headers: Optional[Dict[str, str]] = None,
    auth: Optional[str] = None,
    timeout_s: int = 120,
) -> Dict[str, Any]:
    """拉取 A2A AgentCard，按候选地址依次尝试。"""
    urls = _build_agent_card_urls(endpoint)
    if not urls:
        raise ValueError("A2A 端点不能为空")
    client = await get_async_client()
    merged_headers = _merge_headers(headers or {}, auth)
    last_error = ""
    for url in urls:
        try:
            response = await client.get(url, headers=merged_headers, timeout=timeout_s)
        except httpx.RequestError as exc:
            last_error = f"{url}: {exc}"
            continue
        if response.status_code >= 400:
            last_error = f"{url}: {response.status_code}"
            continue
        try:
            payload = response.json()
        except ValueError:
            last_error = f"{url}: invalid json"
            continue
        if isinstance(payload, dict):
            return payload
        return {"data": payload}
    raise ValueError(f"AgentCard 获取失败：{last_error or '未知错误'}")
