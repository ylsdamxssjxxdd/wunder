---
title: 桌面控制
summary: `desktop_controller` 与 `desktop_monitor` 的最新行为和返回结构。
read_when:
  - 你要控制本机桌面或观察桌面截图
source_docs:
  - src/services/tools/desktop_control.rs
updated_at: 2026-04-10
---

# 桌面控制

这一组工具是：

- `desktop_controller`
- `desktop_monitor`

它们成功时走统一骨架，但 `data` 里会带截图路径、下载地址和 follow-up prompt。  
随后系统还可能自动补一条带图片的后续 user message，供模型继续看图。

## `desktop_controller`

### 最小参数

```json
{
  "bbox": { "x": 100, "y": 100, "width": 80, "height": 40 },
  "action": "left_click",
  "description": "点击保存按钮"
}
```

### 成功返回

```json
{
  "ok": true,
  "action": "desktop_controller",
  "state": "completed",
  "summary": "Completed desktop action left_click.",
  "data": {
    "action": "left_click",
    "description": "点击保存按钮",
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

### 最小参数

```json
{
  "wait_ms": 1000,
  "note": "等页面稳定后再截图"
}
```

### 成功返回

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
    "note": "等页面稳定后再截图"
  }
}
```

## 注意点

- 这是本机桌面，不是网页 DOM
- 每次动作后都会尽量回传新截图
- 前后截图可能一起进入后续多模态判断

## 与浏览器的区别

- 网页 DOM 级操作：用 [浏览器](/docs/zh-CN/tools/browser/)
- 整个桌面与原生应用操作：用桌面控制
