---
title: Realtime
summary: The realtime channel runs on WebSocket; a reconnect catches up by cursor; what the client sees is a projection of backend truth; slow clients are uncoupled to protect the stream.
updated_at: 2026-10-13
---

Realtime is not about text appearing quickly but about three things: the **process stays visible**, a **disconnect can be caught up on**, and **what the client sees matches backend truth**. The approach is durable events, cursor-based catch-up, and a bounded projection — not hoping a reconnect happens to work.

![Realtime diagram: thread facts projected to client state through event streams and recovery mechanisms](/docs/assets/manual/core-realtime.svg)

## The backend is truth, the frontend is a projection

The thread log is the single source of truth. Every event push is **written synchronously to the persistence layer first**, and only then travels the realtime channel. The change wake-up signal only wakes subscribers; it is **not a data source**, and not the truth.

Three things follow directly:

- A dropped push can be recovered; the truth cannot be lost.
- When the interface shows something odd, trust the actual execution state — do not decide from the UI whether a task has finished.
- To tell whether a turn has ended, look only at terminal events, not at "it seems to have gone quiet".

## Chat streaming runs only over WebSocket

Streaming chat output runs only over WebSocket. Requesting the chat stream over HTTP is rejected with a message pointing you to `/wunder/chat/ws`. SSE is for external workflows only and is **not a fallback for chat**.

WebSocket endpoints are distributed by purpose:

| Endpoint | Purpose |
| --- | --- |
| `/wunder/chat/ws` | Main channel for the thread runtime: connect, heartbeat, start, resume, subscribe, and cancel, plus goal-type and approval-type messages |
| `/wunder/ws` | Core request surface: request submission and streaming responses |
| `/wunder/gateway/ws` | The gateway's three roles (operator, node, channel): request/response/event/heartbeat |
| `/wunder/interlink/ws` (paired with `POST /wunder/interlink/channel_ticket`) | Local node reverse tunnel |
| `/wunder/interlink/remote_ws` | A browser observing a remote single thread; forwarding only, nothing persisted |
| `/wunder/user_world/ws`, `/wunder/desktop/lan/ws` | Subscribing to and sending world-session messages; LAN networking |

Handshake and authentication: send a connection envelope first, or pass `access_token` / `token` directly as a query parameter. Any other message before the handshake completes is rejected.

## The events you receive

Event names are not enumerated in one place — each is declared where it is sent, and a new one is added the same way. The events that currently exist fall into these categories by purpose:

| Category | Events |
| --- | --- |
| Projection and catch-up | `thread_view_change`, `thread_view_required`, `thread_snapshot_required`, `thread_status` |
| Thread content | `thread_item_tail`, `thread_item_delta` |
| Model output | `llm_output_delta`, `llm_request` |
| Tools | `tool_call`, `tool_result` |
| Queueing | `queue_enter`, `queue_update`, `queue_start`, `queue_finish`, `queue_fail` |
| Usage and quota | `model_request_usage` |
| Terminal state | `final`, `turn_terminal` |
| Flow control and errors | `heartbeat`, `slow_client`, `stream_overflow`, `error`; `resume_recommended` is emitted together with `slow_client` or `stream_overflow` |

## How a reconnect catches up

The right way to reconnect after a disconnect is not to pull everything again, but to catch up with cursors. Three cursors each cover a different span:

| Cursor | Meaning |
| --- | --- |
| `change_seq` | The thread-change sequence; a request passes `after_change_seq` to say where to start catching up |
| `view_seq` | The display-projection sequence; the client uses it to tell whether its local projection is behind |
| `event_seq` / `event_id` | The sequence number of a single event, sent along with the event |

Catch-up goes over HTTP endpoints with the same authentication as the user side: a session's thread-log changes, snapshot, and events (`/wunder/chat/sessions/{id}/thread-log/changes|snapshot|events`), and the display projection's bootstrap, chat, trajectory, changes, and search (`/wunder/chat/sessions/{id}/view/bootstrap|chat|trajectory|changes|search`). The consumer always fetches 200 changes at a time and immediately continues to the next page once a page is full.

Submission is idempotent: include a `client_message_id`, and a repeated submission is simply judged "accepted" and returns the original result. Re-sending the same message after a reconnect does not write it twice.

## Backpressure and bounds

The realtime channel is bounded everywhere; the key values are:

| Item | Value |
| --- | --- |
| Per-subscription write queue | 256 |
| Slow-client watermark | judged slow when queue remaining ≤ 2 |
| Single-fetch cap | 200 |
| Heartbeat interval | 15 seconds |
| Single message cap | 512 KiB (256 KiB for interlink remote view) |
| User input cap | 1,048,576 characters (image attachments not counted) |

A slow client is handled by **notifying first, then uncoupling**: when the write queue crosses the watermark, a `slow_client` or `stream_overflow` is sent first along with `resume_recommended`, and the subscription is then dropped. This is not a failure but protection — better for one lagging subscription to exit and rebuild through catch-up than for a slow client to drag down the whole stream and the engine.

The right reaction to a `resume_recommended`: rebuild the projection through the catch-up endpoints above, then re-subscribe with the new cursor, rather than waiting for the same connection to bring back what was missed.

## Display reads are bounded and paged

For threads with a lot of data, the timeline and trajectory are not pulled in one shot; they are paged and given a response budget:

| Item | Value |
| --- | --- |
| Chat page size / cap | 12 / 20 |
| Trajectory page size / cap | 80 / 100 |
| Response budget | 128 KB for chat, 192 KB for trajectory |
| Body chunking | 8192 bytes |

While the projection is not ready, reads fall back to a **bounded window** over the source table: the page size is clipped to the cap and then to the response budget. Background rebuilds have a duty-cycle cap (no more than 1%, backing off from 5 seconds to 300 seconds after a failure), so a thread that was just imported or heavily changed may still read from the source table for a short while — showing up as a slightly slower first screen, not as missing data.

Thread listing pagination caps at 100, and keyword search scans at most 10 pages × 100, that is 1000 entries; anything beyond that is not found. This is a deliberate latency cap.

## Interlink: the other realtime path

The realtime surface between local devices (Pupa, Helm) and the cloud is interlink, which differs from the client WebSocket above:

- **Local nodes are outbound-only**: they open no listening port. The local side initiates a single long connection, and the control, data, and command planes all share it.
- **Shadow projection**: the cloud holds a projection of the local threads and workspace directories. Writes are gated on a live channel; once the channel expires, no new shadow is written.
- **Command ledger and approvals**: commands, confirmations, events, and results are all recorded. Risk is tiered L0–L3; `tool.exec` and `agent.spawn` are L3, the default device capability set excludes L3, and L3 approvals cannot be remembered.
- Interlink governance currently exposes only backend interfaces; **the Bridge has no interlink panel**.

Details and protocol are in [Interlink](/docs/en/capabilities/interlink/).

## Further reading

- [Streaming Execution](/docs/en/concepts/streaming/) — token increments and coalescing cadence
- [Stability](/docs/en/concepts/core-stability/) — timeouts, circuit breaking, and resource governance
- [Stream Events](/docs/en/capabilities/stream-events/) — detailed meanings of states and events
- [Chat WebSocket](/docs/en/capabilities/chat-ws/) — handshake and message format
- [Interlink](/docs/en/capabilities/interlink/) — tunnels, shadows, commands, and approvals
