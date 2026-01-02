"""字面知识库服务：负责缓存、检索与工具化输出。"""

from __future__ import annotations

import asyncio
import json
import logging
import re
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Sequence

from app.core.config import KnowledgeBaseConfig, LLMConfig, WunderConfig
from app.core.i18n import get_known_prefixes, get_language, t
from app.knowledge.parser import KnowledgeSection, load_knowledge_sections
from app.llm.base import LLMUnavailableError
from app.llm.factory import build_llm_client
from app.tools.registry import ToolSpec

logger = logging.getLogger(__name__)

_KNOWLEDGE_BLOCK_PATTERN = re.compile(r"<knowledge>(.*?)</knowledge>", re.S)
DEFAULT_MAX_DOCUMENTS = 5
DEFAULT_CANDIDATE_LIMIT = 80
_FULL_TEXT_LABELS = set(get_known_prefixes("knowledge.section.full_text"))
_SYSTEM_PROMPT_TEMPLATES = {
    "zh-CN": (
        "你是一名字面知识库检索助手，需要根据用户提问在给定的知识点列表中挑选最相关的内容。\n"
        "请严格按照以下要求输出：\n"
        "1. 每个知识点都对应唯一编号（如 K0001），请优先依据编号定位章节。\n"
        "2. 最多返回{limit}个知识点，可根据相关度筛选；即便相关度较低，也尽量返回 2-3 条最接近的知识点，避免空列表。\n"
        "3. 输出必须使用 <knowledge></knowledge> 包裹 JSON，字段为 documents(List)。\n"
        "4. documents 中的每个对象需包含 code、name、score(0~1) 与 reason(简述命中原因)。\n"
        "5. 未命中时也要输出空数组，切勿输出 JSON 之外的多余文字。"
    ),
    "en-US": (
        "You are a knowledge-base retrieval assistant. Select the most relevant items from the list based on the user query.\n"
        "Follow these requirements strictly:\n"
        "1. Each knowledge item has a unique code (e.g., K0001). Prefer matching by code.\n"
        "2. Return at most {limit} items. Even if relevance is low, try to return 2-3 closest items to avoid an empty list.\n"
        "3. Output must be JSON wrapped by <knowledge></knowledge>, with field documents (List).\n"
        "4. Each document item must include code, name, score(0~1), and reason (brief).\n"
        "5. If nothing matches, return an empty array only, and do not output extra text outside JSON."
    ),
}

_QUESTION_TEMPLATES = {
    "zh-CN": {
        "base_name": "【知识库名称】",
        "user_query": "【用户提问】",
        "candidates": "【候选知识点列表】",
        "empty": "- （暂无知识点）",
        "footer": "请按要求返回 JSON 结果。",
    },
    "en-US": {
        "base_name": "[Knowledge Base]",
        "user_query": "[User Question]",
        "candidates": "[Candidate Knowledge Items]",
        "empty": "- (No items available)",
        "footer": "Return the JSON result as required.",
    },
}


class KnowledgeQueryError(RuntimeError):
    """知识库检索失败时抛出的统一异常。"""


@dataclass(slots=True)
class KnowledgeDocument:
    """知识库检索返回的结构化知识点。"""

    code: str
    name: str
    content: str
    document: str
    section_path: List[str]
    score: Optional[float] = None
    reason: Optional[str] = None

    def to_dict(self) -> Dict[str, Any]:
        """转换为工具返回的 JSON 结构。"""
        normalized_section_path = [
            t("knowledge.section.full_text") if part in _FULL_TEXT_LABELS else part
            for part in self.section_path
        ]
        payload: Dict[str, Any] = {
            "code": self.code,
            "name": self.name,
            "content": self.content,
            "document": self.document,
            "section_path": normalized_section_path,
        }
        if self.score is not None:
            payload["score"] = self.score
        if self.reason:
            payload["reason"] = self.reason
        return payload


@dataclass(slots=True)
class _KnowledgeCache:
    """单个知识库的缓存快照。"""

    root: str
    sections: List[KnowledgeSection]
    updated_at: float


