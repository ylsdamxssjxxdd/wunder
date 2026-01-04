import os
import re
from functools import lru_cache
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import yaml
from pydantic import BaseModel, ConfigDict, Field

from app.core.i18n import configure_i18n, t
DEFAULT_CONFIG_PATH = Path("config/wunder.yaml")
DEFAULT_OVERRIDE_PATH = Path("data/config/wunder.override.yaml")
LEGACY_OVERRIDE_PATH = Path("data/config/wunder.yaml")


def _default_builtin_tool_names() -> List[str]:
    """延迟加载内置工具名称，避免模块导入循环。"""
    from app.tools.catalog import list_builtin_tool_names

    # 默认内置工具不自动勾选 a2ui，避免调试面板默认选中。
    return [name for name in list_builtin_tool_names() if name != "a2ui"]


class ServerConfig(BaseModel):
    """服务端基础配置。"""

    host: str = "0.0.0.0"
    port: int = 8000
    stream_chunk_size: int = 1024
    max_active_sessions: int = Field(
        default=30, description="全局最大并发会话数，超过后进入排队等待"
    )


class I18nConfig(BaseModel):
    """多语言配置。"""

    default_language: str = "zh-CN"
    supported_languages: List[str] = Field(default_factory=lambda: ["zh-CN", "en-US"])
    aliases: Dict[str, str] = Field(
        default_factory=lambda: {
            "zh": "zh-CN",
            "zh-cn": "zh-CN",
            "zh-hans": "zh-CN",
            "zh-hans-cn": "zh-CN",
            "en": "en-US",
            "en-us": "en-US",
        }
    )


class LLMConfig(BaseModel):
    """大模型配置。"""

    enable: bool = True
    provider: str = "openai_compatible"
    base_url: str = ""
    api_key: str = ""
    model: str = ""
    temperature: float = 0.7
    timeout_s: int = 60
    retry: int = 1
    max_rounds: int = 10  # 单次请求最大执行轮次
    max_context: Optional[int] = None
    max_output: Optional[int] = None
    support_vision: bool = False
    stream: bool = False
    stream_include_usage: bool = Field(
        default=True, description="流式请求是否尝试返回 usage 统计"
    )
    history_compaction_ratio: float = Field(
        default=0.8, description="历史 token 达到比例时触发压缩"
    )
    history_compaction_reset: str = Field(
        default="zero", description="压缩后历史 token 处理策略（zero/current/keep）"
    )
    stop: List[str] = Field(
        default_factory=lambda: ["</tool_call>"],
        description="模型停止词，优先用于截断工具调用输出。",
    )
    mock_if_unconfigured: bool = True


class LLMGroupConfig(BaseModel):
    """多模型配置集合。"""

    default: str = ""
    models: Dict[str, LLMConfig] = Field(default_factory=dict)

    def resolve(self, name: Optional[str] = None) -> Tuple[str, LLMConfig]:
        """根据模型名称返回实际使用的配置。"""
        cleaned = str(name or "").strip()
        if cleaned and cleaned in self.models:
            return cleaned, self.models[cleaned]
        default_name = str(self.default or "").strip()
        if default_name and default_name in self.models:
            return default_name, self.models[default_name]
        for key, value in self.models.items():
            return key, value
        return "", LLMConfig()


class MCPServerConfig(BaseModel):
    """单个 MCP Server 配置。"""

    name: str
    endpoint: str
    allow_tools: List[str] = Field(default_factory=list)
    enabled: bool = True
    transport: Optional[str] = Field(default=None, description="http/sse/streamable-http")
    description: str = ""
    display_name: str = ""
    headers: Dict[str, str] = Field(default_factory=dict)
    auth: Optional[str] = None
    tool_specs: List[Dict[str, Any]] = Field(default_factory=list)

    model_config = ConfigDict(extra="allow")


class MCPConfig(BaseModel):
    """MCP 配置集合。"""

    timeout_s: int = Field(default=120, description="MCP 请求超时秒数")
    servers: List[MCPServerConfig] = Field(default_factory=list)


