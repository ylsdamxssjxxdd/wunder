---
title: wunder API
summary: Call wunder as an execution capability — one request, one result; move to the chat domain when you want a full chat product.
updated_at: 2026-10-13
---

`POST /wunder` suits "calling wunder as an execution capability". For a session list, resume, cancel, or a live workbench, go straight to the chat domain.

## /wunder vs. the Chat Domain

- One request triggers one execution and you only care about the result → use `/wunder`.
- You need history, resume, cancel, stable agent binding, or to read a frozen prompt → use [chat sessions](/docs/en/integration/chat-sessions/).

## Stable Agent Binding

`GET /wunder/agents` → `POST /wunder/chat/sessions` → `POST /wunder/chat/sessions/{session_id}/messages`

1. Get the target agent ID first;
2. Create a session and bind that agent;
3. Send a message;
4. Consume streaming events over WebSocket or the resume endpoint.

## Minimal Request

```json
{
  "user_id": "demo_user",
  "question": "Help me organize today's work plan",
  "stream": true
}
```

Common additions:

- `session_id`: reuse a session
- `agent_id`: specify the agent
- `model_name`: temporarily switch the model
- `tool_names`: explicitly mount tools
- `config_overrides`: partial configuration override
- `attachments`: attachment input

## Don't Dump Attachments Straight In

The official frontend splits attachments by kind:

- Images: can be sent directly as visual context.
- Documents: convert to text first, then submit.
- Audio: transcribe to text first.
- Video: split into an image sequence and an audio track first; the raw video is never sent to the model.

Bypassing these preprocessing steps and calling `/wunder` directly puts attachment breakdown, transcription, frame extraction, and size control on the caller.

## Key Settings and Common Pitfalls

- `user_id` represents the caller and the isolation space; it need not be a registered user.
- `agent_id` represents the target agent.
- `/wunder` supports specifying an agent, but that does not mean it has the full capabilities of the chat domain; session governance has to be filled in yourself.
- Don't read `user_id` as "it must come from the user management table" — that creates needless integration restrictions.

## Related Endpoints

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
