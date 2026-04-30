---
title: Chat Sessions
summary: `/wunder/chat/sessions/*` is Wunder's primary chat domain, responsible for session lifecycle, message sending/receiving, attachment preprocessing, event recovery, and runtime management.
read_when:
  - You are building chat UI, desktop sessions, or messaging workbench
  - You want to upgrade from "capability invocation" to "session productization"
source_docs:
  - docs/API文档.md
  - src/api/chat.rs
---

# Chat Sessions

When you need session-level capabilities, use `/wunder/chat/sessions/*` as the primary domain instead of just calling `/wunder`.

## What This Group of Interfaces Is Responsible For

- Creating, listing, and deleting sessions
- Sending messages and consuming events
- Resuming, canceling, and compacting sessions
- Reading session runtime and system prompt snapshots

## High-Frequency Interfaces

- `GET/POST /wunder/chat/sessions`
- `GET/DELETE /wunder/chat/sessions/{session_id}`
- `POST /wunder/chat/attachments/convert`
- `POST /wunder/chat/attachments/media/process`
- `POST /wunder/chat/sessions/{session_id}/messages`
- `GET /wunder/chat/sessions/{session_id}/events`
- `POST /wunder/chat/sessions/{session_id}/cancel`
- `POST /wunder/chat/sessions/{session_id}/compaction`
- `POST /wunder/chat/sessions/{session_id}/system-prompt`

## Typical Flow

1. `POST /wunder/chat/sessions` to create a session
2. Document attachments go through `POST /wunder/chat/attachments/convert` first
3. Audio/video attachments go through `POST /wunder/chat/attachments/media/process` first
4. `POST /wunder/chat/sessions/{session_id}/messages` to send body text and/or attachments
5. Use [Chat WebSocket](/docs/en/integration/chat-ws/) `start / resume / watch` to consume real-time events
6. Use `GET /wunder/chat/sessions/{session_id}` to render session details
7. Call `cancel` or `compaction` when needed

## Recommended Attachment Flow

- Images: Can be sent directly as `attachments`, using visual context.
- Documents: First go through `/wunder/chat/attachments/convert` to convert documents into text-based attachments.
- Audio: First go through `/wunder/chat/attachments/media/process`, then submit the message after getting transcription results.
- Video: Also goes through `/wunder/chat/attachments/media/process`; the server will first extract image sequences and audio track attachments; the original video will not be sent directly to the model.

Message submission allows "attachments only, no body text". As long as `attachments[]` contains valid `content` or `public_path`, you don't need to pass a text body.

## Relationship with `/wunder`

- `/wunder`: Unified execution entry point
- `/wunder/chat/*`: Session control and productization interfaces

If you are building a chat product, it's recommended to use the chat domain as primary, with `/wunder` as a supplementary capability entry point.

## Why Prompt Preview Matters

The chat domain provides:

- `/wunder/chat/system-prompt`
- `/wunder/chat/sessions/{session_id}/system-prompt`

These can return prompt status and memory preview fields, allowing the frontend to clearly indicate whether the current state is `pending` or `frozen`.

## Common Pitfalls

- The message field is `content`, not `question`.
- For session status, prioritize checking `runtime`, not just the compatibility field `running`.
- Session capabilities and thread capabilities are not semantically equivalent; UI design should handle them separately.
- Chat input areas automatically preprocess attachments; if you're integrating the API yourself, you also need to do this before sending messages.

## Further Reading

- [Chat WebSocket](/docs/en/integration/chat-ws/)
- [wunder API](/docs/en/integration/wunder-api/)
- [Runtime and Online Presence](/docs/en/concepts/presence-and-runtime/)
