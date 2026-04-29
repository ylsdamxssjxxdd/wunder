---
title: 读图工具
summary: `read_image` 的用途与返回结构。
read_when:
  - 你要把本地视觉媒体送入当前模型上下文
source_docs:
  - src/services/tools/read_image_tool.rs
updated_at: 2026-04-29
---

# 读图工具

`read_image` 现在不只读静态图片。  
它会把本地视觉媒体预处理好，并提示系统在后续上下文里附上可供模型继续分析的视觉消息。

当前支持：

- 静态图片
- GIF 动图
- 本地视频

## 最小参数

```json
{
  "path": "screenshots/demo.png"
}
```

GIF 示例：

```json
{
  "path": "captures/demo.gif",
  "frame_step": 2
}
```

视频示例：

```json
{
  "path": "captures/demo.mp4",
  "frame_rate": 1
}
```

## 成功返回

```json
{
  "ok": true,
  "action": "read_image",
  "state": "completed",
  "summary": "Prepared visual media screenshots/demo.png for model inspection.",
  "data": {
    "path": "screenshots/demo.png",
    "resolved_path": "C:/.../screenshots/demo.png",
    "media_kind": "image",
    "size_bytes": 182233,
    "prompt": "Inspect the attached image carefully...",
    "result": {
      "kind": "image"
    }
  }
}
```

## 重点

- 静态图片会直接作为图片输入进入上下文。
- GIF 不会再原样送模型。
  默认只取首帧。
  如果提供 `frame_step`，则按间隔取帧。
- 视频会被规范化成图片帧序列，并在可用时附带音轨转写文本。
- 这个工具结果本身不是图像分析结论，只表示视觉媒体已经准备好，可以继续让模型分析。
