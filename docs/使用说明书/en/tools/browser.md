---
title: Browser
summary: The actions, return characteristics, and division of labor between browser automation and `web_fetch`.
read_when:
  - You need to open a webpage, click, type, take screenshots, or read dynamic pages
source_docs:
  - src/services/tools/browser_tool.rs
  - src/services/browser/runtime.rs
updated_at: 2026-07-02
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

Warm up a session:

```json
{
  "action": "start",
  "browser_session_id": "sess_xxx"
}
```

Open a slow page:

```json
{
  "action": "open",
  "browser_session_id": "sess_xxx",
  "url": "https://example.com",
  "timeout_ms": 60000
}
```

Navigate:

```json
{
  "action": "navigate",
  "browser_session_id": "sess_xxx",
  "url": "https://example.com",
  "timeout_ms": 60000
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
  "sessions": ["sess_xxx"]
}
```

The model-facing browser tool does not return local control endpoint fields. Admin HTTP status endpoints may include `control.host` / `control.port`; those are internal Wunder browser-control settings, not file download URLs, and models should not use them as browsing targets.

### `stop`

```json
{
  "ok": true,
  "closed": true,
  "browser_session_id": "sess_xxx"
}
```

### `screenshot`

When called by an agent tool, the bridge-layer `image_base64` is saved into the current agent workspace. The default path is `browser/screenshots/browser_shot_<id>.png`; pass `path` to choose another workspace-relative path. Typical fields include:

```json
{
  "ok": true,
  "filename": "browser_shot_xxx.png",
  "path": "browser/screenshots/browser_shot_xxx.png",
  "public_path": "/workspaces/<workspace_id>/browser/screenshots/browser_shot_xxx.png",
  "saved_to": "workspace",
  "...": "other browser runtime fields"
}
```

Direct HTTP calls to `/wunder/browser/screenshot` still write to `temp_dir` and return a `/wunder/temp_dir/download?...` URL for admin/debug workflows.

### `read_page`, `snapshot`, `navigate`, `tabs`

The exact fields are defined by the browser bridge. In practice, they usually include at least `ok: true` plus action-specific data.

## Difference from `web_fetch`

- `web_fetch`: prefer this for lower-cost reading of static page content
- `browser`: prefer this for interaction, dynamic rendering, and automation

If you only need the main content of a public webpage, start with [Web Fetch](/docs/en/tools/web-fetch/).  
Only switch to the browser when the page depends on frontend rendering, verification steps, or real interaction.

Browser actions default to a 60-second timeout. For pages that often time out, call `start` first to warm up or reuse the session, then call `open` or `navigate` with `timeout_ms` or `timeout_secs` when a single call needs a different timeout.
