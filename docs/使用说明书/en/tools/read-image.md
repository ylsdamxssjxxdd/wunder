---
title: Read Image
summary: The `read_image` path and frame-sampling arguments, how static images, GIFs, and video enter context, and the returned fields.
updated_at: 2026-10-16
---

# Read Image

Read Image sends local material into the current conversation by path: screenshots, photos, and design drafts, as well as animated GIFs and local video.

## Arguments

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

## Handling by material type

- A static image enters the context as image input unchanged; the model only needs the path, and the intent to look comes from the current conversation.
- A GIF is not handed to the model whole: by default only the first frame is taken, and if you give a frame interval it samples frames at that interval.
- A video is organized into a series of image frames; if the video has an audio track, the transcript is attached as well.

## Return value

The result states the material's type and size, but that only means the material is ready, not that the model has analyzed it: once it is ready, ask about it.

## Errors

Look at the result first: whether the path resolved to a real file, whether the material type is right, and whether the byte size is what you expect. A wrong path or an unsupported material type reports an error directly.
