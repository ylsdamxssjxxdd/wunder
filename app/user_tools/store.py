from __future__ import annotations

import json
import re
import shutil
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional

from app.core.config import MCPServerConfig, WunderConfig


@dataclass
class UserMcpServer:
    """用户自建 MCP 服务配置。"""

    name: str
    endpoint: str
    allow_tools: List[str]
    shared_tools: List[str]
    enabled: bool
    transport: str
    description: str
    display_name: str
    headers: Dict[str, str]
    auth: str
    tool_specs: List[Dict[str, Any]]


@dataclass
class UserSkillConfig:
    """用户技能启用/共享配置。"""

    enabled: List[str]
    shared: List[str]


@dataclass
class UserKnowledgeBase:
    """用户知识库配置。"""

    name: str
    description: str
    enabled: bool
    shared: bool


@dataclass
class UserToolsPayload:
    """用户自建工具配置载荷。"""

    user_id: str
    extra_prompt: str
    mcp_servers: List[UserMcpServer]
    skills: UserSkillConfig
    knowledge_bases: List[UserKnowledgeBase]
    version: float


@dataclass
class _UserToolsCache:
    """用户工具缓存条目。"""

    version: float
    payload: UserToolsPayload


@dataclass
class _SharedToolsCache:
    """共享工具缓存条目。"""

    version: float
    timestamp: float
    payloads: List[UserToolsPayload]


