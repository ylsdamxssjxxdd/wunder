---
title: Desktop Control
summary: The latest behavior and return structures of `desktop_controller` and `desktop_monitor`.
read_when:
  - You need to control the local desktop or inspect desktop screenshots
source_docs:
  - src/services/tools/desktop_control.rs
updated_at: 2026-04-10
---

# Desktop Control

This tool group contains:

- `desktop_controller`
- `desktop_monitor`

They use the unified success envelope, but `data` includes screenshot paths, download URLs, and follow-up prompts.  
After a call completes, the system may also append a follow-up user message with the captured image so the model can continue with multimodal inspection.

## `desktop_controller`

### Minimum arguments

```json
{
  "bbox": { "x": 100, "y": 100, "width": 80, "height": 40 },
  "action": "left_click",
  "description": "Click the Save button"
}
```

### Success result

```json
{
  "ok": true,
  "action": "desktop_controller",
  "state": "completed",
  "summary": "Completed desktop action left_click.",
  "data": {
    "action": "left_click",
    "description": "Click the Save button",
    "center_norm": [640, 360],
    "center_screen": [960, 540],
    "normalized_width": 1280,
    "normalized_height": 720,
    "screen_width": 1920,
    "screen_height": 1080,
    "screenshot_path": "C:/.../desktop_controller/a.png",
    "previous_screenshot_path": null,
    "screenshot_download_url": "/wunder/temp_dir/download?...",
    "screenshot_bytes": 182233,
    "elapsed_ms": 742,
    "followup_prompt": "..."
  }
}
```

## `desktop_monitor`

### Minimum arguments

```json
{
  "wait_ms": 1000,
  "note": "Wait for the page to settle before capturing"
}
```

### Success result

```json
{
  "ok": true,
  "action": "desktop_monitor",
  "state": "completed",
  "summary": "Captured a desktop screenshot for inspection.",
  "data": {
    "wait_ms": 1000,
    "normalized_width": 1280,
    "normalized_height": 720,
    "screen_width": 1920,
    "screen_height": 1080,
    "screenshot_path": "C:/.../desktop_controller/b.png",
    "previous_screenshot_path": "C:/.../desktop_controller/a.png",
    "screenshot_download_url": "/wunder/temp_dir/download?...",
    "screenshot_bytes": 191002,
    "followup_prompt": "...",
    "note": "Wait for the page to settle before capturing"
  }
}
```

## Notes

- This operates on the local desktop, not the webpage DOM
- After each action, the tool tries to return a fresh screenshot
- Previous and current screenshots may both enter the next multimodal reasoning step

## Difference from the browser tool

- For webpage DOM-level operations, use [Browser](/docs/en/tools/browser/)
- For the full desktop and native apps, use desktop control
