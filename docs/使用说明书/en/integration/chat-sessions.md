---
title: Chat Sessions
summary: The session interfaces for building a chat product: creating sessions, sending and receiving messages, attachment preprocessing, resuming, and canceling all live in this domain.
updated_at: 2026-10-13
---

When what you are building is a chat product rather than the occasional one-off capability call, make the session interfaces your primary domain.

## Session Domain Use Cases

- One execution only, no history needed → [wunder API](/docs/en/integration/wunder-api/) is enough.
- Session list, history, resume, cancel, and a real-time workbench → use the session domain.

## Bringing Up a Session

1. Create a session and take note of its ID.
2. Run a text conversion on document attachments first.
3. Run media processing on audio and video attachments first.
4. Send the body text and/or attachments.
5. Consume real-time events through [Chat WebSocket](/docs/en/integration/chat-ws/) with start / resume / watch.
6. Fetch the session details to render the interface.
7. Cancel or compact the session when needed.

## Main Endpoints

Sessions:

- `GET/POST /wunder/chat/sessions`
- `GET/DELETE /wunder/chat/sessions/{session_id}`
- `POST /wunder/chat/sessions/{session_id}/messages`
- `GET /wunder/chat/sessions/{session_id}/events`
- `POST /wunder/chat/sessions/{session_id}/cancel`
- `POST /wunder/chat/sessions/{session_id}/compaction`
- `POST /wunder/chat/sessions/{session_id}/system-prompt`

Attachments:

- `POST /wunder/chat/attachments/convert`
- `POST /wunder/chat/attachments/media/process`

## How Attachments Flow

- Images: send them directly as attachments, and they go through visual context.
- Documents: convert them into text attachments first, then send.
- Audio: run media processing first to get a transcript, then submit the message.
- Video: run media processing first as well; the server splits it into an image sequence and an audio track attachment, and the original video is never sent straight to the model.

A message may carry "attachments only, no body text": as long as the attachments contain valid content or an accessible path, you can leave out the text.

## Key Settings and Common Pitfalls

- Prompt preview: the session domain can return prompt status and a memory preview, so the frontend can tell the user clearly whether the current state is "pending" or "frozen".
- The message body field is `content`, not the `question` used by `/wunder`.
- Judge session state by the runtime field, not only by the "running" flag kept for compatibility.
- Sessions and threads are not the same layer semantically, so handle them separately in the interface design.
- The chat input area preprocesses attachments for you; when you integrate the interface yourself, do the same before sending a message.

## Further Reading

- [Chat WebSocket](/docs/en/integration/chat-ws/)
- [wunder API](/docs/en/integration/wunder-api/)
- [Runtime and Online Presence](/docs/en/concepts/presence-and-runtime/)
