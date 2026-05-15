---
title: Web Fetch
summary: The split between `web_search` and `web_fetch`, plus direct fetch, browser fallback, and success or failure results for `web_fetch`.
read_when:
  - You need to read public webpage content rather than drive a browser
source_docs:
  - src/services/tools/web_search_tool.rs
  - src/services/tools/web_fetch_tool.rs
updated_at: 2026-05-15
---

# Web Fetch

## Search vs. fetch

- `web_search`: takes keywords and returns candidate result titles, URLs, and snippets.
- `web_fetch`: takes one confirmed URL and reads that page's main content.

When you only have keywords, start with `web_search`:

```json
{
  "query": "example project official docs GitHub",
  "count": 5
}
```

If you already have a site or domain and only want to search inside it, pass `site` or `sites`; do not build a search-result URL and send it to `web_fetch`:

```json
{
  "site": "example.com",
  "query": "install configuration",
  "count": 5
}
```

Then pass concrete result URLs to `web_fetch`. Do not pass Bing, Baidu, Google, or other search-result page URLs to `web_fetch`.

`web_fetch` is still one of the deliberate exceptions in the tool system.  
On success, it returns the fetched result object directly rather than the unified `ok/action/state/summary/data` envelope.

## Minimum arguments

```json
{
  "url": "https://example.com"
}
```

## Success result

```json
{
  "url": "https://example.com",
  "final_url": "https://example.com",
  "status": 200,
  "title": "Example Domain",
  "content_type": "text/html; charset=UTF-8",
  "content_kind": "html",
  "fetch_strategy": "direct_http",
  "format": "markdown",
  "extractor": "readability",
  "truncated": false,
  "warning": null,
  "cached": false,
  "fetched_at": "2026-04-10T03:00:00Z",
  "content": "..."
}
```

## Important fields

- `fetch_strategy`: for example `direct_http` or `browser_fallback`
- `provider`: present when an external provider such as `firecrawl` handled the fetch
- `format`: usually `markdown` or `text`
- `extractor`: the extractor that actually produced the content
- `truncated`: whether the main content was cut
- `warning`: extra fetch-time hints or warnings

## Providers

Docker compose now defaults to Wunder's built-in web fetcher and does not start a self-hosted Firecrawl group. `tools.web.search.enabled` is disabled by default; if you want keyword search, configure a search provider and enable `web_search` explicitly.

`web_fetch` keeps the model-facing arguments small. Provider selection is configured by the system:

```yaml
tools:
  web:
    fetch:
      provider: direct # direct | auto | firecrawl
      firecrawl:
        api_key: ${FIRECRAWL_API_KEY:-}
        base_url: ${FIRECRAWL_BASE_URL:-https://api.firecrawl.dev}
```

- `direct`: built-in Wunder HTTP fetcher.
- `firecrawl`: use Firecrawl `/v2/scrape`.
- `auto`: use Firecrawl when an API key or custom base URL is configured, otherwise fall back to `direct`.

Firecrawl Cloud uses `https://api.firecrawl.dev` and requires an API key. The admin settings page only stores Wunder's connection parameters for an external Firecrawl service and does not control Docker service lifecycle.

## Browser fallback

If direct HTTP only returns a frontend shell, an anti-bot page, or meaningless HTML, `web_fetch` may automatically attempt browser fallback.  
When that happens successfully, you will see:

```json
{
  "fetch_strategy": "browser_fallback"
}
```

## Failure results

On failure, the tool falls back to the unified failure envelope and adds fetch diagnostics such as:

- `phase`
- `failure_summary`
- `error_detail_head`
- `next_step_hint`
- `normalized_url`
- `host`
- `error_meta.code`

## When not to use it

- If you have search keywords, use a search tool first. `web_fetch` rejects search-result URLs such as `bing.com/search?...`.
- If you need to click, type, or scroll, use [Browser](/docs/en/tools/browser/)
- If you need to inspect a local HTML file, this is not the right tool
