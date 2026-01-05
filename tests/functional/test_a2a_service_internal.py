import pytest

from app.core.config import A2AServiceConfig, resolve_llm_config
from app.memory.workspace import WorkspaceContext
from app.orchestrator.context import RequestContext
from app.orchestrator.tool_executor import ToolExecutor
from app.skills.registry import SkillRegistry
from app.tools import builtin
from app.tools.mcp import MCPClient
from app.tools.registry import ToolRegistry
from app.tools.types import ToolResult


@pytest.mark.asyncio
async def test_a2a_internal_service_injects_user_id(orchestrator, monkeypatch):
    """验证 internal A2A 服务会自动注入配置的 user_id。"""
    config = orchestrator.config.model_copy(deep=True)
    config.a2a.services = [
        A2AServiceConfig(
            name="wunder",
            endpoint="http://127.0.0.1:8000/a2a",
            service_type="internal",
            user_id="preset_user",
            enabled=True,
        )
    ]
    llm_name, llm_config = resolve_llm_config(config)
    ctx = RequestContext(
        config=config,
        llm_config=llm_config,
        llm_name=llm_name,
        tools=ToolRegistry(),
        skills=SkillRegistry(),
        mcp_client=MCPClient(config),
        workspace_manager=orchestrator.workspace_manager,
    )
    workspace_root = orchestrator.workspace_manager.ensure_workspace("tester")
    workspace = WorkspaceContext(user_id="tester", session_id="sess-1", root=workspace_root)

    captured = {}

    def fake_delegate(context, args):
        captured["args"] = dict(args)
        return ToolResult(ok=True, data={})

    monkeypatch.setattr(builtin, "a2a_delegate", fake_delegate)

    executor = ToolExecutor(orchestrator.user_tool_manager)
    result, _events = await executor.execute(
        "a2a@wunder",
        {"content": "hi"},
        ctx,
        workspace,
    )

    assert result.ok is True
    assert captured["args"].get("user_id") == "preset_user"
