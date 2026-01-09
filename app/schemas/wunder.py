from datetime import datetime
from typing import Any, Dict, List, Optional, Literal

from pydantic import BaseModel, Field


class WunderAttachment(BaseModel):
    """调试附件结构：文件走 Markdown 内容，图片走 data URL。"""

    type: Literal["file", "image"] = Field(..., description="附件类型：file/image")
    name: str = Field(default="", description="附件名称")
    content: str = Field(
        default="", description="附件内容（文件为 Markdown，图片为 data URL）"
    )
    mime_type: Optional[str] = Field(default=None, description="附件 MIME 类型")


class WunderRequest(BaseModel):
    """/wunder 接口请求体。"""

    user_id: str = Field(..., description="用户唯一标识")
    question: str = Field(..., description="用户问题")
    tool_names: Optional[List[str]] = Field(
        default=None, description="需要启用的工具/MCP/技能名称列表"
    )
    stream: bool = Field(default=True, description="是否使用流式响应")
    session_id: Optional[str] = Field(default=None, description="会话标识")
    model_name: Optional[str] = Field(default=None, description="模型配置名称")
    config_overrides: Optional[Dict[str, Any]] = Field(
        default=None, description="配置覆盖项"
    )
    attachments: Optional[List[WunderAttachment]] = Field(
        default=None, description="附件列表（文件为 Markdown，图片为 data URL）"
    )


class WunderResponse(BaseModel):
    """非流式响应结构。"""

    session_id: str
    answer: str
    usage: Optional[Dict[str, Any]] = None
    stop_reason: Optional[str] = Field(
        default=None, description="停止原因（model_response/final_tool/a2ui/max_rounds/unknown）"
    )
    uid: Optional[str] = Field(default=None, description="A2UI Surface 标识")
    a2ui: Optional[List[Dict[str, Any]]] = Field(
        default=None, description="A2UI 消息列表"
    )


class AttachmentConvertResponse(BaseModel):
    """附件解析响应：返回解析后的 Markdown 文本。"""

    ok: bool = True
    name: str = ""
    content: str = ""
    converter: str = ""
    warnings: List[str] = Field(default_factory=list)


class WunderPromptRequest(BaseModel):
    """/wunder/system_prompt 请求体。"""

    user_id: str = Field(..., description="用户唯一标识")
    session_id: Optional[str] = Field(default=None, description="会话标识")
    tool_names: Optional[List[str]] = Field(
        default=None, description="需要启用的工具/MCP/技能名称列表"
    )
    config_overrides: Optional[Dict[str, Any]] = Field(
        default=None, description="配置覆盖项"
    )


class WunderPromptResponse(BaseModel):
    """/wunder/system_prompt 响应体。"""

    prompt: str
    build_time_ms: float = Field(
        default=0.0, description="系统提示词构建耗时（毫秒）"
    )


class I18nConfigResponse(BaseModel):
    """多语言配置响应。"""

    default_language: str = Field(default="zh-CN", description="默认语言")
    supported_languages: List[str] = Field(default_factory=list, description="支持语言列表")
    aliases: Dict[str, str] = Field(default_factory=dict, description="语言别名映射")


class LlmConfigItem(BaseModel):
    """LLM 配置项。"""

    enable: bool = True
    provider: str = "openai_compatible"
    base_url: str = ""
    api_key: str = ""
    model: str = ""
    temperature: float = 0.7
    timeout_s: int = 60
    retry: int = 1
    max_rounds: int = Field(default=10, description="单次会话最多执行轮次")
    max_context: Optional[int] = Field(default=None, description="最大上下文长度")
    max_output: Optional[int] = Field(default=None, description="最大输出长度")
    support_vision: bool = Field(default=False, description="是否支持视觉输入")
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
        description="模型停止词，优先用于截断工具调用输出",
    )
    mock_if_unconfigured: bool = True


class LlmConfigSet(BaseModel):
    """多模型配置集合。"""

    default: str = Field(default="", description="默认模型配置名称")
    models: Dict[str, LlmConfigItem] = Field(
        default_factory=dict, description="模型配置集合"
    )


