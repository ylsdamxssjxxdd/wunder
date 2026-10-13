---
title: Read Image
summary: Send a local image, animated GIF, or video into the current conversation so the agent can look at the material directly.
updated_at: 2026-10-13
---

Read Image sends a local image, animated GIF, or video into the current conversation.

## When to use it

- You have a screenshot, photo, or design draft and want the agent to read what is in it.
- You have an animated GIF or a video and want a rough, frame-by-frame look.

## How to use it

Pass a file path:

```json
{
  "path": "screenshots/demo.png"
}
```

For a GIF you can add a frame interval; for a video you can set how many frames to take per second:

```json
{
  "path": "captures/demo.gif",
  "frame_step": 2
}
```

```json
{
  "path": "captures/demo.mp4",
  "frame_rate": 1
}
```

## What it does automatically

- A static image enters the context as image input unchanged; the model only needs the path, and the intent to look comes from the current conversation.
- A GIF is not handed to the model whole: by default only the first frame is taken, and if you give a frame interval it samples frames at that interval.
- A video is organized into a series of image frames; if the video has an audio track, the transcript is attached as well.
- The result states the material's type and size, but that only means the material is ready, not that the model has analyzed it: once it is ready, ask about it.

## Diagnosing problems

Look at the result first: whether the path resolved to a real file, whether the material type is right, and whether the byte size is what you expect. A wrong path or an unsupported material type reports an error directly.
