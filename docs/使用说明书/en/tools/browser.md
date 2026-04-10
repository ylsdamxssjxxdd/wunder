---
title: Browser
summary: The actions, return characteristics, and division of labor between browser automation and `web_fetch`.
read_when:
  - You need to open a webpage, click, type, take screenshots, or read dynamic pages
source_docs:
  - src/services/tools/browser_tool.rs
  - src/services/browser/runtime.rs
updated_at: 2026-04-10
---

# Browser

The most important property of the browser tool is this: **successful results mainly forward the browser-runtime payload instead of wrapping everything in a unified `summary/data` shell.**

Do not assume every browser action will look exactly like the standard tool envelope.

## When to use it

- the page is dynamically rendered
- you need to click, type, press keys, or wait
- you need browser screenshots
- `web_fetch` cannot extract meaningful content

## Common actions

- `status`
- `profiles`
- `start`
- `stop`
- `tabs`
- `open`
- `focus`
- `close`
- `navigate`
- `snapshot`
- `act`
- `screenshot`
- `read_page`
- shortcut actions: `click`, `type`, `press`, `hover`, `wait`

## Minimum argument examples

Navigate:

```json
{
  "action": "navigate",
  "browser_session_id": "sess_xxx",
  "url": "https://example.com"
}
```

Read the page:

```json
{
  "action": "read_page",
  "browser_session_id": "sess_xxx",
  "max_chars": 12000
}
```

## How to read its result shape

### `status`

This looks more like a runtime status snapshot:

```json
{
  "ok": true,
  "enabled": true,
  "tool_visible": true,
  "default_profile": "default",
  "profiles": ["default"],
  "limits": { ... },
  "playwright": { ... },
  "docker": { ... },
  "control": { ... },
  "sessions": ["sess_xxx"]
}
```

### `stop`

```json
{
  "ok": true,
  "closed": true,
  "browser_session_id": "sess_xxx"
}
```

### `screenshot`

The tool converts the bridge-layer `image_base64` into a real file and then returns download metadata. Typical fields include:

```json
{
  "ok": true,
  "filename": "browser_xxx.png",
  "download_url": "/wunder/temp_dir/download?...",
  "...": "other browser runtime fields"
}
```

### `read_page`, `snapshot`, `navigate`, `tabs`

The exact fields are defined by the browser bridge. In practice, they usually include at least `ok: true` plus action-specific data.

## Difference from `web_fetch`

- `web_fetch`: prefer this for lower-cost reading of static page content
- `browser`: prefer this for interaction, dynamic rendering, and automation

If you only need the main content of a public webpage, start with [Web Fetch](/docs/en/tools/web-fetch/).  
Only switch to the browser when the page depends on frontend rendering, verification steps, or real interaction.
