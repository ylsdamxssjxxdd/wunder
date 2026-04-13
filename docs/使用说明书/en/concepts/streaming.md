---
title: "Streaming Execution"
summary: "Wunder's streaming is not just a character stream, but exposes thread execution, tool calls, and terminal states together."
read_when:
  - "You are integrating SSE or WebSocket"
  - "You are designing a state machine for a chat interface"
source_docs:
  - "docs/API文档.md"
  - "docs/设计方案.md"
  - "src/api/chat.rs"
  - "src/api/core.rs"
---

# Streaming Execution

In Wunder, the key point of streaming is not "text appearing gradually", but "the entire thread state being continuously projected".

## Key Points

This page answers three things:

- Why the streaming pipeline exposes both process events and terminal events
- Why chat prioritizes WebSocket, while the general execution entry point still retains SSE
- What signals the client should use to determine "still running" vs "already finished"

## When to Read This

If you are doing any of the following, you should read this page first:

- Chat window streaming output
- Tool call intermediate process display
- Connection resumption or playback recovery
- Single execution state machine design

## Streaming Doesn't Have Just One Entry Point

Wunder currently has two main streaming paths:

- `/wunder`: Unified execution entry point, returns SSE when `stream=true`
- `/wunder/chat/ws`: Main chat real-time channel, supports start, resume, watch, cancel, approval

So the correct understanding is not "choose between WS and SSE", but:

- Chat scenarios prioritize WS
- General execution and compatibility scenarios use SSE

## What You Really Need to Care About Is Not All Event Names

There are many events, but their semantics are not equally important.

### Process Events

These are used to show what happened:

- `progress`
- `llm_output_delta`
- `tool_call`
- `tool_output_delta`
- `tool_result`

### Status Events

These are used to express where the thread currently is:

- `queued`
- `approval_request`
- `thread_status`

### Terminal Events

These are used to express whether things have been closed-loop:

- `approval_resolved`
- `turn_terminal`

## Common Client Mistakes

### Only Watching `final`

This will miss failures, rejections, cancellations, and pending approvals.

### Only Watching `running`

In the chat domain, this is just a compatibility field, not sufficient to express the complete runtime state.

### Treating SSE as the Complete Chat Protocol

SSE can show results, but WebSocket is more suitable for session control.

## Implementation Suggestions

- To determine if a round has ended, watch `turn_terminal`.
- To determine what state the thread is currently in, watch `thread_status` or session `runtime`.
- For whether approval is completely closed-loop, watch `approval_resolved`.
- Chat panels should default to WebSocket, with SSE as a fallback.

## Further Reading

- [Stream Events Reference](/docs/en/reference/stream-events/)
- [Chat WebSocket](/docs/en/integration/chat-ws/)
- [wunder API](/docs/en/integration/wunder-api/)