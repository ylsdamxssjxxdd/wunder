from __future__ import annotations

from functools import lru_cache
from typing import Any, Dict, Iterable, List, Optional, Sequence, Set, Tuple

from app.core.config import WunderConfig
from app.core.i18n import get_language, t
from app.knowledge.service import build_knowledge_tool_specs, get_knowledge_tool_names
from app.skills.registry import SkillSpec
from app.tools.catalog import build_builtin_tool_aliases
from app.tools.constants import BUILTIN_TOOL_NAMES
from app.tools.registry import ToolSpec
from app.tools.specs import build_eva_tool_specs


_A2A_TOOL_PREFIX = "a2a@"


def normalize_mcp_input_schema(raw_tool: Dict[str, Any]) -> Dict[str, Any]:
    """规范化 MCP 工具的输入结构，统一字段名与空结构表现。"""
    if not isinstance(raw_tool, dict):
        return {"type": "object", "properties": {}}
    input_schema = raw_tool.get("inputSchema") or raw_tool.get("input_schema") or {
        "type": "object",
        "properties": {},
    }
    if not isinstance(input_schema, dict):
        return {"type": "object", "properties": {}}
    return input_schema


def resolve_mcp_description(server: Any, tool: Dict[str, Any]) -> str:
    """统一 MCP 工具描述策略，优先工具描述，缺失时回退到服务描述。"""
    description = str(tool.get("description", "")).strip()
    if description:
        return description
    fallback = (
        str(getattr(server, "description", "") or "")
        or str(getattr(server, "display_name", "") or "")
    )
    return str(fallback).strip()


def is_a2a_tool_name(name: str) -> bool:
    """判断是否为 A2A 服务工具名。"""
    cleaned = str(name or "")
    return cleaned.startswith(_A2A_TOOL_PREFIX) and len(cleaned) > len(_A2A_TOOL_PREFIX)


def extract_a2a_service_name(name: str) -> str:
    """从 A2A 工具名中解析服务名。"""
    if not is_a2a_tool_name(name):
        return ""
    return str(name)[len(_A2A_TOOL_PREFIX) :].strip()


@lru_cache(maxsize=2)
def _build_a2a_args_schema() -> Dict[str, Any]:
    """生成 A2A 服务工具的输入 Schema。"""
    return {
        "type": "object",
        "properties": {
            "content": {
                "type": "string",
                "description": t("tool.spec.a2a_service.args.content"),
            },
            "message": {
                "type": "object",
                "description": t("tool.spec.a2a_service.args.message"),
            },
            "session_id": {
                "type": "string",
                "description": t("tool.spec.a2a_service.args.session_id"),
            },
            "user_id": {
                "type": "string",
                "description": t("tool.spec.a2a_service.args.user_id"),
            },
            "tool_names": {
                "type": "array",
                "description": t("tool.spec.a2a_service.args.tool_names"),
                "items": {"type": "string"},
            },
        },
        "required": ["content"],
    }


def _resolve_agent_card_list_names(items: Any) -> List[str]:
    """从 AgentCard 列表中提取可读名称。"""
    if not isinstance(items, list):
        return []
    names: List[str] = []
    seen: Set[str] = set()
    for item in items:
        if not isinstance(item, dict):
            continue
        raw = str(item.get("name") or item.get("id") or "").strip()
        if not raw or raw in seen:
            continue
        names.append(raw)
        seen.add(raw)
    return names


def _build_a2a_capability_summary(agent_card: Optional[Dict[str, Any]]) -> str:
    """构建 A2A AgentCard 能力摘要，附加到工具描述。"""
    if not isinstance(agent_card, dict):
        return ""
    language = get_language().lower()
    is_en = language.startswith("en")
    item_sep = ", " if is_en else "，"
    section_sep = "; " if is_en else "；"
    max_items = 5

    skills = _resolve_agent_card_list_names(agent_card.get("skills"))
    skills_text = ""
    if skills:
        total = len(skills)
        shown = skills[:max_items]
        joined = item_sep.join(shown)
        if total > max_items:
            skills_text = t(
                "tool.spec.a2a_service.summary.skills_more",
                names=joined,
                count=total,
            )
        else:
            skills_text = t("tool.spec.a2a_service.summary.skills", skills=joined)

    tooling = agent_card.get("tooling") if isinstance(agent_card.get("tooling"), dict) else {}
    tool_parts: List[str] = []
    for key, label_key in (
        ("builtin", "tool.spec.a2a_service.summary.tool.builtin"),
        ("mcp", "tool.spec.a2a_service.summary.tool.mcp"),
        ("a2a", "tool.spec.a2a_service.summary.tool.a2a"),
        ("knowledge", "tool.spec.a2a_service.summary.tool.knowledge"),
    ):
        items = tooling.get(key)
        count = len(items) if isinstance(items, list) else 0
        if count:
            tool_parts.append(t(label_key, count=count))
    tools_text = ""
    if tool_parts:
        tools_text = t("tool.spec.a2a_service.summary.tools", tools=item_sep.join(tool_parts))

    parts = [part for part in (skills_text, tools_text) if part]
    return section_sep.join(parts)


