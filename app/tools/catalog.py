"""统一管理内置工具定义，确保规格、执行入口与策略一致。"""

from __future__ import annotations

from dataclasses import dataclass
from functools import lru_cache
from typing import Any, Callable, Dict, List, Optional

from app.core.i18n import get_language, t
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
    aliases: tuple[str, ...] = ()

    def to_spec(self, name_override: Optional[str] = None) -> ToolSpec:
        """转换为提示词注入使用的工具规格对象。"""
        return ToolSpec(
            name=name_override or self.name,
            description=self.description,
            args_schema=self.args_schema,
        )

    def to_async_handler(self) -> Optional[ToolCallable]:
        """将同步工具封装为异步执行入口，便于统一注册。"""
        if self.handler is None:
            return None
        return ensure_async(self.handler)


@lru_cache(maxsize=4)
def _build_builtin_tool_descriptors(language: str) -> tuple[ToolDescriptor, ...]:
    """按语言生成内置工具描述，避免重复构建。"""
    return (
        ToolDescriptor(
            name="最终回复",
            description=t("tool.spec.final.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": t("tool.spec.final.args.content"),
                    }
                },
                "required": ["content"],
            },
            handler=None,
            runtime=False,
            aliases=("final_response",),
        ),
        ToolDescriptor(
            name="a2ui",
            description=t("tool.spec.a2ui.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "uid": {
                        "type": "string",
                        "description": t("tool.spec.a2ui.args.uid"),
                    },
                    "a2ui": {
                        "type": "array",
                        "description": t("tool.spec.a2ui.args.messages"),
                        "items": {"type": "object"},
                    },
                    "content": {
                        "type": "string",
                        "description": t("tool.spec.a2ui.args.content"),
                    },
                },
                "required": ["uid", "a2ui"],
            },
            handler=None,
            runtime=False,
        ),
        ToolDescriptor(
            name="执行命令",
            description=t("tool.spec.exec.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": t("tool.spec.exec.args.content"),
                    },
                    "workdir": {
                        "type": "string",
                        "description": t("tool.spec.exec.args.workdir"),
                    },
                    "timeout_s": {
                        "type": "integer",
                        "description": t("tool.spec.exec.args.timeout"),
                    },
                    "shell": {
                        "type": "boolean",
                        "description": t("tool.spec.exec.args.shell"),
                    },
                },
                "required": ["content"],
            },
            handler=builtin.execute_command,
            runtime=True,
            sandbox=True,
            mutates_workspace=True,
            aliases=("execute_command",),
        ),
        ToolDescriptor(
            name="ptc",
            description=t("tool.spec.ptc.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "filename": {
                        "type": "string",
                        "description": t("tool.spec.ptc.args.filename"),
                    },
                    "workdir": {
                        "type": "string",
                        "description": t("tool.spec.ptc.args.workdir"),
                    },
                    "content": {
                        "type": "string",
                        "description": t("tool.spec.ptc.args.content"),
                    },
                },
                "required": ["filename", "workdir", "content"],
            },
            handler=builtin.ptc,
            runtime=True,
            sandbox=True,
            mutates_workspace=True,
            aliases=("programmatic_tool_call",),
        ),
        ToolDescriptor(
            name="列出文件",
            description=t("tool.spec.list.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": t("tool.spec.list.args.path"),
                    }
                },
            },
            handler=builtin.list_files,
            runtime=True,
            sandbox=False,
            aliases=("list_files",),
        ),
        ToolDescriptor(
            name="搜索内容",
            description=t("tool.spec.search.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": t("tool.spec.search.args.query"),
                    },
                    "path": {
                        "type": "string",
                        "description": t("tool.spec.search.args.path"),
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": t("tool.spec.search.args.file_pattern"),
                    },
                },
                "required": ["query"],
            },
            handler=builtin.search_content,
            runtime=True,
            sandbox=False,
            aliases=("search_content",),
        ),
        ToolDescriptor(
            name="读取文件",
            description=t("tool.spec.read.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "files": {
                        "type": "array",
                        "description": t("tool.spec.read.args.files"),
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": {
                                    "type": "string",
                                    "description": t("tool.spec.read.args.files.path"),
                                },
                                "start_line": {
                                    "type": "integer",
                                    "minimum": 1,
                                    "description": t("tool.spec.read.args.files.start_line"),
                                },
                                "end_line": {
                                    "type": "integer",
                                    "minimum": 1,
                                    "description": t("tool.spec.read.args.files.end_line"),
                                },
                                "line_ranges": {
                                    "type": "array",
                                    "description": t("tool.spec.read.args.files.line_ranges"),
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
            aliases=("read_file",),
        ),
        ToolDescriptor(
            name="写入文件",
            description=t("tool.spec.write.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": t("tool.spec.write.args.path"),
                    },
                    "content": {
                        "type": "string",
                        "description": t("tool.spec.write.args.content"),
                    },
                },
                "required": ["path", "content"],
            },
            handler=builtin.write_file,
            runtime=True,
            sandbox=False,
            mutates_workspace=True,
            aliases=("write_file",),
        ),
        ToolDescriptor(
            name="替换文本",
            description=t("tool.spec.replace.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": t("tool.spec.replace.args.path"),
                    },
                    "old_string": {
                        "type": "string",
                        "description": t("tool.spec.replace.args.old_string"),
                    },
                    "new_string": {
                        "type": "string",
                        "description": t("tool.spec.replace.args.new_string"),
                    },
                    "expected_replacements": {
                        "type": "number",
                        "description": t("tool.spec.replace.args.expected_replacements"),
                        "minimum": 1,
                    },
                },
                "required": ["path", "old_string", "new_string"],
            },
            handler=builtin.replace_in_file,
            runtime=True,
            sandbox=False,
            mutates_workspace=True,
            aliases=("replace_text",),
        ),
        ToolDescriptor(
            name="编辑文件",
            description=t("tool.spec.edit.description"),
            args_schema={
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": t("tool.spec.edit.args.path"),
                    },
                    "edits": {
                        "type": "array",
                        "description": t("tool.spec.edit.args.edits"),
                        "items": {
                            "type": "object",
                            "properties": {
                                "action": {
                                    "type": "string",
                                    "enum": [
                                        "replace",
                                        "insert_before",
                                        "insert_after",
                                        "delete",
                                    ],
                                    "description": t("tool.spec.edit.args.edits.action"),
                                },
                                "start_line": {
                                    "type": "integer",
                                    "minimum": 1,
                                    "description": t("tool.spec.edit.args.edits.start_line"),
                                },
                                "end_line": {
                                    "type": "integer",
                                    "minimum": 1,
                                    "description": t("tool.spec.edit.args.edits.end_line"),
                                },
                                "new_content": {
                                    "type": "string",
                                    "description": t("tool.spec.edit.args.edits.new_content"),
                                },
                            },
                            "required": ["action", "start_line"],
                            "additionalProperties": False,
                        },
                    },
                    "ensure_newline_at_eof": {
                        "type": "boolean",
                        "description": t("tool.spec.edit.args.ensure_newline"),
                    },
                },
                "required": ["path", "edits"],
            },
            handler=builtin.edit_in_file,
            runtime=True,
            sandbox=False,
            mutates_workspace=True,
            aliases=("edit_file",),
        ),
    )


