---
title: Chat WebSocket
summary: The handshake flow, client message types, server returns, and reconnect behavior of `/wunder/chat/ws`.
updated_at: 2026-10-16
---

`/wunder/chat/ws` is both the event-push channel and the control channel for a session: starting an execution, resuming after a disconnect, watching, canceling, and returning approvals all happen on the same connection. Chat and desktop real-time sessions both use it, and there is no SSE fallback.

## Use Cases

- Chat workbenches and desktop sessions;
- Starting, resuming, watching, and canceling an execution on the same connection;
- Long-lived connection keepalive and consistent real-time state.

## Handshake Flow

1. Create a session and take its ID.
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

## Message Types

Client to server:

| kind | Effect |
| --- | --- |
| `connect` | Handshake and protocol negotiation |
| `start` | Begin an execution |
| `resume` | Catch up on events missed after a disconnect |
| `watch` | Observe a session without driving it |
| `cancel` | Cancel the current execution |
| `approval` | Return an approval decision |
| `ping` | Keepalive |

The server mainly returns `ready`, `event`, `error`, and `pong`.

## Settings and Limits

- Event categories to consume: queued, approval resolved, error, and end of turn. `turn_terminal` is the terminal signal for a single execution round and works as the frontend's convergence condition.
- `start` must explicitly carry the session ID.
- The body text uses `content`, not `/wunder`'s `question`.
- `resume` (catching up on events) and `watch` (observing the runtime state) are two different actions.
- Approvals on this connection only handle approval items that come from the WebSocket.

## Disconnect and Reconnect

Do not fall back to SSE. Reconnect to `/wunder/chat/ws`, then send as the situation requires:

- `resume`: carry the position of the last handled event to fill in the events missed during the disconnect.
- `watch`: observe the current session's runtime state again.
