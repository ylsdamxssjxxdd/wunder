---
title: Sessions and Rounds
summary: How a conversation is numbered: threads, user rounds, model rounds, timeline entries, and the authoritative counting conventions.
updated_at: 2026-10-13
---

The numbering keeps **"how many times the user asked" separate from "how many steps the agent took"**. Statistics, replay, queueing, and token accounting all depend on this coordinate system.

## Hierarchy

```
Agent
└── Task thread (= session; one agent can have several at once)
     └── User round (incremented per accepted input)
          └── Model round (incremented per model call)
               └── Entry (visible reply, tool call, approval, subagent message, …)
```

| Concept | What it is | Decided by |
| --- | --- | --- |
| **Thread / session** | The main body of a continuous stretch of execution and dialogue; one session id runs through scheduling, locking, and state | Starting a thread starts a session; a subagent is an independent session traced back by its parent session id |
| **User round** | One accepted human input | Incremented in the session on acceptance; a uniqueness constraint guarantees "one root per round" |
| **Model round** | **One model call** of the engine's main loop, which may include a batch of tool calls | Incremented once per sample of the main loop |
| **Entry** | The smallest renderable record on the timeline | Upserted by a stable entry id when it is written |

These four layers are the coordinates:

| Coordinate | Meaning |
| --- | --- |
| User round | Which user round it belongs to |
| Model round | Which model call within that round (empty before sampling begins) |
| Round id | The persistent id of that user round, used as the prefix of the entry identifier |
| Target round | This round was started by the target driver rather than by a person, so a goal action that needs human authority is not started twice |

## Counting conventions

- **One model round = one model call**, not "one action". Tool calls belong to a batch within the same model round and are **not** counted as separate model rounds.
- **One model round produces exactly one visible output item**: refreshing several times within the same model round only updates that item and never appends duplicate bubbles.
- **A tool-batch entry on the timeline is not an independent model round.** Each tool call is its own entry (merging "in progress → done / failed" by call id), so the number of steps is usually larger than the number of model rounds.
- **A continuation round does not consume a user round**: a continuation that carries a root-round reference (for example, the target driver taking over) reuses the same user round and only adds model rounds within it.
- A user round publishes a change only when it is **genuinely newly created**; a replayed acceptance is an idempotent no-op that produces no new round and notifies no subscribers.

## Idempotency and queueing

| Mechanism | Convention |
| --- | --- |
| Submit idempotency | The same client message id reuses the existing round, so a network retry does not create an extra round |
| Serialization within a session | The session lock is isolated per session, with a 120-second timeout and a 5-second heartbeat; an abnormal exit releases it on timeout |
| Queueing | The queue toggle is on by default; messages sent while running enter the queue and run in order |
| Turning queueing off | A message sent while running is judged "user busy" and not queued (the hull warns at startup), so do not turn it off as a performance tweak |
| Background delivery | Scheduled messages take the forced-enqueue path so they are not lost while competing for the lock with human messages |

## What appears on the timeline

A session records only events with timeline value:

| Entry type | State flow |
| --- | --- |
| Tool call | In progress → done / failed (merged by the same call id) |
| Model call | Numbered by model round |
| Visible reply | The output item of that model round |
| Approval | Waiting → decided |
| Subagent message | Uses the producer-supplied message id as a persistent identity, so a replay never becomes a new entry |
| Queue ledger | Pending → running (upserted on the same entry); the interface skips it |

**Not recorded**: transport deltas, heartbeats, progress, context occupancy, workspace updates, model usage, and streaming retries, together with lifecycle receipts (ended, status, model response, and so on — failure details are carried by the round's terminal state). These are visible in the stream but should not become empty timeline entries.

## How the interface reads it

- The chat page shows **one user round per row** (12 per page by default, up to 20), folding everything between the final reply into a "**working process**".
- The working process is collapsed by default; the first expansion fetches that user round's persistent entries on demand rather than pulling the entire history into the frontend.
- The entry-by-entry panorama lives in the "**thread trace**" view (80 per page by default, up to 100), with long bodies fetched in 8 KB chunks.
- The directory list reads the tail of each round (highest user round, that round's state, a recent-activity summary, and the backfill cursor), querying a page of sessions in one batch.

## How to use it

- **When to start a new session**: changing topic; needing to apply new agent settings, a prompt template, or long-term memory (an existing session's prompt is already frozen); or when the context has grown long and is starting to lose early detail.
- **When to keep the current one**: the same task is still unfinished, or you need to reference earlier output. Continuing does not recompute the base; it only adds rounds.
- **How to point at "a certain step"**: give the coordinates "user round + model round", or the entry id directly; for tool batches give the entry sequence number only and do not call it "round N".

## Common mistakes

- **"One round = one question and answer"** — one user round can contain many model rounds, and each model round can carry a batch of tool calls.
- **"Step count = round count"** — they are not equal; steps (entries) are usually more.
- **"A session and a thread are two things"** — in this system they are the same: the session is the carrier of the conversation and its records, and the thread is its executing body.
- **"Submitting again charges an extra round"** — a retry with the same client message id is idempotent.
- **"A subagent's input shows up in the parent session"** — a sub-session's opening user message is hidden by default and skips automatic memory extraction, so it does not pollute your memory.
- **"Token statistics = the provider's bill"** — a session records context occupancy (trusting only the value the model reports), not a billing figure.

## Further reading

- [The agent loop](/docs/en/concepts/core-agent-loop/) — how model rounds advance and terminate
- [Streaming execution](/docs/en/concepts/streaming/) — the relationship between entries and deltas
- [Context compression](/docs/en/concepts/core-context-compression/) — how history is trimmed
- [Long-term memory](/docs/en/concepts/memory/) — memory isolated by scope
- [Session states](/docs/en/reference/stream-events/) — events and state fields
