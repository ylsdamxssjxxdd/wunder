import json
from typing import Any, Dict, List


# 该工具函数专门用于测试场景：将 SSE 文本解析为结构化事件列表，便于断言事件顺序与内容。
def parse_sse_events(text: str) -> List[Dict[str, Any]]:
    """解析 SSE 文本为事件列表，适配 event/data 两行结构。"""
    events: List[Dict[str, Any]] = []
    current: Dict[str, Any] = {}

    # SSE 以空行分割事件；逐行扫描并组装 event/data 字段。
    for line in text.splitlines():
        if not line.strip():
            if current:
                events.append(current)
                current = {}
            continue
        if line.startswith("event:"):
            current["event"] = line[len("event:") :].strip()
            continue
        if line.startswith("data:"):
            raw = line[len("data:") :].strip()
            try:
                current["data"] = json.loads(raw)
            except json.JSONDecodeError:
                # 如果 data 不是 JSON，则保留原始文本，便于排查协议异常。
                current["data"] = raw

    if current:
        events.append(current)
    return events
