from __future__ import annotations

import html
import json
import re
import shutil
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
DOCS_SOURCE_DIR = REPO_ROOT / "docs" / "静态站文档"
SITE_CONFIG_PATH = DOCS_SOURCE_DIR / "site.json"
SITE_ASSET_DIR = Path(__file__).resolve().parent / "docs_site"
OUTPUT_DIR = REPO_ROOT / "web" / "docs"
GENERATOR_NAME = "wunder-static-docs-v1"
ROOT_GENERATED_FILES = [
    "index.html",
    "site.js",
    "site.css",
    "manifest.json",
    "search.json",
    "assets",
]
LOGO_SOURCE_PATH = REPO_ROOT / "images" / "eva01-head.svg"
LOGO_TARGET_PATH = Path("assets") / "eva01-head.svg"
FENCE_PATTERN = re.compile(r"^(```|~~~)")
HEADING_PATTERN = re.compile(r"^(#{1,6})\s+(.+?)\s*$")
DATE_PATTERN = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def write_text(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(read_text(path))


def inline_json(data: Any) -> str:
    return json.dumps(data, ensure_ascii=False, separators=(",", ":")).replace("</", "<\\/")


def pretty_json(data: Any) -> str:
    return json.dumps(data, ensure_ascii=False, indent=2) + "\n"


def parse_scalar(raw: str) -> Any:
    value = raw.strip()
    if not value:
        return ""
    if value.startswith('"') and value.endswith('"'):
        return json.loads(value)
    if value.startswith("'") and value.endswith("'"):
        return value[1:-1]
    return value


def parse_frontmatter(text: str) -> tuple[dict[str, Any], str]:
    normalized = text.replace("\r\n", "\n")
    if not normalized.startswith("---\n"):
        return {}, normalized
    end_index = normalized.find("\n---\n", 4)
    if end_index < 0:
        return {}, normalized
    meta_block = normalized[4:end_index]
    body = normalized[end_index + 5 :].lstrip("\n")
    metadata: dict[str, Any] = {}
    active_list_key: str | None = None
    for line in meta_block.splitlines():
        if not line.strip():
            continue
        if line.startswith("  - "):
            if active_list_key is None:
                raise ValueError(f"invalid frontmatter list item: {line}")
            metadata.setdefault(active_list_key, []).append(parse_scalar(line[4:]))
            continue
        key, separator, value = line.partition(":")
        if not separator:
            raise ValueError(f"invalid frontmatter line: {line}")
        normalized_key = key.strip()
        normalized_value = value.strip()
        if normalized_value:
            metadata[normalized_key] = parse_scalar(normalized_value)
            active_list_key = None
            continue
        metadata[normalized_key] = []
        active_list_key = normalized_key
    return metadata, body


def clean_inline_markdown(value: str) -> str:
    text = re.sub(r"`([^`]+)`", r"\1", value)
    text = re.sub(r"!\[([^\]]*)\]\([^)]+\)", r"\1", text)
    text = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", text)
    text = re.sub(r"<[^>]+>", " ", text)
    text = re.sub(r"[*_~]", "", text)
    text = re.sub(r"\s+", " ", text)
    return text.strip()


def extract_headings(markdown: str) -> list[dict[str, Any]]:
    headings: list[dict[str, Any]] = []
    in_fence = False
    index = 0
    for line in markdown.splitlines():
        stripped = line.strip()
        if FENCE_PATTERN.match(stripped):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        match = HEADING_PATTERN.match(line)
        if not match:
            continue
        index += 1
        headings.append(
            {
                "id": f"section-{index}",
                "level": len(match.group(1)),
                "text": clean_inline_markdown(match.group(2)),
            }
        )
    return headings


def extract_first_heading(markdown: str) -> str:
    for heading in extract_headings(markdown):
        if heading["level"] == 1 and heading["text"]:
            return str(heading["text"])
    return ""


def strip_markdown(text: str) -> str:
    body = re.sub(r"```.*?```", " ", text, flags=re.S)
    body = re.sub(r"~~~.*?~~~", " ", body, flags=re.S)
    body = re.sub(r"!\[([^\]]*)\]\([^)]+\)", r"\1", body)
    body = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", body)
    body = re.sub(r"`([^`]+)`", r"\1", body)
    body = re.sub(r"<[^>]+>", " ", body)
    body = re.sub(r"^[>#-]+\s*", "", body, flags=re.M)
    body = re.sub(r"[*_~]", "", body)
    body = re.sub(r"\s+", " ", body)
    return body.strip()


def extract_summary(markdown: str) -> str:
    in_fence = False
    paragraph_lines: list[str] = []
    for line in markdown.splitlines():
        stripped = line.strip()
        if FENCE_PATTERN.match(stripped):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        if not stripped:
            if paragraph_lines:
                break
            continue
        if stripped.startswith("#") or stripped.startswith("<"):
            continue
        paragraph_lines.append(stripped)
        if stripped.endswith(("。", ".", "!", "！", "?", "？")):
            break
    return strip_markdown(" ".join(paragraph_lines))


def ensure_list(value: Any) -> list[str]:
    if value is None:
        return []
    if isinstance(value, list):
        return [str(item) for item in value if str(item).strip()]
    text = str(value).strip()
    return [text] if text else []


def ensure_header_links(value: Any) -> list[dict[str, str]]:
    if not isinstance(value, list):
        return []
    links: list[dict[str, str]] = []
    for item in value:
        if not isinstance(item, dict):
            continue
        label = str(item.get("label") or "").strip()
        url = str(item.get("url") or item.get("href") or "").strip()
        if not label or not url:
            continue
        links.append({"label": label, "url": url})
    return links


def resolve_updated_at(source_path: Path, metadata: dict[str, Any]) -> str:
    for key in ("updated_at", "last_updated", "updated"):
        raw_value = str(metadata.get(key) or "").strip()
        if DATE_PATTERN.fullmatch(raw_value):
            return raw_value
    return datetime.fromtimestamp(source_path.stat().st_mtime).astimezone().strftime("%Y-%m-%d")


def resolve_output(slug: str, home_slug: str) -> tuple[Path, str]:
    if slug == home_slug:
        return Path("index.html"), "/docs/"
    if slug.endswith("/index"):
        prefix = slug.removesuffix("/index")
        return Path(prefix) / "index.html", f"/docs/{prefix}/"
    return Path(slug) / "index.html", f"/docs/{slug}/"


def load_pages(site_config: dict[str, Any]) -> tuple[dict[str, dict[str, Any]], list[str], list[dict[str, Any]]]:
    languages = site_config.get("navigation", {}).get("languages", [])
    if not languages:
        raise ValueError("site.json 缺少 navigation.languages")
    home_slug = str(site_config.get("site", {}).get("home_page") or "").strip()
    if not home_slug:
        raise ValueError("site.json 缺少 site.home_page")

    page_order: list[str] = []
    page_context: dict[str, dict[str, Any]] = {}

    for language in languages:
        language_code = str(language.get("language") or "").strip()
        language_label = str(language.get("label") or language_code).strip()
        if not language_code:
            raise ValueError("site.json 中存在空 language")
        for tab in language.get("tabs", []):
            tab_name = str(tab.get("tab") or "").strip()
            if not tab_name:
                raise ValueError(f"{language_code} 下存在空 tab")
            for group in tab.get("groups", []):
                group_name = str(group.get("group") or "").strip()
                if not group_name:
                    raise ValueError(f"{language_code}/{tab_name} 下存在空 group")
                for slug in group.get("pages", []):
                    page_slug = str(slug).strip()
                    if not page_slug:
                        raise ValueError(f"{language_code}/{tab_name}/{group_name} 下存在空页面 slug")
                    if page_slug in page_context:
                        raise ValueError(f"site.json 页面重复: {page_slug}")
                    page_order.append(page_slug)
                    page_context[page_slug] = {
                        "language": language_code,
                        "language_label": language_label,
                        "tab": tab_name,
                        "group": group_name,
                        "group_path": [group_name],
                    }

    pages: dict[str, dict[str, Any]] = {}
    for slug in page_order:
        source_path = DOCS_SOURCE_DIR / f"{slug}.md"
        if not source_path.exists():
            raise FileNotFoundError(f"文档源文件不存在: {source_path}")
        metadata, markdown = parse_frontmatter(read_text(source_path))
        headings = extract_headings(markdown)
        title = str(metadata.get("title") or extract_first_heading(markdown) or slug.split("/")[-1]).strip()
        summary = str(metadata.get("summary") or extract_summary(markdown)).strip()
        output_path, page_url = resolve_output(slug, home_slug)
        pages[slug] = {
            **page_context[slug],
            "slug": slug,
            "title": title,
            "summary": summary,
            "updated_at": resolve_updated_at(source_path, metadata),
            "read_when": ensure_list(metadata.get("read_when")),
            "source_docs": ensure_list(metadata.get("source_docs")),
            "url": page_url,
            "headings": headings,
            "markdown": markdown.rstrip() + "\n",
            "source_path": source_path.relative_to(DOCS_SOURCE_DIR).as_posix(),
            "output_path": output_path.as_posix(),
            "search_text": strip_markdown(" ".join([title, summary, markdown, " ".join(item["text"] for item in headings)])),
        }

    for index, slug in enumerate(page_order):
        pages[slug]["prev_slug"] = page_order[index - 1] if index > 0 else None
        pages[slug]["next_slug"] = page_order[index + 1] if index + 1 < len(page_order) else None

    resolved_languages: list[dict[str, Any]] = []
    for language in languages:
        tabs_payload: list[dict[str, Any]] = []
        for tab in language.get("tabs", []):
            groups_payload: list[dict[str, Any]] = []
            first_url = "/docs/"
            for group in tab.get("groups", []):
                page_items: list[dict[str, Any]] = []
                for slug in group.get("pages", []):
                    page = pages[str(slug)]
                    if not page_items and first_url == "/docs/":
                        first_url = page["url"]
                    page_items.append(
                        {
                            "slug": page["slug"],
                            "title": page["title"],
                            "summary": page["summary"],
                            "url": page["url"],
                        }
                    )
                groups_payload.append(
                    {
                        "group": str(group["group"]),
                        "pages": page_items,
                    }
                )
            tabs_payload.append(
                {
                    "tab": str(tab["tab"]),
                    "entry_url": first_url,
                    "groups": groups_payload,
                }
            )
        resolved_languages.append(
            {
                "language": str(language["language"]),
                "label": str(language.get("label") or language["language"]),
                "tabs": tabs_payload,
            }
        )

    return pages, page_order, resolved_languages


def render_page_html(site_meta: dict[str, Any], page: dict[str, Any]) -> str:
    page_data = {
        "slug": page["slug"],
        "language": page["language"],
        "title": page["title"],
        "summary": page["summary"],
        "updated_at": page["updated_at"],
        "read_when": page["read_when"],
        "source_docs": page["source_docs"],
        "tab": page["tab"],
        "group": page["group"],
        "group_path": page["group_path"],
        "url": page["url"],
        "headings": page["headings"],
        "prev_slug": page["prev_slug"],
        "next_slug": page["next_slug"],
        "markdown": page["markdown"],
        "source_path": f"静态站文档/{page['source_path']}",
    }
    page_title = html.escape(f"{page['title']} | {site_meta['name']}", quote=False)
    description = html.escape(page["summary"] or site_meta["description"], quote=True)
    language = html.escape(page["language"], quote=True)
    site_name = html.escape(site_meta["name"], quote=False)
    logo_url = html.escape(site_meta["logo_url"], quote=True)
    page_json = inline_json(page_data)
    return f"""<!doctype html>
<html lang="{language}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{page_title}</title>
  <meta name="description" content="{description}">
  <link rel="stylesheet" href="/docs/site.css">
  <script type="module" src="/docs/site.js"></script>
</head>
<body>
  <div class="docs-app">
    <header class="docs-header">
      <div class="docs-topbar">
        <div class="docs-topbar-start">
          <a class="docs-brand" href="/docs/">
            <img class="docs-brand-mark" src="{logo_url}" alt="">
            <span class="docs-brand-text">{site_name}</span>
          </a>
          <div class="docs-language-switcher" id="docs-language-switcher"></div>
        </div>
        <div class="docs-topbar-center">
          <label class="docs-search">
            <span class="docs-search-icon" aria-hidden="true">
              <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <circle cx="8.5" cy="8.5" r="5.75"></circle>
                <path d="M12.7 12.7 17 17"></path>
              </svg>
            </span>
            <input id="docs-search-input" type="search" placeholder="搜索..." aria-label="搜索文档">
            <span class="docs-search-shortcut">Ctrl K</span>
            <div class="docs-search-results" id="docs-search-results" hidden></div>
          </label>
        </div>
        <div class="docs-topbar-end">
          <button class="docs-theme-toggle" id="docs-theme-toggle" type="button" aria-label="切换主题"></button>
        </div>
      </div>
      <div class="docs-nav-row">
        <nav class="docs-tabs" id="docs-tabs" aria-label="主导航"></nav>
      </div>
    </header>
    <div class="docs-layout">
      <aside class="docs-sidebar" id="docs-sidebar"></aside>
      <main class="docs-main">
        <section class="docs-page-header" id="docs-page-header"></section>
        <article class="docs-content" id="docs-content"></article>
        <section class="docs-page-footer" id="docs-page-footer"></section>
      </main>
      <aside class="docs-toc" id="docs-toc-wrap">
        <div class="docs-toc-card">
          <div class="docs-toc-title">在此页面</div>
          <nav id="docs-toc"></nav>
        </div>
      </aside>
    </div>
  </div>
  <script id="docs-page-data" type="application/json">{page_json}</script>
</body>
</html>
"""


def cleanup_previous_build() -> None:
    manifest_path = OUTPUT_DIR / "manifest.json"
    if not manifest_path.exists():
        return
    try:
        manifest = load_json(manifest_path)
    except Exception:
        return
    if manifest.get("generator") != GENERATOR_NAME:
        return
    generated_paths = manifest.get("generated_paths", [])
    ordered_paths = sorted(
        (Path(str(item)) for item in generated_paths if str(item).strip()),
        key=lambda item: len(item.parts),
        reverse=True,
    )
    for relative_path in ordered_paths:
        target = OUTPUT_DIR / relative_path
        if target.is_file() or target.is_symlink():
            target.unlink(missing_ok=True)
        elif target.is_dir():
            shutil.rmtree(target, ignore_errors=True)


def copy_site_assets() -> None:
    for asset_name in ("site.js", "site.css"):
        asset_path = SITE_ASSET_DIR / asset_name
        if not asset_path.exists():
            raise FileNotFoundError(f"缺少站点资源模板: {asset_path}")
        write_text(OUTPUT_DIR / asset_name, read_text(asset_path))
    if not LOGO_SOURCE_PATH.exists():
        raise FileNotFoundError(f"缺少站点品牌图标: {LOGO_SOURCE_PATH}")
    logo_output = OUTPUT_DIR / LOGO_TARGET_PATH
    logo_output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(LOGO_SOURCE_PATH, logo_output)


def build() -> None:
    site_config = load_json(SITE_CONFIG_PATH)
    pages, page_order, resolved_languages = load_pages(site_config)
    home_slug = str(site_config["site"]["home_page"])
    site_meta = {
        "name": str(site_config["site"].get("name") or "wunder 文档"),
        "description": str(site_config["site"].get("description") or ""),
        "default_language": str(site_config["site"].get("default_language") or "zh-CN"),
        "home_page": home_slug,
        "home_url": pages[home_slug]["url"],
        "logo_url": str(site_config["site"].get("logo") or f"/docs/{LOGO_TARGET_PATH.as_posix()}"),
        "header_links": ensure_header_links(site_config["site"].get("header_links")),
    }

    cleanup_previous_build()
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    copy_site_assets()

    for page in pages.values():
        write_text(OUTPUT_DIR / page["output_path"], render_page_html(site_meta, page))

    generated_paths = [*ROOT_GENERATED_FILES, *[item["language"] for item in resolved_languages]]
    manifest = {
        "generator": GENERATOR_NAME,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "site": site_meta,
        "navigation": {"languages": resolved_languages},
        "page_order": page_order,
        "generated_paths": generated_paths,
        "pages": {
            slug: {
                "slug": page["slug"],
                "language": page["language"],
                "title": page["title"],
                "summary": page["summary"],
                "updated_at": page["updated_at"],
                "read_when": page["read_when"],
                "source_docs": page["source_docs"],
                "tab": page["tab"],
                "group": page["group"],
                "group_path": page["group_path"],
                "url": page["url"],
                "headings": page["headings"],
                "prev_slug": page["prev_slug"],
                "next_slug": page["next_slug"],
                "source_path": page["source_path"],
                "output_path": page["output_path"],
            }
            for slug, page in pages.items()
        },
    }
    search_entries = [
        {
            "slug": page["slug"],
            "language": page["language"],
            "tab": page["tab"],
            "group": page["group"],
            "title": page["title"],
            "summary": page["summary"],
            "url": page["url"],
            "headings": [item["text"] for item in page["headings"]],
            "text": page["search_text"],
        }
        for page in pages.values()
    ]

    write_text(OUTPUT_DIR / "manifest.json", pretty_json(manifest))
    write_text(OUTPUT_DIR / "search.json", pretty_json(search_entries))
    print(f"Built docs site: {len(pages)} pages -> {OUTPUT_DIR}")


if __name__ == "__main__":
    build()