class LlmContextProbeRequest(BaseModel):
    """模型上下文探测请求。"""

    provider: str = Field(default="openai_compatible", description="模型提供方类型")
    base_url: str = Field(..., description="模型服务地址")
    api_key: str = Field(default="", description="访问密钥")
    model: str = Field(..., description="模型名称")
    timeout_s: Optional[int] = Field(default=15, description="探测超时秒数")


class LlmContextProbeResponse(BaseModel):
    """模型上下文探测响应。"""

    max_context: Optional[int] = Field(default=None, description="最大上下文长度")
    message: str = Field(default="", description="探测结果说明")


class LlmConfigResponse(BaseModel):
    """LLM 配置响应。"""

    llm: LlmConfigSet


class LlmConfigUpdateRequest(BaseModel):
    """LLM 配置更新请求。"""

    llm: LlmConfigSet


class McpServerItem(BaseModel):
    """MCP 服务信息。"""

    name: str
    endpoint: str
    allow_tools: List[str] = Field(default_factory=list)
    enabled: bool = True
    transport: Optional[str] = None
    description: str = ""
    display_name: str = ""
    headers: Dict[str, str] = Field(default_factory=dict)
    auth: Optional[str] = None
    tool_specs: List[Dict[str, Any]] = Field(default_factory=list)


class McpListResponse(BaseModel):
    """MCP 服务列表响应。"""

    servers: List[McpServerItem]


class McpUpdateRequest(BaseModel):
    """MCP 服务更新请求。"""

    servers: List[McpServerItem]


class McpToolsRequest(BaseModel):
    """MCP 工具探测请求。"""

    name: str = Field(..., description="服务名称")
    endpoint: str = Field(..., description="服务地址")
    transport: Optional[str] = Field(default=None, description="http/sse/streamable-http")
    headers: Dict[str, str] = Field(default_factory=dict)
    auth: Optional[str] = None


class McpToolItem(BaseModel):
    """MCP 工具信息。"""

    name: str
    description: str = ""
    input_schema: Dict[str, Any] = Field(default_factory=dict)


class McpToolsResponse(BaseModel):
    """MCP 工具探测响应。"""

    tools: List[McpToolItem]


class A2aServiceItem(BaseModel):
    """A2A 服务信息。"""

    name: str
    endpoint: str
    service_type: str = "external"
    user_id: str = ""
    enabled: bool = True
    description: str = ""
    display_name: str = ""
    headers: Dict[str, str] = Field(default_factory=dict)
    auth: Optional[str] = None
    agent_card: Dict[str, Any] = Field(default_factory=dict)
    allow_self: bool = False
    max_depth: int = 0
    default_method: str = "SendMessage"


class A2aListResponse(BaseModel):
    """A2A 服务列表响应。"""

    services: List[A2aServiceItem]


class A2aUpdateRequest(BaseModel):
    """A2A 服务更新请求。"""

    services: List[A2aServiceItem]


class A2aCardRequest(BaseModel):
    """A2A AgentCard 探测请求。"""

    endpoint: str = Field(..., description="A2A 服务端点")
    headers: Dict[str, str] = Field(default_factory=dict)
    auth: Optional[str] = None


class A2aCardResponse(BaseModel):
    """A2A AgentCard 探测响应。"""

    agent_card: Dict[str, Any] = Field(default_factory=dict)


class SkillItem(BaseModel):
    """技能信息。"""

    name: str
    description: str
    path: str
    input_schema: Dict[str, Any] = Field(default_factory=dict)
    enabled: bool = False


class SkillsListResponse(BaseModel):
    """技能列表响应。"""

    paths: List[str]
    enabled: List[str]
    skills: List[SkillItem]


class SkillContentResponse(BaseModel):
    """技能内容响应。"""

    name: str
    path: str
    content: str


class SkillsUpdateRequest(BaseModel):
    """技能启用更新请求。"""

    enabled: List[str] = Field(default_factory=list)
    paths: Optional[List[str]] = None


class SkillsUploadResponse(BaseModel):
    """技能压缩包上传响应。"""

    ok: bool
    extracted: int
    message: str = ""


class SkillsDeleteResponse(BaseModel):
    """技能删除响应。"""

    ok: bool
    name: str = ""
    message: str = ""


