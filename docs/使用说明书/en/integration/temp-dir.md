---
title: Temp Directory and Document Conversion
summary: Temporary uploads, downloads, and transit all go through the temp directory; documents, audio, and video get one conversion or media pass before they enter a session.
updated_at: 2026-10-13
---

`temp_dir` is a transit layer, not a formal workspace. Don't treat it as long-term storage.

## Temp Directory vs. Workspace

- Temporary upload for the system to process → the temp directory.
- You need to send an external client a clickable download link → the temp directory.
- You need to turn a doc / pdf / ppt / xlsx into Markdown → run it through document conversion first.
- You need an artifact to persist over time → put it in the workspace, not the temp directory.

## Common Endpoints

- Document to Markdown: `POST /wunder/doc2md/convert`
- Convert (requires authentication): `POST /wunder/attachments/convert`
- Document attachments inside a session: `POST /wunder/chat/attachments/convert`
- Audio / video processing inside a session: `POST /wunder/chat/attachments/media/process`
- Download: `GET /wunder/temp_dir/download`
- Upload: `POST /wunder/temp_dir/upload`
- List: `GET /wunder/temp_dir/list`
- Remove: `POST /wunder/temp_dir/remove`

## How the Four Conversions Differ

| Endpoint | Where it is used | What comes out |
| --- | --- | --- |
| `POST /wunder/doc2md/convert` | Public document conversion | Markdown content |
| `POST /wunder/attachments/convert` | Debug panel, authentication integration testing | The same as above, but requires authentication |
| `POST /wunder/chat/attachments/convert` | Document attachments in the chat input area | Text attachments assembled for the session |
| `POST /wunder/chat/attachments/media/process` | Audio / video in the chat input area | Audio transcription results, or image frames plus an audio track split out of video |

Two additions: images generally do not need conversion — send them straight as session attachments. Video is never sent to the model directly; it is first split into an image sequence and an audio track, and re-extracting frames relies on the source file's public path.

## Why What You Get Back Is a Download Link

Many external clients cannot read wunder's internal workspace paths. So in outgoing content, internal paths are rewritten into clickable temporary download links that outside clients can actually open. During chat media processing the source file usually lands in the workspace public path first, and when it is handed out for external download it may still be swapped for a temporary download link.

## Key Settings and Common Pitfalls

- The temp directory is for transit and distribution, not for holding business files long-term.
- Before converting, tell apart whether it belongs to the public capability, the debug panel, or the chat input area.
- Audio / video go through media processing; do not feed raw video to the model as input.
- External channels can open files precisely because of this download link.

## Further Reading

- [Workspace API](/docs/en/integration/workspace-api/)
- [Channel Webhook](/docs/en/integration/channel-webhook/)
- [Data and Storage](/docs/en/ops/data-and-storage/)
