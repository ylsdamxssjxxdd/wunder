---
title: "Runtime and Online Status"
summary: "Wunder doesn't just return message content; it continuously maintains runtime projections of threads and sessions to express states like loaded, waiting, streaming, and idle."
read_when:
  - "You are building session lists, status badges, or runtime panels"
  - "You are troubleshooting whether a thread is stuck or has finished"
source_docs:
  - "docs/API文档.md"
  - "src/api/chat.rs"
  - "src/core/session_runtime.rs"
---

# Runtime and Online Status

Wunder sessions are not static history records; they always carry a layer of runtime state.

## Key Points on This Page

If you want to know:

- Whether a thread is still alive
- Why session lists need `runtime`
- What `running`, `thread_status`, and `terminal_status` each express

This page is designed to answer these questions.

## Don't Confuse the Two Layers of State

### Thread Event Status

The `thread_status` in the event stream is more like "what the thread is doing right now."

Common values include:

- `running`
- `waiting_approval`
- `waiting_user_input`
- `interrupting`
- `idle`
- `not_loaded`

### Session Runtime Projection

The chat domain further aggregates status into a `runtime` object for direct consumption by list and detail pages.

Key fields include:

- `status`
- `loaded`
- `active`
- `streaming`
- `waiting`
- `watcher_count`
- `pending_approval_count`
- `monitor_status`
- `monitor_stage`
- `terminal_status`

## Why Not Just Keep a Single `running`

Because `running=true/false` is too coarse.

It cannot answer these questions:

- Is it continuously outputting, or waiting for approval
- Is the session loaded into memory
- Are there any watchers attached to this thread
- Has this round already reached `completed/failed/cancelled/rejected`

So Wunder now retains `running` as a compatibility field, but stable integrations should look at `runtime`.

## Most Common Status Judgments on Session Pages

If you are building a list or detail page, this approach is usually sufficient:

- Is it still continuously executing: check `runtime.streaming`
- Is it in a waiting state: check `runtime.waiting`
- Has it reached a terminal state: check `runtime.terminal_status`
- Can it still be observed: check `runtime.loaded` and `runtime.active`

## When It's Easiest to Make Mistakes

### Treating "Waiting for Approval" as "Already Finished"

This causes the frontend to incorrectly restore the input box or hide the approval area.

### Only Reading Status on Detail Pages

Session lists, event pages, and thread panels should all reuse the same runtime semantics.

### Mixing Event Stream and Summary State into One Layer

Events are responsible for the timeline, `runtime` is responsible for the current snapshot.

## Implementation Suggestions

- `thread_status` is suitable for expressing thread state during the process.
- `runtime` is suitable for direct consumption by session lists, detail pages, and control panels.
- `running` is just a compatibility field and should no longer serve as a complete state machine.

## Further Reading

- [Sessions and Rounds](/docs/en/concepts/sessions-and-rounds/)
- [Streaming Execution](/docs/en/concepts/streaming/)
- [Chat Sessions](/docs/en/integration/chat-sessions/)