class KnowledgeStore:
    """知识库缓存管理器，避免重复解析 Markdown。"""

    def __init__(self) -> None:
        self._cache: Dict[str, _KnowledgeCache] = {}
        self._locks: Dict[str, asyncio.Lock] = {}

    @staticmethod
    def _build_cache_key(base_name: str, root_path: str) -> str:
        """使用路径区分缓存，避免同名知识库互相覆盖。"""
        return f"{base_name}::{root_path}"

    async def get_sections(
        self, base: KnowledgeBaseConfig, *, refresh: bool = False
    ) -> List[KnowledgeSection]:
        """获取知识库章节缓存，必要时重新解析。"""
        base_name = base.name.strip()
        if not base_name:
            return []
        root_path = str(base.root or "").strip()
        if not root_path:
            return []
        cache_key = self._build_cache_key(base_name, root_path)
        if not refresh:
            cached = self._cache.get(cache_key)
            if cached and cached.root == root_path:
                return cached.sections
        lock = self._locks.setdefault(cache_key, asyncio.Lock())
        async with lock:
            if not refresh:
                cached = self._cache.get(cache_key)
                if cached and cached.root == root_path:
                    return cached.sections
            sections = await self._load_sections(root_path)
            self._cache[cache_key] = _KnowledgeCache(
                root=root_path, sections=sections, updated_at=time.time()
            )
            return sections

    async def refresh(self, base: KnowledgeBaseConfig) -> List[KnowledgeSection]:
        """强制刷新指定知识库缓存。"""
        return await self.get_sections(base, refresh=True)

    async def _load_sections(self, root: str) -> List[KnowledgeSection]:
        """解析知识库根目录下的 Markdown 文件。"""
        root_path = Path(root).resolve()
        if not root_path.exists() or not root_path.is_dir():
            logger.warning("知识库目录不存在：%s", root_path)
            return []
        return await asyncio.to_thread(load_knowledge_sections, root_path)


_STORE = KnowledgeStore()


def list_enabled_bases(config: WunderConfig) -> List[KnowledgeBaseConfig]:
    """返回可用的知识库配置列表。"""
    bases = []
    for base in config.knowledge.bases:
        if getattr(base, "enabled", True) is False:
            continue
        if not base.name.strip() or not str(base.root or "").strip():
            continue
        bases.append(base)
    return bases


def build_knowledge_tool_specs(
    config: WunderConfig, blocked_names: Optional[set[str]] = None
) -> List[ToolSpec]:
    """构建知识库工具的规格列表，供提示词与工具展示使用。"""
    specs: List[ToolSpec] = []
    seen: set[str] = set()
    blocked = blocked_names or set()
    for base in list_enabled_bases(config):
        if base.name in blocked:
            continue
        if base.name in seen:
            continue
        seen.add(base.name)
        description = base.description.strip() or t(
            "knowledge.tool.description", name=base.name
        )
        specs.append(
            ToolSpec(
                name=base.name,
                description=description,
                args_schema={
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
                },
            )
        )
    return specs


def get_knowledge_tool_names(
    config: WunderConfig, blocked_names: Optional[set[str]] = None
) -> set[str]:
    """获取知识库工具名称集合，便于过滤可用工具。"""
    names = {base.name for base in list_enabled_bases(config)}
    if blocked_names:
        names -= blocked_names
    return names


