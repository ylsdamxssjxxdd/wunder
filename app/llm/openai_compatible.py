import asyncio
import json
import random
from typing import Any, AsyncGenerator, Dict, List, Optional

import httpx

from app.core.config import LLMConfig
from app.core.http_client import get_async_client
from app.llm.base import LLMClient, LLMUnavailableError, LLMResponse, LLMStreamChunk


class OpenAICompatibleClient(LLMClient):
    """兼容 OpenAI API 风格的模型客户端。"""

    def __init__(self, config: LLMConfig) -> None:
        self._config = config
        self._endpoint = self._normalize_base_url(config.base_url)

    @staticmethod
    def _normalize_base_url(base_url: str) -> str:
        """规范化 base_url，确保指向 /v1。"""
        if not base_url:
            return ""
        endpoint = base_url.rstrip("/")
        if not endpoint.endswith("/v1"):
            endpoint = f"{endpoint}/v1"
        return endpoint

    def _build_payload(
        self, messages: List[Dict[str, Any]], stream: bool, include_usage: bool = False
    ) -> Dict[str, Any]:
        """构建 OpenAI 兼容请求体，注入温度等参数。"""
        payload: Dict[str, Any] = {
            "model": self._config.model,
            "messages": messages,
            "stream": stream,
        }
        if stream and include_usage:
            # 部分 OpenAI 兼容实现支持返回 usage 统计
            payload["stream_options"] = {"include_usage": True}
        if self._config.temperature is not None:
            payload["temperature"] = self._config.temperature
        # 最大输出长度映射到 OpenAI 兼容字段 max_tokens
        if self._config.max_output is not None and self._config.max_output > 0:
            payload["max_tokens"] = self._config.max_output
        # stop 序列用于截断工具调用输出，避免额外内容影响解析
        stop_raw = self._config.stop or []
        if isinstance(stop_raw, str):
            stop_raw = [stop_raw]
        stop_list = [str(item).strip() for item in stop_raw if str(item).strip()]
        if stop_list:
            payload["stop"] = stop_list
        return payload

    @staticmethod
    def _should_retry(status_code: Optional[int]) -> bool:
        """判断是否需要重试，主要覆盖限流与暂时性服务错误。"""
        if status_code is None:
            return True
        return status_code in {408, 409, 425, 429, 500, 502, 503, 504}

    @staticmethod
    def _compute_backoff(attempt: int) -> float:
        """计算重试等待时间，带轻微抖动避免拥堵。"""
        base = min(2 ** max(attempt - 1, 0), 8)
        return base + random.uniform(0.0, 0.3)

    @staticmethod
    def _extract_error_detail(resp: httpx.Response) -> Dict[str, Any]:
        """提取错误详情，兼容 JSON 与纯文本响应。"""
        detail: Dict[str, Any] = {"status_code": resp.status_code}
        try:
            payload = resp.json()
            detail["error"] = payload
        except json.JSONDecodeError:
            text = (resp.text or "").strip()
            if text:
                detail["error"] = text[:1000]
        return detail

    @staticmethod
    def _normalize_usage(raw: Any) -> Optional[Dict[str, int]]:
        """统一 usage 字段格式，兼容不同 OpenAI 兼容实现。"""
        if not isinstance(raw, dict):
            return None

        def _as_int(value: Any) -> Optional[int]:
            if isinstance(value, bool):
                return None
            if isinstance(value, int):
                return value
            if isinstance(value, float):
                return int(value)
            if isinstance(value, str) and value.strip().isdigit():
                return int(value.strip())
            return None

        input_tokens = _as_int(raw.get("input_tokens"))
        if input_tokens is None:
            input_tokens = _as_int(raw.get("prompt_tokens"))
        output_tokens = _as_int(raw.get("output_tokens"))
        if output_tokens is None:
            output_tokens = _as_int(raw.get("completion_tokens"))
        total_tokens = _as_int(raw.get("total_tokens"))
        if total_tokens is None:
            if input_tokens is None and output_tokens is None:
                return None
            total_tokens = (input_tokens or 0) + (output_tokens or 0)

        return {
            "input_tokens": input_tokens or 0,
            "output_tokens": output_tokens or 0,
            "total_tokens": total_tokens,
        }

    async def complete(self, messages: List[Dict[str, Any]]) -> LLMResponse:
        """一次性获取完整回复。"""
        if not self._endpoint or not self._config.api_key:
            raise LLMUnavailableError("LLM 未配置 base_url 或 api_key。")

        payload = self._build_payload(messages, stream=False)
        headers = {"Authorization": f"Bearer {self._config.api_key}"}
        timeout = httpx.Timeout(self._config.timeout_s)
        attempts = max(1, int(self._config.retry or 1))
        last_error: Optional[Exception] = None
        for attempt in range(1, attempts + 1):
            try:
                client = await get_async_client()
                resp = await client.post(
                    f"{self._endpoint}/chat/completions",
                    json=payload,
                    headers=headers,
                    timeout=timeout,
                )
                if resp.status_code == 200:
                    data = resp.json()
                    message = data["choices"][0].get("message") or {}
                    content = message.get("content") or ""
                    reasoning = (
                        message.get("reasoning_content")
                        or message.get("reasoning")
                        or ""
                    )
                    usage = self._normalize_usage(data.get("usage"))
                    # 思考类模型会额外返回 reasoning_content，这里统一透传
                    return LLMResponse(
                        content=str(content or ""),
                        reasoning=str(reasoning or ""),
                        usage=usage,
                    )
                if attempt < attempts and self._should_retry(resp.status_code):
                    await asyncio.sleep(self._compute_backoff(attempt))
                    continue
                raise LLMUnavailableError(
                    "LLM 请求失败。",
                    detail=self._extract_error_detail(resp),
                )
            except (httpx.HTTPError, json.JSONDecodeError) as exc:
                last_error = exc
                if attempt < attempts:
                    await asyncio.sleep(self._compute_backoff(attempt))
                    continue
                raise LLMUnavailableError(
                    "LLM 请求失败。",
                    detail={"error": str(exc), "endpoint": self._endpoint},
                ) from exc
        raise LLMUnavailableError(
            "LLM 请求失败。",
            detail={"error": str(last_error) if last_error else "", "endpoint": self._endpoint},
        )

    async def stream_complete(
        self, messages: List[Dict[str, Any]]
    ) -> AsyncGenerator[LLMStreamChunk, None]:
        """流式获取回复内容，逐段输出。"""
        if not self._endpoint or not self._config.api_key:
            raise LLMUnavailableError("LLM 未配置 base_url 或 api_key。")

        include_usage = bool(self._config.stream_include_usage)
        usage_fallback = include_usage
        headers = {"Authorization": f"Bearer {self._config.api_key}"}
        timeout = httpx.Timeout(self._config.timeout_s)
        attempts = max(1, int(self._config.retry or 1))
        last_error: Optional[Exception] = None
        for attempt in range(1, attempts + 1):
            emitted_chunks = 0
            saw_done = False
            saw_data_line = False
            try:
                payload = self._build_payload(messages, stream=True, include_usage=include_usage)
                client = await get_async_client()
                async with client.stream(
                    "POST",
                    f"{self._endpoint}/chat/completions",
                    json=payload,
                    headers=headers,
                    timeout=timeout,
                ) as resp:
                    if resp.status_code != 200:
                        detail = self._extract_error_detail(resp)
                        if usage_fallback and include_usage and resp.status_code in {400, 422}:
                            # 部分服务端不支持 stream_options，尝试去掉 usage 参数后重试
                            include_usage = False
                            usage_fallback = False
                            continue
                        if attempt < attempts and self._should_retry(resp.status_code):
                            await asyncio.sleep(self._compute_backoff(attempt))
                            continue
                        raise LLMUnavailableError("LLM 请求失败。", detail=detail)
                    async for line in resp.aiter_lines():
                        if not line:
                            continue
                        if not line.startswith("data: "):
                            continue
                        data = line[6:].strip()
                        saw_data_line = True
                        if data == "[DONE]":
                            saw_done = True
                            return
                        try:
                            chunk = json.loads(data)
                        except json.JSONDecodeError:
                            continue
                        usage = self._normalize_usage(chunk.get("usage"))
                        choices = chunk.get("choices") or []
                        delta = choices[0].get("delta") if choices else {}
                        if not isinstance(delta, dict):
                            delta = {}
                        content = delta.get("content") or ""
                        reasoning = (
                            delta.get("reasoning_content")
                            or delta.get("reasoning")
                            or ""
                        )
                        if content or reasoning or usage:
                            emitted_chunks += 1
                            yield LLMStreamChunk(
                                content=str(content or ""),
                                reasoning=str(reasoning or ""),
                                usage=usage,
                            )
                if not saw_done:
                    # 流式连接提前断开时抛出可重试异常，交由上层决定是否重连
                    if emitted_chunks == 0 and attempt < attempts:
                        await asyncio.sleep(self._compute_backoff(attempt))
                        continue
                    raise LLMUnavailableError(
                        "LLM 流式响应中断。",
                        detail={
                            "stream_incomplete": True,
                            "endpoint": self._endpoint,
                            "emitted_chunks": emitted_chunks,
                            "saw_data_line": saw_data_line,
                        },
                    )
                return
            except (httpx.HTTPError, json.JSONDecodeError) as exc:
                last_error = exc
                # 已经输出过内容时不在此处重试，交由上层触发重连并通知前端回滚
                if emitted_chunks == 0 and attempt < attempts:
                    await asyncio.sleep(self._compute_backoff(attempt))
                    continue
                raise LLMUnavailableError(
                    "LLM 流式响应中断。",
                    detail={
                        "stream_incomplete": True,
                        "error": str(exc),
                        "endpoint": self._endpoint,
                        "emitted_chunks": emitted_chunks,
                    },
                ) from exc
        raise LLMUnavailableError(
            "LLM 请求失败。",
            detail={"error": str(last_error) if last_error else "", "endpoint": self._endpoint},
        )


