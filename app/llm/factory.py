from typing import List, Dict, Any, AsyncGenerator

from app.core.config import LLMConfig
from app.core.i18n import t
from app.llm.base import LLMClient, LLMUnavailableError, LLMResponse, LLMStreamChunk
from app.llm.openai_compatible import OpenAICompatibleClient


class MockLLMClient(LLMClient):
    """当模型未配置时的降级实现。"""

    async def complete(self, messages: List[Dict[str, Any]]) -> LLMResponse:
        raise LLMUnavailableError(t("error.llm_not_configured"))

    async def stream_complete(
        self, messages: List[Dict[str, Any]]
    ) -> AsyncGenerator[LLMStreamChunk, None]:
        raise LLMUnavailableError(t("error.llm_not_configured"))


def build_llm_client(config: LLMConfig) -> LLMClient:
    """根据配置创建 LLM 客户端。"""
    if not config.enable:
        return MockLLMClient()
    if config.provider == "openai_compatible":
        return OpenAICompatibleClient(config)
    return MockLLMClient()
