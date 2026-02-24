from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from ...common.config import get_config_section, get_section_value
from ...common.env import parse_int

DEFAULT_KB_KEY = "default"
DEFAULT_TIMEOUT_S = 10
DEFAULT_PAGE_SIZE = 20
MAX_PAGE_SIZE = 200

REQUEST_KEYS = {
    "page",
    "page_size",
    "similarity_threshold",
    "vector_similarity_weight",
    "top_k",
    "rerank_id",
    "keyword",
    "highlight",
    "cross_languages",
    "metadata_condition",
    "use_kg",
    "toc_enhance",
}


@dataclass(frozen=True)
class KnowledgeTargetConfig:
    key: str
    base_url: str
    api_key: str | None
    dataset_ids: list[str]
    description: str | None
    timeout_s: int
    request: dict[str, Any]


def _get_kb_section() -> dict[str, Any]:
    section = get_config_section("knowledge")
    if section:
        return section
    return get_config_section("kb")


def _normalize_base_url(value: Any) -> str:
    base = str(value).strip() if value is not None else ""
    if not base:
        raise ValueError("知识库 base_url 未配置。")
    return base.rstrip("/")


def _normalize_dataset_ids(value: Any) -> list[str]:
    if value is None:
        return []
    if isinstance(value, str):
        return [item.strip() for item in value.split(",") if item.strip()]
    if isinstance(value, (list, tuple)):
        ids: list[str] = []
        for item in value:
            text = str(item).strip()
            if text:
                ids.append(text)
        return ids
    return []


def _normalize_request_defaults(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict):
        return {}
    defaults: dict[str, Any] = {}
    for key, raw in value.items():
        if key not in REQUEST_KEYS:
            continue
        if raw in (None, ""):
            continue
        if key in {"page", "page_size", "top_k", "rerank_id"}:
            parsed = parse_int(str(raw), 0)
            if parsed > 0:
                defaults[key] = parsed
            continue
        defaults[key] = raw
    return defaults


def _merge_request_defaults(
    base: dict[str, Any],
    override: Any,
) -> dict[str, Any]:
    merged = dict(base)
    if not isinstance(override, dict):
        return merged
    for key, raw in override.items():
        if key not in REQUEST_KEYS:
            continue
        if raw in (None, ""):
            continue
        if key in {"page", "page_size", "top_k", "rerank_id"}:
            parsed = parse_int(str(raw), 0)
            if parsed > 0:
                merged[key] = parsed
            continue
        merged[key] = raw
    return merged


def _get_target_description_map() -> dict[str, str]:
    config = _get_kb_section()
    raw = get_section_value(config, "target_descriptions", "descriptions")
    mapping: dict[str, str] = {}
    if isinstance(raw, dict):
        mapping.update({str(key): str(value) for key, value in raw.items() if value})
    default_desc = get_section_value(config, "description", "desc")
    if default_desc:
        mapping.setdefault(get_default_kb_key(), str(default_desc))
    return mapping


def _parse_target_config(
    key: str,
    raw: Any,
    base_url: str,
    api_key: str | None,
    timeout_s: int,
    request_defaults: dict[str, Any],
    description_map: dict[str, str],
) -> KnowledgeTargetConfig:
    description = description_map.get(key)
    dataset_ids: list[str] = []
    target_request: dict[str, Any] = request_defaults

    if isinstance(raw, dict):
        dataset_ids = _normalize_dataset_ids(
            raw.get("dataset_ids")
            or raw.get("dataset_id")
            or raw.get("datasets")
        )
        if not dataset_ids:
            raise ValueError(f"知识库目标 '{key}' 未配置 dataset_ids。")
        base_url = _normalize_base_url(raw.get("base_url") or base_url)
        api_key = raw.get("api_key") or api_key
        timeout_s = parse_int(
            str(raw.get("timeout_s") or raw.get("timeout") or timeout_s),
            timeout_s,
        )
        target_request = _merge_request_defaults(request_defaults, raw.get("request"))
        description = raw.get("description") or raw.get("desc") or description
    elif isinstance(raw, (list, tuple, str)):
        dataset_ids = _normalize_dataset_ids(raw)
    else:
        raise ValueError(f"知识库目标 '{key}' 配置格式错误。")

    if not dataset_ids:
        raise ValueError(f"知识库目标 '{key}' 未配置 dataset_ids。")

    return KnowledgeTargetConfig(
        key=key,
        base_url=base_url,
        api_key=str(api_key) if api_key else None,
        dataset_ids=dataset_ids,
        description=str(description) if description else None,
        timeout_s=timeout_s,
        request=target_request,
    )


def get_default_kb_key() -> str:
    config = _get_kb_section()
    return get_section_value(config, "default_key") or DEFAULT_KB_KEY


def load_kb_targets() -> dict[str, KnowledgeTargetConfig]:
    config = _get_kb_section()
    if not config:
        return {}
    base_url = _normalize_base_url(get_section_value(config, "base_url", "endpoint"))
    api_key = get_section_value(config, "api_key", "token")
    timeout_s = parse_int(
        str(get_section_value(config, "timeout_s") or ""),
        DEFAULT_TIMEOUT_S,
    )
    request_defaults = _normalize_request_defaults(get_section_value(config, "request"))
    description_map = _get_target_description_map()

    raw_targets = get_section_value(config, "targets")
    if raw_targets is None:
        dataset_ids = get_section_value(config, "dataset_ids", "dataset_id", "datasets")
        if dataset_ids is None:
            return {}
        raw_targets = {
            get_default_kb_key(): {"dataset_ids": dataset_ids},
        }
    if not isinstance(raw_targets, dict):
        raise ValueError("knowledge.targets 必须是 JSON 对象。")

    targets: dict[str, KnowledgeTargetConfig] = {}
    for key, raw in raw_targets.items():
        targets[key] = _parse_target_config(
            key=str(key),
            raw=raw,
            base_url=base_url,
            api_key=api_key,
            timeout_s=timeout_s,
            request_defaults=request_defaults,
            description_map=description_map,
        )
    return targets


def get_kb_config(kb_key: str | None) -> KnowledgeTargetConfig:
    targets = load_kb_targets()
    if not targets:
        raise ValueError("知识库配置未找到，请在 mcp_config.json 中配置 knowledge。")
    target_key = kb_key or get_default_kb_key()
    if target_key not in targets:
        available = ", ".join(sorted(targets))
        raise ValueError(f"未知知识库 key '{target_key}'，可选：{available}")
    return targets[target_key]


def build_kb_description_hint() -> str:
    try:
        targets = load_kb_targets()
    except Exception:
        return ""
    if not targets:
        return ""

    items: list[str] = []
    for key, cfg in targets.items():
        if cfg.description:
            items.append(f"{key}（{cfg.description}）")
        else:
            items.append(key)
    return "知识库说明：" + "；".join(items)


def normalize_page_size(value: int) -> int:
    if value <= 0:
        return DEFAULT_PAGE_SIZE
    return min(value, MAX_PAGE_SIZE)