_PRIMARY_CONTEXT_KEYS = (
    "context_length",
    "context_window",
    "max_context",
    "max_context_length",
    "context_tokens",
    "max_model_len",
    "max_sequence_length",
    "max_input_tokens",
)

_FALLBACK_CONTEXT_KEYS = (
    "max_total_tokens",
    "max_tokens",
)

_LLAMA_CPP_CONTEXT_KEYS = (
    "n_ctx",
    "n_ctx_train",
)


def _extract_int(value: Any) -> Optional[int]:
    """从返回值中提取正整数。"""
    if isinstance(value, bool):
        return None
    if isinstance(value, int) and value > 0:
        return value
    if isinstance(value, float) and value > 0:
        return int(value)
    if isinstance(value, str):
        stripped = value.strip()
        if stripped.isdigit():
            return int(stripped)
    return None


def _find_context_value(payload: Any, keys: tuple[str, ...]) -> Optional[int]:
    """递归查找可能的上下文长度字段。"""
    if isinstance(payload, dict):
        for key in keys:
            if key in payload:
                parsed = _extract_int(payload.get(key))
                if parsed:
                    return parsed
        for value in payload.values():
            found = _find_context_value(value, keys)
            if found:
                return found
        return None
    if isinstance(payload, list):
        for item in payload:
            found = _find_context_value(item, keys)
            if found:
                return found
    return None


