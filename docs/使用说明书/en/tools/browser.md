---
title: Browser
summary: The 17 native browser-automation tools, their minimum arguments, return shape (content blocks), and the division of labor with `web_fetch`.
read_when:
  - You need to open a webpage, click, type, take screenshots, or read dynamic pages
source_docs:
  - src/services/tools/browser_tool.rs
  - src/services/browser/provider.rs
  - src/services/browser/providers/playwright.rs
  - src/services/browser/runtime.rs
updated_at: 2026-10-10
---

# Browser

Browser automation is exposed as a set of **native tools** (the tool surface matches the upstream harness), not as a single tool with an `action` switch. Every action is its own tool name, and the model calls it directly.

The tools are driven by a `BrowserProvider`; the default implementation is `playwright` (reusing the existing Playwright bridge engine). Switch providers via the `browser.provider` config key; an unknown provider falls back to the default and logs a warning.

## Tool list

| Tool | Purpose | Minimum arguments |
| --- | --- | --- |
| `browser_navigate` | Navigate the current tab to a URL | `url` |
| `browser_navigate_back` | Go back | none |
| `browser_tabs` | List / open / select / close tabs | `action` (`list`/`new`/`select`/`close`) |
| `browser_snapshot` | Capture an accessibility snapshot with element refs for click/type | none |
| `browser_click` | Click an element | `ref` or `selector` |
| `browser_type` | Type text into an element | `text` (with `ref`/`selector`) |
| `browser_hover` | Hover over an element | `ref` or `selector` |
| `browser_select_option` | Select option(s) in a `<select>` | `ref`/`selector` + `value` |
| `browser_drag` | Drag one element onto another | `from_ref`/`from_selector` + `to_ref`/`to_selector` |
| `browser_press_key` | Press a key, e.g. `Enter`, `Tab`, `ArrowDown` | `key` |
| `browser_evaluate` | Evaluate a JS expression in the page context and return its value | `expression` |
| `browser_wait_for` | Wait for a duration, a load state, or text to appear | one of `wait_ms` / `load_state` / `text` |
| `browser_take_screenshot` | Take a screenshot and return it as an image block | none (optional `path`, `full_page`) |
| `browser_read_page` | Read the current page as Markdown / text | none (optional `max_chars`) |
| `browser_batch` | Run a batch of interaction steps in order (max 10) | `steps` |
| `browser_close` | Close the browser session for this conversation | none |
| `browser_status` | Report runtime status and open sessions | none |

Multi-tab actions (`browser_navigate_back`, `browser_click`, `browser_type`, and so on) target the active tab by default; pass `target_id` to target a specific tab.

## When to use it

- the page is dynamically rendered
- you need to click, type, press keys, or wait
- you need browser screenshots
- `web_fetch` cannot extract meaningful content

## Minimum argument examples

Navigate:

```json
{
  "url": "https://example.com"
}
```

Take a snapshot, then click (the ref comes from the snapshot):

```json
{
  "ref": "e12"
}
```

Type and submit:

```json
{
  "ref": "e3",
  "text": "hello",
  "submit": true
}
```

Read the page:

```json
{
  "max_chars": 12000
}
```

Screenshot to a workspace-relative path:

```json
{
  "path": "browser/screenshots/home.png",
  "full_page": true
}
```

Run several interactions in order:

```json
{
  "steps": [
    { "kind": "click", "ref": "e1" },
    { "kind": "type", "ref": "e2", "text": "wunder" }
  ]
}
```

## How to read the result shape

A successful result uses an MCP-style content-block structure and carries an extra `provider` field identifying the provider actually used:

```json
{
  "provider": "playwright",
  "ok": true,
  "content": [
    { "type": "text", "text": "Navigated to https://example.com." }
  ],
  "data": { "...": "runtime data passed through by the provider" }
}
```

