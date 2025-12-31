from dataclasses import dataclass
from typing import Any, AsyncGenerator, Dict, List, Optional


@dataclass
class LLMResponse:
    """LLM 完整回复结构，包含最终输出与思考过程。"""

    content: str
    reasoning: str = ""
    usage: Optional[Dict[str, int]] = None


@dataclass
class LLMStreamChunk:
    """LLM 流式分片结构，兼容输出正文与思考增量。"""

    content: str = ""
    reasoning: str = ""
    usage: Optional[Dict[str, int]] = None

    def is_empty(self) -> bool:
        """判断分片是否为空，避免生成无效事件。"""
        return not self.content and not self.reasoning and not self.usage


class LLMClient:
    """LLM 客户端接口定义。"""

    async def complete(self, messages: List[Dict[str, Any]]) -> LLMResponse:
        """生成完整回复。"""
        raise NotImplementedError

    async def stream_complete(
        self, messages: List[Dict[str, Any]]
    ) -> AsyncGenerator[LLMStreamChunk, None]:
        """流式生成回复。"""
        raise NotImplementedError


class LLMUnavailableError(RuntimeError):
    """当模型不可用时抛出。"""

    def __init__(self, message: str, detail: Optional[Dict[str, Any]] = None) -> None:
        super().__init__(message)
        self.detail = detail or {}
