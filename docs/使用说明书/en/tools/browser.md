---
title: Browser Tool
summary: Enablement, strict actions, viewport, file upload, dialogs, network and console observation, batching, ref recovery across refreshes, denoised observations, screenshot delivery, and limits for the single `browser` tool.
updated_at: 2026-11-22
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
| `observe_refs` | Read the accessibility snapshot | Returns element refs for later interaction; refs are re-resolved by selector across same-page re-renders and refreshes |
| `observe_text` | Read current-page text by page | Reuse `next_cursor` only on the unchanged page |
| `screenshot` | Capture an image | An optional workspace-relative `path` controls output; it is saved as a workspace attachment and the image is appended for the model to view |
| `click` / `type` / `hover` / `select` / `drag` / `press` | Interact with the page | Element actions provide exactly one of `ref` or `selector`; drag provides complete source and target pairs |
| `wait` | Wait for load, selector, text, URL, or bounded time | Prefer a known success condition to a fixed delay |
| `resize` | Set the logical viewport in CSS pixels | Requires `width` and `height` within 240–3840; returns the effective viewport so responsive layouts can be checked at desktop sizes |
| `file_upload` | Submit uploaded files | Click the upload control and answer its `filechooser`, or point `ref`/`selector` at an `input[type=file]`; `files` holds 1–16 workspace-relative paths |
| `dialog` | Answer an alert/confirm/prompt | The page dialog stays pending; `accept` accepts or dismisses it and `prompt_text` fills a prompt |
| `console` | Read console output and page errors | Bounded buffer; `clear` empties it after reading |
| `network` | Read request, response and failure events | Bounded buffer; `clear` empties it after reading; response bodies are not included |
| `batch` | Run 1–10 dependent steps in order | Steps use strict kind schemas and cannot nest batch |

A common flow is `navigate` → `observe_refs` → interaction or `batch` → `wait` → `observe_text`. Ordinary DOM updates, SPA re-renders and same-URL refreshes do not kill a ref: the action re-resolves it from the recorded selector and waits up to about 0.8 s for a refreshed page to render it again. Re-observe only after a real navigation to another document where the selector is also gone.

The observation text skips links and icon buttons with no accessible name and merges repeated controls with the same role, name and state (for example the same navigation link twenty times). The omitted count is reported as `stats.duplicates` instead of listing every copy.

`resize` changes the page's logical viewport independently of the device screen. The Android system WebView scales that viewport uniformly inside its floating window, so `window.innerWidth/innerHeight` match the tool viewport. On Android, `network` records request-level events only (no response status or body); `console`, `dialog` and `file_upload` behave like desktop and cloud.

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

Model results are projected by action to the smallest shape needed for the next step. Observations retain the snapshot text, truncation state, and cursors; the text already names every ref, so the separate refs array is no longer repeated. Interactions retain the action, target ref, URL, and page generation, and carry `ref_recovered` when a ref survived a refresh by selector. Screenshots remove base64, become workspace attachments, and add a user message with the image so the model can view it. Events and audit retain the complete structured result.

Text cursors are signed and bound to the tab, page identity and a DOM revision digest. A page change or bridge restart rejects an old cursor and requires a fresh observation. The runtime does not keep an unbounded full-page cache.

Only http/https URLs are allowed. `file://` and intranet addresses are blocked by default. Snapshot and text observations default to 12,000 characters and allow up to 40,000; batches allow at most 10 steps, and the runtime also bounds sessions, tabs, downloads, and timeouts.

`evaluate`, raw JavaScript, browser status, and closing a browser session are control-plane operations, not model actions.