def _select_model_entry(payload: Dict[str, Any], model: str) -> Optional[Dict[str, Any]]:
    """从模型列表中匹配指定模型项。"""
    candidates = payload.get("data") or payload.get("models") or payload.get("result") or []
    if not isinstance(candidates, list):
        return None
    for item in candidates:
        if not isinstance(item, dict):
            continue
        if item.get("id") == model or item.get("name") == model or item.get("model") == model:
            return item
    return None


def _normalize_llama_props_url(base_url: str) -> Optional[str]:
    """组装 llama.cpp /props 请求地址，便于读取服务端默认生成配置。"""
    if not base_url:
        return None
    cleaned = base_url.strip()
    if not cleaned:
        return None
    # llama.cpp 的 /props 位于服务根路径，若用户传入 /v1 需要先剥离
    cleaned = cleaned.rstrip("/")
    if cleaned.endswith("/v1"):
        cleaned = cleaned[:-3].rstrip("/")
    return f"{cleaned}/props"


def _extract_llama_props_context(payload: Any) -> Optional[int]:
    """从 llama.cpp /props 响应中提取最大上下文长度（n_ctx）。"""
    if not isinstance(payload, dict):
        return None
    # 优先读取 default_generation_settings.n_ctx，这是 llama.cpp 推荐的默认上下文配置
    settings = payload.get("default_generation_settings")
    value = _find_context_value(settings, _LLAMA_CPP_CONTEXT_KEYS)
    if value:
        return value
    # 次优读取 model_meta.n_ctx_train，兼容模型元信息里暴露的训练上下文长度
    model_meta = payload.get("model_meta")
    value = _find_context_value(model_meta, _LLAMA_CPP_CONTEXT_KEYS)
    if value:
        return value
    # 最后在整个响应里兜底搜索，提升对不同版本的兼容性
    return _find_context_value(payload, _LLAMA_CPP_CONTEXT_KEYS)


