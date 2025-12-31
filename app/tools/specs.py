"""工具规格统一出口，保证提示词与运行期定义一致。"""

from functools import lru_cache
from typing import Dict

from app.tools.catalog import build_builtin_tool_specs
from app.tools.registry import ToolSpec


@lru_cache(maxsize=1)
def build_eva_tool_specs() -> Dict[str, ToolSpec]:
    """构建 EVA 风格工具规格，直接复用内置工具目录生成结果。"""
    # 内置工具规格为静态数据，缓存后可减少重复构建与对象创建开销
    return build_builtin_tool_specs()
