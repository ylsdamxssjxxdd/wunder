import asyncio
import json
import os
import re
import shutil
import threading
import time
from datetime import datetime
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple

from app.core.config import WunderConfig
from app.core.i18n import t
from app.storage.sqlite import SQLiteStorage, get_storage


@dataclass
class WorkspaceContext:
    """工作区上下文信息，便于在工具与编排之间传递。"""

    user_id: str
    session_id: str
    root: Path


def build_workspace_tree(root: Path, max_depth: int = 2) -> str:
    """生成工作区目录树（最多两层）。"""
    if not root.exists():
        return t("workspace.tree.empty")

    lines: List[str] = []
    root_parts = len(root.parts)
    for current, dirs, files in os.walk(root):
        depth = len(Path(current).parts) - root_parts
        if depth > max_depth:
            dirs[:] = []
            continue
        prefix = "  " * depth
        for name in sorted(dirs):
            lines.append(f"{prefix}{name}/")
        for name in sorted(files):
            lines.append(f"{prefix}{name}")
    return "\n".join(lines) if lines else t("workspace.tree.empty")


class WorkspaceManager:
    """按用户维度管理持久化工作区。"""

    _MIGRATION_MARKER = ".wunder_workspace_v2"

    def __init__(self, config: WunderConfig) -> None:
        self._root = Path(config.workspace.root).resolve()
        # 历史记录使用 SQLite 持久化，保留 data/historys 目录用于旧数据迁移
        self._history_root = Path("data/historys").resolve()
        self._storage: SQLiteStorage = get_storage(config.storage.db_path)
        self._locks: Dict[str, asyncio.Lock] = {}
        self._locks_guard = asyncio.Lock()
        self._tree_cache: Dict[str, str] = {}
        self._tree_versions: Dict[str, int] = {}
        self._tree_dirty: Dict[str, bool] = {}
        self._tree_lock = threading.Lock()
        self._user_usage_cache: Dict[str, Dict[str, int]] = {}
        self._user_usage_cache_time = 0.0
        self._user_usage_cache_ttl_s = 5.0
        self._user_usage_cache_lock = threading.Lock()
        # 历史清理策略：retention_days <= 0 表示不做清理
        self._retention_days = self._normalize_retention_days(config.workspace.retention_days)
        self._retention_cleanup_interval_s = 3600.0
        self._retention_last_cleanup = 0.0
        self._retention_cleanup_running = False
        self._retention_cleanup_lock = threading.Lock()
        self._root.mkdir(parents=True, exist_ok=True)
        self._storage.ensure_initialized()

    def _safe_user_id(self, user_id: str) -> str:
        """将用户 id 转为安全路径名，避免非法字符。"""
        return re.sub(r"[^a-zA-Z0-9_-]", "_", user_id.strip()) or "anonymous"

    def _history_migration_key(self, user_id: str) -> str:
        """生成历史迁移标记键，避免重复导入。"""
        return f"history_migrated:{self._safe_user_id(user_id)}"

    def _session_token_usage_key(self, user_id: str, session_id: str) -> str:
        """生成会话 token 统计键，隔离用户与会话维度。"""
        safe_user = self._safe_user_id(user_id)
        safe_session = re.sub(r"[^a-zA-Z0-9_-]", "_", str(session_id or "").strip()) or "default"
        return f"session_token_usage:{safe_user}:{safe_session}"

    @staticmethod
    def _normalize_retention_days(value: Any) -> int:
        """解析 retention_days，异常时回退为 0。"""
        try:
            return int(value)
        except (TypeError, ValueError):
            return 0

    @staticmethod
    def _normalize_history_limit(limit: Any) -> Optional[int]:
        """解析历史条目上限，<= 0 表示不限制条数。"""
        try:
            value = int(limit)
        except (TypeError, ValueError):
            return None
        if value <= 0:
            return None
        return value

    def _maybe_schedule_retention_cleanup(self) -> None:
        """按节流策略触发历史清理，避免频繁阻塞写入。"""
        if self._retention_days <= 0:
            return
        now = time.time()
        with self._retention_cleanup_lock:
            if self._retention_cleanup_running:
                return
            if now - self._retention_last_cleanup < self._retention_cleanup_interval_s:
                return
            self._retention_last_cleanup = now
            self._retention_cleanup_running = True
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            # 无事件循环时同步清理，避免泄露运行状态
            try:
                self._storage.cleanup_retention(self._retention_days)
            finally:
                with self._retention_cleanup_lock:
                    self._retention_cleanup_running = False
            return
        try:
            loop.create_task(self._run_retention_cleanup())
        except RuntimeError:
            # 事件循环异常时回退为同步清理，确保状态可恢复
            try:
                self._storage.cleanup_retention(self._retention_days)
            finally:
                with self._retention_cleanup_lock:
                    self._retention_cleanup_running = False

    async def _run_retention_cleanup(self) -> None:
        """异步执行历史清理，减少对主流程的影响。"""
        try:
            await asyncio.to_thread(self._storage.cleanup_retention, self._retention_days)
        finally:
            with self._retention_cleanup_lock:
                self._retention_cleanup_running = False

    @staticmethod
    def _read_jsonl(path: Path) -> Iterable[Dict[str, Any]]:
        """逐行读取 JSONL 文件，过滤无效记录。"""
        try:
            with path.open("r", encoding="utf-8", errors="ignore") as file:
                for line in file:
                    text = line.strip()
                    if not text:
                        continue
                    try:
                        payload = json.loads(text)
                    except json.JSONDecodeError:
                        continue
                    if isinstance(payload, dict):
                        yield payload
        except OSError:
            return

    async def get_user_lock(self, user_id: str) -> asyncio.Lock:
        """为用户获取独占锁，防止并发写入冲突。"""
        safe_id = self._safe_user_id(user_id)
        async with self._locks_guard:
            if safe_id not in self._locks:
                self._locks[safe_id] = asyncio.Lock()
            return self._locks[safe_id]

    def workspace_root(self, user_id: str) -> Path:
        """获取用户工作区根目录。"""
        safe_id = self._safe_user_id(user_id)
        return self._root / safe_id

    def workspace_files_root(self, user_id: str) -> Path:
        """获取用户可见文件根目录。"""
        return self.workspace_root(user_id) / "files"

    def history_root(self, user_id: str) -> Path:
        """获取用户历史记录目录（用于旧版迁移）。"""
        safe_id = self._safe_user_id(user_id)
        return self._history_root / safe_id

    def ensure_workspace(self, user_id: str) -> Path:
        """创建用户工作区基础结构。"""
        root = self.workspace_root(user_id)
        files_root = self.workspace_files_root(user_id)
        root.mkdir(parents=True, exist_ok=True)
        files_root.mkdir(parents=True, exist_ok=True)
        self._ensure_history_storage(user_id)
        self._migrate_legacy_history(user_id, root)
        self._migrate_legacy_files(user_id, root, files_root)
        return files_root

    def _ensure_history_storage(self, user_id: str) -> None:
        """确保 SQLite 已初始化，并准备旧版历史目录。"""
        self._storage.ensure_initialized()

    def _migrate_legacy_history(self, user_id: str, root: Path) -> None:
        """迁移旧版 JSONL 历史记录到 SQLite。"""
        migration_key = self._history_migration_key(user_id)
        if self._storage.get_meta(migration_key) == "1":
            return
        history_root = self.history_root(user_id)
        legacy_roots = [root]
        if history_root.exists():
            legacy_roots.append(history_root)
        seen_paths: set[Path] = set()
        migrated = False
        for name, writer in (
            ("chat_history.jsonl", self._storage.append_chat),
            ("tool_log.jsonl", self._storage.append_tool_log),
        ):
            for legacy_root in legacy_roots:
                legacy_path = (legacy_root / name).resolve()
                if legacy_path in seen_paths:
                    continue
                seen_paths.add(legacy_path)
                if not legacy_path.exists():
                    continue
                for payload in self._read_jsonl(legacy_path):
                    writer(user_id, payload)
                    migrated = True
                if legacy_root == root:
                    target = history_root / name
                    if target.exists():
                        target = self._resolve_collision(target)
                    try:
                        history_root.mkdir(parents=True, exist_ok=True)
                        shutil.move(str(legacy_path), str(target))
                    except Exception:
                        continue
        if migrated:
            self._storage.set_meta(migration_key, "1")

    def _migrate_legacy_files(self, user_id: str, root: Path, files_root: Path) -> None:
        """迁移旧版工作区文件到 files 目录，避免用户文件散落。"""
        marker = root / self._MIGRATION_MARKER
        if marker.exists():
            return
        reserved = {
            "files",
            "chat_history.jsonl",
            "tool_log.jsonl",
            self._MIGRATION_MARKER,
        }
        for item in root.iterdir():
            if item.name in reserved:
                continue
            dest = files_root / item.name
            if dest.exists():
                dest = self._resolve_collision(dest)
            try:
                shutil.move(str(item), str(dest))
            except Exception:
                continue
        marker.touch(exist_ok=True)

    @staticmethod
    def _resolve_collision(path: Path) -> Path:
        """处理目标文件重名冲突，避免覆盖。"""
        suffix = 1
        candidate = path
        while candidate.exists():
            candidate = path.with_name(f"{path.name}.migrated_{suffix}")
            suffix += 1
        return candidate

    def resolve_path(self, user_id: str, relative_path: str) -> Path:
        """在工作区内解析文件路径，禁止越界访问。"""
        root = self.workspace_files_root(user_id)
        rel = Path(relative_path)
        if rel.is_absolute():
            raise ValueError(t("error.absolute_path_forbidden"))
        target = (root / rel).resolve()
        if root not in target.parents and target != root:
            raise ValueError(t("error.path_out_of_bounds"))
        return target

    def list_workspace_entries(
        self,
        user_id: str,
        relative_path: str,
        keyword: Optional[str] = None,
        offset: int = 0,
        limit: int = 0,
        sort_by: str = "name",
        order: str = "asc",
    ) -> tuple[List[Dict[str, Any]], int, str, Optional[str], int]:
        """列出指定目录下的文件与目录条目，支持关键字过滤与分页。"""
        normalized = str(relative_path or "").replace("\\", "/").strip()
        if normalized in {"", ".", "/"}:
            normalized = ""
        else:
            normalized = normalized.lstrip("/")
        target = self.resolve_path(user_id, normalized or ".")
        if not target.exists():
            raise ValueError(t("workspace.error.path_not_found"))
        if not target.is_dir():
            raise ValueError(t("workspace.error.path_not_dir"))

        root = self.workspace_files_root(user_id)
        keyword_lower = str(keyword or "").strip().lower()
        entries: List[Tuple[Dict[str, Any], float]] = []
        for item in target.iterdir():
            if keyword_lower and keyword_lower not in item.name.lower():
                continue
            entry, updated_ts = self._build_entry_payload(root, item)
            entries.append((entry, updated_ts))

        total = len(entries)
        sort_field = sort_by if sort_by in {"name", "size", "updated_time"} else "name"
        reverse = str(order or "").lower() == "desc"

        def sort_key(payload: Tuple[Dict[str, Any], float]) -> Any:
            entry, updated_ts = payload
            if sort_field == "size":
                return entry.get("size", 0)
            if sort_field == "updated_time":
                return updated_ts
            return entry.get("name", "").lower()

        dirs = [payload for payload in entries if payload[0].get("type") == "dir"]
        files = [payload for payload in entries if payload[0].get("type") != "dir"]
        dirs_sorted = sorted(dirs, key=sort_key, reverse=reverse)
        files_sorted = sorted(files, key=sort_key, reverse=reverse)
        entries_sorted = [entry for entry, _ in dirs_sorted + files_sorted]

        safe_offset = max(int(offset or 0), 0)
        safe_limit = max(int(limit or 0), 0)
        if safe_offset or safe_limit:
            if safe_limit:
                entries_sorted = entries_sorted[safe_offset : safe_offset + safe_limit]
            else:
                entries_sorted = entries_sorted[safe_offset:]
        parent: Optional[str] = None
        if normalized:
            parent_path = Path(normalized).parent.as_posix()
            parent = "" if parent_path in {".", ""} else parent_path
        tree_version = self.get_tree_version(user_id)
        return entries_sorted, tree_version, normalized, parent, total

    def search_workspace_entries(
        self,
        user_id: str,
        keyword: str,
        offset: int = 0,
        limit: int = 100,
        include_files: bool = True,
        include_dirs: bool = True,
    ) -> tuple[List[Dict[str, Any]], int]:
        """按名称搜索工作区条目，返回匹配结果与总数。"""
        keyword_lower = str(keyword or "").strip().lower()
        if not keyword_lower:
            return [], 0
        root = self.workspace_files_root(user_id)
        matched = 0
        results: List[Dict[str, Any]] = []
        safe_offset = max(int(offset or 0), 0)
        safe_limit = max(int(limit or 0), 0)
        for current, dirs, files in os.walk(root):
            current_path = Path(current)
            if include_dirs:
                for name in dirs:
                    if keyword_lower not in name.lower():
                        continue
                    matched += 1
                    if matched <= safe_offset:
                        continue
                    entry, _ = self._build_entry_payload(root, current_path / name)
                    results.append(entry)
                    if safe_limit and len(results) >= safe_limit:
                        # 继续统计总数，避免分页总量失真
                        continue
            if include_files:
                for name in files:
                    if keyword_lower not in name.lower():
                        continue
                    matched += 1
                    if matched <= safe_offset:
                        continue
                    entry, _ = self._build_entry_payload(root, current_path / name)
                    results.append(entry)
                    if safe_limit and len(results) >= safe_limit:
                        continue
        return results, matched

    @staticmethod
    def _build_entry_payload(root: Path, item: Path) -> tuple[Dict[str, Any], float]:
        """生成工作区条目元信息，返回条目与更新时间戳。"""
        entry_type = "dir" if item.is_dir() else "file"
        stat = item.stat()
        updated_ts = float(stat.st_mtime)
        entry = {
            "name": item.name,
            "path": item.relative_to(root).as_posix(),
            "type": entry_type,
            "size": 0 if entry_type == "dir" else int(stat.st_size),
            "updated_time": datetime.fromtimestamp(updated_ts).isoformat(),
        }
        return entry, updated_ts

    async def append_chat(self, user_id: str, payload: Dict[str, Any]) -> None:
        """追加一条对话记录到 SQLite。"""
        await asyncio.to_thread(self._storage.append_chat, user_id, payload)
        self._maybe_schedule_retention_cleanup()

    async def append_tool_log(self, user_id: str, payload: Dict[str, Any]) -> None:
        """追加一条工具调用记录到 SQLite。"""
        await asyncio.to_thread(self._storage.append_tool_log, user_id, payload)
        self._maybe_schedule_retention_cleanup()

    async def append_artifact_log(self, user_id: str, payload: Dict[str, Any]) -> None:
        """追加一条产物索引日志，方便后续生成结构化索引。"""
        await asyncio.to_thread(self._storage.append_artifact_log, user_id, payload)
        self._maybe_schedule_retention_cleanup()

    async def load_artifact_logs(
        self, user_id: str, session_id: str, limit: int
    ) -> List[Dict[str, Any]]:
        """读取指定会话的产物索引日志。"""
        return await asyncio.to_thread(
            self._storage.load_artifact_logs, user_id, session_id, limit
        )

    async def load_history(
        self, user_id: str, session_id: str, limit: int
    ) -> List[Dict[str, Any]]:
        """读取指定会话的历史记录。"""
        normalized_limit = self._normalize_history_limit(limit)
        return await asyncio.to_thread(
            self._storage.load_chat_history, user_id, session_id, normalized_limit
        )

    async def load_session_system_prompt(
        self, user_id: str, session_id: str, *, language: Optional[str] = None
    ) -> Optional[str]:
        """读取指定会话固定系统提示词。"""
        return await asyncio.to_thread(
            self._storage.get_session_system_prompt, user_id, session_id, language
        )

    async def load_session_token_usage(self, user_id: str, session_id: str) -> int:
        """读取会话累计 token 占用，用于压缩阈值判断。"""
        key = self._session_token_usage_key(user_id, session_id)
        raw = await asyncio.to_thread(self._storage.get_meta, key)
        try:
            return int(raw) if raw is not None else 0
        except (TypeError, ValueError):
            return 0

    async def save_session_token_usage(
        self, user_id: str, session_id: str, total_tokens: int
    ) -> None:
        """保存会话累计 token 占用，确保跨请求可追踪。"""
        key = self._session_token_usage_key(user_id, session_id)
        value = str(max(0, int(total_tokens)))
        await asyncio.to_thread(self._storage.set_meta, key, value)

    async def add_session_token_usage(
        self, user_id: str, session_id: str, delta_tokens: int
    ) -> int:
        """累加会话 token 占用并返回最新值。"""
        if not isinstance(delta_tokens, int):
            try:
                delta_tokens = int(delta_tokens)
            except (TypeError, ValueError):
                delta_tokens = 0
        delta_tokens = max(0, delta_tokens)
        if delta_tokens <= 0:
            return await self.load_session_token_usage(user_id, session_id)
        key = self._session_token_usage_key(user_id, session_id)
        return await asyncio.to_thread(self._storage.incr_meta, key, delta_tokens)

    async def save_session_system_prompt(
        self, user_id: str, session_id: str, prompt: str, *, language: Optional[str] = None
    ) -> None:
        """保存会话固定系统提示词，便于后续审查与恢复。"""
        content = str(prompt or "").strip()
        if not content:
            return
        payload = {
            "role": "system",
            "content": content,
            "session_id": session_id,
            "timestamp": datetime.utcnow().isoformat() + "Z",
            "meta": {
                "type": "system_prompt",
                "language": str(language or "").strip(),
            },
        }
        await self.append_chat(user_id, payload)

    def get_workspace_tree(self, user_id: str) -> str:
        """获取缓存的工作区树，不存在则生成并缓存。"""
        safe_id = self._safe_user_id(user_id)
        with self._tree_lock:
            cached = self._tree_cache.get(safe_id)
            dirty = self._tree_dirty.get(safe_id, False)
        if cached is not None and not dirty:
            return cached
        return self.refresh_workspace_tree(user_id)

    def mark_tree_dirty(self, user_id: str) -> None:
        """标记工作区目录树已变化，触发提示词缓存失效。"""
        safe_id = self._safe_user_id(user_id)
        with self._tree_lock:
            self._tree_dirty[safe_id] = True
            self._tree_versions[safe_id] = self._tree_versions.get(safe_id, 0) + 1
            self._tree_cache.pop(safe_id, None)

    def refresh_workspace_tree(self, user_id: str) -> str:
        """刷新工作区树缓存，仅在变化时递增版本号。"""
        root = self.workspace_files_root(user_id)
        tree = build_workspace_tree(root)
        safe_id = self._safe_user_id(user_id)
        with self._tree_lock:
            previous = self._tree_cache.get(safe_id)
            dirty = self._tree_dirty.pop(safe_id, False)
            if previous != tree or previous is None:
                self._tree_cache[safe_id] = tree
            if not dirty and previous != tree:
                self._tree_versions[safe_id] = self._tree_versions.get(safe_id, 0) + 1
            return self._tree_cache.get(safe_id, tree)

    def get_tree_version(self, user_id: str) -> int:
        """获取工作区树版本号，用于提示词缓存失效判断。"""
        safe_id = self._safe_user_id(user_id)
        with self._tree_lock:
            return self._tree_versions.get(safe_id, 0)

    def get_user_usage_stats(self) -> Dict[str, Dict[str, int]]:
        """汇总用户对话与工具调用数量，用于管理页展示。"""
        now = time.time()
        with self._user_usage_cache_lock:
            if (
                self._user_usage_cache_time > 0
                and now - self._user_usage_cache_time < self._user_usage_cache_ttl_s
            ):
                return {
                    user_id: dict(stats) for user_id, stats in self._user_usage_cache.items()
                }
        chat_stats = self._storage.get_user_chat_stats()
        tool_stats = self._storage.get_user_tool_stats()
        combined: Dict[str, Dict[str, int]] = {}
        for user_id, stats in chat_stats.items():
            combined[user_id] = {
                "chat_records": int(stats.get("chat_records", 0)),
                "tool_records": 0,
            }
        for user_id, stats in tool_stats.items():
            entry = combined.setdefault(
                user_id, {"chat_records": 0, "tool_records": 0}
            )
            entry["tool_records"] = int(stats.get("tool_records", 0))
        with self._user_usage_cache_lock:
            self._user_usage_cache = combined
            self._user_usage_cache_time = time.time()
        return combined

    def get_tool_usage_stats(
        self, since_time: Optional[float] = None, until_time: Optional[float] = None
    ) -> List[Dict[str, Any]]:
        """汇总工具调用次数，支持按时间窗口过滤。"""
        stats = self._storage.get_tool_usage_stats(since_time, until_time)
        return [{"tool": tool, "calls": count} for tool, count in stats.items()]

    def get_tool_session_usage(
        self,
        tool: str,
        since_time: Optional[float] = None,
        until_time: Optional[float] = None,
    ) -> List[Dict[str, Any]]:
        """按工具返回使用会话列表，便于监控页查看详情。"""
        return self._storage.get_tool_session_usage(tool, since_time, until_time)

    def purge_user_data(self, user_id: str) -> Dict[str, Any]:
        """清理用户历史记录与工作区文件，返回清理结果摘要。"""
        cleaned = user_id.strip()
        if not cleaned:
            return {
                "chat_records": 0,
                "tool_records": 0,
                "workspace_deleted": False,
                "legacy_history_deleted": False,
            }
        chat_deleted = self._storage.delete_chat_history(cleaned)
        tool_deleted = self._storage.delete_tool_logs(cleaned)
        # 清理长期记忆记录与开关配置，避免遗留脏数据
        try:
            self._storage.delete_memory_records_by_user(cleaned)
            self._storage.delete_memory_settings_by_user(cleaned)
        except Exception:
            pass
        # 清理产物索引，避免遗留占用存储空间
        try:
            self._storage.delete_artifact_logs(cleaned)
        except Exception:
            pass
        workspace_deleted = False
        legacy_deleted = False
        workspace_root = self.workspace_root(cleaned)
        if workspace_root.exists():
            try:
                shutil.rmtree(workspace_root)
                workspace_deleted = True
            except OSError:
                workspace_deleted = False
        legacy_root = self.history_root(cleaned)
        if legacy_root.exists():
            try:
                shutil.rmtree(legacy_root)
                legacy_deleted = True
            except OSError:
                legacy_deleted = False
        safe_id = self._safe_user_id(cleaned)
        # 清理工作区树缓存，避免已删除用户仍占用内存
        with self._tree_lock:
            self._tree_cache.pop(safe_id, None)
            self._tree_versions.pop(safe_id, None)
            self._tree_dirty.pop(safe_id, None)
        try:
            # 清理会话 token 使用统计，避免残留影响新会话
            self._storage.delete_meta_prefix(f"session_token_usage:{safe_id}:")
        except Exception:
            pass
        try:
            # 清理会话锁与溢出事件，避免遗留锁阻塞与存储膨胀
            self._storage.delete_session_locks_by_user(cleaned)
            self._storage.delete_stream_events_by_user(cleaned)
        except Exception:
            pass
        return {
            "chat_records": chat_deleted,
            "tool_records": tool_deleted,
            "workspace_deleted": workspace_deleted,
            "legacy_history_deleted": legacy_deleted,
        }
