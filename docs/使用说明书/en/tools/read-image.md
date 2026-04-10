---
title: Read Image
summary: The purpose and return structure of `read_image`.
read_when:
  - You need to send a local image into the current model context
source_docs:
  - src/services/tools/read_image_tool.rs
updated_at: 2026-04-10
---

# Read Image

`read_image` does one simple thing:  
it prepares a local image and prompts the system to append that image into the upcoming context as an additional message.

## Minimum arguments

```json
{
  "path": "screenshots/demo.png"
}
```

## Success result

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

## Key point

- The actual image content usually enters the conversation after this tool call as an extra user message
- So the tool result itself is not an image-analysis conclusion. It only means the image has been prepared for inspection
