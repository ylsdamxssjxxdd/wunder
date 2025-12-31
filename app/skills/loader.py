import asyncio
import importlib.util
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import yaml

from app.core.config import WunderConfig
from app.skills.registry import SkillRegistry, SkillSpec


_SKILL_FILE_NAME = "SKILL.md"
_ENTRY_FILES = ("run.py", "skill.py", "main.py")


def _load_module_from_path(path: Path):
    """从指定路径动态加载技能入口模块。"""
    spec = importlib.util.spec_from_file_location(path.stem, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"无法加载技能模块: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _normalize_newlines(text: str) -> str:
    """统一换行符格式，便于解析 frontmatter。"""
    return text.replace("\r\n", "\n").replace("\r", "\n")


def _parse_frontmatter(text: str) -> Optional[Tuple[Dict[str, Any], str]]:
    """解析 SKILL.md 的 YAML frontmatter，返回元数据与原始 frontmatter 内容。"""
    normalized = _normalize_newlines(text).lstrip("\ufeff")
    lines = normalized.split("\n")
    if not lines or lines[0].strip() != "---":
        return None
    end_index = None
    for idx in range(1, len(lines)):
        if lines[idx].strip() == "---":
            end_index = idx
            break
    if end_index is None:
        return None
    body = "\n".join(lines[1:end_index])
    try:
        meta = yaml.safe_load(body) or {}
    except Exception:
        return None
    if not isinstance(meta, dict):
        meta = {}
    return meta, body


def _discover_skill_dirs(base: Path) -> List[Path]:
    """发现包含 SKILL.md 的技能目录，兼容直接传入技能目录或根目录。"""
    if not base.exists() or base.is_file():
        return []
    if (base / _SKILL_FILE_NAME).exists():
        return [base]
    candidates: List[Path] = []
    for child in base.iterdir():
        if child.is_dir() and (child / _SKILL_FILE_NAME).exists():
            candidates.append(child)
    return candidates


def _find_entrypoint(skill_dir: Path) -> Optional[Path]:
    """查找技能入口文件，避免误加载 scripts 下的独立脚本。"""
    for filename in _ENTRY_FILES:
        candidate = skill_dir / filename
        if candidate.exists() and candidate.is_file():
            return candidate
    return None


def _build_input_schema(meta: Dict[str, Any]) -> Dict[str, Any]:
    """从 frontmatter 中提取输入结构，保持与工具协议兼容。"""
    schema = (
        meta.get("input_schema")
        or meta.get("args_schema")
        or meta.get("输入结构")
        or meta.get("参数结构")
        or {
            "type": "object",
            "properties": {},
        }
    )
    if not isinstance(schema, dict):
        return {"type": "object", "properties": {}}
    return schema


def load_skills(
    config: WunderConfig, *, load_entrypoints: bool = True, only_enabled: bool = True
) -> SkillRegistry:
    """加载技能目录中的 SKILL.md 元信息，并按需绑定执行入口。"""
    registry = SkillRegistry()
    enabled = set(config.skills.enabled)
    if only_enabled and not enabled:
        return registry
    seen_dirs: set[str] = set()
    remaining = set(enabled)

    for raw_path in config.skills.paths:
        if only_enabled and not remaining:
            break
        base = Path(raw_path).resolve()
        for skill_dir in _discover_skill_dirs(base):
            skill_dir_key = str(skill_dir.resolve())
            if skill_dir_key in seen_dirs:
                continue
            seen_dirs.add(skill_dir_key)
            skill_file = skill_dir / _SKILL_FILE_NAME
            try:
                content = skill_file.read_text(encoding="utf-8")
            except Exception:
                continue

            parsed = _parse_frontmatter(content)
            if not parsed:
                continue
            meta, frontmatter_body = parsed

            name = str(
                meta.get("name")
                or meta.get("名称")
                or meta.get("技能名称")
                or ""
            ).strip()
            if not name:
                continue
            if only_enabled and name not in enabled:
                continue

            description = str(
                meta.get("description")
                or meta.get("描述")
                or meta.get("技能描述")
                or ""
            ).strip() or "未提供描述"
            input_schema = _build_input_schema(meta)
            spec = SkillSpec(
                name=name,
                description=description,
                path=str(skill_file),
                input_schema=input_schema,
                frontmatter=frontmatter_body,
            )
            registry.register(spec)
            if only_enabled and name in remaining:
                remaining.discard(name)
                if not remaining:
                    break

            if not load_entrypoints:
                continue

            entrypoint = _find_entrypoint(skill_dir)
            if not entrypoint:
                continue
            try:
                module = _load_module_from_path(entrypoint)
            except Exception:
                continue
            run_func = getattr(module, "run", None)
            if run_func is None:
                continue

            async def _async_run(payload: Dict[str, Any], func=run_func):
                """将同步技能包装为异步执行，避免阻塞事件循环。"""
                if asyncio.iscoroutinefunction(func):
                    return await func(payload)
                return await asyncio.to_thread(func, payload)

            registry.register(spec, _async_run)

    return registry
