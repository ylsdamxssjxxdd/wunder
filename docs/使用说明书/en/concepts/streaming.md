---
title: Streaming Execution
summary: Events are persisted before they are pushed, a reconnect resumes by cursor, and the frontend consumes bounded views.
updated_at: 2026-10-13
---

Streaming is not about making characters appear one by one: **the process is visible in real time, what you see is what was persisted, and a dropped connection can reconnect**.

## Persist first, then notify

When the engine emits an event, it first writes it synchronously into the session record (one serial channel per session, a single transaction), and only after the transaction succeeds does it publish the change cursor and wake subscribers. That gives three guarantees:

- **Commit order = change order**: each session has its own channel and writes complete in short transactions; a failed commit does **not** publish a cursor.
- **Idempotent and quiet**: a replayed acceptance, an unchanged state, or duplicate feedback produces no change and notifies no one.
- **Waking is only an accelerator**: the in-process wake channel is not a correctness guarantee, so the server always keeps a polling fallback for resumption (starting at 250 ms and backing off exponentially to 4 seconds), and multiple instances converge through polling.

So the text shown in the interface and the text read after a refresh come from the same source; there is no in-between state where the stream has something the store does not.

## WebSocket only

| Channel | Purpose |
| --- | --- |
| `/wunder/chat/ws` | The main channel for chat sessions: submit, resume, watch, cancel, and approve |
| `/wunder/ws` | The general request surface (submit one execution and receive it as a stream) |
| `/wunder/interlink/remote_ws` | A browser watching a single session on a remote device; pure forwarding, not persisted |
| `/wunder/user_world/ws`, `/wunder/desktop/lan/ws` | In-site sessions and LAN collaboration |

The default Pupa form **does not go through the local HTTP / WebSocket**: an in-process native event channel (bounded at 128 items) delivers directly, and losing a subscription or closing a window does not cancel a round already accepted on the server.

⚠️ **Chat has no SSE fallback.** The server does provide SSE, but only for the external integration surface and model-proxy calls, not as a degradation channel for chat.

## Four classes of events

| Class | Enters the timeline | Usage |
| --- | --- | --- |
| Entry events (tool call, model call, visible reply, approval, subagent message, compaction, round terminal) | Yes | Upserted by a stable entry id |
| Transport deltas (assistant body tail chunk, command output delta) | No | Add characters only: the assistant body uses a tail chunk, command output uses a delta |
| Status and receipts (session state, ended, error, session closed, queueing) | Partly (queueing is a persistent record) | Judge the lifecycle; not rendered as entries |
| Wake and backfill (view changed, view needs refetching, snapshot needed, stream overflow, slow client, reconnect suggested) | No | The client resumes or refetches wholesale based on these |

Statistics (context occupancy, token usage, round usage, model usage, quota) are only for panel refreshes and do not enter the timeline.

## Disconnection and backfill

The client keeps a **persistent cursor** (a change sequence number) and sends it when reconnecting:

| Situation | Server signal | What the client does |
| --- | --- | --- |
| Behind, but not by much | Resume sending change rows from the cursor | Apply incrementally in place |
| Behind beyond the window | Stream overflow + reconnect suggested | Receive the signal first, then drop that subscription and reconnect by cursor |
| The persistent window has been trimmed | Snapshot needed | Give up on resuming and pull one atomic snapshot |
| The displayed view is behind | View needs refetching / view changed | Refetch the paged view rather than stitching entry by entry |

The backfill interfaces come as a set: session change list, atomic snapshot, event paging, and single-entry body.

## Bounds are the default

| Parameter | Value |
| --- | --- |
| Write queue per connection | 256 (full means slow client) |
| Slow-client watermark | 2 (alarm / degrade once the queue is deeper) |
| Resume page size | 200 |
| Event read limit | 200 |
| Heartbeat | 15 seconds |
| Single message | 512 KiB (rejected when exceeded; 256 KiB for remote watch) |
| Chat page / trace page | 12 (up to 20) / 80 (up to 100), response budget 128 KB / 192 KB |
| Body chunk | 8 KB |

The frontend consumes a **view**: a chat page holds only 12 user rounds, and everything between the final reply in each round folds into a "working process" that fetches that round's entries only when expanded. The trace is the entry-by-entry view (up to 8 subagent cards per row).

## How to judge

- **Whether a round is over**: look at the round's terminal state (with status, stop reason, and optional error and recovery action) rather than guessing that "it seems stopped". While waiting for approval or more information, the session is still within its lifecycle.
- **Duplicate bubbles**: most likely the client appended a new item instead of upserting by entry id. Locate the same item by "round id + model round".
- **Missing characters**: check whether the delta was treated as the only source. A delta only provides continuous display; the truth of the body is in the entry, so after reconnecting you should backfill rather than guess.
- **Content gets longer after a refresh**: that is normal. A refresh reads the persisted body, which may be more complete than the streamed tail chunk.

## Common mistakes

- Streaming and persistence are not two paths: events are persisted synchronously when emitted, and the cursor is published after the transaction.
- A disconnect is not an interrupted execution: execution continues on the server, and the client simply reconnects by cursor.
- SSE is not a fallback for chat; chat is WebSocket only.
- Fetching a few more times will not return the full history: views are deliberately bounded, and moving the full history is exactly what the design avoids.
- "Seeing text stream out" does not mean "the loop ran many rounds": text deltas and model rounds are two different granularities, and one round can contain thousands of deltas.

## Further reading

- [Session states](/docs/en/capabilities/stream-events/) — the list of events and fields
- [Chat WebSocket](/docs/en/capabilities/chat-ws/) — protocol and envelope
- [Sessions and rounds](/docs/en/concepts/sessions-and-rounds/) — entry and round granularity
- [Real-time behavior](/docs/en/concepts/core-realtime/) — design principles
- [System architecture](/docs/en/concepts/architecture/) — the layering of view and truth
