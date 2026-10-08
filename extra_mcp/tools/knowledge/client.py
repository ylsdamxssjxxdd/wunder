from __future__ import annotations

import json
import time
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from .config import KnowledgeTargetConfig, normalize_page_size

RETRIEVAL_PATH = "/api/v1/retrieval"


def _build_headers(api_key: str | None) -> dict[str, str]:
    headers = {"Content-Type": "application/json"}
    if api_key:
        headers["Authorization"] = f"Bearer {api_key}"
    return headers


def _post_json(
    url: str,
    payload: dict[str, Any],
    api_key: str | None,
    timeout_s: int,
) -> dict[str, Any]:
    data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    request = Request(url, data=data, headers=_build_headers(api_key), method="POST")
    try:
        with urlopen(request, timeout=timeout_s) as response:
            body = response.read().decode("utf-8")
    except HTTPError as exc:
        body = ""
        if exc.fp is not None:
            body = exc.fp.read().decode("utf-8", errors="ignore")
        message = f"知识库请求失败：HTTP {exc.code} {exc.reason}"
        if body:
            message = f"{message} - {body}"
        raise RuntimeError(message) from exc
    except URLError as exc:
        raise RuntimeError(f"知识库连接失败：{exc.reason}") from exc

    if not body:
        return {}
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError as exc:
        raise RuntimeError("知识库返回不是合法 JSON。") from exc
    if not isinstance(parsed, dict):
        raise RuntimeError("知识库返回格式异常。")
    return parsed


def _normalize_chunks(chunks: Any) -> list[dict[str, Any]]:
    if not isinstance(chunks, list):
        return []
    compacted: list[dict[str, Any]] = []
    for item in chunks:
        if not isinstance(item, dict):
            continue
        compacted.append(
            {
                "content": item.get("content"),
                "highlight": item.get("highlight") or item.get("content_ltks"),
                "document_id": item.get("document_id"),
                "document_name": item.get("document_keyword") or item.get("document_name"),
                "similarity": item.get("similarity"),
                "term_similarity": item.get("term_similarity"),
                "vector_similarity": item.get("vector_similarity"),
                "kb_id": item.get("kb_id"),
            }
        )
    return compacted


def _normalize_doc_aggs(doc_aggs: Any) -> list[dict[str, Any]]:
    if not isinstance(doc_aggs, list):
        return []
    documents: list[dict[str, Any]] = []
    for item in doc_aggs:
        if not isinstance(item, dict):
            continue
        documents.append(
            {
                "id": item.get("doc_id"),
                "name": item.get("doc_name"),
                "count": item.get("count"),
            }
        )
    return documents


def query_kb_sync(
    cfg: KnowledgeTargetConfig,
    query: str,
    page_size: int,
) -> dict[str, Any]:
    if not cfg.api_key:
        raise ValueError("知识库 API Key 未配置。")
    if not cfg.dataset_ids:
        raise ValueError("知识库未配置 dataset_ids。")

    if page_size <= 0:
        page_size = normalize_page_size(cfg.request.get("page_size", 0))
    else:
        page_size = normalize_page_size(page_size)
    payload = dict(cfg.request)
    payload.update(
        {
            "question": query,
            "dataset_ids": cfg.dataset_ids,
            "page": payload.get("page", 1),
            "page_size": page_size,
        }
    )

    url = f"{cfg.base_url}{RETRIEVAL_PATH}"
    start = time.perf_counter()
    response = _post_json(url, payload, cfg.api_key, cfg.timeout_s)
    elapsed_ms = round((time.perf_counter() - start) * 1000, 2)

    code = response.get("code")
    if code not in (0, "0"):
        message = response.get("message") or response.get("error") or "知识库返回错误。"
        return {"ok": False, "error": message, "code": code}

    data = response.get("data") if isinstance(response.get("data"), dict) else {}
    chunks = _normalize_chunks(data.get("chunks"))
    documents = _normalize_doc_aggs(data.get("doc_aggs"))
    total = data.get("total")
    if not isinstance(total, int):
        total = len(chunks)

    return {
        "ok": True,
        "total": total,
        "chunks": chunks,
        "documents": documents,
        "elapsed_ms": elapsed_ms,
    }
