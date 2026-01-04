from pathlib import Path
import json
import re
from typing import Any, Dict, List, Optional

import yaml

from app.core.config import WunderConfig, get_config, load_config, resolve_override_config_path
from app.core.i18n import t
from app.tools.catalog import resolve_builtin_tool_name
from app.tools.constants import BUILTIN_TOOL_NAMES

_SAFE_KNOWLEDGE_DIR_PATTERN = re.compile(r"[\\/:*?\"<>|]+")


def _sanitize_knowledge_dir_name(name: str) -> str:
    """将知识库名称转换为安全目录名，避免路径注入。"""
    cleaned = str(name or "").strip()
    cleaned = _SAFE_KNOWLEDGE_DIR_PATTERN.sub("_", cleaned)
    cleaned = cleaned.strip(". ")
    cleaned = cleaned.replace("..", "_")
    return cleaned


def _build_default_knowledge_root(name: str) -> str:
    """当未填写目录时，生成默认目录（./knowledge/<name>）。"""
    safe_dir = _sanitize_knowledge_dir_name(name)
    if not safe_dir:
        raise ValueError(t("error.knowledge_base_name_invalid"))
    return f"./knowledge/{safe_dir}"


def update_knowledge_config(path: Path, knowledge: Dict[str, Any]) -> WunderConfig:
    """更新字面知识库配置并返回最新配置。"""
    override_path = resolve_override_config_path()
    raw = _read_raw_config(override_path)
    target = raw.setdefault("knowledge", {})
    # 简化知识库配置，仅保留知识库列表，其余参数不再保存。
    target.pop("system_prompt", None)
    target.pop("max_documents", None)
    target.pop("candidate_limit", None)
    bases: List[Dict[str, Any]] = []
    for base in knowledge.get("bases", []) or []:
        if not isinstance(base, dict):
            continue
        name = str(base.get("name", "")).strip()
        if not name:
            continue
        root = str(base.get("root", "")).strip()
        enabled = base.get("enabled", True) is not False
        # 未填写目录时自动落到 ./knowledge/<name>
        if not root:
            root = _build_default_knowledge_root(name)
            try:
                Path(root).resolve().mkdir(parents=True, exist_ok=True)
            except OSError as exc:
                raise ValueError(
                    t("error.knowledge_root_create_failed", root=root, detail=str(exc))
                ) from exc
        bases.append(
            {
                "name": name,
                "description": str(base.get("description", "")).strip(),
                "root": root,
                "enabled": enabled,
            }
        )
    target["bases"] = bases
    _write_raw_config(override_path, raw)
    get_config.cache_clear()
    return load_config(path)


def _read_raw_config(path: Path) -> Dict[str, Any]:
    """读取原始配置内容，避免意外丢失字段。"""
    if not path.exists():
        return {}
    return yaml.safe_load(path.read_text(encoding="utf-8")) or {}


