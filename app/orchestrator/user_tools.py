import os
import threading
from collections import OrderedDict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional

from app.core.config import MCPServerConfig
from app.core.i18n import t
from app.tools.availability import (
    build_knowledge_tool_spec_map,
    build_mcp_tool_spec_map,
    build_skill_tool_spec_map,
    normalize_mcp_input_schema,
    resolve_mcp_description,
)
from app.orchestrator.context import (
    RequestContext,
    UserSkillSource,
    UserToolAlias,
    UserToolBindings,
)
from app.skills.loader import load_skills
from app.skills.registry import SkillRegistry, SkillSpec
from app.tools.constants import BUILTIN_TOOL_NAMES
from app.tools.registry import ToolSpec
from app.user_tools.store import UserToolStore


@dataclass(frozen=True)
class _SkillSpecCache:
    """缓存用户技能规格，降低重复加载成本。"""

    signature: tuple
    specs: List[SkillSpec]


class UserToolManager:
    """用户工具绑定与技能索引构建器。"""

    def __init__(self, store: UserToolStore) -> None:
        self._store = store
        # 用户技能规格缓存：按 owner_id + 技能根目录复用，避免频繁扫描 SKILL.md
        self._skill_spec_cache: "OrderedDict[str, _SkillSpecCache]" = OrderedDict()
        self._skill_cache_lock = threading.Lock()
        self._skill_cache_max = 128

    @property
    def store(self) -> UserToolStore:
        """暴露用户工具存储实例，便于执行阶段复用。"""
        return self._store

    def clear_skill_cache(self, owner_id: Optional[str] = None) -> None:
        """清理技能规格缓存，确保后续读取到最新技能内容。"""
        with self._skill_cache_lock:
            if not owner_id:
                self._skill_spec_cache.clear()
                return
            prefix = f"{owner_id}::"
            to_remove = [key for key in self._skill_spec_cache if key.startswith(prefix)]
            for key in to_remove:
                self._skill_spec_cache.pop(key, None)

    @staticmethod
    def _build_skill_cache_signature(root: Path, names: List[str]) -> tuple:
        """构建技能缓存签名，基于目录时间与启用列表判断是否需要刷新。"""
        cleaned = sorted({name for name in names if str(name).strip()})
        try:
            root_mtime = root.stat().st_mtime
        except OSError:
            root_mtime = 0.0
        return (root_mtime, tuple(cleaned))

    def _load_cached_skill_specs(
        self,
        ctx: RequestContext,
        owner_id: str,
        skill_root: Path,
        names: List[str],
    ) -> List[SkillSpec]:
        """按缓存加载用户技能规格，减少重复扫描与解析。"""
        if not names:
            return []
        if not skill_root.exists():
            return []
        signature = self._build_skill_cache_signature(skill_root, names)
        cache_key = f"{owner_id}::{skill_root}"
        with self._skill_cache_lock:
            cached = self._skill_spec_cache.get(cache_key)
            if cached and cached.signature == signature:
                self._skill_spec_cache.move_to_end(cache_key)
                return cached.specs

        scan_config = ctx.config.model_copy(deep=True)
        scan_config.skills.paths = [str(skill_root)]
        scan_config.skills.enabled = list(signature[1])
        registry = load_skills(scan_config, load_entrypoints=False, only_enabled=True)
        specs = registry.list_specs()
        with self._skill_cache_lock:
            self._skill_spec_cache[cache_key] = _SkillSpecCache(
                signature=signature,
                specs=specs,
            )
            self._skill_spec_cache.move_to_end(cache_key)
            while len(self._skill_spec_cache) > self._skill_cache_max:
                self._skill_spec_cache.popitem(last=False)
        return specs

    @staticmethod
    def _extract_read_file_paths(args: Any) -> List[str]:
        """从读取文件参数中提取文件路径列表。"""
        if not isinstance(args, dict):
            return []
        paths: List[str] = []
        files = args.get("files")
        if isinstance(files, list):
            for item in files:
                if not isinstance(item, dict):
                    continue
                path = str(item.get("path", "")).strip()
                if path:
                    paths.append(path)
        return paths

    @staticmethod
    def _normalize_fs_path(raw_path: str, base: Optional[Path] = None) -> str:
        """规范化文件路径，便于匹配 SKILL.md。"""
        text = str(raw_path or "").strip()
        if not text:
            return ""
        try:
            path = Path(text).expanduser()
            if not path.is_absolute() and base is not None:
                path = base / path
            resolved = path.resolve(strict=False)
        except Exception:
            return ""
        return os.path.normcase(str(resolved))

    def build_skill_path_index(
        self,
        ctx: RequestContext,
        user_tool_bindings: Optional[UserToolBindings],
    ) -> Dict[str, List[str]]:
        """构建 SKILL.md 路径到技能名称的索引，用于统计技能调用次数。"""
        index: Dict[str, set[str]] = {}

        def _append_spec(spec: SkillSpec) -> None:
            raw_path = str(spec.path or "").strip()
            if not raw_path:
                return
            key = self._normalize_fs_path(raw_path)
            if not key:
                return
            index.setdefault(key, set()).add(spec.name)

        for spec in ctx.skills.list_specs():
            _append_spec(spec)
        if user_tool_bindings:
            for spec in user_tool_bindings.skill_specs:
                _append_spec(spec)
        return {key: sorted(names) for key, names in index.items()}

    def collect_skill_hits_from_read_file(
        self,
        args: Any,
        workspace_root: Path,
        skill_path_index: Dict[str, List[str]],
    ) -> Dict[str, str]:
        """识别读取 SKILL.md 的技能名称，返回技能名到触发路径的映射。"""
        hits: Dict[str, str] = {}
        if not skill_path_index:
            return hits
        for raw_path in self._extract_read_file_paths(args):
            normalized = self._normalize_fs_path(raw_path, workspace_root)
            if not normalized:
                continue
            for skill_name in skill_path_index.get(normalized, []):
                hits.setdefault(skill_name, raw_path)
        return hits

    def build_bindings(self, ctx: RequestContext, user_id: str) -> UserToolBindings:
        """构建自建/共享工具别名映射，供提示词注入与执行使用。"""
        user_payload = self._store.load_user_tools(user_id)
        shared_payloads = self._store.list_shared_payloads(user_id)

        mcp_specs = self._build_mcp_tool_spec_map(ctx)
        skill_specs = self._build_skill_tool_spec_map(ctx)
        knowledge_specs = self._build_knowledge_tool_spec_map(ctx)
        enabled_builtin = set(ctx.config.tools.builtin.enabled or [])
        builtin_names = {name for name in BUILTIN_TOOL_NAMES if name in enabled_builtin}

        blocked_names = set().union(
            builtin_names, mcp_specs.keys(), skill_specs.keys(), knowledge_specs.keys()
        )
        alias_specs: Dict[str, ToolSpec] = {}
        alias_map: Dict[str, UserToolAlias] = {}
        user_skill_specs: List[SkillSpec] = []
        skill_sources: Dict[str, UserSkillSource] = {}
        skill_registries: Dict[str, SkillRegistry] = {}
        mcp_servers: Dict[str, Dict[str, MCPServerConfig]] = {}

        knowledge_schema = {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": t("knowledge.tool.query.description"),
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": t("knowledge.tool.limit.description"),
                },
            },
            "required": ["query"],
        }

        def _append_alias(
            alias: str, spec: ToolSpec, *, kind: str, owner_id: str, target: str
        ) -> None:
            if alias in blocked_names or alias in alias_specs:
                return
            alias_specs[alias] = spec
            alias_map[alias] = UserToolAlias(kind=kind, owner_id=owner_id, target=target)
            blocked_names.add(alias)

        def _register_skill_source(owner_id: str, root: Path, names: List[str]) -> None:
            if not names:
                return
            existing = skill_sources.get(owner_id)
            if existing:
                merged = set(existing.names)
                merged.update(names)
                existing.names = sorted(merged)
                return
            skill_sources[owner_id] = UserSkillSource(root=root, names=sorted(set(names)))

        def _normalize_mcp_schema(tool: Dict[str, Any]) -> Dict[str, Any]:
            """?????????? MCP ????????"""
            return normalize_mcp_input_schema(tool)


        def _collect_mcp_tools(
            owner_id: str, servers: List[Any], *, shared_only: bool = False
        ) -> None:
            for server in servers:
                server_name = str(getattr(server, "name", "") or "").strip()
                if not server_name or "@" in server_name:
                    continue
                if not getattr(server, "enabled", True):
                    continue
                cached_tools = getattr(server, "tool_specs", None)
                if not isinstance(cached_tools, list) or not cached_tools:
                    continue
                allow_tools = list(getattr(server, "allow_tools", []) or [])
                shared_tools = list(getattr(server, "shared_tools", []) or [])
                tool_pool = [
                    str(tool.get("name", "")).strip()
                    for tool in cached_tools
                    if str(tool.get("name", "")).strip()
                ]
                enabled_names = set(allow_tools or tool_pool)
                if shared_only:
                    enabled_names &= set(shared_tools)
                if not enabled_names:
                    continue
                owner_map = mcp_servers.setdefault(owner_id, {})
                if server_name not in owner_map:
                    owner_map[server_name] = self._store.to_mcp_server_config(server)
                for tool in cached_tools:
                    tool_name = str(tool.get("name", "")).strip()
                    if tool_name not in enabled_names:
                        continue
                    description = resolve_mcp_description(server, tool)
                    if not description:
                        description = str(getattr(server, "description", "") or "")
                    alias_name = self._store.build_alias_name(
                        owner_id, f"{server_name}@{tool_name}"
                    )
                    _append_alias(
                        alias_name,
                        ToolSpec(
                            name=alias_name,
                            description=description,
                            args_schema=_normalize_mcp_schema(tool),
                        ),
                        kind="mcp",
                        owner_id=owner_id,
                        target=f"{server_name}@{tool_name}",
                    )

        def _collect_skill_tools(owner_id: str, names: List[str]) -> None:
            if not names:
                return
            skill_root = self._store.get_skill_root(owner_id)
            specs = self._load_cached_skill_specs(ctx, owner_id, skill_root, list(names))
            if not specs:
                return
            _register_skill_source(owner_id, skill_root, names)
            for spec in specs:
                alias_name = self._store.build_alias_name(owner_id, spec.name)
                if alias_name in blocked_names or alias_name in alias_map:
                    continue
                # 技能仅用于提示词技能区块展示，不注入 <tools> 工具定义
                blocked_names.add(alias_name)
                alias_map[alias_name] = UserToolAlias(
                    kind="skill",
                    owner_id=owner_id,
                    target=spec.name,
                )
                user_skill_specs.append(
                    SkillSpec(
                        name=alias_name,
                        description=spec.description,
                        path=spec.path,
                        input_schema=spec.input_schema or {},
                        frontmatter=spec.frontmatter,
                    )
                )

        def _collect_knowledge_tools(
            owner_id: str, bases: List[Any], *, shared_only: bool = False
        ) -> None:
            for base in bases:
                base_name = str(getattr(base, "name", "") or "").strip()
                if not base_name:
                    continue
                if getattr(base, "enabled", True) is False:
                    continue
                if shared_only and not getattr(base, "shared", False):
                    continue
                description = str(getattr(base, "description", "") or "").strip()
                if not description:
                    description = t("knowledge.tool.description", name=base_name)
                alias_name = self._store.build_alias_name(owner_id, base_name)
                _append_alias(
                    alias_name,
                    ToolSpec(
                        name=alias_name,
                        description=description,
                        args_schema=knowledge_schema,
                    ),
                    kind="knowledge",
                    owner_id=owner_id,
                    target=base_name,
                )

        owner_id = user_payload.user_id or user_id
        _collect_mcp_tools(owner_id, user_payload.mcp_servers)
        _collect_skill_tools(owner_id, list(user_payload.skills.enabled))
        _collect_knowledge_tools(owner_id, user_payload.knowledge_bases)

        for shared_payload in shared_payloads:
            shared_owner = str(shared_payload.user_id or "").strip()
            if not shared_owner:
                continue
            _collect_mcp_tools(shared_owner, shared_payload.mcp_servers, shared_only=True)
            _collect_skill_tools(shared_owner, list(shared_payload.skills.shared))
            _collect_knowledge_tools(shared_owner, shared_payload.knowledge_bases, shared_only=True)

        return UserToolBindings(
            alias_specs=alias_specs,
            alias_map=alias_map,
            skill_specs=user_skill_specs,
            skill_sources=skill_sources,
            skill_registries=skill_registries,
            mcp_servers=mcp_servers,
            extra_prompt=user_payload.extra_prompt or "",
            user_version=user_payload.version,
            shared_version=self._store.shared_version(),
        )

    def get_user_skill_registry(
        self, ctx: RequestContext, bindings: UserToolBindings, owner_id: str
    ) -> Optional[SkillRegistry]:
        """按需加载并缓存用户技能注册表。"""
        registry = bindings.skill_registries.get(owner_id)
        if registry is not None:
            return registry
        source = bindings.skill_sources.get(owner_id)
        if not source or not source.names:
            return None
        scan_config = ctx.config.model_copy(deep=True)
        scan_config.skills.paths = [str(source.root)]
        scan_config.skills.enabled = list(source.names)
        registry = load_skills(scan_config, load_entrypoints=True, only_enabled=True)
        bindings.skill_registries[owner_id] = registry
        return registry

    @staticmethod
    def _build_skill_tool_spec_map(ctx: RequestContext) -> Dict[str, ToolSpec]:
        """????????????????????"""
        return build_skill_tool_spec_map(ctx.skills.list_specs())

    @staticmethod

    def _build_knowledge_tool_spec_map(ctx: RequestContext) -> Dict[str, ToolSpec]:
        """?????????????????????"""
        skill_names = {spec.name for spec in ctx.skills.list_specs()}
        return build_knowledge_tool_spec_map(ctx.config, blocked_names=skill_names)

    @staticmethod

    def _build_mcp_tool_spec_map(ctx: RequestContext) -> Dict[str, ToolSpec]:
        """?? MCP ?????????????????"""
        return build_mcp_tool_spec_map(ctx.config)

