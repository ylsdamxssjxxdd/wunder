from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List

from app.core.config import LLMConfig, MCPServerConfig, WunderConfig
from app.memory.workspace import WorkspaceManager
from app.skills.registry import SkillRegistry, SkillSpec
from app.tools.mcp import MCPClient
from app.tools.registry import ToolRegistry, ToolSpec


@dataclass
class RequestContext:
    """单次请求的执行上下文。"""

    config: WunderConfig
    llm_config: LLMConfig
    llm_name: str
    tools: ToolRegistry
    skills: SkillRegistry
    mcp_client: MCPClient
    workspace_manager: WorkspaceManager


@dataclass
class UserToolAlias:
    """用户工具别名映射条目。"""

    kind: str
    owner_id: str
    target: str


@dataclass
class UserSkillSource:
    """用户技能来源目录与技能列表。"""

    root: Path
    names: List[str]


@dataclass
class UserToolBindings:
    """自建/共享工具别名绑定信息。"""

    alias_specs: Dict[str, ToolSpec]
    alias_map: Dict[str, UserToolAlias]
    skill_specs: List[SkillSpec]
    skill_sources: Dict[str, UserSkillSource]
    skill_registries: Dict[str, SkillRegistry]
    mcp_servers: Dict[str, Dict[str, MCPServerConfig]]
    extra_prompt: str
    user_version: float
    shared_version: float
