---
title: Web Fetch
summary: Direct fetch, browser fallback, and success or failure results for `web_fetch`.
read_when:
  - You need to read public webpage content rather than drive a browser
source_docs:
  - src/services/tools/web_fetch_tool.rs
updated_at: 2026-04-10
---

# Web Fetch

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
- `format`: usually `markdown` or `text`
- `extractor`: the extractor that actually produced the content
- `truncated`: whether the main content was cut
- `warning`: extra fetch-time hints or warnings

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

- If you need to click, type, or scroll, use [Browser](/docs/en/tools/browser/)
- If you need to inspect a local HTML file, this is not the right tool