class A2AServiceConfig(BaseModel):
    """A2A 服务配置。"""

    name: str
    endpoint: str
    enabled: bool = True
    description: str = ""
    display_name: str = ""
    headers: Dict[str, str] = Field(default_factory=dict)
    auth: Optional[str] = None
    agent_card: Dict[str, Any] = Field(default_factory=dict)
    allow_self: bool = False
    max_depth: int = 0
    default_method: str = "SendMessage"

    model_config = ConfigDict(extra="allow")


class A2AConfig(BaseModel):
    """A2A 服务配置集合。"""

    timeout_s: int = Field(default=120, description="A2A 请求超时秒数")
    services: List[A2AServiceConfig] = Field(default_factory=list)


class SkillsConfig(BaseModel):
    """技能配置。"""

    paths: List[str] = Field(default_factory=list)
    enabled: List[str] = Field(default_factory=list)


class BuiltinToolsConfig(BaseModel):
    """内置工具配置。"""

    enabled: List[str] = Field(default_factory=_default_builtin_tool_names)


class ToolsConfig(BaseModel):
    """工具配置集合。"""

    builtin: BuiltinToolsConfig = Field(default_factory=BuiltinToolsConfig)


class KnowledgeBaseConfig(BaseModel):
    """字面知识库单库配置。"""

    name: str = ""
    description: str = ""
    root: str = ""
    enabled: bool = True


class KnowledgeConfig(BaseModel):
    """字面知识库配置集合。"""

    bases: List[KnowledgeBaseConfig] = Field(default_factory=list)


class WorkspaceConfig(BaseModel):
    """工作区配置。"""

    root: str = "./data/workspaces"
    max_history_items: int = 200
    retention_days: int = 30


class StorageConfig(BaseModel):
    """SQLite 持久化配置。"""

    db_path: str = "./data/wunder.db"


class SecurityConfig(BaseModel):
    """安全与权限配置。"""

    api_key: str = Field(default="", description="API/MCP 访问密钥")
    allow_commands: List[str] = Field(default_factory=list)
    allow_paths: List[str] = Field(default_factory=list)  # 允许工具访问的额外目录白名单
    deny_globs: List[str] = Field(default_factory=list)


class ObservabilityConfig(BaseModel):
    """日志与可观测性配置。"""

    log_level: str = "INFO"
    log_path: str = "./data/logs/wunder.log"
    monitor_event_limit: int = 500
    monitor_payload_max_chars: int = 4000
    monitor_drop_event_types: List[str] = Field(
        default_factory=lambda: ["llm_output_delta"],
        description="监控事件需要丢弃的类型列表，用于控制内存占用。",
    )


class CorsConfig(BaseModel):
    """跨域配置。"""

    allow_origins: List[str] = Field(default_factory=lambda: ["*"])
    allow_credentials: bool = False
    allow_methods: List[str] = Field(default_factory=lambda: ["*"])
    allow_headers: List[str] = Field(default_factory=lambda: ["*"])


class SandboxResourcesConfig(BaseModel):
    """沙盒资源限额配置。"""

    cpu: float = 1.0
    memory_mb: int = 2048
    pids: int = 256


class SandboxConfig(BaseModel):
    """沙盒调度配置。"""

    mode: str = "local"  # local | sandbox
    endpoint: str = "http://127.0.0.1:9001"
    image: str = ""
    container_root: str = "/workspaces"
    network: str = "bridge"
    readonly_rootfs: bool = True
    idle_ttl_s: int = 0
    timeout_s: int = 120
    resources: SandboxResourcesConfig = Field(default_factory=SandboxResourcesConfig)


class WunderConfig(BaseModel):
    """wunder 统一配置模型。"""

    server: ServerConfig = Field(default_factory=ServerConfig)
    i18n: I18nConfig = Field(default_factory=I18nConfig)
    llm: LLMGroupConfig = Field(default_factory=LLMGroupConfig)
    mcp: MCPConfig = Field(default_factory=MCPConfig)
    a2a: A2AConfig = Field(default_factory=A2AConfig)
    skills: SkillsConfig = Field(default_factory=SkillsConfig)
    tools: ToolsConfig = Field(default_factory=ToolsConfig)
    knowledge: KnowledgeConfig = Field(default_factory=KnowledgeConfig)
    workspace: WorkspaceConfig = Field(default_factory=WorkspaceConfig)
    storage: StorageConfig = Field(default_factory=StorageConfig)
    security: SecurityConfig = Field(default_factory=SecurityConfig)
    observability: ObservabilityConfig = Field(default_factory=ObservabilityConfig)
    cors: CorsConfig = Field(default_factory=CorsConfig)
    sandbox: SandboxConfig = Field(default_factory=SandboxConfig)


