from dataclasses import dataclass
from typing import Any, Callable, Dict, Optional

from app.memory.workspace import WorkspaceContext


@dataclass
class ToolContext:
    """工具执行上下文。"""

    workspace: WorkspaceContext
    config: Dict[str, Any]
    # 用于工具内部上报调试事件，避免写入模型上下文
    emit_event: Optional[Callable[[str, Dict[str, Any]], None]] = None


@dataclass
class ToolResult:
    """统一工具执行结果结构。"""

    ok: bool
    data: Dict[str, Any]
    error: str = ""