async def probe_openai_context_window(
    base_url: str,
    api_key: str,
    model: str,
    timeout_s: int = 15,
) -> Optional[int]:
    """尝试从 OpenAI 兼容服务探测模型最大上下文长度。"""
    endpoint = OpenAICompatibleClient._normalize_base_url(base_url)
    if not endpoint or not model:
        return None
    headers: Dict[str, str] = {}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    timeout = httpx.Timeout(max(timeout_s, 5))

    client = await get_async_client()
    # 优先请求单模型详情
    try:
        detail_resp = await client.get(
            f"{endpoint}/models/{model}", headers=headers, timeout=timeout
        )
        if detail_resp.status_code == 200:
            detail = detail_resp.json()
            value = _find_context_value(detail, _PRIMARY_CONTEXT_KEYS)
            if value:
                return value
            value = _find_context_value(detail, _FALLBACK_CONTEXT_KEYS)
            if value:
                return value
    except (httpx.HTTPError, json.JSONDecodeError):
        pass

    # 回退到模型列表
    try:
        list_resp = await client.get(f"{endpoint}/models", headers=headers, timeout=timeout)
        if list_resp.status_code == 200:
            payload = list_resp.json()
            entry = _select_model_entry(payload, model)
            target = entry if entry is not None else payload
            value = _find_context_value(target, _PRIMARY_CONTEXT_KEYS)
            if value:
                return value
            value = _find_context_value(target, _FALLBACK_CONTEXT_KEYS)
            if value:
                return value
    except (httpx.HTTPError, json.JSONDecodeError):
        pass

    # llama.cpp 服务可以通过 /props 获取默认生成设置，直接读取 n_ctx 作为上下文长度
    props_url = _normalize_llama_props_url(base_url)
    if props_url:
        try:
            props_resp = await client.get(props_url, headers=headers, timeout=timeout)
            if props_resp.status_code == 200:
                props_payload = props_resp.json()
                value = _extract_llama_props_context(props_payload)
                if value:
                    return value
        except (httpx.HTTPError, json.JSONDecodeError):
            pass

    return None
