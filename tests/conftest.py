from pathlib import Path
from typing import List, Dict, Any, AsyncGenerator

import pytest
import pytest_asyncio
import httpx

from app.main import app
from app.api import wunder as wunder_api
from app.core.config import get_config, resolve_llm_config, WunderConfig
from app.monitor.registry import monitor
import app.orchestrator.engine as engine


class FakeLLMClient:
    """测试用 LLM 客户端，固定输出，避免真实模型调用。"""

    def __init__(self, answer: str = "测试回复") -> None:
        self._answer = answer

    async def complete(self, messages: List[Dict[str, Any]]) -> str:
        # 非流式路径直接返回固定答案，便于断言。
        return self._answer

    async def stream_complete(self, messages: List[Dict[str, Any]]) -> AsyncGenerator[str, None]:
        # 流式路径拆分输出，验证服务端能正确拼接。
        for chunk in ("测试", "回复"):
            yield chunk


@pytest.fixture()
def test_config(tmp_path: Path) -> WunderConfig:
    """构造测试配置：隔离工作区、禁用外部依赖，避免污染真实数据。"""
    base = get_config()
    config = base.model_copy(deep=True)
    config.workspace.root = str(tmp_path / "workspaces")
    llm_name, llm_config = resolve_llm_config(config)
    llm_config.stream = True
    config.llm.models[llm_name] = llm_config
    # 清理技能/MCP，避免扫描或联网开销。
    config.skills.enabled = []
    config.skills.paths = []
    config.mcp.servers = []
    return config


@pytest.fixture()
def orchestrator(test_config: WunderConfig, monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    """替换全局调度器配置，并注入假 LLM。"""
    orchestrator = wunder_api._orchestrator
    orchestrator.apply_config(test_config)

    # 将历史记录与监控日志写入临时目录，避免污染仓库数据。
    workspace_manager = orchestrator.workspace_manager
    workspace_manager._history_root = (tmp_path / "historys").resolve()
    workspace_manager._history_root.mkdir(parents=True, exist_ok=True)

    monitor._history_dir = (tmp_path / "monitor").resolve()
    monitor._history_dir.mkdir(parents=True, exist_ok=True)
    with monitor._lock:
        monitor._sessions.clear()

    fake_client = FakeLLMClient(answer="测试回复")
    monkeypatch.setattr(engine, "build_llm_client", lambda _: fake_client)
    return orchestrator


@pytest_asyncio.fixture()
async def client(orchestrator):
    """使用 ASGITransport 直接调用 FastAPI 应用，避免启动真实服务。"""
    transport = httpx.ASGITransport(app=app)
    api_key = getattr(app.state, "api_key", "")
    headers = {"X-API-Key": api_key} if api_key else {}
    async with httpx.AsyncClient(
        transport=transport, base_url="http://test", headers=headers
    ) as http_client:
        yield http_client