class BuiltinToolItem(BaseModel):
    """内置工具信息。"""

    name: str
    description: str = ""
    input_schema: Dict[str, Any] = Field(default_factory=dict)
    enabled: bool = False


class BuiltinToolsResponse(BaseModel):
    """内置工具列表响应。"""

    enabled: List[str]
    tools: List[BuiltinToolItem]


class BuiltinToolsUpdateRequest(BaseModel):
    """内置工具启用更新请求。"""

    enabled: List[str] = Field(default_factory=list)


class AvailableToolItem(BaseModel):
    """可选工具信息。"""

    name: str
    description: str = ""
    input_schema: Dict[str, Any] = Field(default_factory=dict)


class SharedToolItem(AvailableToolItem):
    """共享工具信息。"""

    owner_id: str


class UserMcpServerItem(BaseModel):
    """用户自建 MCP 服务信息。"""

    name: str
    endpoint: str
    allow_tools: List[str] = Field(default_factory=list)
    shared_tools: List[str] = Field(default_factory=list)
    enabled: bool = True
    transport: Optional[str] = None
    description: str = ""
    display_name: str = ""
    headers: Dict[str, str] = Field(default_factory=dict)
    auth: Optional[str] = None
    tool_specs: List[Dict[str, Any]] = Field(default_factory=list)


class UserMcpListResponse(BaseModel):
    """用户 MCP 服务列表响应。"""

    servers: List[UserMcpServerItem]


class UserMcpUpdateRequest(BaseModel):
    """用户 MCP 服务更新请求。"""

    user_id: str
    servers: List[UserMcpServerItem] = Field(default_factory=list)


class UserSkillItem(BaseModel):
    """用户技能信息。"""

    name: str
    description: str
    path: str
    input_schema: Dict[str, Any] = Field(default_factory=dict)
    enabled: bool = False
    shared: bool = False


class UserSkillsResponse(BaseModel):
    """用户技能列表响应。"""

    enabled: List[str] = Field(default_factory=list)
    shared: List[str] = Field(default_factory=list)
    skills: List[UserSkillItem]


class UserSkillsUpdateRequest(BaseModel):
    """用户技能启用更新请求。"""

    user_id: str
    enabled: List[str] = Field(default_factory=list)
    shared: List[str] = Field(default_factory=list)


class AvailableToolsResponse(BaseModel):
    """/wunder/tools 响应体。"""

    builtin_tools: List[AvailableToolItem]
    mcp_tools: List[AvailableToolItem]
    a2a_tools: List[AvailableToolItem] = Field(default_factory=list)
    skills: List[AvailableToolItem]
    knowledge_tools: List[AvailableToolItem] = Field(default_factory=list)
    user_tools: List[AvailableToolItem] = Field(default_factory=list)
    shared_tools: List[SharedToolItem] = Field(default_factory=list)
    extra_prompt: str = ""


class UserKnowledgeBaseItem(BaseModel):
    """用户知识库配置项。"""

    name: str
    description: str = ""
    root: str = ""
    enabled: bool = True
    shared: bool = False


class UserKnowledgeConfigItem(BaseModel):
    """用户知识库配置集合。"""

    bases: List[UserKnowledgeBaseItem] = Field(default_factory=list)


class UserKnowledgeConfigResponse(BaseModel):
    """用户知识库配置响应。"""

    knowledge: UserKnowledgeConfigItem


class UserKnowledgeConfigUpdateRequest(BaseModel):
    """用户知识库配置更新请求。"""

    user_id: str
    knowledge: UserKnowledgeConfigItem


class UserKnowledgeFileUpdateRequest(BaseModel):
    """用户知识库文件保存请求。"""

    user_id: str
    base: str
    path: str
    content: str


class UserExtraPromptResponse(BaseModel):
    """用户附加提示词响应。"""

    user_id: str
    extra_prompt: str = ""


class UserExtraPromptUpdateRequest(BaseModel):
    """用户附加提示词更新请求。"""

    user_id: str
    extra_prompt: str = ""


class KnowledgeBaseItem(BaseModel):
    """知识库配置项。"""

    name: str
    description: str = ""
    root: str
    enabled: bool = True


class KnowledgeConfigItem(BaseModel):
    """知识库配置集合。"""

    bases: List[KnowledgeBaseItem] = Field(default_factory=list)


