import asyncio
import json
from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Dict, List

from app.core.errors import ErrorCodes, WunderError
from app.tools.types import ToolContext, ToolResult


ToolCallable = Callable[[ToolContext, Dict[str, Any]], Awaitable[ToolResult]]


@dataclass
class ToolSpec:
    """工具描述信息，用于提示词构建。"""

    name: str
    description: str
    args_schema: Dict[str, Any]

    def to_prompt_text(self) -> str:
        """转换为 EVA 风格的工具描述文本。"""
        cached = getattr(self, "_prompt_text", None)
        if isinstance(cached, str):
            # 缓存 JSON 文本以减少重复序列化开销
            return cached
        payload = {
            "name": self.name,
            "description": self.description,
            "arguments": self.args_schema,
        }
        rendered = json.dumps(payload, ensure_ascii=False)
        # ToolSpec 是只读对象，缓存序列化结果可复用并降低 CPU 消耗
        setattr(self, "_prompt_text", rendered)
        return rendered


class ToolRegistry:
    """工具注册与执行管理。"""

    def __init__(self) -> None:
        self._tools: Dict[str, ToolCallable] = {}
        self._specs: Dict[str, ToolSpec] = {}

    def register(self, spec: ToolSpec, func: ToolCallable) -> None:
        """注册一个工具实现。"""
        self._tools[spec.name] = func
        self._specs[spec.name] = spec

    def has_tool(self, name: str) -> bool:
        """检查工具是否已注册。"""
        return name in self._tools

    def list_specs(self) -> List[ToolSpec]:
        """列出工具规格，用于提示词展示。"""
        return list(self._specs.values())

    async def execute(self, name: str, context: ToolContext, args: Dict[str, Any]) -> ToolResult:
        """执行指定工具，并捕获异常。"""
        if name not in self._tools:
            raise WunderError(
                code=ErrorCodes.TOOL_NOT_FOUND,
                message=f"未找到工具: {name}",
            )
        try:
            return await self._tools[name](context, args)
        except WunderError:
            raise
        except Exception as exc:
            raise WunderError(
                code=ErrorCodes.TOOL_EXECUTION_ERROR,
                message=f"工具执行失败: {name}",
                detail={"error": str(exc)},
            ) from exc


def ensure_async(func: Callable[[ToolContext, Dict[str, Any]], ToolResult]) -> ToolCallable:
    """将同步工具包装为异步执行，避免阻塞事件循环。"""

    async def _wrapper(context: ToolContext, args: Dict[str, Any]) -> ToolResult:
        return await asyncio.to_thread(func, context, args)

    return _wrapper
