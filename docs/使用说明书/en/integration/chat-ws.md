---
title: Chat WebSocket
summary: The real-time channel for chat and also the session control channel: starting an execution, resuming after a disconnect, watching, canceling, and returning approvals all happen on the same connection.
updated_at: 2026-10-13
---

`/wunder/chat/ws` does more than push events to you; it is also the control channel for a session. Both chat and desktop real-time sessions use it, and there is no SSE fallback.

## Use Cases

- You are building a chat workbench or a desktop session.
- You need to start, resume, watch, and cancel an execution on the same connection.
- You need long-lived connection keepalive and consistent real-time state.

## Connecting

1. Create a session first and take note of its ID.
2. Open the WebSocket connection.
3. Send the `connect` handshake.
4. Receive `ready`.
5. Send `start` to begin an execution.
6. Keep consuming `event` until this round's terminal state, `turn_terminal`.

Handshake example:

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

Starting an execution:

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

## What You Can Do on the Connection

- `connect`: handshake and protocol negotiation
- `start`: begin an execution
- `resume`: catch up on what was missed after a disconnect
- `watch`: observe a session without driving it
- `cancel`: cancel the current execution
- `approval`: return an approval decision
- `ping`: keepalive

The server mainly returns `ready`, `event`, `error`, and `pong`.

## Key Settings and Common Pitfalls

- Watch these event categories in particular: queued, approval resolved, error, and end of turn. `turn_terminal` is the terminal signal for a single execution round and works well as the frontend's convergence condition.
- `start` must explicitly carry the session ID.
- The body text uses `content`, not `/wunder`'s `question`.
- `resume` (catching up on events) and `watch` (observing the runtime state) are two different actions.
- Approvals on this connection only handle approval items that come from the WebSocket.

## Handling a Disconnect

Do not fall back to SSE. Reconnect to `/wunder/chat/ws`, then send as the situation requires:

- `resume`: carry the position of the last handled event to fill in the events missed during the disconnect.
- `watch`: observe the current session's runtime state again.

## Further Reading

- [Chat Sessions](/docs/en/integration/chat-sessions/)
- [wunder API](/docs/en/integration/wunder-api/)
- [Stream Events Reference](/docs/en/reference/stream-events/)