class KnowledgeConfigResponse(BaseModel):
    """知识库配置响应。"""

    knowledge: KnowledgeConfigItem


class KnowledgeConfigUpdateRequest(BaseModel):
    """知识库配置更新请求。"""

    knowledge: KnowledgeConfigItem


class KnowledgeFilesResponse(BaseModel):
    """知识库文件列表响应。"""

    base: str
    files: List[str]


class KnowledgeFileResponse(BaseModel):
    """知识库文件内容响应。"""

    base: str
    path: str
    content: str


class KnowledgeFileUpdateRequest(BaseModel):
    """知识库文件保存请求。"""

    base: str
    path: str
    content: str


class KnowledgeActionResponse(BaseModel):
    """知识库操作响应。"""

    ok: bool
    message: str = ""


class KnowledgeUploadResponse(BaseModel):
    """知识库上传转换响应。"""

    ok: bool
    message: str = ""
    path: str = ""
    converter: str = ""
    warnings: List[str] = Field(default_factory=list)


class WorkspaceEntry(BaseModel):
    """工作区条目信息。"""

    name: str
    path: str
    type: str
    size: int = 0
    updated_time: str = ""


class WorkspaceListResponse(BaseModel):
    """工作区列表响应。"""

    user_id: str
    path: str
    parent: Optional[str] = None
    entries: List[WorkspaceEntry]
    tree_version: int = 0
    total: int = 0
    offset: int = 0
    limit: int = 0


class WorkspaceActionResponse(BaseModel):
    """工作区操作响应。"""

    ok: bool
    message: str = ""
    tree_version: int = 0
    files: List[str] = Field(default_factory=list)


class WorkspaceDirRequest(BaseModel):
    """工作区新建目录请求。"""

    user_id: str = Field(..., description="用户唯一标识")
    path: str = Field(..., description="目录相对路径")


class WorkspaceMoveRequest(BaseModel):
    """工作区移动/重命名请求。"""

    user_id: str = Field(..., description="用户唯一标识")
    source: str = Field(..., description="源路径（相对路径）")
    destination: str = Field(..., description="目标路径（相对路径）")


class WorkspaceFileUpdateRequest(BaseModel):
    """工作区文件编辑请求。"""

    user_id: str = Field(..., description="用户唯一标识")
    path: str = Field(..., description="文件相对路径")
    content: str = Field(default="", description="文件内容")
    create_if_missing: bool = Field(default=False, description="文件不存在时是否创建")


class WorkspaceContentEntry(BaseModel):
    """工作区内容条目。"""

    name: str
    path: str
    type: str
    size: int = 0
    updated_time: str = ""
    children: List["WorkspaceContentEntry"] = Field(default_factory=list)


class WorkspaceContentResponse(BaseModel):
    """工作区内容响应。"""

    user_id: str
    path: str
    type: str
    size: int = 0
    updated_time: str = ""
    content: Optional[str] = None
    format: str = "text"
    truncated: bool = False
    entries: List[WorkspaceContentEntry] = Field(default_factory=list)
    total: int = 0
    offset: int = 0
    limit: int = 0


class WorkspaceSearchResponse(BaseModel):
    """工作区搜索响应。"""

    user_id: str
    keyword: str
    entries: List[WorkspaceEntry] = Field(default_factory=list)
    total: int = 0
    offset: int = 0
    limit: int = 0


class WorkspaceCopyRequest(BaseModel):
    """工作区复制请求。"""

    user_id: str = Field(..., description="用户唯一标识")
    source: str = Field(..., description="源路径（相对路径）")
    destination: str = Field(..., description="目标路径（相对路径）")


class WorkspaceBatchRequest(BaseModel):
    """工作区批量操作请求。"""

    user_id: str = Field(..., description="用户唯一标识")
    action: Literal["delete", "move", "copy"] = Field(..., description="批量操作类型")
    paths: List[str] = Field(default_factory=list, description="待处理的路径列表")
    destination: Optional[str] = Field(default=None, description="批量移动/复制的目标目录")


class WorkspaceBatchFailure(BaseModel):
    """工作区批量操作失败条目。"""

    path: str
    message: str