def _resolve_a2a_description(service: Any) -> str:
    """统一 A2A 服务工具描述。"""
    description = str(getattr(service, "description", "") or "").strip()
    agent_card = getattr(service, "agent_card", None)
    if not description and isinstance(agent_card, dict):
        description = str(agent_card.get("description") or "").strip()
    if not description:
        display_name = str(getattr(service, "display_name", "") or "").strip()
        name = str(getattr(service, "name", "") or "").strip()
        label = display_name or name or "A2A"
        description = t("tool.spec.a2a_service.description", name=label)
    summary = _build_a2a_capability_summary(agent_card)
    if summary and summary not in description:
        is_en = get_language().lower().startswith("en")
        left = "(" if is_en else "（"
        right = ")" if is_en else "）"
        return f"{description}{left}{summary}{right}"
    return description


def _iter_a2a_tool_specs(config: WunderConfig) -> Iterable[Tuple[str, ToolSpec]]:
    """遍历 A2A 服务配置，生成工具规格。"""
    schema = _build_a2a_args_schema()
    a2a_config = getattr(config, "a2a", None)
    services = a2a_config.services if a2a_config else []
    for service in services or []:
        if not getattr(service, "enabled", True):
            continue
        name = str(getattr(service, "name", "") or "").strip()
        if not name or "@" in name:
            continue
        tool_name = f"{_A2A_TOOL_PREFIX}{name}"
        yield tool_name, ToolSpec(
            name=tool_name,
            description=_resolve_a2a_description(service),
            args_schema=schema,
        )


def _iter_mcp_tool_specs(config: WunderConfig) -> Iterable[Tuple[str, ToolSpec]]:
    """遍历 MCP 工具规格，输出标准化 ToolSpec，供多处复用。"""
    for server in config.mcp.servers:
        if not getattr(server, "enabled", True):
            continue
        cached_tools = getattr(server, "tool_specs", None)
        if not isinstance(cached_tools, list) or not cached_tools:
            continue
        allow_tools = list(getattr(server, "allow_tools", []) or [])
        for tool in cached_tools:
            if not isinstance(tool, dict):
                continue
            tool_name = str(tool.get("name", "")).strip()
            if not tool_name:
                continue
            if allow_tools and tool_name not in allow_tools:
                continue
            full_name = f"{server.name}@{tool_name}"
            yield full_name, ToolSpec(
                name=full_name,
                description=resolve_mcp_description(server, tool),
                args_schema=normalize_mcp_input_schema(tool),
            )


def build_enabled_builtin_spec_map(config: WunderConfig) -> Dict[str, ToolSpec]:
    """构建已启用内置工具的规格映射，便于快速查找。"""
    specs = build_eva_tool_specs()
    enabled = set(config.tools.builtin.enabled or [])
    return {
        name: specs[name]
        for name in BUILTIN_TOOL_NAMES
        if name in enabled and name in specs
    }


def build_enabled_builtin_specs(
    config: WunderConfig, *, allowed_names: Optional[Set[str]] = None
) -> List[ToolSpec]:
    """按内置工具顺序输出规格列表，可选过滤允许的工具名称集合。"""
    specs = build_eva_tool_specs()
    enabled = set(config.tools.builtin.enabled or [])
    output: List[ToolSpec] = []
    # 内置工具支持英文别名，按语言选择展示名称。
    aliases_by_name = build_builtin_tool_aliases()
    language = get_language()
    prefer_alias = language.startswith("en")
    for name in BUILTIN_TOOL_NAMES:
        if name not in enabled:
            continue
        if name not in specs:
            continue
        aliases = aliases_by_name.get(name, ())
        candidates: List[str] = []
        if prefer_alias and aliases:
            candidates.extend(aliases)
        candidates.append(name)
        selected_name = ""
        if allowed_names is None:
            selected_name = candidates[0]
        else:
            for candidate in candidates:
                if candidate in allowed_names:
                    selected_name = candidate
                    break
        if not selected_name:
            continue
        if selected_name == name:
            output.append(specs[name])
        else:
            output.append(
                ToolSpec(
                    name=selected_name,
                    description=specs[name].description,
                    args_schema=specs[name].args_schema,
                )
            )
    return output


def build_mcp_tool_spec_map(config: WunderConfig) -> Dict[str, ToolSpec]:
    """构建 MCP 工具规格映射，统一过滤与 schema 规范化策略。"""
    return {name: spec for name, spec in _iter_mcp_tool_specs(config)}


def build_a2a_tool_spec_map(config: WunderConfig) -> Dict[str, ToolSpec]:
    """构建 A2A 服务工具规格映射。"""
    return {name: spec for name, spec in _iter_a2a_tool_specs(config)}


