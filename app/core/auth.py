from __future__ import annotations

from typing import Mapping, Optional

API_KEY_HEADER = "X-API-Key"
AUTHORIZATION_HEADER = "Authorization"
_BEARER_PREFIX = "bearer "


def normalize_api_key(value: Optional[str]) -> str:
    """规范化 API Key 字符串，统一去掉空白并避免 None。"""
    return (value or "").strip()


def extract_api_key(headers: Mapping[str, str]) -> str:
    """从请求头中提取 API Key，优先 X-API-Key，其次 Authorization: Bearer。"""
    # 先查自定义头，避免误解析其它认证方案
    api_key = (headers.get(API_KEY_HEADER) or "").strip()
    if api_key:
        return api_key
    # 再查标准 Authorization: Bearer 形式
    auth = (headers.get(AUTHORIZATION_HEADER) or "").strip()
    if not auth:
        return ""
    if auth.lower().startswith(_BEARER_PREFIX):
        return auth[len(_BEARER_PREFIX) :].strip()
    return ""
