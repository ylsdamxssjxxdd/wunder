---
title: Chat WebSocket
summary: The `/wunder/chat/ws` handshake, client message kinds, server replies, and how to reconnect.
updated_at: 2026-11-14
---

# Chat WebSocket

`/wunder/chat/ws` is both the event stream and the control channel: starting a run, resuming after a drop, watching, cancelling and returning approvals all happen on one connection. Chat and the desktop live session share it.

## Handshake

1. Create a session and keep its id.
2. Open the WebSocket.
3. Send `connect` to shake hands.
4. Receive `ready`.
5. Send `start` to begin a run.
6. Consume `event` until the turn's terminal signal.

```json
{
  "kind": "connect",
  "request_id": "req_connect_01",
  "payload": { "protocol_version": "1.0", "client_name": "my-chat-ui" }
}
```

```json
{
  "kind": "start",
  "request_id": "req_start_01",
  "session_id": "sess_xxx",
  "payload": { "content": "Continue with the weekly report", "stream": true }
}
```

## Message kinds

| kind | Purpose |
| --- | --- |
| `connect` | Handshake and protocol negotiation |
| `start` | Begin one run |
| `resume` | Replay events missed during a drop |
| `watch` | Observe a session only |
| `cancel` | Cancel the current run |
| `approval` | Return an approval decision |
| `ping` | Keepalive |

The server replies with `ready`, `event`, `error`, `pong`. `turn_terminal` is the terminal signal of a single turn and works well as the client's settling condition.

## Conventions

- `start` must carry the session id; the body field is `content`, not the HTTP endpoint's `question`.
- `resume` (replay) and `watch` (observe) are different actions; one does not replace the other.
- Only approvals delivered over this connection are handled on it.
- After a drop, reconnect to the same URL and send `resume` or `watch`. The desktop reconnects automatically.