def build_a2a_tool_specs(
    config: WunderConfig, *, allowed_names: Optional[Set[str]] = None
) -> List[ToolSpec]:
    """输出 A2A 服务工具规格列表，可选按工具名过滤。"""
    specs: List[ToolSpec] = []
    for name, spec in _iter_a2a_tool_specs(config):
        if allowed_names is not None and name not in allowed_names:
            continue
        specs.append(spec)
    return specs


def build_mcp_tool_specs(
    config: WunderConfig, *, allowed_names: Optional[Set[str]] = None
) -> List[ToolSpec]:
    """输出 MCP 工具规格列表，可选按工具名过滤。"""
    specs: List[ToolSpec] = []
    for name, spec in _iter_mcp_tool_specs(config):
        if allowed_names is not None and name not in allowed_names:
            continue
        specs.append(spec)
    return specs


def build_skill_tool_spec_map(skill_specs: Sequence[SkillSpec]) -> Dict[str, ToolSpec]:
    """将技能元数据转换为 ToolSpec 映射，统一供工具别名与列表使用。"""
    output: Dict[str, ToolSpec] = {}
    for spec in skill_specs:
        output[spec.name] = ToolSpec(
            name=spec.name,
            description=spec.description,
            args_schema=spec.input_schema or {},
        )
    return output


def build_knowledge_tool_spec_map(
    config: WunderConfig, *, blocked_names: Optional[Set[str]] = None
) -> Dict[str, ToolSpec]:
    """构建知识库工具规格映射，自动处理与技能名称冲突。"""
    return {
        spec.name: spec
        for spec in build_knowledge_tool_specs(config, blocked_names=blocked_names)
    }


def build_knowledge_tool_specs_filtered(
    config: WunderConfig,
    *,
    blocked_names: Optional[Set[str]] = None,
    allowed_names: Optional[Set[str]] = None,
) -> List[ToolSpec]:
    """输出知识库工具规格列表，支持冲突过滤与允许列表过滤。"""
    output: List[ToolSpec] = []
    for spec in build_knowledge_tool_specs(config, blocked_names=blocked_names):
        if allowed_names is not None and spec.name not in allowed_names:
            continue
        output.append(spec)
    return output


def collect_available_tool_names(
    config: WunderConfig,
    skill_specs: Sequence[SkillSpec],
    user_tool_bindings: Optional[Any] = None,
) -> Set[str]:
    """汇总当前可用的工具名称集合，统一内置/MCP/技能/知识库/自建工具策略。"""
    names: Set[str] = set()
    enabled_builtin = set(build_enabled_builtin_spec_map(config).keys())
    names.update(enabled_builtin)
    names.update(build_mcp_tool_spec_map(config).keys())
    names.update(build_a2a_tool_spec_map(config).keys())

    skill_names = {spec.name for spec in skill_specs}
    names.update(skill_names)
    names.update(get_knowledge_tool_names(config, blocked_names=skill_names))

    if user_tool_bindings:
        alias_specs = getattr(user_tool_bindings, "alias_specs", None)
        if isinstance(alias_specs, dict):
            names.update(alias_specs.keys())
        user_skill_specs = getattr(user_tool_bindings, "skill_specs", None)
        if isinstance(user_skill_specs, list):
            names.update(
                {
                    getattr(spec, "name", "")
                    for spec in user_skill_specs
                    if getattr(spec, "name", "")
                }
            )
    # 内置工具别名仅在未与其他工具名冲突时加入可用集合。
    aliases_by_name = build_builtin_tool_aliases()
    for canonical_name in enabled_builtin:
        for alias in aliases_by_name.get(canonical_name, ()):
            if alias in names:
                continue
            names.add(alias)
    return names


def collect_prompt_tool_specs(
    config: WunderConfig,
    skill_specs: Sequence[SkillSpec],
    allowed_tool_names: Set[str],
    user_tool_bindings: Optional[Any] = None,
) -> List[ToolSpec]:
    """按提示词注入顺序收集可用工具规格，确保多处输出一致。"""
    if not allowed_tool_names:
        return []
    tools: List[ToolSpec] = []
    tools.extend(build_enabled_builtin_specs(config, allowed_names=allowed_tool_names))
    tools.extend(build_mcp_tool_specs(config, allowed_names=allowed_tool_names))
    tools.extend(build_a2a_tool_specs(config, allowed_names=allowed_tool_names))
    skill_names = {spec.name for spec in skill_specs}
    tools.extend(
        build_knowledge_tool_specs_filtered(
            config, blocked_names=skill_names, allowed_names=allowed_tool_names
        )
    )
    if user_tool_bindings:
        alias_specs = getattr(user_tool_bindings, "alias_specs", None)
        if isinstance(alias_specs, dict):
            for name, spec in alias_specs.items():
                if name not in allowed_tool_names:
                    continue
                tools.append(spec)
    return tools