class WorkspaceBatchResponse(BaseModel):
    """工作区批量操作响应。"""

    ok: bool
    message: str = ""
    tree_version: int = 0
    succeeded: List[str] = Field(default_factory=list)
    failed: List[WorkspaceBatchFailure] = Field(default_factory=list)


WorkspaceContentEntry.model_rebuild()


class MonitorSystem(BaseModel):
    """系统资源监控信息。"""

    cpu_percent: float
    memory_total: int
    memory_used: int
    memory_available: int
    process_rss: int
    process_cpu_percent: float
    load_avg_1: Optional[float] = None
    load_avg_5: Optional[float] = None
    load_avg_15: Optional[float] = None
    disk_total: int = 0
    disk_used: int = 0
    disk_free: int = 0
    disk_percent: float = 0.0
    disk_read_bytes: int = 0
    disk_write_bytes: int = 0
    net_sent_bytes: int = 0
    net_recv_bytes: int = 0
    uptime_s: float = 0.0


class MonitorService(BaseModel):
    """服务层监控信息。"""

    active_sessions: int = 0
    history_sessions: int = 0
    finished_sessions: int = 0
    error_sessions: int = 0
    cancelled_sessions: int = 0
    total_sessions: int = 0
    recent_completed: int = 0
    avg_elapsed_s: float = 0.0


class MonitorSandboxResources(BaseModel):
    """沙盒资源配置摘要。"""

    cpu: float = 0.0
    memory_mb: int = 0
    pids: int = 0


class MonitorSandbox(BaseModel):
    """沙盒运行状态与使用概览。"""

    mode: str = "local"
    network: str = ""
    readonly_rootfs: bool = True
    idle_ttl_s: int = 0
    timeout_s: int = 0
    endpoint: str = ""
    image: str = ""
    resources: MonitorSandboxResources = Field(default_factory=MonitorSandboxResources)
    recent_calls: int = 0
    recent_sessions: int = 0


class MonitorToolStat(BaseModel):
    """工具调用统计。"""

    tool: str
    calls: int = 0


class MonitorSessionItem(BaseModel):
    """会话线程摘要信息。"""

    session_id: str
    user_id: str
    question: str = ""
    status: str
    stage: str
    summary: str
    start_time: str
    updated_time: str
    elapsed_s: float
    cancel_requested: bool = False
    token_usage: int = Field(default=0, description="当前占用的 token 数量")


class MonitorSessionDetail(MonitorSessionItem):
    """会话线程详情信息。"""

    prefill_tokens: Optional[int] = Field(
        default=None, description="预填充阶段上下文 token 数量"
    )
    prefill_duration_s: Optional[float] = Field(
        default=None, description="预填充阶段耗时（秒）"
    )
    prefill_speed_tps: Optional[float] = Field(
        default=None, description="预填充速度（token/s）"
    )
    prefill_speed_lower_bound: bool = Field(
        default=False, description="预填充速度是否为缓存下限估计"
    )
    decode_tokens: Optional[int] = Field(
        default=None, description="解码阶段输出 token 数量"
    )
    decode_duration_s: Optional[float] = Field(
        default=None, description="解码阶段耗时（秒）"
    )
    decode_speed_tps: Optional[float] = Field(
        default=None, description="解码速度（token/s）"
    )


class MonitorEventItem(BaseModel):
    """会话事件详情。"""

    timestamp: str
    type: str
    data: Dict[str, Any] = Field(default_factory=dict)


class MonitorListResponse(BaseModel):
    """监控列表响应。"""

    system: MonitorSystem
    service: MonitorService
    sandbox: MonitorSandbox
    sessions: List[MonitorSessionItem]
    tool_stats: List[MonitorToolStat] = Field(default_factory=list)


class MonitorDetailResponse(BaseModel):
    """监控详情响应。"""

    session: MonitorSessionDetail
    events: List[MonitorEventItem]


class MonitorToolSessionItem(BaseModel):
    """工具调用会话详情。"""

    session_id: str
    user_id: str = ""
    question: str = ""
    status: str = ""
    stage: str = ""
    start_time: str = ""
    updated_time: str = ""
    elapsed_s: float = 0.0
    token_usage: int = 0
    tool_calls: int = 0
    last_time: str = ""


