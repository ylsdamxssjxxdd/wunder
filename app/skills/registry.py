from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Dict, List, Optional

from app.core.i18n import t

SkillCallable = Callable[[Dict[str, Any]], Awaitable[Dict[str, Any]]]


@dataclass
class SkillSpec:
    """技能描述信息。"""

    name: str
    description: str
    path: str
    input_schema: Dict[str, Any]
    frontmatter: str = ""


class SkillRegistry:
    """技能注册表，统一管理技能元数据与执行入口。"""

    def __init__(self) -> None:
        self._skills: Dict[str, SkillCallable] = {}
        self._specs: Dict[str, SkillSpec] = {}

    def register(self, spec: SkillSpec, func: Optional[SkillCallable] = None) -> None:
        """注册技能描述信息，可选绑定执行函数。"""
        self._specs[spec.name] = spec
        if func is not None:
            self._skills[spec.name] = func

    def list_specs(self) -> List[SkillSpec]:
        """列出技能描述信息。"""
        return list(self._specs.values())

    def get(self, name: str) -> SkillCallable:
        """获取指定技能的执行函数。"""
        if name not in self._skills:
            raise KeyError(t("error.skill_not_executable", name=name))
        return self._skills[name]
