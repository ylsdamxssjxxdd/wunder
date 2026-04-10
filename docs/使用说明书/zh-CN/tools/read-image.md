---
title: 读图工具
summary: `read_image` 的用途与返回结构。
read_when:
  - 你要把本地图片送入当前模型上下文
source_docs:
  - src/services/tools/read_image_tool.rs
updated_at: 2026-04-10
---

# 读图工具

`read_image` 做的事很简单：  
把本地图片准备好，并提示系统在后续上下文里附上一条图片消息。

## 最小参数

```json
{
  "path": "screenshots/demo.png"
}
```

## 成功返回

```json
{
  "ok": true,
  "action": "read_image",
  "state": "completed",
  "summary": "Prepared image screenshots/demo.png for model inspection.",
  "data": {
    "path": "screenshots/demo.png",
    "resolved_path": "C:/.../screenshots/demo.png",
    "mime_type": "image/png",
    "size_bytes": 182233,
    "prompt": "Inspect the attached image carefully..."
  }
}
```

## 重点

- 真正的图片内容通常会在这次工具调用之后，以额外 user message 的形式进上下文
- 所以这个工具结果本身不是图像分析结论，只是“图片已准备好”
