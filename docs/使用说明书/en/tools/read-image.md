---
title: Read Image
summary: The `read_image` path, frame (index/step/sheet) and comparison arguments, how static images, GIFs, and video enter context, and the returned fields.
updated_at: 2026-11-20
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

For a GIF you can read one exact frame; indices are 0-based and match the numbers stamped on contact-sheet cells:

```json
{
  "path": "captures/pelican.gif",
  "frame": 12
}
```

To read several frames at once, pass an array (up to 8):

```json
{
  "path": "captures/pelican.gif",
  "frames": [0, 6, 12]
}
```

Sampling by interval still works; for video you set how many frames to take per second:

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

To see the whole motion first, pass `sheet`: it works for GIFs and video and returns one labelled contact sheet of at most 4 columns and 16 cells. For a long animation, `frame_start` pages the sheet from a later frame.

```json
{
  "path": "captures/demo.mp4",
  "sheet": true
}
```

## Handling by material type

- A static image enters the context as image input unchanged; the model only needs the path, and the intent to look comes from the current conversation.
- A GIF is not handed to the model whole: without frame arguments it returns a labelled contact sheet (no longer just the first frame). `frame` / `frames` read exact frames, `frame_step` samples at an interval.
- A video is organized into a series of image frames; if the video has an audio track, the transcript is attached as well.
- With `sheet: true` the frames are not returned one by one: the whole clip is sampled into a single contact sheet. For video the sampling is capped at 16 frames, cells are laid out in rows, and each frame is scaled to fit without stretching.
- Cell aspect follows the real frame aspect (16:9 stays 16:9), so no picture band is cropped to fit a fixed cell; each cell is stamped with its 0-based frame index.

## Comparing two images

To judge how a new version differs from the previous one, put the other image in `compare_with`:

```json
{
  "path": "renders/v2.png",
  "compare_with": "renders/v1.png"
}
```

The default is one side-by-side image. Add `compare_mode: "diff"` to get three panels (left image, red diff heatmap, right image) plus SSIM, mean absolute difference, and the changed-pixel ratio:

```json
{
  "path": "renders/v2.png",
  "compare_with": "renders/v1.png",
  "compare_mode": "diff"
}
```

GIF frames compare too: pick the left frame with `frame` and, when the right side is also an animation, pick its frame with `compare_frame`.

## Return value

The result states the material's type and size, but that only means the material is ready, not that the model has analyzed it: once it is ready, ask about it.

A contact sheet also reports `sheet.columns`, `sheet.rows`, `sheet.cell_width`, `sheet.cell_height`, and `sheet.frame_indices`, the row-major source frame index of each cell; for video the `warnings` also state roughly which second cell k falls on.

Reading by frame index reports `frame_scope`: total frames, total duration, and each frame's index, timestamp, and saved path. A diff comparison reports `compare.metrics` (`ssim`, `mean_abs_diff`, `changed_ratio`).

## Errors

Look at the result first: whether the path resolved to a real file, whether the material type is right, and whether the byte size is what you expect. A wrong path or an unsupported material type reports an error directly.

A crop (`crop` / `crop_xyxy`) that runs past the image edge is clamped to the boundary and the actual region is reported; only a crop that starts completely outside the image is an error.