def list_builtin_tool_descriptors() -> List[ToolDescriptor]:
    """返回内置工具定义列表，用于提示词、注册与配置一致性。"""
    language = get_language()
    return list(_build_builtin_tool_descriptors(language))


def list_builtin_tool_names() -> List[str]:
    """列出内置工具名称，供配置默认值与工具选择使用。"""
    return [descriptor.name for descriptor in list_builtin_tool_descriptors()]


def list_sandbox_tool_names() -> List[str]:
    """列出需要进入沙盒执行的工具名称。"""
    return [
        descriptor.name
        for descriptor in list_builtin_tool_descriptors()
        if descriptor.sandbox
    ]


@lru_cache(maxsize=1)
def build_builtin_tool_aliases() -> Dict[str, tuple[str, ...]]:
    """构建内置工具别名列表映射，供英文调用与校验复用。"""
    output: Dict[str, tuple[str, ...]] = {}
    for descriptor in list_builtin_tool_descriptors():
        aliases: List[str] = []
        for raw_alias in descriptor.aliases:
            alias = str(raw_alias or "").strip()
            if not alias or alias == descriptor.name:
                continue
            if alias in aliases:
                continue
            aliases.append(alias)
        output[descriptor.name] = tuple(aliases)
    return output


@lru_cache(maxsize=1)
def build_builtin_tool_alias_map() -> Dict[str, str]:
    """构建内置工具别名到标准名称的映射，便于统一解析。"""
    alias_map: Dict[str, str] = {}
    aliases_by_name = build_builtin_tool_aliases()
    for canonical_name, aliases in aliases_by_name.items():
        alias_map.setdefault(canonical_name, canonical_name)
        for alias in aliases:
            if alias in alias_map:
                continue
            alias_map[alias] = canonical_name
    return alias_map


def resolve_builtin_tool_name(raw_name: str) -> str:
    """解析内置工具别名为标准名称，未命中则原样返回。"""
    name = str(raw_name or "").strip()
    if not name:
        return ""
    return build_builtin_tool_alias_map().get(name, name)


def build_builtin_tool_specs() -> Dict[str, ToolSpec]:
    """构建内置工具规格映射，用于提示词注入与管理展示。"""
    return {
        descriptor.name: descriptor.to_spec()
        for descriptor in list_builtin_tool_descriptors()
    }


def build_builtin_tool_handlers() -> Dict[str, ToolCallable]:
    """构建内置工具执行入口映射，保证与规格一致。"""
    handlers: Dict[str, ToolCallable] = {}
    for descriptor in list_builtin_tool_descriptors():
        async_handler = descriptor.to_async_handler()
        if async_handler is not None and descriptor.runtime:
            handlers[descriptor.name] = async_handler
    return handlers


def build_sandbox_tool_handlers() -> Dict[str, ToolHandler]:
    """构建沙盒可执行的同步工具映射，仅包含允许沙盒运行的工具。"""
    handlers: Dict[str, ToolHandler] = {}
    for descriptor in list_builtin_tool_descriptors():
        if descriptor.handler is None or not descriptor.sandbox:
            continue
        handlers[descriptor.name] = descriptor.handler
    return handlers


def is_workspace_mutation_tool(tool_name: str) -> bool:
    """判断工具是否可能改写工作区内容，用于刷新目录树缓存。"""
    canonical_name = resolve_builtin_tool_name(tool_name)
    for descriptor in list_builtin_tool_descriptors():
        if descriptor.name == canonical_name:
            return descriptor.mutates_workspace
    return False
