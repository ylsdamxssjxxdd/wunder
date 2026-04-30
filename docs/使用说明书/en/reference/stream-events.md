---
title: Stream Events
summary: When integrating with Wunder's streaming pipeline, the most important thing is not memorizing every event, but knowing which events carry true lifecycle semantics.
read_when:
  - You are integrating SSE or WebSocket
  - You want to understand why turn_terminal, approval_resolved, and thread_status matter
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/orchestrator/execute.rs
  - src/api/chat.rs
---

# Stream Events

Wunder has many streaming events, but what truly matters is not "how many", but "which ones carry state semantics".

## Core Event Priority

- `thread_status`
- `approval_resolved`
- `turn_terminal`

If you only consume the legacy `final` or `error` events, you can easily misjudge thread state.

## Category 1: Process Events

These events are primarily used to visualize the execution process:

- `progress`
- `llm_output_delta`
- `round_usage`
- `tool_call`
- `tool_output_delta`
- `tool_result`

They tell you what the model and tools are doing, but they are not responsible for determining "whether a turn has ended."

### How to Understand `round_usage`

If you need to display token statistics, the event you should consume first is `round_usage`:

- `round_usage.total_tokens`: the actual context occupancy after the current request completes
- `round_usage.context_occupancy_tokens`: same as above, but with a more explicit field name
- `round_usage.request_consumed_tokens`: the consumption of the current request; the cumulative consumption for the entire session is the sum of these values across requests

`token_usage` is still valuable, but it is more focused on individual model call details and is no longer the sole authority for thread-level context occupancy.

## Category 2: Queueing & Waiting Events

Typical events include:

- `queued`
- `approval_request`

They indicate:

- The request has entered the queue
- The current turn is waiting for approval

## Category 3: Closure Events

### `approval_resolved`

This indicates that an approval has reached a terminal state.

Whether approved, rejected, or cancelled, this event should be the definitive signal that "the approval flow is fully resolved."

### `turn_terminal`

This is the sole terminal semantic for the current execution turn.

Its `status` may include:

- `completed`
- `failed`
- `cancelled`
- `rejected`

If you are building a state machine, use this as the primary indicator that "a turn has ended."

### `thread_status`

This describes the current runtime state of the thread.

Typical states include:

- `running`
- `waiting_approval`
- `waiting_user_input`
- `interrupting`
- `idle`
- `not_loaded`

It answers "is the thread alive right now, and where is it stuck?"

## Why You Cannot Rely on `final` Alone

Because `final` is more like "there is a final answer text."

But in real-world execution you will also encounter:

- Rejections
- Cancellations
- Waiting for approval
- Mid-execution failures

In these scenarios, looking only at `final` is insufficient.

## Minimum Handling for New Integrations

If you are building a new client, at minimum correctly handle:

- `queued`
- `thread_status`
- `approval_request`
- `approval_resolved`
- `turn_terminal`
- `error`

This ensures your state machine does not only work on the "happy path."

## Chat WebSocket Boundary

For chat sessions, `/wunder/chat/ws` is the realtime authority:

- Use WebSocket for `start`, `resume`, `watch`, `cancel`, approval decisions, and realtime status.
- Do not fall back to SSE when a chat WebSocket disconnects; reconnect and replay with `resume/watch`.
- Non-chat streaming endpoints, such as `/wunder` or A2A, keep their own protocol semantics.

This boundary prevents two transport projections from disagreeing about the same chat turn.

## A Simple Decision Guide

If you just want to know whether a turn has ended:

- Check `turn_terminal`

If you want to know the current thread state:

- Check `thread_status`

If you want to know whether the approval flow has fully resolved:

- Check `approval_resolved`

## Further Reading

- [Wunder API](/docs/en/integration/wunder-api/)
- [Chat WebSocket](/docs/en/integration/chat-ws/)
- [API Index](/docs/en/reference/api-index/)
