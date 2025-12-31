"""统一管理内置工具定义，确保规格、执行入口与策略一致。"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable, Dict, List, Optional

from app.tools import builtin
from app.tools.registry import ToolCallable, ToolSpec, ensure_async
from app.tools.types import ToolContext, ToolResult

ToolHandler = Callable[[ToolContext, Dict[str, Any]], ToolResult]


@dataclass(frozen=True)
class ToolDescriptor:
    """工具定义信息，包含规格、执行入口与运行策略。"""

    name: str
    description: str
    args_schema: Dict[str, Any]
    handler: Optional[ToolHandler] = None
    runtime: bool = False
    sandbox: bool = False
    mutates_workspace: bool = False

    def to_spec(self) -> ToolSpec:
        """转换为提示词注入使用的工具规格对象。"""
        return ToolSpec(
            name=self.name,
            description=self.description,
            args_schema=self.args_schema,
        )

    def to_async_handler(self) -> Optional[ToolCallable]:
        """将同步工具封装为异步执行入口，便于统一注册。"""
        if self.handler is None:
            return None
        return ensure_async(self.handler)


_BUILTIN_TOOL_DESCRIPTORS: List[ToolDescriptor] = [
    ToolDescriptor(
        name="最终回复",
        description="返回给用户的最终回复。",
        args_schema={
            "type": "object",
            "properties": {
                "content": {"type": "string", "description": "最终回复内容。"}
            },
            "required": ["content"],
        },
        handler=None,
        runtime=False,
    ),
    ToolDescriptor(
        name="执行命令",
        description=(
            "请求在系统上执行 CLI 命令。当需要进行系统操作或运行特定命令以完成用户任务的任一步骤时使用。"
            "默认在工作区根目录执行，可通过 workdir 指定子目录或白名单目录。"
            "当 allow_commands 为 * 时，shell 默认开启，可显式传 shell=false 关闭。"
            "若需 cd/&& 等 shell 语法，请确保 shell 为 true。"
        ),
        args_schema={
            "type": "object",
            "properties": {
                "content": {"type": "string", "description": "CLI 命令。"},
                "workdir": {
                    "type": "string",
                    "description": "可选，工作目录，相对工作区或白名单目录的绝对路径。",
                },
                "timeout_s": {
                    "type": "integer",
                    "description": "可选，命令超时秒数，默认 30 秒。",
                },
                "shell": {
                    "type": "boolean",
                    "description": "可选，使用 shell 执行，仅在 allow_commands 为 * 时允许。",
                },
            },
            "required": ["content"],
        },
        handler=builtin.execute_command,
        runtime=True,
        sandbox=True,
        mutates_workspace=True,
    ),
    ToolDescriptor(
        name="ptc",
        description=(
            "程序化工具调用：当 CLI 命令过多、解析脆弱或需要结构化处理时，编写并运行临时 Python 脚本。"
            "脚本会保存到工作区的 ptc_temp 目录并立即执行，返回 stdout/stderr。"
        ),
        args_schema={
            "type": "object",
            "properties": {
                "filename": {
                    "type": "string",
                    "description": "Python 脚本文件名，例如 helper.py。",
                },
                "workdir": {
                    "type": "string",
                    "description": "相对工作区的工作目录，使用 . 表示根目录。",
                },
                "content": {
                    "type": "string",
                    "description": "完整的 Python 脚本内容。",
                },
            },
            "required": ["filename", "workdir", "content"],
        },
        handler=builtin.ptc,
        runtime=True,
        sandbox=True,
        mutates_workspace=True,
    ),
    ToolDescriptor(
        name="列出文件",
        description=(
            "列出目录下所有直接子文件夹和文件。"
            "如果未提供路径，默认使用工程师工作目录。"
        ),
        args_schema={
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": (
                        "可选，要列出的目录（相对工程师工作目录）。"
                        "留空则使用当前工作目录。"
                    ),
                }
            },
        },
        handler=builtin.list_files,
        runtime=True,
        sandbox=False,
    ),
    ToolDescriptor(
        name="搜索内容",
        description=(
            "在工程师工作目录下搜索所有文本文件中的查询字符串（不区分大小写）。"
            "返回格式为 <path>:<line>:<content>。"
        ),
        args_schema={
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "要搜索的文本（不区分大小写的字面量）。",
                },
                "path": {
                    "type": "string",
                    "description": (
                        "可选，限制搜索范围的目录，相对工程师工作目录。"
                    ),
                },
                "file_pattern": {
                    "type": "string",
                    "description": "可选，用于过滤文件的 glob，例如 *.cpp 或 src/**.ts。",
                },
            },
            "required": ["query"],
        },
        handler=builtin.search_content,
        runtime=True,
        sandbox=False,
    ),
    ToolDescriptor(
        name="读取文件",
        description="读取指定路径的文件内容，支持批量读取并指定行号范围。",
        args_schema={
            "type": "object",
            "properties": {
                "files": {
                    "type": "array",
                    "description": "要读取的文件列表，可选指定行号范围。",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "要读取的路径。",
                            },
                            "start_line": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "可选起始行（包含）。",
                            },
                            "end_line": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "可选结束行（包含）。",
                            },
                            "line_ranges": {
                                "type": "array",
                                "description": "可选的行号范围列表。",
                                "items": {
                                    "type": "array",
                                    "items": [
                                        {"type": "integer", "minimum": 1},
                                        {"type": "integer", "minimum": 1},
                                    ],
                                },
                            },
                        },
                        "required": ["path"],
                    },
                },
            },
            "required": ["files"],
        },
        handler=builtin.read_file,
        runtime=True,
        sandbox=False,
    ),
    ToolDescriptor(
        name="写入文件",
        description=(
            "请求向指定路径写入内容。如果文件已存在，将被提供的内容覆盖。"
        ),
        args_schema={
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "要写入的文件路径。"},
                "content": {"type": "string", "description": "文件内容。"},
            },
            "required": ["path", "content"],
        },
        handler=builtin.write_file,
        runtime=True,
        sandbox=False,
        mutates_workspace=True,
    ),
    ToolDescriptor(
        name="替换文本",
        description=(
            "请求在现有文件中替换文本。默认只替换一次 old_string 的出现。"
        ),
        args_schema={
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "要编辑的文件路径。"},
                "old_string": {
                    "type": "string",
                    "description": "要被替换的精确文本（请包含上下文）。",
                },
                "new_string": {
                    "type": "string",
                    "description": "用于替换 old_string 的精确文本。",
                },
                "expected_replacements": {
                    "type": "number",
                    "description": "期望替换的次数，省略则默认 1。",
                    "minimum": 1,
                },
            },
            "required": ["path", "old_string", "new_string"],
        },
        handler=builtin.replace_in_file,
        runtime=True,
        sandbox=False,
        mutates_workspace=True,
    ),
    ToolDescriptor(
        name="编辑文件",
        description="对现有文本文件应用结构化的行级编辑。",
        args_schema={
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "要编辑的文件路径。"},
                "edits": {
                    "type": "array",
                    "description": "按顺序执行的编辑操作列表。",
                    "items": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "enum": ["replace", "insert_before", "insert_after", "delete"],
                                "description": "要应用的编辑类型。",
                            },
                            "start_line": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "编辑开始的行号（从 1 开始）。",
                            },
                            "end_line": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "编辑结束的行号（包含）。",
                            },
                            "new_content": {
                                "type": "string",
                                "description": "替换或插入的内容。",
                            },
                        },
                        "required": ["action", "start_line"],
                        "additionalProperties": False,
                    },
                },
                "ensure_newline_at_eof": {
                    "type": "boolean",
                    "description": "为 true 时确保文件以换行结束。",
                },
            },
            "required": ["path", "edits"],
        },
        handler=builtin.edit_in_file,
        runtime=True,
        sandbox=False,
        mutates_workspace=True,
    ),
]


def list_builtin_tool_descriptors() -> List[ToolDescriptor]:
    """返回内置工具定义列表，用于提示词、注册与配置一致性。"""
    return list(_BUILTIN_TOOL_DESCRIPTORS)


def list_builtin_tool_names() -> List[str]:
    """列出内置工具名称，供配置默认值与工具选择使用。"""
    return [descriptor.name for descriptor in _BUILTIN_TOOL_DESCRIPTORS]


def list_sandbox_tool_names() -> List[str]:
    """列出需要进入沙盒执行的工具名称。"""
    return [descriptor.name for descriptor in _BUILTIN_TOOL_DESCRIPTORS if descriptor.sandbox]


def build_builtin_tool_specs() -> Dict[str, ToolSpec]:
    """构建内置工具规格映射，用于提示词注入与管理展示。"""
    return {descriptor.name: descriptor.to_spec() for descriptor in _BUILTIN_TOOL_DESCRIPTORS}


def build_builtin_tool_handlers() -> Dict[str, ToolCallable]:
    """构建内置工具执行入口映射，保证与规格一致。"""
    handlers: Dict[str, ToolCallable] = {}
    for descriptor in _BUILTIN_TOOL_DESCRIPTORS:
        async_handler = descriptor.to_async_handler()
        if async_handler is not None and descriptor.runtime:
            handlers[descriptor.name] = async_handler
    return handlers


def build_sandbox_tool_handlers() -> Dict[str, ToolHandler]:
    """构建沙盒可执行的同步工具映射，仅包含允许沙盒运行的工具。"""
    handlers: Dict[str, ToolHandler] = {}
    for descriptor in _BUILTIN_TOOL_DESCRIPTORS:
        if descriptor.handler is None or not descriptor.sandbox:
            continue
        handlers[descriptor.name] = descriptor.handler
    return handlers


def is_workspace_mutation_tool(tool_name: str) -> bool:
    """判断工具是否可能改写工作区内容，用于刷新目录树缓存。"""
    for descriptor in _BUILTIN_TOOL_DESCRIPTORS:
        if descriptor.name == tool_name:
            return descriptor.mutates_workspace
    return False
