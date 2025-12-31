import json
import time
from pathlib import Path
from collections import OrderedDict
from typing import Dict, List, Optional, Set

from app.tools.availability import collect_available_tool_names, collect_prompt_tool_specs
from app.orchestrator.context import RequestContext, UserToolBindings
from app.orchestrator.prompt_builder import build_system_prompt, read_prompt_template
from app.skills.registry import SkillSpec
from app.tools.registry import ToolSpec


class PromptComposer:
    """系统提示词构建与缓存管理。"""

    def __init__(self, cache_ttl_s: int = 10, cache_max_items: int = 128) -> None:
        self._cache: "OrderedDict[str, Dict[str, object]]" = OrderedDict()
        self._cache_ttl_s = cache_ttl_s
        self._cache_max_items = max(1, int(cache_max_items))
        self._config_version = 0

    def clear_cache(self) -> None:
        """清空系统提示词缓存。"""
        self._cache.clear()

    def set_config_version(self, version: int) -> None:
        """更新配置版本号，用于缓存键隔离。"""
        self._config_version = version

    @staticmethod
    def _normalize_tool_names(tool_names: Optional[List[str]]) -> List[str]:
        """清洗工具名称输入，去除空值与重复项。"""
        if not tool_names:
            return []
        normalized: List[str] = []
        seen: set[str] = set()
        for raw in tool_names:
            name = str(raw).strip()
            if not name or name in seen:
                continue
            normalized.append(name)
            seen.add(name)
        return normalized

    def resolve_allowed_tool_names(
        self,
        ctx: RequestContext,
        tool_names: Optional[List[str]],
        user_tool_bindings: Optional[UserToolBindings] = None,
    ) -> Set[str]:
        """?????????????????/?????"""
        selected = self._normalize_tool_names(tool_names)
        if not selected:
            return set()

        # ???????????????????????
        available = collect_available_tool_names(
            ctx.config, ctx.skills.list_specs(), user_tool_bindings
        )
        return {name for name in selected if name in available}

    @staticmethod
    def _filter_skill_specs(
        skills: List[SkillSpec], allowed_tool_names: Set[str]
    ) -> List[SkillSpec]:
        """筛选与工具选择匹配的技能列表。"""
        if not allowed_tool_names:
            return []
        return [spec for spec in skills if spec.name in allowed_tool_names]

    @staticmethod
    def _build_skill_prompt_block(workdir: Path, skills: List[SkillSpec]) -> str:
        """构建技能提示块，引导模型先读取 SKILL.md 再执行。"""
        if not skills:
            return ""
        lines: List[str] = []
        lines.append("[技能使用协议]")
        lines.append("1) 技能是可选流程手册，仅在任务匹配其 YAML 前置信息时使用。")
        lines.append("2) 使用下方列出的 SKILL.md 路径，先用`读取文件`阅读后再使用技能。")
        lines.append("3) 严格遵循技能流程，不要编造缺失步骤。优先使用随附脚本/模板/资产。")
        lines.append("4) 特别注意，随附的脚本/模板/资产默认与 SKILL.md 位于同一目录下。")
        lines.append("5) 只有在实际执行了技能步骤后，才可以声明已使用该技能。")
        lines.append(
            "6) 使用 `执行命令` 运行相关技能脚本，"
            f"并将输出写回工程师工作区（{workdir}）。"
        )
        lines.append("")
        lines.append("[已挂载技能]")
        for spec in sorted(skills, key=lambda item: item.name.lower()):
            lines.append("")
            lines.append(f"- {spec.name}")
            lines.append(f"  SKILL.md: {spec.path}")
            if spec.frontmatter:
                lines.append("  Frontmatter:")
                for raw_line in spec.frontmatter.splitlines():
                    line = raw_line.strip()
                    lines.append(f"    {line}")
        return "\n".join(lines)

    def _build_system_prompt(
        self,
        workdir: Path,
        tools: List[ToolSpec],
        skills: List[SkillSpec],
        workspace_tree: str | None = None,
        include_tools_protocol: bool = True,
    ) -> str:
        """构建系统提示词，注入工具协议、环境信息与技能提示块。"""
        base_path = Path(__file__).resolve().parent.parent / "prompts" / "system.txt"
        # 读取基础模板时使用缓存，避免频繁磁盘 IO 影响提示词构建速度
        base_prompt = read_prompt_template(base_path)
        prompt = build_system_prompt(
            base_prompt,
            tools,
            workdir,
            workspace_tree,
            include_tools_protocol=include_tools_protocol,
        )
        skill_block = self._build_skill_prompt_block(workdir, skills)
        if skill_block:
            return prompt.rstrip() + "\n\n" + skill_block.strip()
        return prompt

    def _build_prompt_cache_key(
        self,
        user_id: str,
        overrides: Optional[Dict[str, object]],
        workdir: Path,
        workspace_version: int,
        tool_key: str,
        user_tool_version: float,
        shared_tool_version: float,
    ) -> str:
        """生成系统提示词缓存键，避免重复构建。"""
        if overrides:
            try:
                overrides_key = json.dumps(overrides, sort_keys=True, ensure_ascii=False)
            except TypeError:
                overrides_key = str(overrides)
        else:
            overrides_key = ""
        return (
            f"{user_id}|{self._config_version}|{workspace_version}|{workdir}|"
            f"{overrides_key}|{tool_key}|{user_tool_version}|{shared_tool_version}"
        )

    async def build_system_prompt_cached(
        self,
        ctx: RequestContext,
        workdir: Path,
        user_id: str,
        overrides: Optional[Dict[str, object]],
        allowed_tool_names: Set[str],
        user_tool_bindings: Optional[UserToolBindings] = None,
    ) -> str:
        """带缓存地构建系统提示词，降低重复计算开销。"""
        tool_key = ",".join(sorted(allowed_tool_names))
        workspace_version = ctx.workspace_manager.get_tree_version(user_id)
        user_tool_version = user_tool_bindings.user_version if user_tool_bindings else 0.0
        shared_tool_version = user_tool_bindings.shared_version if user_tool_bindings else 0.0
        cache_key = self._build_prompt_cache_key(
            user_id,
            overrides,
            workdir,
            workspace_version,
            tool_key,
            user_tool_version,
            shared_tool_version,
        )
        cached = self._cache.get(cache_key)
        if cached and time.time() - cached.get("timestamp", 0) < self._cache_ttl_s:
            self._cache.move_to_end(cache_key)
            return cached.get("prompt", "")

        base_skill_specs = ctx.skills.list_specs()
        tools_for_prompt = await self._collect_prompt_tools(
            ctx, base_skill_specs, allowed_tool_names, user_tool_bindings
        )
        skills_for_prompt = self._filter_skill_specs(base_skill_specs, allowed_tool_names)
        if user_tool_bindings and user_tool_bindings.skill_specs:
            user_skill_specs = self._filter_skill_specs(
                user_tool_bindings.skill_specs, allowed_tool_names
            )
            if user_skill_specs:
                merged: List[SkillSpec] = []
                seen_names: set[str] = set()
                for spec in skills_for_prompt + user_skill_specs:
                    if spec.name in seen_names:
                        continue
                    seen_names.add(spec.name)
                    merged.append(spec)
                skills_for_prompt = merged
        include_tools_protocol = bool(allowed_tool_names)
        workspace_tree = ctx.workspace_manager.get_workspace_tree(user_id)
        workspace_version = ctx.workspace_manager.get_tree_version(user_id)
        cache_key = self._build_prompt_cache_key(
            user_id,
            overrides,
            workdir,
            workspace_version,
            tool_key,
            user_tool_version,
            shared_tool_version,
        )
        prompt = self._build_system_prompt(
            workdir,
            tools_for_prompt,
            skills_for_prompt,
            workspace_tree,
            include_tools_protocol=include_tools_protocol,
        )
        extra_prompt = user_tool_bindings.extra_prompt.strip() if user_tool_bindings else ""
        if extra_prompt:
            prompt = prompt.rstrip() + "\n\n" + extra_prompt
        self._cache[cache_key] = {"prompt": prompt, "timestamp": time.time()}
        self._cache.move_to_end(cache_key)
        while len(self._cache) > self._cache_max_items:
            self._cache.popitem(last=False)
        return prompt

    async def _collect_prompt_tools(
        self,
        ctx: RequestContext,
        skill_specs: List[SkillSpec],
        allowed_tool_names: Set[str],
        user_tool_bindings: Optional[UserToolBindings] = None,
    ) -> List[ToolSpec]:
        """按提示词注入顺序收集可用工具规格。"""
        # 复用已读取的技能列表，避免重复读取技能注册表
        return collect_prompt_tool_specs(
            ctx.config,
            skill_specs,
            allowed_tool_names,
            user_tool_bindings,
        )
