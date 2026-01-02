"""字面知识库解析器：将 Markdown 拆分为章节级知识点。"""

from __future__ import annotations

import re
import textwrap
from dataclasses import dataclass
from pathlib import Path
from typing import List, Sequence

from app.core.i18n import get_known_prefixes, t

@dataclass(slots=True)
class KnowledgeSection:
    """知识库章节结构体，包含文档信息与章节正文。"""

    document: str
    section_path: Sequence[str]
    content: str
    document_content: str
    code: str = ""

    @property
    def identifier(self) -> str:
        """组合文档名与章节路径，作为知识点的唯一可读标识。"""
        parts = [self.document, *[item for item in self.section_path if item]]
        cleaned: list[str] = []
        for part in parts:
            name = part.strip()
            if not name:
                continue
            if name in _FULL_TEXT_LABELS:
                name = t("knowledge.section.full_text")
            if cleaned and name == cleaned[-1]:
                continue
            cleaned.append(name)
        # 若章节标题已包含文档名，避免重复展示。
        if len(cleaned) >= 2 and cleaned[1].startswith(cleaned[0]):
            cleaned = cleaned[1:]
        return " - ".join(cleaned) if cleaned else self.document

    @property
    def preview(self) -> str:
        """生成简短摘要，用于候选列表展示。"""
        plain = _strip_markdown(self.content)
        if not plain:
            return ""
        return textwrap.shorten(plain, width=80, placeholder="…")


H1_HEADING_PATTERN = re.compile(r"^#\s+(.+?)\s*$")
MARKDOWN_CLEAN_PATTERN = re.compile(r"[#>`*_`]+")
_FULL_TEXT_LABELS = set(get_known_prefixes("knowledge.section.full_text"))


def load_knowledge_sections(root: Path) -> List[KnowledgeSection]:
    """遍历目录下所有 Markdown，汇总知识点列表。"""
    sections: List[KnowledgeSection] = []
    for path in sorted(root.rglob("*.md")):
        sections.extend(parse_markdown_sections(path))
    for idx, section in enumerate(sections, start=1):
        section.code = f"K{idx:04d}"
    return sections


def parse_markdown_sections(path: Path) -> List[KnowledgeSection]:
    """将 Markdown 文档按一级标题切分知识点，并保留整篇文档内容。"""
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        text = path.read_text(encoding="gbk", errors="ignore")
    except OSError:
        return []
    text = text.replace("\ufeff", "")
    document_content = text.strip()
    current_h1 = None
    buffer: List[str] = []
    sections: List[KnowledgeSection] = []

    def flush() -> None:
        nonlocal buffer
        if not buffer or not current_h1:
            return
        content = "\n".join(buffer).strip()
        buffer.clear()
        if not content:
            return
        section_path = [current_h1]
        sections.append(
            KnowledgeSection(
                document=path.stem,
                section_path=section_path,
                content=content,
                document_content=document_content,
            )
        )

    for line in text.splitlines():
        heading = H1_HEADING_PATTERN.match(line.strip())
        if heading:
            # 仅按一级标题拆分知识点，避免碎片化。
            flush()
            current_h1 = heading.group(1).strip()
            buffer = []
            continue
        buffer.append(line)

    flush()
    if not sections and document_content:
        sections.append(
            KnowledgeSection(
                document=path.stem,
                section_path=["全文"],
                content=document_content,
                document_content=document_content,
            )
        )
    return sections


def _strip_markdown(content: str) -> str:
    """移除 Markdown 语法符号，便于生成纯文本预览。"""
    cleaned = MARKDOWN_CLEAN_PATTERN.sub("", content)
    cleaned = re.sub(r"\s+", " ", cleaned).strip()
    return cleaned