_ENV_PATTERN = re.compile(r"\$\{([A-Z0-9_]+)(:-[^}]+)?\}")


def resolve_llm_config(
    config: "WunderConfig", model_name: Optional[str] = None
) -> Tuple[str, LLMConfig]:
    """从多模型配置中解析当前要使用的模型配置。"""
    return config.llm.resolve(model_name)


def _expand_env_value(value: str) -> str:
    """将配置字符串中的 ${VAR} 替换为环境变量值。"""

    def _replace(match: re.Match[str]) -> str:
        env_name = match.group(1)
        default = match.group(2)
        value = os.getenv(env_name, "")
        if value:
            return value
        if default:
            return default[2:]
        return ""

    return _ENV_PATTERN.sub(_replace, value)


def _expand_env(data: Any) -> Any:
    """递归展开配置中的环境变量占位符。"""
    if isinstance(data, str):
        return _expand_env_value(data)
    if isinstance(data, list):
        return [_expand_env(item) for item in data]
    if isinstance(data, dict):
        return {key: _expand_env(value) for key, value in data.items()}
    return data


def resolve_config_path(path: Optional[Path] = None) -> Path:
    """解析基础配置路径，以 config/wunder.yaml 为主。"""
    env_path = os.getenv("WUNDER_CONFIG_PATH")
    if env_path:
        return Path(env_path).resolve()
    return (path or DEFAULT_CONFIG_PATH).resolve()


def resolve_override_config_path() -> Path:
    """解析持久化配置路径，保存管理端修改内容。"""
    env_path = os.getenv("WUNDER_CONFIG_OVERRIDE_PATH")
    if env_path:
        return Path(env_path).resolve()
    override_path = DEFAULT_OVERRIDE_PATH.resolve()
    legacy_path = LEGACY_OVERRIDE_PATH.resolve()
    base_path = resolve_config_path()
    # 未显式指定覆盖路径时，迁移旧版 data/config/wunder.yaml 到新路径
    if legacy_path.exists() and not override_path.exists() and base_path != legacy_path:
        try:
            override_path.parent.mkdir(parents=True, exist_ok=True)
            legacy_path.replace(override_path)
        except OSError:
            return legacy_path
    return override_path


def _deep_update(target: Dict[str, Any], overrides: Dict[str, Any]) -> Dict[str, Any]:
    """递归合并配置，允许覆盖任意层级的字段。"""
    for key, value in overrides.items():
        if isinstance(value, dict) and isinstance(target.get(key), dict):
            target[key] = _deep_update(target[key], value)
        else:
            target[key] = value
    return target


def _read_raw_config(path: Path) -> Dict[str, Any]:
    """读取 YAML 配置，不存在时返回空字典。"""
    if not path.exists():
        return {}
    return yaml.safe_load(path.read_text(encoding="utf-8")) or {}


def load_config(path: Path, overrides: Optional[Dict[str, Any]] = None) -> WunderConfig:
    """从 YAML 读取基础配置并合并持久化覆盖与临时覆盖。"""
    base_path = resolve_config_path(path)
    if not base_path.exists():
        raise FileNotFoundError(
            t("error.config_file_not_found", path=base_path)
        )
    raw = _read_raw_config(base_path)
    override_path = resolve_override_config_path()
    override_raw = _read_raw_config(override_path)
    raw = _deep_update(raw, override_raw)
    if overrides:
        raw = _deep_update(raw, overrides)
    raw = _expand_env(raw)
    config = WunderConfig.model_validate(raw)
    configure_i18n(
        default_language=config.i18n.default_language,
        supported_languages=config.i18n.supported_languages,
        aliases=config.i18n.aliases,
    )
    return config


@lru_cache(maxsize=1)
def get_config(config_path: str = "config/wunder.yaml") -> WunderConfig:
    """加载并缓存配置，避免每次请求重复解析。"""
    path = resolve_config_path(Path(config_path))
    return load_config(path)