class UserToolStore:
    """用户自建工具存储管理器，按用户隔离保存。"""

    _SAFE_USER_PATTERN = re.compile(r"[^a-zA-Z0-9_-]")

    def __init__(self, config: WunderConfig) -> None:
        self._root = Path(config.workspace.root).resolve().parent / "user_tools"
        self._root.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        self._user_cache: Dict[str, _UserToolsCache] = {}
        self._shared_cache: Optional[_SharedToolsCache] = None
        self._shared_cache_ttl_s = 5.0
        self._shared_version = time.time()

    @staticmethod
    def build_alias_name(owner_id: str, tool_name: str) -> str:
        """构造统一的工具别名：user_id@tool_name。"""
        return f"{owner_id}@{tool_name}"

    def shared_version(self) -> float:
        """获取共享工具版本号，供提示词缓存失效判断。"""
        return self._shared_version

    def load_user_tools(self, user_id: str) -> UserToolsPayload:
        """读取指定用户的自建工具配置。"""
        safe_id = self._safe_user_id(user_id)
        path = self._config_path(safe_id)
        version = path.stat().st_mtime if path.exists() else 0.0
        with self._lock:
            cached = self._user_cache.get(safe_id)
            if cached and cached.version == version:
                return cached.payload
        payload = self._read_payload(path, fallback_user_id=user_id)
        payload.version = version
        with self._lock:
            self._user_cache[safe_id] = _UserToolsCache(version=version, payload=payload)
        return payload

    def update_mcp_servers(
        self, user_id: str, servers: List[Dict[str, Any]]
    ) -> UserToolsPayload:
        """更新用户 MCP 服务配置。"""
        payload = self.load_user_tools(user_id)
        payload.mcp_servers = self._normalize_mcp_servers(servers)
        return self._save_payload(user_id, payload)

    def update_skills(
        self, user_id: str, enabled: List[str], shared: List[str]
    ) -> UserToolsPayload:
        """更新用户技能启用/共享配置。"""
        payload = self.load_user_tools(user_id)
        payload.skills = self._normalize_skill_config(
            {"enabled": enabled, "shared": shared}
        )
        return self._save_payload(user_id, payload)

    def update_knowledge_bases(
        self, user_id: str, bases: List[Dict[str, Any]]
    ) -> UserToolsPayload:
        """更新用户知识库配置。"""
        payload = self.load_user_tools(user_id)
        previous_names = {base.name for base in payload.knowledge_bases if base.name}
        normalized = self._normalize_knowledge_bases(bases)
        next_names = {base.name for base in normalized if base.name}
        # 对比前后配置，删除被移除的知识库目录，避免残留文件
        removed = previous_names - next_names
        if removed:
            self._cleanup_knowledge_dirs(user_id, removed)
        payload.knowledge_bases = normalized
        return self._save_payload(user_id, payload)

    def update_extra_prompt(self, user_id: str, extra_prompt: str) -> UserToolsPayload:
        """更新用户附加提示词。"""
        payload = self.load_user_tools(user_id)
        payload.extra_prompt = extra_prompt or ""
        return self._save_payload(user_id, payload)

    def list_shared_payloads(self, exclude_user_id: str) -> List[UserToolsPayload]:
        """列出所有共享配置载荷，排除指定用户。"""
        now = time.time()
        with self._lock:
            cached = self._shared_cache
            if cached and now - cached.timestamp < self._shared_cache_ttl_s:
                return [
                    item
                    for item in cached.payloads
                    if item.user_id != exclude_user_id
                ]
        payloads = self._scan_shared_payloads()
        with self._lock:
            self._shared_cache = _SharedToolsCache(
                version=self._shared_version,
                timestamp=now,
                payloads=payloads,
            )
        return [item for item in payloads if item.user_id != exclude_user_id]

    def get_user_dir(self, user_id: str) -> Path:
        """获取用户目录路径。"""
        return self._user_dir(self._safe_user_id(user_id))

    def get_skill_root(self, user_id: str) -> Path:
        """获取用户技能根目录。"""
        return self.get_user_dir(user_id) / "skills"

    def get_knowledge_root(self, user_id: str) -> Path:
        """获取用户知识库根目录。"""
        return self.get_user_dir(user_id) / "knowledge"

    def resolve_knowledge_base_root(
        self, user_id: str, base_name: str, *, create: bool = False
    ) -> Path:
        """解析用户知识库目录路径，阻止路径穿越。"""
        cleaned = str(base_name or "").strip()
        if not cleaned:
            raise ValueError("知识库名称不能为空")
        if "/" in cleaned or "\\" in cleaned or ".." in cleaned:
            raise ValueError("知识库名称包含非法路径")
        root = self.get_knowledge_root(user_id)
        target = (root / cleaned).resolve()
        if target != root and root not in target.parents:
            raise ValueError("知识库路径越界访问被禁止")
        if create and not target.exists():
            target.mkdir(parents=True, exist_ok=True)
        return target

    def _cleanup_knowledge_dirs(self, user_id: str, removed: set[str]) -> None:
        """清理被移除的知识库目录，避免残留文件占用磁盘。"""
        for base_name in removed:
            try:
                target = self.resolve_knowledge_base_root(user_id, base_name, create=False)
            except ValueError:
                continue
            if target.exists() and target.is_dir():
                shutil.rmtree(target)

    def to_mcp_server_config(self, server: UserMcpServer) -> MCPServerConfig:
        """将用户 MCP 配置转换为 MCPServerConfig。"""
        return MCPServerConfig(
            name=server.name,
            endpoint=server.endpoint,
            allow_tools=list(server.allow_tools),
            enabled=server.enabled,
            transport=server.transport or None,
            description=server.description,
            display_name=server.display_name,
            headers=dict(server.headers),
            auth=server.auth or None,
            tool_specs=list(server.tool_specs),
        )

    def _scan_shared_payloads(self) -> List[UserToolsPayload]:
        """扫描全部用户目录，汇总共享配置。"""
        payloads: List[UserToolsPayload] = []
        for folder in self._root.iterdir():
            if not folder.is_dir():
                continue
            payload = self._read_payload(folder / "config.json")
            if not payload.user_id:
                payload.user_id = folder.name
            payloads.append(payload)
        return payloads

    def _read_payload(self, path: Path, fallback_user_id: str = "") -> UserToolsPayload:
        """读取磁盘配置并进行字段清洗。"""
        if not path.exists():
            return UserToolsPayload(
                user_id=fallback_user_id or "",
                extra_prompt="",
                mcp_servers=[],
                skills=UserSkillConfig(enabled=[], shared=[]),
                knowledge_bases=[],
                version=0.0,
            )
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            raw = {}
        raw_user_id = str(raw.get("user_id", "")).strip()
        extra_prompt = str(raw.get("extra_prompt", "") or "")
        mcp = self._normalize_mcp_servers(raw.get("mcp", {}).get("servers") or [])
        skills = self._normalize_skill_config(raw.get("skills") or {})
        knowledge = self._normalize_knowledge_bases(raw.get("knowledge", {}).get("bases") or [])
        return UserToolsPayload(
            user_id=raw_user_id or fallback_user_id or "",
            extra_prompt=extra_prompt,
            mcp_servers=mcp,
            skills=skills,
            knowledge_bases=knowledge,
            version=0.0,
        )

    def _save_payload(self, user_id: str, payload: UserToolsPayload) -> UserToolsPayload:
        """写入用户配置并刷新缓存。"""
        safe_id = self._safe_user_id(user_id)
        folder = self._user_dir(safe_id)
        folder.mkdir(parents=True, exist_ok=True)
        data = {
            "user_id": user_id,
            "extra_prompt": payload.extra_prompt or "",
            "mcp": {
                "servers": [self._mcp_server_to_dict(item) for item in payload.mcp_servers],
            },
            "skills": {
                "enabled": list(payload.skills.enabled),
                "shared": list(payload.skills.shared),
            },
            "knowledge": {
                "bases": [self._knowledge_base_to_dict(item) for item in payload.knowledge_bases],
            },
        }
        path = self._config_path(safe_id)
        path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
        version = path.stat().st_mtime
        payload.user_id = user_id
        payload.version = version
        with self._lock:
            self._user_cache[safe_id] = _UserToolsCache(version=version, payload=payload)
            self._shared_cache = None
            self._shared_version = time.time()
        return payload

    @classmethod
    def _safe_user_id(cls, user_id: str) -> str:
        """将 user_id 转为安全目录名，避免路径注入。"""
        cleaned = str(user_id or "").strip()
        safe = cls._SAFE_USER_PATTERN.sub("_", cleaned)
        return safe or "anonymous"

    def _user_dir(self, safe_user_id: str) -> Path:
        """获取用户目录路径。"""
        return self._root / safe_user_id

    def _config_path(self, safe_user_id: str) -> Path:
        """获取用户配置文件路径。"""
        return self._user_dir(safe_user_id) / "config.json"

    @staticmethod
    def _normalize_name_list(values: Any) -> List[str]:
        """清理字符串列表，去重并移除空值。"""
        items = values if isinstance(values, list) else []
        result: List[str] = []
        seen: set[str] = set()
        for raw in items:
            name = str(raw or "").strip()
            if not name or name in seen:
                continue
            seen.add(name)
            result.append(name)
        return result

    def _normalize_mcp_servers(self, raw_servers: Any) -> List[UserMcpServer]:
        """规范化 MCP 服务列表，保留草稿条目以便前端继续编辑。"""
        servers = raw_servers if isinstance(raw_servers, list) else []
        normalized: List[UserMcpServer] = []
        for raw in servers:
            if not isinstance(raw, dict):
                continue
            name = str(raw.get("name", "")).strip()
            endpoint = str(raw.get("endpoint", "")).strip()
            allow_tools = self._normalize_name_list(raw.get("allow_tools"))
            shared_tools = self._normalize_name_list(raw.get("shared_tools"))
            if allow_tools:
                shared_tools = [name for name in shared_tools if name in set(allow_tools)]
            tool_specs = raw.get("tool_specs")
            if not isinstance(tool_specs, list):
                tool_specs = []
            headers = raw.get("headers") if isinstance(raw.get("headers"), dict) else {}
            normalized.append(
                UserMcpServer(
                    name=name,
                    endpoint=endpoint,
                    allow_tools=allow_tools,
                    shared_tools=shared_tools,
                    enabled=raw.get("enabled", True) is not False,
                    transport=str(raw.get("transport", "") or ""),
                    description=str(raw.get("description", "") or ""),
                    display_name=str(raw.get("display_name", "") or ""),
                    headers={str(key): str(value) for key, value in headers.items()},
                    auth=str(raw.get("auth", "") or ""),
                    tool_specs=tool_specs,
                )
            )
        return normalized

    def _normalize_skill_config(self, raw: Any) -> UserSkillConfig:
        """规范化技能启用/共享配置，保证共享列表是启用子集。"""
        data = raw if isinstance(raw, dict) else {}
        enabled = self._normalize_name_list(data.get("enabled"))
        shared = self._normalize_name_list(data.get("shared"))
        shared = [name for name in shared if name in set(enabled)]
        return UserSkillConfig(enabled=enabled, shared=shared)

    def _normalize_knowledge_bases(self, raw: Any) -> List[UserKnowledgeBase]:
        """规范化知识库配置列表，保留草稿条目。"""
        items = raw if isinstance(raw, list) else []
        output: List[UserKnowledgeBase] = []
        seen: set[str] = set()
        for item in items:
            if not isinstance(item, dict):
                continue
            name = str(item.get("name", "")).strip()
            description = str(item.get("description", "") or "")
            enabled = item.get("enabled", True) is not False
            shared = bool(item.get("shared", False))
            if name:
                if name in seen:
                    continue
                seen.add(name)
            output.append(
                UserKnowledgeBase(
                    name=name,
                    description=description,
                    enabled=enabled,
                    shared=shared,
                )
            )
        return output

    @staticmethod
    def _mcp_server_to_dict(server: UserMcpServer) -> Dict[str, Any]:
        """序列化 MCP 服务配置。"""
        return {
            "name": server.name,
            "display_name": server.display_name,
            "endpoint": server.endpoint,
            "transport": server.transport,
            "description": server.description,
            "headers": server.headers,
            "auth": server.auth,
            "tool_specs": server.tool_specs,
            "allow_tools": server.allow_tools,
            "shared_tools": server.shared_tools,
            "enabled": bool(server.enabled),
        }

    @staticmethod
    def _knowledge_base_to_dict(base: UserKnowledgeBase) -> Dict[str, Any]:
        """序列化知识库配置。"""
        return {
            "name": base.name,
            "description": base.description,
            "enabled": bool(base.enabled),
            "shared": bool(base.shared),
        }
