import platform
import threading
from datetime import datetime
from pathlib import Path
from typing import List

from app.core.i18n import get_language, t
from app.memory.workspace import build_workspace_tree
from app.tools.registry import ToolSpec


_PROMPT_FILE_CACHE: dict[str, tuple[float, str]] = {}
_PROMPT_FILE_LOCK = threading.Lock()


def _resolve_prompt_path(path: Path) -> Path:
    """按语言解析提示词模板路径，优先使用对应语言版本。"""
    language = get_language()
    if language.startswith("en"):
        candidate = path.parent / "en" / path.name
        if candidate.exists():
            return candidate
    return path


def read_prompt_template(path: Path) -> str:
    """读取提示词模板文件，基于文件更新时间缓存内容以减少磁盘 IO。"""
    # 模板文件会在每次构建提示词时读取，使用 mtime 缓存可以显著降低重复磁盘访问开销。
    resolved = _resolve_prompt_path(path)
    try:
        mtime = resolved.stat().st_mtime
    except OSError:
        return resolved.read_text(encoding="utf-8")
    cache_key = str(resolved)
    with _PROMPT_FILE_LOCK:
        cached = _PROMPT_FILE_CACHE.get(cache_key)
        if cached and cached[0] == mtime:
            return cached[1]
    text = resolved.read_text(encoding="utf-8")
    with _PROMPT_FILE_LOCK:
        _PROMPT_FILE_CACHE[cache_key] = (mtime, text)
    return text


def _render_template(template: str, mapping: dict) -> str:
    """使用占位符替换渲染模板，避免 format 处理 JSON 大括号。"""
    rendered = template
    for key, value in mapping.items():
        rendered = rendered.replace("{" + key + "}", str(value))
    return rendered


def _read_prompt_file(path: Path) -> str:
    """读取提示词模板文件。"""
    return read_prompt_template(path)


def _build_engineer_system_info(workdir: Path, workspace_tree: str | None = None) -> str:
    """构建工程师环境信息块。"""
    os_name = f"{platform.system()} {platform.release()}".strip()
    date_str = datetime.now().strftime("%Y-%m-%d")
    workspace_tree = workspace_tree if workspace_tree is not None else build_workspace_tree(workdir)

    template_path = Path(__file__).resolve().parent.parent / "prompts" / "engineer_system_info.txt"
    template = _read_prompt_file(template_path)
    return _render_template(
        template,
        {
            "OS": os_name,
            "DATE": date_str,
            "DIR": str(workdir),
            "WORKSPACE_TREE": workspace_tree,
        },
    )


def _build_engineer_info(
    workdir: Path, workspace_tree: str | None = None, include_ptc: bool = False
) -> str:
    """构建工程师信息块。"""
    template_path = Path(__file__).resolve().parent.parent / "prompts" / "engineer_info.txt"
    template = _read_prompt_file(template_path)
    ptc_guidance = ""
    if include_ptc:
        ptc_guidance = t("prompt.engineer.ptc_guidance")
    return _render_template(
        template,
        {
            "engineer_system_info": _build_engineer_system_info(workdir, workspace_tree),
            "PTC_GUIDANCE": ptc_guidance,
        },
    )


def build_system_prompt(
    base_prompt: str,
    tools: List[ToolSpec],
    workdir: Path,
    workspace_tree: str | None = None,
    include_tools_protocol: bool = True,
) -> str:
    """根据 EVA 风格模板组合系统提示词。"""
    # 未启用工具协议时仅注入工程师信息，避免提示词暴露工具细节
    if not include_tools_protocol:
        engineer_info = _build_engineer_info(workdir, workspace_tree, include_ptc=False)
        return base_prompt.strip() + "\n\n" + engineer_info.strip()

    tools_text = "\n".join([spec.to_prompt_text() for spec in tools])
    include_ptc = any(spec.name == "ptc" for spec in tools)
    extra_path = Path(__file__).resolve().parent.parent / "prompts" / "extra_prompt_template.txt"
    extra_template = _read_prompt_file(extra_path)
    extra_prompt = _render_template(
        extra_template,
        {
            "available_tools_describe": tools_text,
            "engineer_info": _build_engineer_info(
                workdir, workspace_tree, include_ptc=include_ptc
            ),
        },
    )
    return base_prompt.strip() + "\n\n" + extra_prompt.strip()