async def query_knowledge_documents(
    query: str,
    base: KnowledgeBaseConfig,
    llm_config: LLMConfig,
    limit: Optional[int] = None,
    request_logger: Optional[Callable[[Dict[str, Any]], None]] = None,
) -> List[KnowledgeDocument]:
    """执行知识库检索并返回结构化知识点。"""
    normalized_query = (query or "").strip()
    if not normalized_query:
        return []

    sections = await _STORE.get_sections(base)
    if not sections:
        return []

    max_docs = _resolve_positive_int(limit, DEFAULT_MAX_DOCUMENTS)
    candidate_limit = DEFAULT_CANDIDATE_LIMIT
    candidates = _select_candidate_sections(sections, normalized_query, candidate_limit)
    prompt = _build_system_prompt(max_docs)
    question = _build_question(base.name, normalized_query, candidates)
    messages = [
        {"role": "system", "content": prompt},
        {"role": "user", "content": question},
    ]
    if request_logger:
        # 记录知识库工具的模型请求体，便于调试请求日志展示
        request_logger(
            {
                "knowledge_base": base.name,
                "query": normalized_query,
                "limit": max_docs,
                "candidate_count": len(candidates),
                "payload": _build_llm_payload(messages, llm_config),
                "base_url": llm_config.base_url,
            }
        )

    try:
        llm_client = build_llm_client(llm_config)
        result = await llm_client.complete(messages)
        if isinstance(result, str):
            reply = result
        else:
            reply = str(getattr(result, "content", "") or "")
    except LLMUnavailableError as exc:
        logger.warning("知识库检索模型不可用，已回退词面匹配: %s", exc)
        return _fallback_documents(candidates, max_docs)
    except Exception as exc:  # noqa: BLE001
        logger.warning("知识库检索请求失败，已回退词面匹配: %s", exc)
        return _fallback_documents(candidates, max_docs)

    structured = _extract_structured_documents(reply)
    documents = _materialize_documents(structured, sections, max_docs)
    if documents:
        return documents
    # 解析失败时回退到词面匹配结果，保证工具可用性。
    fallback = _fallback_documents(candidates, max_docs)
    return fallback


async def refresh_knowledge_cache(base: KnowledgeBaseConfig) -> List[KnowledgeSection]:
    """触发知识库刷新，用于保存/删除后更新缓存。"""
    return await _STORE.refresh(base)


def _resolve_positive_int(value: Optional[int], default: int) -> int:
    """解析正整数，确保落在可用范围内。"""
    if value is None:
        return max(1, int(default))
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return max(1, int(default))
    if parsed <= 0:
        return max(1, int(default))
    return parsed


def _build_system_prompt(limit: int) -> str:
    """构建知识库检索专用的系统提示词。"""
    language = get_language()
    template = _SYSTEM_PROMPT_TEMPLATES.get(language) or _SYSTEM_PROMPT_TEMPLATES["zh-CN"]
    return template.format(limit=limit)


def _build_llm_payload(
    messages: List[Dict[str, Any]], llm_config: LLMConfig
) -> Dict[str, Any]:
    """构建知识库检索的模型请求体，便于调试记录。"""
    payload: Dict[str, Any] = {
        "model": llm_config.model,
        "messages": messages,
        "stream": False,
    }
    if llm_config.temperature is not None:
        payload["temperature"] = llm_config.temperature
    if llm_config.max_output is not None and llm_config.max_output > 0:
        payload["max_tokens"] = llm_config.max_output
    return payload


def _build_question(
    base_name: str, query: str, candidates: Sequence[KnowledgeSection]
) -> str:
    """构建知识库检索请求的问题描述。"""
    language = get_language()
    labels = _QUESTION_TEMPLATES.get(language) or _QUESTION_TEMPLATES["zh-CN"]
    listing_lines: List[str] = []
    for section in candidates:
        preview = section.preview
        if preview:
            listing_lines.append(f"- [{section.code}] {section.identifier} | {preview}")
        else:
            listing_lines.append(f"- [{section.code}] {section.identifier}")
    listing = "\n".join(listing_lines) if listing_lines else labels["empty"]
    return (
        f"{labels['base_name']}\n{base_name}\n\n"
        f"{labels['user_query']}\n{query}\n\n"
        f"{labels['candidates']}\n"
        f"{listing}\n\n"
        f"{labels['footer']}"
    )


def _extract_structured_documents(reply: str) -> List[Dict[str, Any]]:
    """从模型输出中解析 <knowledge> JSON。"""
    if not reply:
        return []
    match = _KNOWLEDGE_BLOCK_PATTERN.search(reply)
    if not match:
        logger.warning("知识库 AI 回复缺少 <knowledge> 标签，已回退词面检索。")
        return []
    block = match.group(1).strip()
    try:
        payload = json.loads(block)
    except json.JSONDecodeError:
        logger.warning("知识库 JSON 解析失败，已回退词面检索。")
        return []
    documents = payload.get("documents")
    if not isinstance(documents, list):
        return []
    return [doc for doc in documents if isinstance(doc, dict)]