def _write_raw_config(path: Path, data: Dict[str, Any]) -> None:
    """写回配置文件，保持键顺序与中文内容。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    content = yaml.safe_dump(data, allow_unicode=True, sort_keys=False)
    path.write_text(content, encoding="utf-8")


def _clean_strings(items: List[str]) -> List[str]:
    """清理列表中的空项与重复项。"""
    seen: set[str] = set()
    output: List[str] = []
    for item in items:
        value = str(item).strip()
        if not value or value in seen:
            continue
        output.append(value)
        seen.add(value)
    return output


def update_mcp_servers(path: Path, servers: List[Dict[str, Any]]) -> WunderConfig:
    """更新 MCP 服务配置并返回最新配置。"""
    override_path = resolve_override_config_path()
    raw = _read_raw_config(override_path)
    cleaned: List[Dict[str, Any]] = []
    for server in servers:
        name = str(server.get("name", "")).strip()
        endpoint = str(
            server.get("endpoint")
            or server.get("baseUrl")
            or server.get("base_url")
            or server.get("url")
            or ""
        ).strip()
        if not name or not endpoint:
            continue
        allow_tools = server.get("allow_tools") or []
        enabled = bool(server.get("enabled", server.get("isActive", True)))
        transport = str(server.get("transport") or server.get("type") or "").strip()
        description = str(server.get("description", "")).strip()
        display_name = str(server.get("display_name") or server.get("displayName") or "").strip()
        headers = server.get("headers") or {}
        if isinstance(headers, str):
            try:
                headers = json.loads(headers)
            except json.JSONDecodeError:
                headers = {}
        if not isinstance(headers, dict):
            headers = {}
        headers = {str(key): str(value) for key, value in headers.items()}
        auth = server.get("auth")
        raw_tool_specs = server.get("tool_specs") or server.get("toolSpecs") or []
        tool_specs: List[Dict[str, Any]] = []
        if isinstance(raw_tool_specs, list):
            for item in raw_tool_specs:
                if not isinstance(item, dict):
                    continue
                tool_name = str(item.get("name", "")).strip()
                if not tool_name:
                    continue
                tool_specs.append(
                    {
                        "name": tool_name,
                        "description": str(item.get("description", "")).strip(),
                        "input_schema": item.get("input_schema")
                        or item.get("inputSchema")
                        or {},
                    }
                )
        cleaned.append(
            {
                "name": name,
                "endpoint": endpoint,
                "allow_tools": _clean_strings(list(allow_tools)),
                "enabled": enabled,
                "transport": transport or None,
                "description": description,
                "display_name": display_name,
                "headers": headers,
                "auth": auth,
                "tool_specs": tool_specs,
            }
        )
    raw.setdefault("mcp", {})["servers"] = cleaned
    _write_raw_config(override_path, raw)
    get_config.cache_clear()
    return load_config(path)


def update_a2a_services(path: Path, services: List[Dict[str, Any]]) -> WunderConfig:
    """更新 A2A 服务配置并返回最新配置。"""
    override_path = resolve_override_config_path()
    raw = _read_raw_config(override_path)
    cleaned: List[Dict[str, Any]] = []
    for service in services:
        name = str(service.get("name", "")).strip()
        endpoint = str(
            service.get("endpoint")
            or service.get("url")
            or service.get("baseUrl")
            or service.get("base_url")
            or ""
        ).strip()
        if not name or not endpoint or "@" in name:
            continue
        enabled = bool(service.get("enabled", service.get("isActive", True)))
        description = str(service.get("description", "")).strip()
        display_name = str(service.get("display_name") or service.get("displayName") or "").strip()
        headers = service.get("headers") or {}
        if isinstance(headers, str):
            try:
                headers = json.loads(headers)
            except json.JSONDecodeError:
                headers = {}
        if not isinstance(headers, dict):
            headers = {}
        headers = {str(key): str(value) for key, value in headers.items()}
        auth = service.get("auth")
        agent_card = service.get("agent_card") or service.get("agentCard") or {}
        if not isinstance(agent_card, dict):
            agent_card = {}
        allow_self = bool(service.get("allow_self", service.get("allowSelf", False)))
        max_depth = service.get("max_depth", service.get("maxDepth", 0))
        default_method = str(
            service.get("default_method") or service.get("defaultMethod") or "SendMessage"
        ).strip() or "SendMessage"
        cleaned.append(
            {
                "name": name,
                "endpoint": endpoint,
                "enabled": enabled,
                "description": description,
                "display_name": display_name,
                "headers": headers,
                "auth": auth,
                "agent_card": agent_card,
                "allow_self": allow_self,
                "max_depth": max_depth,
                "default_method": default_method,
            }
        )
    raw.setdefault("a2a", {})["services"] = cleaned
    _write_raw_config(override_path, raw)
    get_config.cache_clear()
    return load_config(path)


def update_skills(
    path: Path, enabled: List[str], paths: Optional[List[str]] = None
) -> WunderConfig:
    """更新技能启用列表与路径配置。"""
    override_path = resolve_override_config_path()
    raw = _read_raw_config(override_path)
    skills = raw.setdefault("skills", {})
    skills["enabled"] = _clean_strings(enabled)
    if paths is not None:
        skills["paths"] = _clean_strings(paths)
    _write_raw_config(override_path, raw)
    get_config.cache_clear()
    return load_config(path)


def update_builtin_tools(path: Path, enabled: List[str]) -> WunderConfig:
    """更新内置工具启用配置。"""
    override_path = resolve_override_config_path()
    raw = _read_raw_config(override_path)
    tools = raw.setdefault("tools", {})
    builtin = tools.setdefault("builtin", {})
    # 允许使用英文别名更新内置工具开关，统一回写为标准名称。
    cleaned = [resolve_builtin_tool_name(name) for name in _clean_strings(enabled)]
    allowed = set(BUILTIN_TOOL_NAMES)
    builtin["enabled"] = [name for name in cleaned if name in allowed]
    _write_raw_config(override_path, raw)
    get_config.cache_clear()
    return load_config(path)


def update_llm_config(path: Path, llm: Dict[str, Any]) -> WunderConfig:
    """更新 LLM 配置并返回最新配置。"""
    override_path = resolve_override_config_path()
    raw = _read_raw_config(override_path)
    allowed_keys = {
        "enable",
        "provider",
        "base_url",
        "api_key",
        "model",
        "temperature",
        "timeout_s",
        "retry",
        "max_rounds",
        "max_context",
        "max_output",
        "support_vision",
        "stream",
        "stream_include_usage",
        "history_compaction_ratio",
        "history_compaction_reset",
        "stop",
        "mock_if_unconfigured",
    }
    raw_models = llm.get("models") or {}
    cleaned_models: Dict[str, Dict[str, Any]] = {}
    for name, config in raw_models.items():
        model_name = str(name or "").strip()
        if not model_name or not isinstance(config, dict):
            continue
        cleaned: Dict[str, Any] = {}
        for key, value in config.items():
            if key in allowed_keys:
                cleaned[key] = value
        cleaned_models[model_name] = cleaned

    if not cleaned_models:
        raise ValueError(t("error.llm_config_required"))

    default_name = str(llm.get("default") or "").strip()
    if default_name not in cleaned_models:
        default_name = next(iter(cleaned_models))

    # 使用清洗后的结构覆盖旧配置，避免残留旧字段。
    raw["llm"] = {
        "default": default_name,
        "models": cleaned_models,
    }
    _write_raw_config(override_path, raw)
    get_config.cache_clear()
    return load_config(path)
