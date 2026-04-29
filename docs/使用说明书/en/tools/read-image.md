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

`read_image` prepares local visual media and prompts the system to append the derived visual context into the upcoming conversation as an additional message.

It now supports:

- static images
- animated GIFs
- local videos

## Minimum arguments

```json
{
  "path": "screenshots/demo.png"
}
```

GIF example:

```json
{
  "path": "captures/demo.gif",
  "frame_step": 2
}
```

Video example:

```json
{
  "path": "captures/demo.mp4",
  "frame_rate": 1
}
```

## Success result

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

## Key point

- Static images are forwarded as image input.
- GIFs are normalized before entering the model context.
  Default behavior is first frame only.
  Set `frame_step` to sample frames by interval.
- Videos are normalized into image frames and optional audio transcript context.
- The tool result itself is not the analysis conclusion. It only means the visual media has been prepared for inspection.
