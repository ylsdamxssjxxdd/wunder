---
title: Multimodal Generation
summary: Model dependencies, arguments, artifact landing rules, returned fields, and failure handling for the speech-generation / speech-to-text / image-generation / video-generation tools.
updated_at: 2026-11-13
---

# Multimodal Generation

| Canonical name | Alias | Purpose |
| --- | --- | --- |
| `语音生成` | `generate_speech` | Read text aloud into audio (narration, announcements, accessibility playback) |
| `声转文` | `transcribe_speech` | Turn audio in the workspace into text (meeting recordings, voice messages) |
| `图像生成` | `generate_image` (the old name `绘图生成` remains an alias) | Text-to-image, or edit / redraw an input image |
| `视频生成` | `generate_video` | Generate a short video from text |

## Model dependencies

These four are **not members of the default tool set**. Whether they appear on the capability surface depends on whether a model of the corresponding type is configured:

| Tool | Required model type |
| --- | --- |
| `语音生成` | Speech synthesis (TTS) |
| `声转文` | Speech recognition (ASR) |
| `图像生成` | Image generation |
| `视频生成` | Video generation |

You can name a specific model profile or use the default for that type; if neither resolves, the tool is unavailable. See the model configuration in settings for profiles and types.

## Arguments

### Speech generation

- Required: the text to synthesize (must not be blank after trimming).
- Optional: output path, model, voice, style instruction, response format, speed.
- For voice cloning, provide a reference audio clip (a workspace path, or a direct audio URL / path) together with its reference text — both must be supplied together or it errors; an empty reference audio errors immediately.

### Speech to text

- Required: an audio path inside the workspace (a normal path or a public path, one of the two); the file must exist and be a regular file.
- Optional: model, language hint, transcription prompt, output format, sampling temperature, upload filename and content type.

### Image generation

- Required: the prompt (must not be blank after trimming).
- Optional: output path, model, size, output format, negative prompt, sampling parameters.
- Providing an input image switches to edit mode; you can also provide a mask, reference images, and provider-specific advanced parameters passed through as-is.
- **Size alignment**: some providers require both width and height to be divisible by 16; otherwise the call fails before dispatch and suggests a size.

### Video generation

- Required: the prompt (must not be blank after trimming).
- Optional: output path, model, resolution, duration, frame rate / frame count, negative prompt, sampling parameters, frame-interpolation toggle.

Video generation is issued in synchronous mode.

## Artifact paths

The three generation tools write artifacts into the current workspace:

- An explicit output path → write to that relative path; add an extension based on the response type when missing.
- No output path → `generated_media/<type>_<UTC timestamp>.<ext>`, where the type prefix is `speech` / `image` / `video`.
- Default extensions: speech `wav`, image `png`, video `mp4`.
- After writing, return the external presentation path and the workspace-relative path.

`声转文` does not write to disk; it returns text only.

## Return structure

All four tools use the shared envelope on success, with artifact paths, byte counts, and content types: generation tools carry the output path and workspace-relative path; speech-to-text carries the transcript and the raw response.

## Errors and handling

- **`图像生成` does not throw**: an unconfigured model, an invalid size, or a provider error all become a structured failure result carrying the provider, model name, mode, size, input image, failure summary, and "suggested size", so parameters can be fixed directly.
- The other three use the normal error path; common cases include empty text / prompt, a missing or empty audio source file, reference text without reference audio, advanced parameters that are not a JSON object, and an unconfigured video model.
- For retryable failures such as timeouts and upstream busy, narrow the scope or reduce steps before retrying; unconfigured models, invalid sizes, and malformed requests are not retryable — fix the parameters as indicated.

## Limits

- Do not use it to generate "temporary decoration images" for an answer: artifacts land in the workspace and consume storage.
- There is no declared length cap on the argument surface: text and prompts are bounded by the upstream model API, and the platform does not truncate them for you.
- Artifacts are workspace-relative paths; read them back with [Read file](/docs/en/tools/workspace-files/) or [Read image](/docs/en/tools/read-image/). Do not read audio or video directly into the context.
- In the Cloud form, generation and landing are also constrained by the sandbox and workspace ownership; they write into the current user's directory.
- Generation calls are generally slow and expensive: call once per task, adjust parameters as indicated on failure, and do not loop over the same request.

## Related

- [Tools](/docs/en/tools/index/) — the built-in tool matrix and shared result shell
- [Read image](/docs/en/tools/read-image/) — how images and video enter the context
- Model configuration in settings — profiles and types