- `content`: array of content blocks. A text block is `{ "type": "text", "text": "..." }`; an image block is `{ "type": "image", "data": "<base64>", "mime_type": "image/png" }`.
- `error`: present on failure.
- `data`: the raw runtime payload passed through by the provider; fields depend on the action.
- Long text payloads are truncated, and the text is marked as truncated.

### `browser_status`

This looks more like a runtime status snapshot; `data` usually contains:

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

The model-facing browser tool does not return local control endpoint fields. Admin HTTP status endpoints may include `control.host` / `control.port`; those are internal wunder browser-control settings, not file download URLs, and models should not use them as browsing targets.

### `browser_take_screenshot`

The screenshot is returned as an image content block, and the bridge-layer image is also saved into the current agent workspace. The default path is `browser/screenshots/browser_shot_<id>.png`; pass `path` to choose another workspace-relative path (a `.png` extension is added automatically if missing).

Direct HTTP calls to `/wunder/browser/screenshot` still write to `temp_dir` and return a `/wunder/temp_dir/download?...` URL for admin/debug workflows.

### Other actions

The exact fields for `browser_read_page`, `browser_snapshot`, `browser_navigate`, `browser_tabs`, and the rest are defined by the provider / bridge. In practice they include at least `ok: true` plus action-specific data, all inside `data`.

## Runtime and dependencies (aligned with dsh)

The browser tools are driven by the `playwright` provider; the bridge process uses Python Playwright to launch or adopt a browser. Use `browser.playwright` to choose the browser source, mirroring dsh's `launch` / `attach` modes:

```yaml
browser:
  provider: playwright
  playwright:
    mode: launch            # launch=new controlled browser; attach=adopt a running browser
    channel: null           # reuse a locally installed browser: chrome / msedge / chromium ...
    executable_path: null   # explicit browser executable (takes precedence over channel)
    attach_endpoint: null   # CDP endpoint for mode=attach (http(s)://host:port or ws://...)
    browsers_path: null     # empty = auto-detect <python_root>/ms-playwright (supplement location)
    headless: true
    viewport_width: 1280
    viewport_height: 720
    timeout_secs: 60
```

- `mode: launch` (default): start a new controlled browser. **With the defaults (`channel` and `executable_path` left empty) the Chromium bundled inside the supplement is used**, so browser tasks never depend on the user's own browser; set `channel` or `executable_path` only when you want to reuse a locally installed browser.
- `mode: attach`: connect to an already-running browser (`attach_endpoint`) and reuse its tabs and login state; wunder only adopts pages and will not close the user's own browser.
- `browser_status` echoes `launch_mode` / `channel` / `executable_path` under `data.playwright`, and reports `attach_endpoint_configured` as a boolean (the raw endpoint is never returned, to avoid leaking embedded tokens).

Dependencies:

- Requires the Python `playwright` driver package. The desktop supplement pins `playwright` in `requirements-*-common.txt`; the Linux supplement also downloads **Chromium** into `opt/python/ms-playwright` and the bridge auto-detects it as the default browser (leave `browsers_path` empty), so browser tasks run offline out of the box without the user installing any browser.
- The supplement bundles **Chromium** (open source), not the branded Google Chrome build: Chrome binaries are not redistributable under their license, whereas Chromium shares the same engine and can be redistributed freely.
- The Win7 supplement ships the driver only, not Chromium (Playwright's bundled Chromium needs Windows 10+, which Win7 cannot run). On Win7, reuse a locally installed browser via `mode: attach` or `channel` / `executable_path`; a Windows 10+ x64 supplement would be required to ship a browser for Windows out of the box.

## Difference from `web_fetch`

- `web_fetch`: prefer this for lower-cost reading of static page content
- browser tools: prefer these for interaction, dynamic rendering, and automation

If you only need the main content of a public webpage, start with [Web Fetch](/docs/en/tools/web-fetch/).  
Only switch to the browser when the page depends on frontend rendering, verification steps, or real interaction.

Browser actions default to a 60-second timeout. For pages that often time out, reuse an existing session first, then call `browser_navigate` with `timeout_ms` when a single call needs a different timeout.