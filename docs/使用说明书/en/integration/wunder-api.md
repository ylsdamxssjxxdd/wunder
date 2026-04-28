---
title: "wunder API"
summary: "`/wunder` is the unified execution entry point, suitable for capability calls; when building a complete chat product, prefer `/wunder/chat/*`."
read_when:
  - "You need to integrate Wunder from a business system"
  - "You need to decide between `/wunder` and `/wunder/chat/*`"
source_docs:
  - "docs/API文档.md"
  - "docs/设计文档/01-系统总体设计.md"
---

# wunder API

`POST /wunder` is suitable for "calling Wunder as an execution capability".

If you need session lists, resumption, cancellation, or real-time workbench, it's recommended to go directly to the chat domain: `/wunder/chat/*`.

## When to Prefer `/wunder`

- One request triggers one execution
- Caller only needs `user_id + question` semantics
- Need quick integration with streaming output support (SSE)

## When Not to Use Only `/wunder`

- You need complete session lifecycle management
- You need stable agent binding and read frozen prompts
- You want to build chat product-level UI (history, resume, cancel, observe)

## Recommended Integration Path (Stable Agent Binding)

`GET /wunder/agents` -> `POST /wunder/chat/sessions` -> `POST /wunder/chat/sessions/{session_id}/messages`

1. Use `GET /wunder/agents` to get target `agent_id`
2. Use `POST /wunder/chat/sessions` to create a session and bind an agent
3. Use `POST /wunder/chat/sessions/{session_id}/messages` to send messages
4. Use WS or `resume` to consume streaming events

## Request Fields (Minimum Set)

```json
{
  "user_id": "demo_user",
  "question": "Help me organize today's work plan",
  "stream": true
}
```

Common additional fields:

- `session_id`: Reuse session
- `agent_id`: Specify agent scope
- `model_name`: Temporarily override model
- `tool_names`: Explicitly mount tools
- `config_overrides`: Partial config override
- `attachments`: Attachment input (images can be passed directly; documents, audio, video should be preprocessed first)

## Don't Throw Attachments Directly to `/wunder`

The official frontend now handles attachments with different paths:

- Images: Can be directly used as visual context in `attachments`
- Documents: Usually go through `/wunder/chat/attachments/convert` or `/wunder/doc2md/convert` first, converted to text before submission
- Audio: Go through `/wunder/chat/attachments/media/process` first to convert speech to text attachments
- Video: Go through `/wunder/chat/attachments/media/process` first, split into image sequences and audio track attachments; raw video is not sent directly to the model

If you bypass these preprocessing steps and call `/wunder` directly, you are responsible for attachment decomposition, transcription, frame extraction, and size control.

## Identity Field Notes

- `user_id` represents the caller and isolation space, does not require a registered user.
- `agent_id` represents the target agent.

## Common Misconceptions

- `/wunder` supporting `agent_id` doesn't mean it's equivalent to full chat domain capabilities.
- If calling `/wunder` directly, many session governance capabilities need to be implemented by yourself.
- Misunderstanding `user_id` as "must come from user management table" leads to unnecessary integration restrictions.

## Related Interfaces

- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`
- `GET /wunder/agents`
- `POST /wunder/chat/sessions`
- `POST /wunder/chat/sessions/{session_id}/messages`

## Further Reading

- [Chat Sessions](/docs/en/integration/chat-sessions/)
- [Chat WebSocket](/docs/en/integration/chat-ws/)
- [API Index](/docs/en/reference/api-index/)