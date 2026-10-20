---
title: Browser Tool
summary: Enablement, strict actions, batching, result attachments, and limits for the single `browser` tool.
updated_at: 2026-10-20
---

# Browser Tool

Once enabled, the model calls one `browser` tool. Each call selects one `action`. Every action has its own allowed fields, required fields, and mutual-exclusion rules, which the server validates again before calling the Playwright bridge. Old `browser_*` tool names cannot be called.

Use [Web Fetch](/docs/en/tools/web-fetch/) for a static public page. Use this tool for navigation, tabs, clicks, typing, dynamic-page waits, screenshots, or actionable page refs.

## Enablement

Enable both the browser-tool switch and the browser runtime. `browser` registers automatically; it does not need a built-in allowlist entry.

## Actions

| action | Purpose | Key rule |
| --- | --- | --- |
| `navigate` / `back` | Open a URL or go back | `navigate` requires an http(s) URL and does not include page text by default |
| `tabs_list` / `tabs_new` / `tabs_select` / `tabs_close` | List, create, select, or close tabs | select/close requires `target_id` or `index` |
| `observe_refs` | Read the accessibility snapshot | Returns element refs for later interaction |
| `observe_text` | Read current-page text by page | Reuse `next_cursor` only on the unchanged page |
| `screenshot` | Capture an image | An optional workspace-relative `path` controls output; the result is an attachment |
| `click` / `type` / `hover` / `select` / `drag` / `press` | Interact with the page | Element actions provide exactly one of `ref` or `selector`; drag provides complete source and target pairs |
| `wait` | Wait for load, selector, text, URL, or bounded time | Prefer a known success condition to a fixed delay |
| `batch` | Run 1–10 dependent steps in order | Steps use strict kind schemas and cannot nest batch |

A common flow is `navigate` → `observe_refs` → interaction or `batch` → `wait` → `observe_text`. Navigation invalidates old refs, so observe again after a page change.

## Batch

```json
{
  "action": "batch",
  "steps": [
    {"kind": "type", "ref": "e1", "text": "sample"},
    {"kind": "click", "ref": "e2"},
    {"kind": "wait", "url": "**/complete", "timeout_ms": 5000}
  ]
}
```

## Results and limits

Model results are projected by action to the smallest shape needed for the next step. Observations retain text, refs, truncation state, and cursors. Interactions retain the action, target ref, URL, and page generation. Screenshots remove base64 and become workspace attachments. Events and audit retain the complete structured result.

Text cursors are signed and bound to the tab and `page_generation`. A page change or bridge restart rejects an old cursor and requires a fresh observation. The runtime does not keep an unbounded full-page cache.

Only http/https URLs are allowed. `file://` and intranet addresses are blocked by default. Snapshot and text requests allow at most 40,000 characters, batches allow at most 10 steps, and the runtime also bounds sessions, tabs, downloads, and timeouts.

`evaluate`, raw JavaScript, browser status, and closing a browser session are control-plane operations, not model actions.