def _materialize_documents(
    structured_docs: List[Dict[str, Any]],
    sections: Sequence[KnowledgeSection],
    limit: int,
) -> List[KnowledgeDocument]:
    """将模型命中的知识点映射到真实章节内容。"""
    if not structured_docs:
        return []
    candidate_map: Dict[str, KnowledgeSection] = {section.identifier: section for section in sections}
    candidate_code_map: Dict[str, KnowledgeSection] = {
        section.code: section for section in sections if section.code
    }
    resolved: List[KnowledgeDocument] = []
    for item in structured_docs:
        if len(resolved) >= limit:
            break
        code = str(item.get("code") or "").strip().upper()
        name = str(item.get("name") or "").strip()
        section = None
        if code:
            section = candidate_code_map.get(code)
        if not section and name:
            section = _resolve_section(name, candidate_map)
        if not section:
            continue
        score = _safe_float(item.get("score"))
        reason = str(item.get("reason") or "").strip() or None
        resolved.append(
            KnowledgeDocument(
                code=section.code or "",
                name=section.identifier,
                content=section.content,
                document=section.document,
                section_path=[part for part in section.section_path if part],
                score=score,
                reason=reason,
            )
        )
    return resolved


def _fallback_documents(
    candidates: Sequence[KnowledgeSection], limit: int
) -> List[KnowledgeDocument]:
    """当模型解析失败时，回退到词面匹配结果。"""
    fallback: List[KnowledgeDocument] = []
    for section in candidates[:limit]:
        fallback.append(
            KnowledgeDocument(
                code=section.code or "",
                name=section.identifier,
                content=section.content,
                document=section.document,
                section_path=[part for part in section.section_path if part],
                score=None,
                reason=t("knowledge.fallback_reason"),
            )
        )
    return fallback


def _resolve_section(name: str, candidates: Dict[str, KnowledgeSection]) -> KnowledgeSection | None:
    """根据名称匹配知识点，兼容末尾对齐。"""
    if name in candidates:
        return candidates[name]
    matches = [section for key, section in candidates.items() if key.endswith(name)]
    if len(matches) == 1:
        return matches[0]
    return None


def _safe_float(value: Any) -> Optional[float]:
    """尝试将评分转换为浮点数。"""
    if value is None:
        return None
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return None
    if parsed < 0:
        return 0.0
    if parsed > 1:
        return 1.0
    return parsed


def _select_candidate_sections(
    sections: Sequence[KnowledgeSection], query: str, limit: int
) -> List[KnowledgeSection]:
    """基于词面匹配筛选候选知识点，减少模型输入长度。"""
    if not sections:
        return []
    normalized_query = query.lower()
    tokens = _extract_tokens(query)
    if not tokens:
        return list(sections)[:limit]
    scored: List[tuple[int, KnowledgeSection]] = []
    for section in sections:
        score = _score_section(section, normalized_query, tokens)
        if score > 0:
            scored.append((score, section))
    if not scored:
        return list(sections)[:limit]
    scored.sort(key=lambda item: (-item[0], item[1].code))
    return [section for _, section in scored[:limit]]


def _extract_tokens(query: str) -> List[str]:
    """拆分关键词，兼容英文与中文字符。"""
    lowered = query.lower()
    ascii_tokens = re.findall(r"[a-z0-9]+", lowered)
    chinese_tokens = [ch for ch in query if "\u4e00" <= ch <= "\u9fff"]
    tokens: List[str] = []
    for token in ascii_tokens:
        if len(token) >= 2:
            tokens.append(token)
    tokens.extend(chinese_tokens)
    # 去重并限制数量，避免过长输入。
    deduped: List[str] = []
    seen = set()
    for token in tokens:
        if token in seen:
            continue
        seen.add(token)
        deduped.append(token)
        if len(deduped) >= 24:
            break
    return deduped


def _score_section(
    section: KnowledgeSection, normalized_query: str, tokens: Sequence[str]
) -> int:
    """计算章节与查询的词面匹配得分。"""
    text = f"{section.identifier}\n{section.content}".lower()
    score = 0
    if normalized_query and normalized_query in text:
        score += 4
    for token in tokens:
        if token and token.lower() in text:
            score += 1
    return score
