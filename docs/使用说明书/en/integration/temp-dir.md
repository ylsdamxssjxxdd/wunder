---
title: Temp Directory and Document Conversion
summary: `/wunder/temp_dir/*` handles temporary upload, download, and transit; document conversion and chat attachment preprocessing are handled by separate public conversion, debug conversion, and chat-domain conversion endpoints.
read_when:
  - You need to send download links to external systems
  - You need to distinguish the responsibilities of workspace, temp_dir, and doc2md
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/api/temp_dir.rs
  - src/api/doc2md.rs
  - src/api/chat.rs
---

# Temp Directory and Document Conversion

In Wunder, `temp_dir` is a transit layer, not a formal workspace.

## What This Page Covers

This page explains only four things:

- What should go into `temp_dir`
- When you should run files through document conversion first
- How chat-domain attachment preprocessing endpoints differ
- Why many external channels ultimately receive `/wunder/temp_dir/download` links

## Most Commonly Used Endpoints

- `POST /wunder/doc2md/convert`
- `POST /wunder/attachments/convert`
- `POST /wunder/chat/attachments/convert`
- `POST /wunder/chat/attachments/media/process`
- `GET /wunder/temp_dir/download`
- `POST /wunder/temp_dir/upload`
- `GET /wunder/temp_dir/list`
- `POST /wunder/temp_dir/remove`

## When to Use These Endpoints

- You need to temporarily upload a file for system processing
- You need to send a clickable download link to an external client
- You need to convert a doc/pdf/ppt/xlsx file to Markdown first
- You are building debug panel attachment parsing
- You are preprocessing document, audio, or video attachments in the chat input area

## How the Four High-Frequency Endpoints Differ

You can remember them like this:

| Endpoint | Target Audience | Typical Output |
|----------|----------------|----------------|
| `/wunder/doc2md/convert` | Public document conversion | Markdown content |
| `/wunder/attachments/convert` | Debug panel / auth integration testing | Same as `doc2md`, but requires authentication |
| `/wunder/chat/attachments/convert` | Chat input area document attachments | Text-type `attachments` for chat domain assembly |
| `/wunder/chat/attachments/media/process` | Chat input area audio / video attachments | Audio transcription results, or extracted image frames + audio track attachments from video |

Two additional notes:

- Images generally do not go through these conversion endpoints -- they can be sent directly as chat attachments.
- Video is never sent directly to the model; it is first split into an image sequence and an audio track. Re-extracting frames relies on `source_public_path`.

## Why Many Files End Up as `temp_dir` Download Links

Because many external clients do not understand Wunder's internal workspace paths.

So the system rewrites:

- `/workspaces/...`

into:

- `/wunder/temp_dir/download?...`

This way channel clients or external web pages can actually open the link.

During chat media preprocessing, source files typically land in the workspace public path first; when it is time to serve them to external clients for download, the system may still rewrite them as `temp_dir` download links.

## Common Misconceptions

### Treating `temp_dir` as Long-Term Storage

Incorrect.

It is a transit area, not a long-term business data store.

### Treating Converted Markdown as the Workspace Master File

Not necessarily.

First determine whether you need the file for "temporary consumption" or "ongoing processing."

### Assuming `temp_dir` Is Only for the Admin Panel

Also incorrect.

It is a formal public transit layer used by many external channel workflows.

## Implementation Guidelines

- `temp_dir` is suitable for transit and distribution, not for long-term business file storage.
- For document conversion, first determine whether it falls under public capability, debug panel, or chat input area.
- Audio / video attachments should go through `POST /wunder/chat/attachments/media/process` -- do not feed raw video directly as model input.
- External channels can open files via clickable links, which typically rely on `temp_dir` download links.

## Further Reading

- [Workspace API](/docs/en/integration/workspace-api/)
- [Channel Webhook](/docs/en/integration/channel-webhook/)
- [Data and Storage](/docs/en/ops/data-and-storage/)
