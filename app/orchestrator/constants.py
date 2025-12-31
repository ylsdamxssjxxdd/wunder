import re


TOOL_CALL_PATTERN = re.compile(
    r"<(?P<tag>tool_call|tool)\b[^>]*>(?P<payload>.*?)</(?P=tag)\s*>",
    re.S | re.I,
)
TOOL_CALL_OPEN_PATTERN = re.compile(r"<(?P<tag>tool_call|tool)\b[^>]*>", re.I)
TOOL_CALL_CLOSE_PATTERN = re.compile(r"</(?P<tag>tool_call|tool)\s*>", re.I)
OBSERVATION_PREFIX = "tool_response: "
COMPACTION_SUMMARY_PREFIX = "[上下文摘要]"
COMPACTION_META_TYPE = "compaction_summary"
COMPACTION_KEEP_RECENT_TOKENS = 2000
COMPACTION_RATIO = 0.9
# 历史占用达到阈值即触发压缩
COMPACTION_HISTORY_RATIO = 0.8
# 自动压缩时预留的输出 token，避免模型回复被挤占
COMPACTION_OUTPUT_RESERVE = 1024
# 自动压缩的安全冗余，留给系统提示词/结构性开销
COMPACTION_SAFETY_MARGIN = 512
# 压缩摘要的最大输出 token，避免摘要过长反噬上下文
COMPACTION_SUMMARY_MAX_OUTPUT = 1024
# 单条工具结果允许保底保留的 token，避免被截断得过度
COMPACTION_MIN_OBSERVATION_TOKENS = 128
# 压缩输入中单条消息的最大 token，防止单条消息撑爆摘要预算
COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS = 2048

# 产物索引在上下文中的统一前缀
ARTIFACT_INDEX_PREFIX = "[产物索引]"
# 产物索引最多读取的日志条目数量
ARTIFACT_INDEX_MAX_ITEMS = 200

# 会话锁默认存活时间（秒），心跳会自动续租
SESSION_LOCK_TTL_S = 120
# 会话锁心跳续租间隔（秒）
SESSION_LOCK_HEARTBEAT_S = 5
# 并发超限时的轮询等待间隔（秒）
SESSION_LOCK_POLL_INTERVAL_S = 0.2
# SSE 事件队列大小
STREAM_EVENT_QUEUE_SIZE = 256
# SSE 溢出事件回放轮询间隔（秒）
STREAM_EVENT_POLL_INTERVAL_S = 0.2
# SSE 溢出事件单次回放上限
STREAM_EVENT_FETCH_LIMIT = 200
# SSE 溢出事件保留时间（秒）
STREAM_EVENT_TTL_S = 3600
# SSE 溢出事件清理节流间隔（秒）
STREAM_EVENT_CLEANUP_INTERVAL_S = 60