class MonitorToolUsageResponse(BaseModel):
    """工具使用会话列表响应。"""

    tool: str
    tool_name: str = Field(default="", description="工具真实名称（用于事件定位）")
    sessions: List[MonitorToolSessionItem] = Field(default_factory=list)


class MonitorCancelResponse(BaseModel):
    """终止会话响应。"""

    ok: bool
    message: str = ""


class MonitorDeleteResponse(BaseModel):
    """删除历史会话响应。"""

    ok: bool
    message: str = ""


class UserStatsItem(BaseModel):
    """用户统计信息。"""

    user_id: str
    active_sessions: int = 0
    history_sessions: int = 0
    total_sessions: int = 0
    chat_records: int = 0
    tool_calls: int = 0
    token_usage: int = 0  # 当前占用的 Token 数量


class UserStatsResponse(BaseModel):
    """用户统计列表响应。"""

    users: List[UserStatsItem]


class UserSessionsResponse(BaseModel):
    """用户会话列表响应。"""

    user_id: str
    sessions: List[MonitorSessionItem]


class UserDeleteResponse(BaseModel):
    """用户数据删除响应。"""

    ok: bool
    message: str = ""
    cancelled_sessions: int = 0
    deleted_sessions: int = 0
    deleted_chat_records: int = 0
    deleted_tool_records: int = 0
    workspace_deleted: bool = False
    legacy_history_deleted: bool = False


class MemoryUserItem(BaseModel):
    """用户长期记忆状态条目。"""

    user_id: str
    enabled: bool = False
    record_count: int = 0
    last_updated_time: str = ""
    last_updated_time_ts: float = 0.0


class MemoryUsersResponse(BaseModel):
    """长期记忆用户列表响应。"""

    users: List[MemoryUserItem]


class MemoryRecordItem(BaseModel):
    """长期记忆记录条目。"""

    session_id: str
    summary: str
    created_time: str = ""
    updated_time: str = ""
    created_time_ts: float = 0.0
    updated_time_ts: float = 0.0


class MemoryRecordsResponse(BaseModel):
    """长期记忆记录列表响应。"""

    user_id: str
    enabled: bool = False
    records: List[MemoryRecordItem]


class MemoryRecordUpdateRequest(BaseModel):
    """长期记忆记录更新请求。"""

    summary: str = ""


class MemoryEnabledUpdateRequest(BaseModel):
    """长期记忆开关更新请求。"""

    enabled: bool = False


class MemoryStatusResponse(BaseModel):
    """长期记忆开关响应。"""

    user_id: str
    enabled: bool = False


class MemoryActionResponse(BaseModel):
    """长期记忆删除响应。"""

    ok: bool = True
    message: str = ""
    deleted: int = 0


class MemoryQueueItem(BaseModel):
    """长期记忆队列条目。"""

    task_id: str
    user_id: str
    session_id: str
    status: str = ""
    queued_time: str = ""
    queued_time_ts: float = 0.0
    started_time: str = ""
    started_time_ts: float = 0.0
    finished_time: str = ""
    finished_time_ts: float = 0.0
    elapsed_s: float = 0.0


class MemoryQueueStatusResponse(BaseModel):
    """长期记忆队列状态响应。"""

    active: List[MemoryQueueItem] = []
    history: List[MemoryQueueItem] = []


class MemoryQueueDetailResponse(BaseModel):
    """长期记忆队列详情响应。"""

    task_id: str
    user_id: str
    session_id: str
    status: str = ""
    queued_time: str = ""
    queued_time_ts: float = 0.0
    started_time: str = ""
    started_time_ts: float = 0.0
    finished_time: str = ""
    finished_time_ts: float = 0.0
    elapsed_s: float = 0.0
    request: Dict[str, Any] = {}
    result: str = ""
    error: str = ""


class StreamEvent(BaseModel):
    """流式事件结构，供 SSE 序列化。"""

    type: str
    session_id: str
    data: Dict[str, Any]
    timestamp: str = Field(default_factory=lambda: datetime.utcnow().isoformat() + "Z")
    event_id: Optional[int] = Field(
        default=None,
        exclude=True,
        description="内部事件序号（不参与 SSE data 序列化）",
    )
