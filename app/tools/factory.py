from typing import Any, Dict, Optional

from app.core.config import LLMConfig, WunderConfig, resolve_llm_config
from app.core.i18n import t
from app.tools.catalog import build_builtin_tool_handlers
from app.tools.registry import ToolRegistry
from app.tools.specs import build_eva_tool_specs
from app.tools.types import ToolContext, ToolResult
from app.knowledge.service import (
    KnowledgeQueryError,
    build_knowledge_tool_specs,
    list_enabled_bases,
    query_knowledge_documents,
)


def build_tool_registry(
    config: WunderConfig, *, llm_config: Optional[LLMConfig] = None
) -> ToolRegistry:
    """构建内置工具、知识库与 MCP 工具注册表。"""
    registry = ToolRegistry()
    specs = build_eva_tool_specs()
    active_llm = llm_config or resolve_llm_config(config)[1]

    for name, handler in build_builtin_tool_handlers().items():
        spec = specs.get(name)
        if spec:
            registry.register(spec, handler)

    # 注册字面知识库工具：每个知识库对应一个独立工具
    knowledge_specs = {
        spec.name: spec
        for spec in build_knowledge_tool_specs(config, blocked_names=set(specs.keys()))
    }
    for base in list_enabled_bases(config):
        spec = knowledge_specs.get(base.name)
        if not spec:
            continue
        if registry.has_tool(spec.name):
            continue

        async def _knowledge_tool(
            tool_ctx: ToolContext, args: Dict[str, Any], base_config=base
        ) -> ToolResult:
            """触发知识库检索并返回命中结果。"""
            query = str(args.get("query", "")).strip()
            if not query:
                return ToolResult(
                    ok=False, data={}, error=t("error.knowledge_query_required")
                )
            limit = args.get("limit")

            def _log_request(payload: Dict[str, Any]) -> None:
                # 通过上下文回调上报知识库请求，供调试面板展示
                if tool_ctx.emit_event:
                    tool_ctx.emit_event("knowledge_request", payload)
            try:
                docs = await query_knowledge_documents(
                    query=query,
                    base=base_config,
                    llm_config=active_llm,
                    limit=limit,
                    request_logger=_log_request,
                )
            except KnowledgeQueryError as exc:
                return ToolResult(ok=False, data={}, error=str(exc))
            return ToolResult(
                ok=True,
                data={
                    "knowledge_base": base_config.name,
                    "documents": [doc.to_dict() for doc in docs],
                },
            )

        registry.register(spec, _knowledge_tool)
    return registry
