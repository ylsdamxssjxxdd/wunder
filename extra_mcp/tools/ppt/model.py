from __future__ import annotations

import json
import re
import xml.etree.ElementTree as ET
from copy import deepcopy
from dataclasses import dataclass, field
from typing import Any, Iterable
from uuid import uuid4

SUPPORTED_LANGS = {"xml", "json"}
SLIDE_ID_PREFIX = "slide_"


@dataclass
class SlideSpec:
    slide_id: str
    prompt: str
    slide_type: str = "content"
    title: str = ""
    subtitle: str = ""
    body: str = ""
    bullets: list[str] = field(default_factory=list)
    sections: list[dict[str, str]] = field(default_factory=list)
    items: list[dict[str, Any]] = field(default_factory=list)
    metrics: list[dict[str, Any]] = field(default_factory=list)
    images: list[dict[str, str]] = field(default_factory=list)
    template_id: str = ""
    template_slide_id: str = ""
    layout: str = ""

    def to_dict(self) -> dict[str, Any]:
        return {
            "slide_id": self.slide_id,
            "prompt": self.prompt,
            "type": self.slide_type,
            "title": self.title,
            "subtitle": self.subtitle,
            "body": self.body,
            "bullets": list(self.bullets),
            "sections": deepcopy(self.sections),
            "items": deepcopy(self.items),
            "metrics": deepcopy(self.metrics),
            "images": deepcopy(self.images),
            "template_id": self.template_id,
            "template_slide_id": self.template_slide_id,
            "layout": self.layout,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "SlideSpec":
        return cls(
            slide_id=str(data.get("slide_id") or ""),
            prompt=str(data.get("prompt") or ""),
            slide_type=str(data.get("type") or data.get("slide_type") or "content"),
            title=str(data.get("title") or ""),
            subtitle=str(data.get("subtitle") or ""),
            body=str(data.get("body") or ""),
            bullets=_string_list(data.get("bullets")),
            sections=_dict_list(data.get("sections")),
            items=_dict_list(data.get("items")),
            metrics=_dict_list(data.get("metrics")),
            images=_dict_list(data.get("images")),
            template_id=str(data.get("template_id") or ""),
            template_slide_id=str(data.get("template_slide_id") or ""),
            layout=str(data.get("layout") or ""),
        )


@dataclass
class PresentationManifest:
    presentation_id: str
    presentation_name: str
    slides: list[SlideSpec]
    output_path: str = ""
    public_path: str = ""
    workspace_relative_path: str = ""
    theme: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "presentation_id": self.presentation_id,
            "presentation_name": self.presentation_name,
            "output_path": self.output_path,
            "public_path": self.public_path,
            "workspace_relative_path": self.workspace_relative_path,
            "theme": deepcopy(self.theme),
            "slides": [slide.to_dict() for slide in self.slides],
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> "PresentationManifest":
        slides_raw = data.get("slides") if isinstance(data.get("slides"), list) else []
        return cls(
            presentation_id=str(data.get("presentation_id") or ""),
            presentation_name=str(data.get("presentation_name") or ""),
            output_path=str(data.get("output_path") or ""),
            public_path=str(data.get("public_path") or ""),
            workspace_relative_path=str(data.get("workspace_relative_path") or ""),
            theme=data.get("theme") if isinstance(data.get("theme"), dict) else {},
            slides=[SlideSpec.from_dict(item) for item in slides_raw if isinstance(item, dict)],
        )


def make_presentation_id() -> str:
    return f"ppt_{uuid4().hex[:12]}"


def parse_slides_payload(content: str, lang: str) -> list[SlideSpec]:
    normalized_lang = (lang or "xml").strip().lower()
    if normalized_lang not in SUPPORTED_LANGS:
        raise ValueError("lang must be xml or json.")
    if not (content or "").strip():
        raise ValueError("content is required.")
    if normalized_lang == "json":
        return _parse_json_payload(content)
    return _parse_xml_payload(content)


def merge_refine_slide(existing: SlideSpec, update: SlideSpec) -> SlideSpec:
    merged = SlideSpec.from_dict(existing.to_dict())
    if update.prompt:
        merged.prompt = update.prompt
    if update.slide_type and update.slide_type != "content":
        merged.slide_type = update.slide_type
    for field_name in (
        "title",
        "subtitle",
        "body",
        "template_id",
        "template_slide_id",
        "layout",
    ):
        value = getattr(update, field_name)
        if value:
            setattr(merged, field_name, value)
    for field_name in ("bullets", "sections", "items", "metrics", "images"):
        value = getattr(update, field_name)
        if value:
            setattr(merged, field_name, deepcopy(value))
    _fill_from_prompt(merged)
    return merged


def normalize_slide_ids(slides: Iterable[SlideSpec]) -> list[SlideSpec]:
    normalized: list[SlideSpec] = []
    seen: set[str] = set()
    for index, slide in enumerate(slides, start=1):
        item = SlideSpec.from_dict(slide.to_dict())
        candidate = _safe_slide_id(item.slide_id) or f"{SLIDE_ID_PREFIX}{index:03d}"
        while candidate in seen:
            candidate = f"{SLIDE_ID_PREFIX}{index:03d}_{len(seen) + 1}"
        item.slide_id = candidate
        seen.add(candidate)
        normalized.append(item)
    return normalized


def _parse_json_payload(content: str) -> list[SlideSpec]:
    try:
        data = json.loads(content)
    except json.JSONDecodeError as exc:
        raise ValueError("content JSON parse failed.") from exc
    if isinstance(data, dict):
        raw_slides = data.get("slides")
        if raw_slides is None:
            raw_slides = [data]
    elif isinstance(data, list):
        raw_slides = data
    else:
        raise ValueError("content JSON must be an object or an array.")
    if not isinstance(raw_slides, list) or not raw_slides:
        raise ValueError("content must contain at least one slide.")
    slides: list[SlideSpec] = []
    for index, raw in enumerate(raw_slides, start=1):
        if not isinstance(raw, dict):
            raise ValueError("each slide must be an object.")
        slide = SlideSpec.from_dict(raw)
        if not slide.slide_id:
            slide.slide_id = f"{SLIDE_ID_PREFIX}{index:03d}"
        _fill_from_prompt(slide)
        slides.append(slide)
    return normalize_slide_ids(slides)


def _parse_xml_payload(content: str) -> list[SlideSpec]:
    try:
        root = ET.fromstring(content)
    except ET.ParseError as exc:
        raise ValueError("content XML parse failed.") from exc
    slide_nodes = list(root.findall(".//slide")) if root.tag != "slide" else [root]
    if not slide_nodes:
        raise ValueError("content XML must contain at least one <slide>.")
    slides: list[SlideSpec] = []
    for index, node in enumerate(slide_nodes, start=1):
        prompt = _text(node, "prompt")
        slide = SlideSpec(
            slide_id=_text(node, "slide_id") or f"{SLIDE_ID_PREFIX}{index:03d}",
            prompt=prompt,
            slide_type=_text(node, "type") or _text(node, "slide_type") or "",
            title=_text(node, "title"),
            subtitle=_text(node, "subtitle"),
            body=_text(node, "body") or _text(node, "description"),
            template_id=_text(node, "template_id"),
            template_slide_id=_text(node, "template_slide_id"),
            layout=_text(node, "layout"),
            bullets=[item.text.strip() for item in node.findall(".//bullet") if item.text and item.text.strip()],
            sections=_parse_named_nodes(node, "section"),
            items=_parse_named_nodes(node, "item"),
            metrics=_parse_named_nodes(node, "metric"),
            images=_parse_image_nodes(node),
        )
        _fill_from_prompt(slide)
        slides.append(slide)
    return normalize_slide_ids(slides)


def _parse_named_nodes(node: ET.Element, tag_name: str) -> list[dict[str, Any]]:
    output: list[dict[str, Any]] = []
    for child in node.findall(f".//{tag_name}"):
        data: dict[str, Any] = {}
        for key, value in child.attrib.items():
            if value.strip():
                data[key] = value.strip()
        for nested in list(child):
            if nested.text and nested.text.strip():
                data[nested.tag] = nested.text.strip()
        text = (child.text or "").strip()
        if text and "text" not in data and "body" not in data:
            data["text"] = text
        if data:
            output.append(data)
    return output


def _parse_image_nodes(node: ET.Element) -> list[dict[str, str]]:
    output: list[dict[str, str]] = []
    for child in node.findall(".//image"):
        src = (child.get("src") or child.get("path") or "").strip()
        if not src and child.text:
            src = child.text.strip()
        if not src:
            continue
        output.append(
            {
                "src": src,
                "caption": (child.get("caption") or "").strip(),
            }
        )
    return output


def _fill_from_prompt(slide: SlideSpec) -> None:
    prompt = slide.prompt.strip()
    if not slide.slide_type:
        slide.slide_type = _infer_type(prompt, slide)
    if not slide.title:
        slide.title = _infer_title(prompt, slide.slide_type)
    if not slide.bullets:
        slide.bullets = _infer_bullets(prompt)
    if not slide.body:
        slide.body = _infer_body(prompt, slide.title, slide.bullets)
    if not slide.items and slide.slide_type in {"comparison", "timeline", "process"}:
        slide.items = [{"title": item, "body": ""} for item in slide.bullets[:6]]
    if not slide.metrics and slide.slide_type in {"data", "chart"}:
        slide.metrics = _infer_metrics(prompt)
    if not slide.subtitle and slide.slide_type == "cover":
        slide.subtitle = slide.body
    slide.slide_type = _normalize_type(slide.slide_type)


def _infer_type(prompt: str, slide: SlideSpec) -> str:
    text = " ".join([prompt, slide.title, slide.layout]).lower()
    if any(word in text for word in ("cover", "封面", "标题页", "首页")):
        return "cover"
    if any(word in text for word in ("目录", "大纲", "agenda", "contents", "toc")):
        return "toc"
    if any(word in text for word in ("章节", "过渡", "分节", "section divider")):
        return "section"
    if any(word in text for word in ("对比", "比较", "comparison", "versus", "vs", "优缺点")):
        return "comparison"
    if any(word in text for word in ("时间线", "发展历程", "timeline", "步骤", "流程", "process")):
        return "timeline"
    if any(word in text for word in ("图表", "chart", "数据", "指标", "统计", "metric")):
        return "data"
    if any(word in text for word in ("总结", "结尾", "致谢", "closing", "summary")):
        return "closing"
    return "content"


def _normalize_type(value: str) -> str:
    raw = value.strip().lower().replace("_", "-")
    aliases = {
        "封面": "cover",
        "标题页": "cover",
        "首页": "cover",
        "目录": "toc",
        "大纲": "toc",
        "table-of-contents": "toc",
        "agenda": "toc",
        "contents": "toc",
        "章节": "section",
        "过渡页": "section",
        "分节": "section",
        "section-divider": "section",
        "divider": "section",
        "对比": "comparison",
        "比较": "comparison",
        "优缺点": "comparison",
        "流程": "timeline",
        "步骤": "timeline",
        "时间线": "timeline",
        "发展历程": "timeline",
        "process": "timeline",
        "数据": "data",
        "图表": "data",
        "指标": "data",
        "统计": "data",
        "chart": "data",
        "总结": "closing",
        "结尾": "closing",
        "致谢": "closing",
        "summary": "closing",
        "end": "closing",
    }
    return aliases.get(raw, raw or "content")


def _infer_title(prompt: str, slide_type: str) -> str:
    for pattern in (
        r"(?:标题|title)\s*[:：]\s*(.+)",
        r"(?:页面主题|主题)\s*[:：]\s*(.+)",
    ):
        match = re.search(pattern, prompt, re.IGNORECASE)
        if match:
            return _clean_inline(match.group(1))[:80]
    for line in prompt.splitlines():
        cleaned = _clean_inline(line)
        if cleaned:
            return cleaned[:80]
    defaults = {
        "cover": "演示文稿",
        "toc": "目录",
        "section": "章节",
        "closing": "总结",
    }
    return defaults.get(slide_type, "页面")


def _infer_bullets(prompt: str) -> list[str]:
    bullets: list[str] = []
    for line in prompt.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        match = re.match(r"^(?:[-*•]|[0-9一二三四五六七八九十]+[.)、])\s*(.+)$", stripped)
        if match:
            bullets.append(_clean_inline(match.group(1)))
    if bullets:
        return [item for item in bullets if item][:8]
    for label in ("要点", "内容", "points"):
        match = re.search(rf"{label}\s*[:：]\s*(.+)", prompt, re.IGNORECASE)
        if match:
            parts = re.split(r"[;；。、|,\n]+", match.group(1))
            return [_clean_inline(part) for part in parts if _clean_inline(part)][:8]
    return []


def _infer_body(prompt: str, title: str, bullets: list[str]) -> str:
    lines = [_clean_inline(line) for line in prompt.splitlines()]
    candidates = [
        line
        for line in lines
        if line and line != title and not any(line.endswith(bullet) for bullet in bullets)
    ]
    if len(candidates) >= 2:
        return candidates[1][:180]
    if candidates:
        return candidates[0][:180]
    return ""


def _infer_metrics(prompt: str) -> list[dict[str, Any]]:
    metrics: list[dict[str, Any]] = []
    for label, value in re.findall(r"([\w\u4e00-\u9fff]{1,18})\s*[:：]\s*([0-9]+(?:\.[0-9]+)?)", prompt):
        try:
            metrics.append({"label": label, "value": float(value)})
        except ValueError:
            continue
    return metrics[:6]


def _text(node: ET.Element, tag_name: str) -> str:
    child = node.find(tag_name)
    if child is None or child.text is None:
        return ""
    return child.text.strip()


def _clean_inline(value: str) -> str:
    return re.sub(r"\s+", " ", value.strip(" \t\r\n-•*"))


def _safe_slide_id(value: str) -> str:
    raw = value.strip()
    if not raw:
        return ""
    cleaned = re.sub(r"[^A-Za-z0-9_.-]+", "_", raw)
    return cleaned[:64] or ""


def _string_list(value: Any) -> list[str]:
    if not isinstance(value, list):
        return []
    return [str(item).strip() for item in value if str(item).strip()]


def _dict_list(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        return []
    return [dict(item) for item in value if isinstance(item, dict)]
