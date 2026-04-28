---
title: Chat WebSocket
summary: `/wunder/chat/ws` is Wunder's primary real-time chat channel; real-time sessions prefer WS, with SSE as fallback.
read_when:
  - You are developing chat UI or desktop real-time sessions
  - You need to handle start/resume/watch/cancel/approval
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/api/chat_ws.rs
---

# Chat WebSocket

`/wunder/chat/ws` is not just an "event stream"; it's also a session control channel.

## Applicable Scenarios

- Chat workbench or desktop sessions
- Execute `start / resume / watch / cancel` within the same connection
- Requires long-lived connection keepalive and real-time state consistency

## Endpoint

- `GET /wunder/chat/ws`

## Actions Available After Connection

- `connect`: Handshake and protocol negotiation
- `start`: Start an execution
- `resume`: Continue receiving events after disconnection
- `watch`: Only observe a session
- `cancel`: Cancel current execution
- `approval`: Return approval decision
- `ping`: Keepalive

Key server responses: `ready`, `event`, `error`, `pong`

## Minimal Integration Sequence

1. `POST /wunder/chat/sessions` to get `session_id`
2. Establish `WebSocket /wunder/chat/ws`
3. Send `connect`
4. Receive `ready`
5. Send `start`
6. Consume subsequent `event` until `turn_terminal`

## Minimal Handshake Example

```json
{
  "kind": "connect",
  "request_id": "req_connect_01",
  "payload": {
    "protocol_version": "1.0",
    "client_name": "my-chat-ui"
  }
}
```

## Start an Execution

```json
{
  "kind": "start",
  "request_id": "req_start_01",
  "session_id": "sess_xxx",
  "payload": {
    "content": "Continue helping me organize that weekly report from earlier",
    "stream": true
  }
}
```

## Key Events Clients Should Focus On

- `queued`
- `approval_resolved`
- `error`
- `turn_terminal`

`turn_terminal` is the terminal state signal for a single execution round, recommended as the frontend convergence condition.

## Common Pitfalls

- `start` must explicitly pass `session_id`.
- The text field is `content`, not `/wunder`'s `question`.
- `resume` (catch up on events) and `watch` (observe session) are not the same action.
- WS approval only handles approval items with `source=chat_ws`.

## Fallback on Failure

When WS is unavailable:

- Fallback to `POST /wunder` (SSE)
- Or fallback to `GET /wunder/chat/sessions/{session_id}/resume`

## Further Reading

- [Chat Sessions](/docs/en/integration/chat-sessions/)
- [wunder API](/docs/en/integration/wunder-api/)
- [Stream Events Reference](/docs/en/reference/stream-events/)