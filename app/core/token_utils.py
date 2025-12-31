import json
import math
import re
from typing import Any, Dict, List

# 近似按 4 字节 = 1 token 估算，避免引入额外 tokenizer 依赖
_APPROX_BYTES_PER_TOKEN = 4
# 为每条消息预留少量结构性开销，避免低估
_MESSAGE_TOKEN_OVERHEAD = 4
# 多模态图片的粗略 token 估算值，用于避免 base64 误判触发压缩
_IMAGE_TOKEN_ESTIMATE = 256
_DATA_URL_PATTERN = re.compile(
    r"data:image/[a-zA-Z0-9+.-]+;base64,[A-Za-z0-9+/=\r\n]+"
)


def _estimate_content_tokens(content: Any) -> int:
    """估算消息内容的 token 开销，兼容多模态数组结构。"""
    if not content:
        return 0
    if isinstance(content, str):
        if content.startswith("data:image/"):
            return _IMAGE_TOKEN_ESTIMATE
        if "data:image/" in content:
            # 将内嵌 data URL 替换为短占位，避免 base64 触发误判
            image_count = len(_DATA_URL_PATTERN.findall(content))
            stripped = _DATA_URL_PATTERN.sub("[image]", content)
            return approx_token_count(stripped) + image_count * _IMAGE_TOKEN_ESTIMATE
        return approx_token_count(content)
    if isinstance(content, list):
        total = 0
        for item in content:
            total += _estimate_content_tokens(item)
        return total
    if isinstance(content, dict):
        part_type = str(content.get("type", "")).lower()
        if part_type == "text":
            return approx_token_count(str(content.get("text", "") or ""))
        if part_type == "image_url" or "image_url" in content:
            return _IMAGE_TOKEN_ESTIMATE
        if "text" in content:
            return approx_token_count(str(content.get("text", "") or ""))
    try:
        return approx_token_count(json.dumps(content, ensure_ascii=False))
    except TypeError:
        return approx_token_count(str(content))


def approx_token_count(text: str) -> int:
    """粗略估算文本 token 数量。"""
    if not text:
        return 0
    return math.ceil(len(text) / _APPROX_BYTES_PER_TOKEN)


def trim_text_to_tokens(text: str, max_tokens: int, suffix: str = "...(truncated)") -> str:
    """按粗略 token 上限裁剪文本，避免单条内容撑爆上下文预算。"""
    if not text:
        return ""
    if max_tokens <= 0:
        return suffix
    if approx_token_count(text) <= max_tokens:
        return text
    suffix_text = suffix or ""
    suffix_tokens = approx_token_count(suffix_text)
    if max_tokens <= suffix_tokens:
        max_chars = max(1, max_tokens * _APPROX_BYTES_PER_TOKEN)
        return suffix_text[:max_chars]
    max_chars = max_tokens * _APPROX_BYTES_PER_TOKEN - len(suffix_text)
    trimmed = text[: max(0, max_chars)]
    return trimmed + suffix_text


def estimate_message_tokens(message: Dict[str, Any]) -> int:
    """估算单条消息的 token 开销。"""
    content = message.get("content", "")
    content_tokens = _estimate_content_tokens(content)
    # 思考类模型会返回 reasoning_content，需计入上下文与占用统计
    reasoning = message.get("reasoning_content") or message.get("reasoning") or ""
    if not isinstance(reasoning, str):
        try:
            reasoning = json.dumps(reasoning, ensure_ascii=False)
        except TypeError:
            reasoning = str(reasoning)
    return content_tokens + approx_token_count(reasoning) + _MESSAGE_TOKEN_OVERHEAD


def estimate_messages_tokens(messages: List[Dict[str, Any]]) -> int:
    """估算消息列表的总 token 开销。"""
    return sum(estimate_message_tokens(message) for message in messages)


def trim_messages_to_budget(messages: List[Dict[str, Any]], max_tokens: int) -> List[Dict[str, Any]]:
    """按预算保留最近消息，避免上下文持续膨胀。"""
    if not messages:
        return []
    if max_tokens <= 0:
        return [messages[-1]]

    selected: List[Dict[str, Any]] = []
    remaining = max_tokens
    for message in reversed(messages):
        cost = estimate_message_tokens(message)
        if cost <= remaining:
            selected.append(message)
            remaining -= cost
            continue
        if not selected:
            # 最后一条消息必须保留，确保上下文连续
            selected.append(message)
        break

    return list(reversed(selected))
