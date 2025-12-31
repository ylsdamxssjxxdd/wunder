"""工具系统常量定义。"""

from typing import List

from app.tools.catalog import list_builtin_tool_names, list_sandbox_tool_names

# 内置工具名称列表，用于提示词注入与管理页面展示。
BUILTIN_TOOL_NAMES: List[str] = list_builtin_tool_names()

# 仅这些工具在沙盒模式下执行。
SANDBOX_TOOL_NAMES: List[str] = list_sandbox_tool_names()
